"""PPO self-play: learn Ants from a reward instead of from a teacher.

    python -m tb_baselines.train.ppo --class micro --iters 200

This is the interesting half of the matrix and the risky one. Ants is sparse-reward, long-horizon,
multi-agent and fogged; behaviour cloning is none of those. Read `eval.py`'s table before believing
a rising return.

## Both seats are the same policy, and that is the point

Every match seats the learner against itself, so one match yields two trajectories and the opponent
improves exactly as fast as the learner does. It also means the observation must be **symmetric** —
which it now is: engine `sha256:f17b51b6c92b` made a view's owner labels relative to the observer,
and before that the two seats of one match were, quietly, two different games.

## The action space is factorised, not joint

A seat commands up to 180 ants from one forward pass. The joint action space is 5^180, and the joint
distribution is not what the network produces: it produces a per-cell distribution, and each ant
reads its own cell. So

    log pi(a | s) = sum over ants of log pi(a_i | s, pos_i)

which is the standard factorised policy, and the entropy sums the same way. The consequence to keep
in mind is that **a seat's log-probability scales with its ant count**, so a colony of 90 has a
gradient signal thirty times a colony of 3. The ratio in the PPO objective is formed per ANT rather
than per seat for exactly that reason: a per-seat ratio is `exp(sum of 90 differences)`, which
overflows the clip range on the first update and makes the algorithm a no-op.

## The critic is privileged and is thrown away

`tinybrains env --scores every` reports each match's true score every turn — `f_finish` answers it
for a live match — and the value head reads it. That is asymmetric actor-critic, not a hole in the
fog: `export.py` ships the trunk and the policy head only, so nothing that saw a score survives into
anything that plays.

## Rollouts hold observations, not boards

A `[7, 128, 128]` int8 board is 114 KiB, so a rollout of 64 steps across 100 live seats would be
730 MB of tensors. The observations behind them are about 2 KB each. They are re-encoded during the
update, which costs about a second per epoch and makes the rollout length a free parameter instead
of a memory budget.

## Throughput is shapes and waits, not arithmetic

On the M2 Pro this is developed on, micro's forward+backward is 2.7 ms a 128x128 board on MPS (fp32;
the CPU is ten times slower), which made an iteration's update about ten seconds of arithmetic — and
it took twenty-four. The difference was two things, and neither shows in a profile as itself: both
surface as time in whichever op next touches the device.

- **MPS compiles a graph per tensor shape.** The per-ant tensors had a new length on nearly every
  call, because the ant count of a minibatch almost never repeats: 153 ms a minibatch at a fresh
  length against 60 ms at one it had seen. They are padded to `_bucket` now, with zero weight.
- **A blocking copy to MPS waits for everything already queued.** Every minibatch moved eight
  arrays and read four statistics back with `float()`, so each one waited out the previous one's
  backward pass. `_Group` moves a board size's data once an update and the loop only slices it.

With both gone the update is the arithmetic, about ten seconds, and a 48-turn iteration went from
23 / 34 / 39 s (it grew as colonies grew, and with them the number of unseen lengths) to 13 / 13 /
14 s. What is left is the convolutions. `--amp` (fp16 autocast) measured another 8% and is off by
default: its gradient is about 1% from fp32's, which is not nothing for a small speed-up.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import time
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from .. import nets
from ..env import Env, orders_from_indices
from ..export import budget, classes
from ..planes import N_MOVES
from .bc import device

ANT_BUCKET = 256


def _bucket(n: int, q: int = ANT_BUCKET) -> int:
    """`n` rounded up to a multiple of `q`, and never zero.

    MPS caches a compiled graph per tensor shape, so a per-ant tensor whose length never repeats is
    a compile on every call. Measured on one minibatch of 32 boards at 96x96: 59.6 ms at a length
    already seen, 153.5 ms at a fresh one, 57.6 ms padded to a multiple of 256 (128 was not enough:
    69.4). Padded rows are masked out of every sum, so the arithmetic is the same.
    """
    return max(q, -(-n // q) * q)


def _autocast(dev: torch.device, enabled: bool):
    """fp16 activations for the trunk, and nothing else: every caller casts the logits and the value
    back to fp32 before a softmax, a ratio or a loss touches them."""
    if not enabled:
        return contextlib.nullcontext()
    return torch.autocast(dev.type, dtype=torch.float16)


@dataclass
class Reward:
    """What a turn is worth. Every term is a training-time choice and none is the ladder's.

    The ladder pays for rank, and rank comes from score: +2 for razing an enemy hill, -1 for losing
    one of yours. That signal arrives a handful of times in three hundred turns, so the shaping
    terms exist to make the first thousand updates about something. They are declared here rather
    than buried so that a run's card can name them.
    """

    score: float = 1.0          # your own score, turn on turn
    rival: float = 0.5          # minus theirs: Ants is not zero-sum, but taking a hill is
    ants: float = 0.02          # the food economy, which is the only dense signal there is
    win: float = 1.0            # at the end, on rank rather than on margin
    gamma: float = 0.997        # about a 300-turn horizon
    lam: float = 0.95


@dataclass
class Trajectory:
    """One seat's episode, keyed by `(ep, seat)`. `ep` is stable across a wave refill and `(w, m)`
    is not."""

    obs: list[dict] = field(default_factory=list)
    # The encoded board, kept from the rollout. `encode` is not free and the update would otherwise
    # redo it once per epoch for every sample -- work that was already done to choose the action.
    boards: list[np.ndarray] = field(default_factory=list)
    actions: list[np.ndarray] = field(default_factory=list)
    logp: list[np.ndarray] = field(default_factory=list)
    values: list[float] = field(default_factory=list)
    rewards: list[float] = field(default_factory=list)
    last_score: int = 0
    last_rival: int = 0
    last_ants: int = 0
    primed: bool = False        # a match opens at score 1, so the first delta must not count it
    finished: bool = False


def gae(rewards: list[float], values: list[float], bootstrap: float, r: Reward):
    """Advantages and returns for one seat. `bootstrap` is 0 at a real terminal and V(s_T) when the
    rollout simply stopped -- collapsing the two is how a truncated episode teaches the agent that
    the world ends."""
    adv, out = [], 0.0
    nxt = bootstrap
    for t in reversed(range(len(rewards))):
        delta = rewards[t] + r.gamma * nxt - values[t]
        out = delta + r.gamma * r.lam * out
        adv.append(out)
        nxt = values[t]
    adv.reverse()
    returns = [a + v for a, v in zip(adv, values)]
    return adv, returns


class Runner:
    """The rollout loop: env in, trajectories out."""

    def __init__(self, model: nets.ActorCritic, env: Env, dev: torch.device, reward: Reward,
                 seed: int = 0, amp: bool = False):
        self.model = model
        self.env = env
        self.dev = dev
        self.reward = reward
        self.amp = amp
        # Moves are drawn on the host, from its own stream, so `update`'s minibatch permutations
        # stay on the global numpy stream they have always used.
        self.rng = np.random.default_rng(seed)
        self.step = env.reset()
        self.open: dict[tuple[int, int], Trajectory] = {}
        self.done: list[Trajectory] = []
        self.returns: list[float] = []

    @torch.no_grad()
    def act(self, boards: np.ndarray, rows: "_AntIndex"):
        """Sample a move for every ant on every board in one group.

        The policy at an ant's cell is `softmax(logits)` there — the factorised policy `update`
        differentiates — and the draw is by inverse CDF on the host, an exact sample from it.
        `torch.distributions.Categorical` on MPS cost a `multinomial` that waited on the forward
        pass and then two more transfers back; this is one.
        """
        t = torch.from_numpy(boards).to(self.dev)
        with _autocast(self.dev, self.amp):
            logits, values = self.model(t)
        per_ant = logits[rows.board, :, rows.r, rows.c].float()      # [_bucket(n), moves]
        logp_all = torch.log_softmax(per_ant, dim=-1)
        host = torch.cat([logp_all.reshape(-1), values.float()]).cpu().numpy()
        cut = logp_all.numel()
        logp = host[:cut].reshape(-1, N_MOVES)[: rows.n]
        cdf = np.cumsum(np.exp(logp.astype(np.float64)), axis=1)
        u = self.rng.random(rows.n) * cdf[:, -1]
        # the first move whose cumulative probability exceeds u; never one of probability zero
        picks = np.minimum((cdf <= u[:, None]).sum(axis=1), N_MOVES - 1)
        return picks, logp[np.arange(rows.n), picks], host[cut:]

    def collect(self, steps: int) -> dict:
        """`steps` env turns. Returns statistics; the trajectories accumulate on `self`."""
        self.done, self.returns = [], []
        for _ in range(steps):
            actions: list[str | None] = [None] * len(self.step.seats)

            for group in self.step.groups:
                rows = _AntIndex(self.step.seats, group.indices).to(self.dev)
                picks, logp, values = self.act(group.boards, rows)
                orders = orders_from_indices(picks, rows.counts)

                at = 0
                for k, i in enumerate(group.indices):
                    n = rows.counts[k]
                    actions[i] = orders[k]
                    self._record(self.step.seats[i], group.boards[k : k + 1], picks[at : at + n],
                                 logp[at : at + n], float(values[k]))
                    at += n

            prev = self.step
            self.step = self.env.step([a if a is not None else "" for a in actions])
            self._reward(prev)
            self._close(self.step.ended)

        # Whatever is still open bootstraps rather than pretending to have ended.
        return {
            "episodes": len(self.returns),
            "mean_return": float(np.mean(self.returns)) if self.returns else 0.0,
        }

    def _record(self, seat, board, picks, logp, value) -> None:
        key = (seat.ep, seat.seat)
        traj = self.open.setdefault(key, Trajectory())
        if not traj.obs:
            traj.last_ants = len(seat.obs["mine"])
        traj.obs.append(seat.obs)
        traj.boards.append(board)
        traj.actions.append(picks.copy())
        traj.logp.append(logp.copy())
        traj.values.append(value)

    def _reward(self, prev) -> None:
        """One reward a seat, from the score the env reported and the colony it kept."""
        r = self.reward
        for seat in prev.seats:
            key = (seat.ep, seat.seat)
            traj = self.open.get(key)
            if traj is None or len(traj.rewards) >= len(traj.obs):
                continue
            scores = self.step.scores.get(seat.ep) or {e.ep: e.scores for e in self.step.ended}.get(seat.ep)
            if scores is None:
                traj.rewards.append(0.0)
                continue
            mine = scores[seat.seat]
            rival = max(s for i, s in enumerate(scores) if i != seat.seat)
            ants = len(seat.obs["mine"])
            if not traj.primed:
                # Ants opens every seat on 1 point, and a colony starts with one ant. Priming from
                # zero would pay a point and an ant for turning up.
                traj.last_score, traj.last_rival, traj.last_ants = mine, rival, ants
                traj.primed = True
            value = (
                r.score * (mine - traj.last_score)
                - r.rival * (rival - traj.last_rival)
                + r.ants * (ants - traj.last_ants)
            )
            traj.last_score, traj.last_rival, traj.last_ants = mine, rival, ants
            traj.rewards.append(value)

    def _close(self, ended) -> None:
        for e in ended:
            for seat in range(e.seat_count):
                traj = self.open.pop((e.ep, seat), None)
                if traj is None or not traj.obs:
                    continue
                best = min(e.ranks)
                outcome = 1.0 if e.ranks[seat] == best and e.ranks.count(best) == 1 else (
                    0.0 if e.ranks[seat] == best else -1.0
                )
                while len(traj.rewards) < len(traj.obs):
                    traj.rewards.append(0.0)
                traj.rewards[-1] += self.reward.win * outcome
                traj.finished = True
                self.done.append(traj)
                self.returns.append(float(sum(traj.rewards)))

    def harvest(self) -> list[Trajectory]:
        """Finished episodes, plus a snapshot of everything still running so a long match still
        contributes. The open ones keep their running score, so nothing is collected twice and the
        next segment's first delta is measured from where this one stopped."""
        batch = list(self.done)
        for key, traj in list(self.open.items()):
            if len(traj.rewards) >= len(traj.obs) and traj.obs:
                batch.append(traj)
                self.open[key] = Trajectory(
                    last_score=traj.last_score, last_rival=traj.last_rival,
                    last_ants=traj.last_ants, primed=True,
                )
        return batch


class _AntIndex:
    """Flat `(board, row, col)` for every ant in a group, which is the projection the policy head
    and the adapter's `out` program both do.

    Padded to `_bucket(n)` rows so the gather has a shape MPS has seen before; the padding reads
    board 0's cell (0, 0), and `act` drops it after the transfer back."""

    def __init__(self, seats, indices):
        mine = [np.asarray(seats[i].obs["mine"], dtype=np.int64).reshape(-1, 2) for i in indices]
        self.counts = [len(m) for m in mine]
        self.n = sum(self.counts)
        idx = np.zeros((3, _bucket(self.n)), dtype=np.int64)
        if self.n:
            idx[0, : self.n] = np.repeat(np.arange(len(mine)), self.counts)
            idx[1:, : self.n] = np.concatenate(mine).T
        self._idx = torch.from_numpy(idx)
        self.board, self.r, self.c = self._idx

    def to(self, dev):
        self._idx = self._idx.to(dev)
        self.board, self.r, self.c = self._idx
        return self


@dataclass
class _Minibatch:
    boards: torch.Tensor        # [B, planes, rows, cols] int8
    board: torch.Tensor         # [M] which of those boards the ant is on
    r: torch.Tensor             # [M]
    c: torch.Tensor             # [M]
    act: torch.Tensor           # [M] the move it was given
    ret: torch.Tensor           # [B]
    old_lp: torch.Tensor        # [M]
    adv: torch.Tensor           # [M] its seat's advantage, repeated
    mask: torch.Tensor          # [M] 1 for an ant, 0 for the padding up to `_bucket`
    n: int                      # ants, not counting padding


class _Group:
    """One board size's samples, moved to the device once an update.

    The boards and every per-ant array go over once; each epoch's minibatch plan goes over as two
    arrays; and the loop itself only slices tensors that are already there, so nothing inside it
    waits on the host.
    """

    def __init__(self, samples, advs: np.ndarray, members: list[int], dev):
        self.n = len(members)
        mine = [np.asarray(samples[i][0]["mine"], dtype=np.int64).reshape(-1, 2) for i in members]
        self.counts = np.array([len(m) for m in mine], dtype=np.int64)
        self.starts = np.cumsum(self.counts) - self.counts
        rc = np.concatenate(mine)
        self.r, self.c = rc[:, 0], rc[:, 1]
        self.act = np.concatenate([samples[i][2] for i in members]).astype(np.int64)
        self.old_lp = np.concatenate([samples[i][3] for i in members]).astype(np.float32)
        self.adv = np.repeat(advs[members], self.counts)
        self.ret = np.array([samples[i][5] for i in members], dtype=np.float32)
        self.boards = torch.from_numpy(
            np.concatenate([samples[i][1] for i in members], 0)).to(dev)

    def minibatches(self, order: np.ndarray, size: int, dev):
        """This epoch's minibatches, `size` samples at a time in `order`."""
        ints, flts, cuts = [], [], []
        for start in range(0, len(order), size):
            sel = order[start : start + size]
            cnt = self.counts[sel]
            n = int(cnt.sum())
            m = _bucket(n)
            ant = np.arange(n) + np.repeat(self.starts[sel] - (np.cumsum(cnt) - cnt), cnt)
            ib = np.zeros(len(sel) + 4 * m, dtype=np.int64)
            ib[: len(sel)] = sel
            per = ib[len(sel):].reshape(4, m)
            per[0, :n] = np.repeat(np.arange(len(sel)), cnt)
            per[1, :n] = self.r[ant]
            per[2, :n] = self.c[ant]
            per[3, :n] = self.act[ant]
            fb = np.zeros(len(sel) + 3 * m, dtype=np.float32)
            fb[: len(sel)] = self.ret[sel]
            fper = fb[len(sel):].reshape(3, m)
            fper[0, :n] = self.old_lp[ant]
            fper[1, :n] = self.adv[ant]
            fper[2, :n] = 1.0
            ints.append(ib)
            flts.append(fb)
            cuts.append((len(sel), m, n))

        ints_t = torch.from_numpy(np.concatenate(ints)).to(dev)
        flts_t = torch.from_numpy(np.concatenate(flts)).to(dev)
        io = fo = 0
        for b, m, n in cuts:
            iv = ints_t[io : io + b + 4 * m]
            fv = flts_t[fo : fo + b + 3 * m]
            io += b + 4 * m
            fo += b + 3 * m
            board, r, c, act = iv[b:].view(4, m)
            old_lp, adv, mask = fv[b:].view(3, m)
            yield _Minibatch(self.boards.index_select(0, iv[:b]), board, r, c, act,
                             fv[:b], old_lp, adv, mask, n)


def update(model, opt, batch: list[Trajectory], r: Reward, dev, epochs: int,
           clip: float, entropy: float, minibatch: int, amp: bool = False,
           scaler: "torch.amp.GradScaler | None" = None) -> dict:
    """PPO, with the ratio formed per ant.

    A per-seat ratio would be `exp(sum over 90 ants of the log-probability difference)`, which
    leaves any sane clip range on the first update. Per ant is both correct for a factorised policy
    and numerically the only version that does anything.

    The per-ant log-probability and entropy are `log_softmax` and `-sum(p log p)` written out, which
    is what `torch.distributions.Categorical` computes; the means are masked sums over the real ants
    divided by their count. The statistics stay on the device until the last minibatch.
    """
    samples = []
    for traj in batch:
        adv, ret = gae(traj.rewards, traj.values, 0.0 if traj.finished else traj.values[-1], r)
        for t in range(len(traj.obs)):
            if len(traj.actions[t]) == 0:
                continue
            samples.append((traj.obs[t], traj.boards[t], traj.actions[t], traj.logp[t],
                            adv[t], ret[t]))
    if not samples:
        return {"samples": 0}

    advs = np.array([s[4] for s in samples], dtype=np.float32)
    advs = (advs - advs.mean()) / (advs.std() + 1e-8)

    by_size: dict[tuple[int, int], list[int]] = defaultdict(list)
    for i, sample in enumerate(samples):
        by_size[tuple(sample[0]["size"])].append(i)
    groups = [_Group(samples, advs, members, dev) for members in by_size.values()]

    rows = []
    for _ in range(epochs):
        for group in groups:
            for mb in group.minibatches(np.random.permutation(group.n), minibatch, dev):
                with _autocast(dev, amp):
                    logits, values = model(mb.boards)
                per_ant = logits[mb.board, :, mb.r, mb.c].float()
                logp = torch.log_softmax(per_ant, dim=-1)
                new_lp = logp.gather(1, mb.act.unsqueeze(1)).squeeze(1)

                ratio = torch.exp(new_lp - mb.old_lp)
                policy_loss = -(torch.min(
                    ratio * mb.adv, torch.clamp(ratio, 1 - clip, 1 + clip) * mb.adv
                ) * mb.mask).sum() / mb.n
                value_loss = F.mse_loss(values.float(), mb.ret)
                # Clamped as Categorical clamps it, so a -inf log-probability times a zero
                # probability is 0 rather than NaN.
                p_log_p = logp.exp() * logp.clamp(min=torch.finfo(logp.dtype).min)
                ent = (-p_log_p.sum(-1) * mb.mask).sum() / mb.n
                loss = policy_loss + 0.5 * value_loss - entropy * ent

                opt.zero_grad(set_to_none=True)
                if scaler is None:
                    loss.backward()
                    torch.nn.utils.clip_grad_norm_(model.parameters(), 0.5)
                    opt.step()
                else:
                    scaler.scale(loss).backward()
                    scaler.unscale_(opt)
                    torch.nn.utils.clip_grad_norm_(model.parameters(), 0.5)
                    scaler.step(opt)
                    scaler.update()

                clipped = (((ratio - 1).abs() > clip).float() * mb.mask).sum() / mb.n
                rows.append(torch.stack([policy_loss, value_loss, ent, clipped]).detach())
    if not rows:
        return {"samples": len(samples)}

    means = torch.stack(rows).cpu().numpy().astype(np.float64).mean(axis=0)
    return dict(zip(("policy", "value", "entropy", "clipped"), map(float, means))) | {
        "samples": len(samples)}


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--class", dest="cls", required=True)
    ap.add_argument("--iters", type=int, default=200)
    ap.add_argument("--rollout", type=int, default=48, help="env turns an iteration")
    ap.add_argument("--waves", type=int, default=4)
    ap.add_argument("--matches-per-wave", type=int, default=8)
    ap.add_argument("--max-turns", type=int, default=300)
    ap.add_argument("--epochs", type=int, default=2)
    ap.add_argument("--minibatch", type=int, default=32)
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--clip", type=float, default=0.2)
    ap.add_argument("--entropy", type=float, default=0.01)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--amp", action="store_true",
                    help="fp16 autocast for the trunk's forward and backward, with a GradScaler; "
                         "softmax, ratio, losses and the optimizer stay fp32")
    ap.add_argument("--init", type=Path, default=None, help="a bc checkpoint to start from")
    ap.add_argument("--out", type=Path, default=None)
    a = ap.parse_args(argv)

    torch.manual_seed(a.seed)
    np.random.seed(a.seed)
    dev = device()

    spec = classes()[a.cls]
    trunk = nets.build(spec)
    if a.init:
        state = torch.load(a.init, map_location="cpu")
        trunk.load_state_dict(state.get("trunk", state))
    model = nets.ActorCritic(trunk).to(dev)

    b = budget(a.cls)
    method = "bc-ppo" if a.init else "ppo"
    print(f"{a.cls} {method}: {nets.policy_params(model):,} policy parameters "
          f"(budget about {b['params_at_target']:,}), {dev.type}{' fp16 autocast' if a.amp else ''}")

    out = a.out or Path("runs") / f"{a.cls}-{method}"
    out.mkdir(parents=True, exist_ok=True)
    opt = torch.optim.AdamW(model.parameters(), lr=a.lr)
    scaler = torch.amp.GradScaler(dev.type) if a.amp else None
    reward = Reward()

    history = []
    best = -1e9
    with Env(waves=a.waves, matches_per_wave=a.matches_per_wave,
             max_turns=a.max_turns, seed=1000 + a.seed) as env:
        runner = Runner(model, env, dev, reward, seed=a.seed, amp=a.amp)
        print(f"engine {env.engine_digest[:20]}")
        for it in range(1, a.iters + 1):
            t0 = time.time()
            rolled = runner.collect(a.rollout)
            batch = runner.harvest()
            t1 = time.time()
            stats = update(model, opt, batch, reward, dev, a.epochs, a.clip,
                           a.entropy, a.minibatch, amp=a.amp, scaler=scaler)
            took = time.time() - t0
            row = {"iter": it, **rolled, **stats, "seconds": round(took, 1),
                   "collect_seconds": round(t1 - t0, 1)}
            history.append(row)
            print(f"  {it:4d}  return {rolled['mean_return']:+7.2f} over {rolled['episodes']:3d} eps"
                  f"   pi {stats.get('policy', 0):+.4f}  V {stats.get('value', 0):.3f}"
                  f"  H {stats.get('entropy', 0):.3f}  {stats['samples']:5d} samples  {took:.0f}s"
                  f" (collect {t1 - t0:.0f}s)")
            if rolled["episodes"] and rolled["mean_return"] > best:
                best = rolled["mean_return"]
                torch.save({"trunk": trunk.state_dict(), "class": a.cls,
                            "engine_digest": env.engine_digest}, out / "best.pt")
            if it % 10 == 0:
                (out / "history.json").write_text(json.dumps(
                    {"class": a.cls, "method": method, "reward": vars(reward), "amp": a.amp,
                     "iters": history}, indent=2) + "\n")

    (out / "history.json").write_text(json.dumps(
        {"class": a.cls, "method": method, "reward": vars(reward), "amp": a.amp,
         "iters": history}, indent=2) + "\n")
    print(f"  -> {out / 'best.pt'}")
    print("  a rising return is not strength. Play it: `python -m tb_baselines.eval`")


if __name__ == "__main__":
    main()
