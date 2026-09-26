"""Behaviour cloning over sequences: a memory the graph learns.

    python -m tb_baselines.train.seq --class nano --data data/teacher-memory.jsonl.gz --epochs 5

`train/bc.py --memory` carries a memory that is a fixed function of the turns before a row
(`planes.MEMORY`, kept with `Max`): the trainer computes it in numpy, so the rows stay independent
and the loop is bc.py's. This trainer carries one the graph computes (`nets.LearnedMemory`): a
row's memory is the net's own output on the row before it, so a seat's rows have to be replayed
in order, which is what `k` in the dataset (`collect.py`) is for.

## Truncated backpropagation through time

A batch is a few seats of one board size, stepped together from each seat's first row. Every
`--bptt` turns the loss so far is backpropagated, the optimiser steps and the memory is detached,
so a gradient reaches back `bptt` turns while the memory itself reaches back to the seat's first
turn. Seats end at different turns; one past its last row is dropped from the batch, and a batch
is seats of similar length so little is dropped early.

The memory carried between turns is the rounded `i8` the runner will carry: in training through a
straight-through estimator (`nets.LearnedMemory`), in the held-out pass as the integer itself, so
the number reported here is the number the ladder will see.

## What the number means

The loss is bc.py's, per ant against the teacher's move, on the same dataset a fixed memory and a
board-only net train on. With the remembering teacher's labels, a memory that learns to keep what
the teacher reads back -- a hill seen once, a food cell out of sight -- raises the held-out
agreement toward the fixed memory's; one that learns nothing sits where the board-only net sits.
That comparison, three runs on one dataset, is the experiment this file exists for. Play it
afterwards: agreement is not strength.
"""

from __future__ import annotations

import argparse
import json
import random
import re
import time
from collections import defaultdict
from pathlib import Path

import torch
import torch.nn.functional as F

from .. import nets
from ..export import budget, classes
from ..planes import ANT_MEMORY, ANT_TABLE, N_MEMORY
from .bc import Rows, ant_logits, device, override, tensors

KEY_RE = re.compile(rb'"k":\[(\d+),(\d+),(\d+)\]')


def sequences(rows: Rows) -> list[list[int]]:
    """Each seat's rows in turn order, as lists of row indices; a gap in the turns starts a new
    sequence, since nothing was carried across it. A dataset without `k` cannot be ordered."""
    by_seat: dict[tuple[int, int], list[tuple[int, int]]] = defaultdict(list)
    for i, line in enumerate(rows.lines):
        m = KEY_RE.search(line)
        if not m:
            raise SystemExit("this dataset's rows carry no `k` (episode, seat, turn): collect it again")
        ep, seat, turn = (int(g) for g in m.groups())
        by_seat[(ep, seat)].append((turn, i))
    out = []
    for items in by_seat.values():
        items.sort()
        run = [items[0][1]]
        for (t0, _), (t1, j) in zip(items, items[1:]):
            if t1 == t0 + 1:
                run.append(j)
            else:
                out.append(run)
                run = [j]
        out.append(run)
    return out


def batches(rows: Rows, seqs: list[list[int]], size: int, rng: random.Random, shuffle: bool = True):
    """Batches of sequences on one board size, neighbours in length so a batch thins slowly."""
    by_size: dict[tuple[int, int], list[list[int]]] = defaultdict(list)
    for s in seqs:
        by_size[rows.sizes[s[0]]].append(s)
    out = []
    for group in by_size.values():
        group.sort(key=len, reverse=True)
        out += [group[i : i + size] for i in range(0, len(group), size)]
    if shuffle:
        rng.shuffle(out)
    return out


class AntCarry:
    """One seat's `ant_memory` between turns, joined by id in torch so a gradient reaches back
    through it: the adapter's table (`planes._ant_rows_logic`) and scatter (`ant_planes_adapter`),
    written with `index_put` and `index_select` instead, and the same answer."""

    def __init__(self, dev):
        self.dev = dev
        self.rows: torch.Tensor | None = None       # [P, K+1], as the graph wrote them

    def planes(self, obs: dict) -> torch.Tensor:
        """`[K, rows, cols]`: last turn's values at this turn's ants, zeros for a new ant or on
        turn 0, in the u8 scale the graph reads."""
        size = obs["size"]
        out = torch.zeros((ANT_MEMORY, *size), device=self.dev)
        ids = obs.get("ids") or []
        if self.rows is None or not ids:
            return out
        table = torch.zeros((ANT_TABLE, ANT_MEMORY), device=self.dev)
        keys = (self.rows[:, 0].detach().round().long() % ANT_TABLE)
        table = table.index_put((keys,), self.rows[:, 1:].float().clamp(0, 255))
        picked = table.index_select(0, torch.tensor([i % ANT_TABLE for i in ids], device=self.dev))
        mine = torch.tensor(obs["mine"], dtype=torch.long, device=self.dev)
        return out.index_put((torch.arange(ANT_MEMORY, device=self.dev).repeat_interleave(len(ids)),
                              mine[:, 0].repeat(ANT_MEMORY), mine[:, 1].repeat(ANT_MEMORY)),
                             picked.t().reshape(-1))

    @staticmethod
    def inputs(observations: list[dict], dev) -> tuple[torch.Tensor, torch.Tensor]:
        """`ids` and `cells` for a batch of seats, padded to the widest colony with a repeat of
        each seat's last ant (a repeated gather changes nothing that is read back)."""
        n = max(1, max(len(o["mine"]) for o in observations))
        ids = torch.zeros((len(observations), n), dtype=torch.long)
        cells = torch.zeros((len(observations), n), dtype=torch.long)
        for i, o in enumerate(observations):
            k = len(o["mine"])
            if k:
                ids[i, :k] = torch.tensor([x % ANT_TABLE for x in o["ids"]])
                ids[i, k:] = ids[i, k - 1]
                cells[i, :k] = torch.tensor([r * o["size"][1] + c for r, c in o["mine"]])
                cells[i, k:] = cells[i, k - 1]
        return ids.to(dev), cells.to(dev)


def run_epoch(model, rows, chunks, dev, opt=None, bptt: int = 32) -> tuple[float, float, int]:
    train = opt is not None
    model.train(train)
    board_memory = getattr(model, "memory_ports", False)
    per_ant = getattr(model, "ant_ports", False)
    total_loss = correct = seen = 0
    for seqs in chunks:
        # Longest first, so the live seats at any turn are a prefix of the batch.
        seqs = sorted(seqs, key=len, reverse=True)
        h = None
        carries = [AntCarry(dev) for _ in seqs]
        loss_sum, ants = 0.0, 0
        with torch.set_grad_enabled(train):
            for t in range(len(seqs[0])):
                live = [s[t] for s in seqs if t < len(s)]
                boards, _, b, r, c, y = tensors(rows, live, dev)
                args = [boards]
                if board_memory:
                    if h is None:
                        h = torch.zeros((len(live), N_MEMORY, *boards.shape[2:]), device=dev)
                    else:
                        h = h[: len(live)]
                    args.append(h)
                if per_ant:
                    observations = [rows.parse(i)["o"] for i in live]
                    args.append(torch.stack([carries[k].planes(o) for k, o in enumerate(observations)]))
                    args += list(AntCarry.inputs(observations, dev))
                outputs = model(*args)
                if not isinstance(outputs, tuple):
                    outputs = (outputs,)
                policy, outputs = outputs[0], list(outputs[1:])
                if board_memory:
                    h = outputs.pop(0)
                if per_ant:
                    ant_rows = outputs.pop(0)                       # [live, N, K+1]
                    for k, o in enumerate(observations):
                        carries[k].rows = ant_rows[k, : len(o["mine"])]
                if y.numel():
                    logits = ant_logits(policy, b, r, c)
                    loss_sum = loss_sum + F.cross_entropy(logits, y, reduction="sum")
                    ants += y.numel()
                    correct += int((logits.argmax(1) == y).sum())
                last = t + 1 == len(seqs[0])
                if ants and ((t + 1) % bptt == 0 or last):
                    loss = loss_sum / ants
                    if train:
                        opt.zero_grad(set_to_none=True)
                        loss.backward()
                        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                        opt.step()
                    total_loss += loss.detach().item() * ants
                    seen += ants
                    loss_sum, ants = 0.0, 0
                    if h is not None:
                        h = h.detach()
                    for carry in carries:
                        if carry.rows is not None:
                            carry.rows = carry.rows.detach()
    return total_loss / max(seen, 1), correct / max(seen, 1), seen


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--class", dest="cls", required=True)
    ap.add_argument("--arch", default=None, help="override the class's architecture")
    ap.add_argument("--channels", type=int, default=None)
    ap.add_argument("--blocks", type=int, default=None)
    ap.add_argument("--memory", default="learned", choices=["learned", "max", "none"],
                    help="the board memory the graph carries; `none` for a memory per ant alone")
    ap.add_argument("--ants", action="store_true", help="carry a memory per ant too (nets.PerAnt)")
    ap.add_argument("--data", type=Path, default=Path("data/teacher-memory.jsonl.gz"))
    ap.add_argument("--epochs", type=int, default=5)
    ap.add_argument("--batch", type=int, default=16, help="seats stepped together")
    ap.add_argument("--bptt", type=int, default=32, help="turns a gradient reaches back")
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--holdout", type=float, default=0.05)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", type=Path, default=None)
    a = ap.parse_args(argv)

    torch.manual_seed(a.seed)
    rng = random.Random(a.seed)
    dev = device()

    rows = Rows(a.data)
    header = rows.header
    seqs = sequences(rows)
    rng.shuffle(seqs)
    cut = int(len(seqs) * (1 - a.holdout))
    train_seqs, val_seqs = seqs[:cut], seqs[cut:]

    memory = False if a.memory == "none" else a.memory
    if not memory and not a.ants:
        raise SystemExit("nothing to carry: give a board memory (`--memory learned|max`) or `--ants`")
    spec = override(classes()[a.cls], a)
    model = nets.build(spec, memory=memory, ants=a.ants).to(dev)
    b = budget(a.cls)
    what = " + ".join(([f"{memory} memory"] if memory else []) + (["a memory per ant"] if a.ants else []))
    print(f"{a.cls} + {what}: {nets.policy_params(model):,} parameters "
          f"(budget about {b['params_at_target']:,}), {dev.type}")
    print(f"data: {len(train_seqs):,} sequences train / {len(val_seqs):,} held out over "
          f"{len(rows):,} rows, teacher {header.get('teacher')}, engine {header['engine_digest'][:20]}")

    opt = torch.optim.AdamW(model.parameters(), lr=a.lr, weight_decay=1e-4)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=max(a.epochs, 1))
    val_chunks = batches(rows, val_seqs, a.batch, rng, shuffle=False)

    out = a.out or Path("runs") / f"{a.cls}-bc-{a.memory if memory else 'no'}-memory{'-ants' if a.ants else ''}"
    out.mkdir(parents=True, exist_ok=True)
    best = float("inf")
    history = []
    for epoch in range(1, a.epochs + 1):
        t0 = time.time()
        chunks = batches(rows, train_seqs, a.batch, rng)
        tl, ta, n = run_epoch(model, rows, chunks, dev, opt, a.bptt)
        vl, va, _ = run_epoch(model, rows, val_chunks, dev, bptt=a.bptt)
        sched.step()
        took = time.time() - t0
        print(f"  epoch {epoch}/{a.epochs}  train {tl:.4f} / {ta:.1%}   "
              f"held out {vl:.4f} / {va:.1%}   {n:,} ants  {took:.0f}s")
        history.append({"epoch": epoch, "train_loss": tl, "train_acc": ta,
                        "val_loss": vl, "val_acc": va, "seconds": round(took, 1)})
        if vl < best:
            best = vl
            torch.save({"trunk": model.state_dict(), "class": a.cls, "memory": memory,
                        "ants": a.ants, "engine_digest": header["engine_digest"]}, out / "best.pt")

    (out / "history.json").write_text(json.dumps(
        {"class": a.cls, "method": "bc", "arch": spec["arch"], "spec": spec, "memory": memory,
         "ants": a.ants, "bptt": a.bptt, "batch": a.batch, "data": str(a.data), "teacher": header.get("teacher"),
         "epochs": history, "engine_digest": header["engine_digest"], "device": dev.type},
        indent=2) + "\n")
    print(f"  -> {out / 'best.pt'}  (best held-out loss {best:.4f})")
    print("  agreement with the teacher is not strength. Play it: `python -m tb_baselines.eval`")


if __name__ == "__main__":
    main()
