"""A client for `tinybrains env`: the cartridge as a batched training environment.

One subprocess, JSON Lines both ways, and the rule that makes it safe: **actions are positionally
aligned with the seats the last call returned**. That is the game protocol's own rule, and it holds
here because a training env never forfeits a seat, so every live seat is played every turn.

    env = Env(waves=4, matches_per_wave=16, max_turns=300, seed=1)
    step = env.reset()
    while True:
        orders = my_policy(step.boards)          # [live, planes, rows, cols] -> [live] strings
        step = env.step(orders)

`Step.boards` is already encoded by `planes.encode` — the same function
`tests/test_adapter_conformance.py` proves equal to `manifest.json`. `Step.observations` is the raw
JSON beside it, because reward is computed from the game and not from the tensor.

## What this is not

It is not the referee. There is no turn deadline, no strike ceiling, no forfeit and no adapter
evaluation here, so a policy that trains happily can still fail at play. `tinybrains check` and a
real `tinybrains <match>` are the gates; see `eval.py`.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from .planes import MOVES, encode_batch

ROOT = Path(__file__).resolve().parents[2]


@dataclass
class Seat:
    """One live seat this turn. `ep` is the only stable key: `(w, m)` is reused after a refill."""

    ep: int
    seat: int
    wave: int
    match: int
    turn: int
    obs: dict


@dataclass
class Ended:
    ep: int
    seat_count: int
    ranks: list[int]
    scores: list[int]
    reason: str
    turns: int
    map: str                                # the board the match was played on
    map_id: str
    seed: int


@dataclass
class Group:
    """Every live seat on one board size, stacked.

    **A batch is per board size, and that is not an implementation detail.** The pool holds several
    waves, each wave is played on one board, and the boards come in many sizes — the five basic
    ones run from 24x24 to 120x124, and a season's anywhere inside that. The policy is fully
    convolutional so it runs on any of them, but a single tensor cannot hold two, so a trainer loops
    the groups. The ladder never has this problem: every seat of a match is its own `model_infer`
    call.
    """

    size: tuple[int, int]
    indices: list[int]                      # into `Step.seats`
    boards: np.ndarray                      # [n, planes, rows, cols], int8


@dataclass
class Step:
    seats: list[Seat]
    groups: list[Group]
    scores: dict[int, list[int]]            # ep -> current score per seat
    ended: list[Ended]
    turn: int
    observations: list[dict] = field(default_factory=list)

    @property
    def boards(self) -> np.ndarray:
        """The one group, for a run pinned to boards of one size. Raises when the pool is mixed,
        rather than silently handing back a fraction of the batch."""
        if len(self.groups) != 1:
            raise EnvError(
                f"this step spans {len(self.groups)} board sizes "
                f"({', '.join(f'{r}x{c}' for r, c in (g.size for g in self.groups))}). "
                "Loop `step.groups`, or pass `maps=` boards of one size to pin the Env to it."
            )
        return self.groups[0].boards


class EnvError(RuntimeError):
    pass


class Env:
    """The subprocess, its protocol, and nothing else.

    Deterministic: given `seed` and the action stream, every board, every food respawn and every
    match is reproducible. That is what makes a training run something you can repeat.

    `maps` is the board pool: ids the release ships, paths to board files, or a directory of them
    (a season's, say) -- a list or one comma-separated string. None is the release's basic boards.
    There are no presets: the engine carries no boards, so the pool is boards (ants N28).
    """

    def __init__(
        self,
        waves: int = 4,
        matches_per_wave: int = 16,
        max_turns: int = 300,
        seed: int = 1,
        maps: str | list[str] | None = None,
        game: str | None = None,
        scores: str = "every",
        cwd: Path | None = None,
    ):
        binary = os.environ.get("TINYBRAINS") or shutil.which("tinybrains")
        if not binary:
            raise EnvError(
                "no `tinybrains` on PATH.\n"
                "  brew tap tiny-brains/cli https://github.com/Tiny-Brains/cli\n"
                "  brew install tiny-brains/cli/tinybrains   (or set TINYBRAINS to the binary)"
            )
        argv = [
            binary, "env",
            "--waves", str(waves),
            "--matches-per-wave", str(matches_per_wave),
            "--max-turns", str(max_turns),
            "--seed", str(seed),
            "--scores", scores,
        ]
        if maps:
            argv += ["--maps", maps if isinstance(maps, str) else ",".join(str(m) for m in maps)]
        if game:
            argv += ["--game", game]

        self.proc = subprocess.Popen(
            argv, cwd=str(cwd or ROOT),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            text=True, bufsize=1,
        )
        self.hello = self._read()["hello"]
        self.closed = False
        # Every model card names these. A run that cannot say which engine produced its data is a
        # run nobody can reproduce.
        self.engine_digest = self.hello["engine_digest"]
        # The evaluator is datalogic now and is named by version rather than digest: an adapter is
        # priced and run by the same library on a node, so a version skew is what would make a
        # local pass and a remote refusal disagree.
        self.evaluator = self.hello["evaluator"]

    # ---- the wire ------------------------------------------------------------------

    def _read(self) -> dict:
        line = self.proc.stdout.readline()
        if not line:
            raise EnvError("the environment exited")
        reply = json.loads(line)
        if not reply.get("ok"):
            raise EnvError(reply.get("error", "the environment refused a request"))
        return reply

    def _write(self, request: dict) -> None:
        self.proc.stdin.write(json.dumps(request) + "\n")
        self.proc.stdin.flush()

    def _pack(self, reply: dict) -> Step:
        seats = [
            Seat(ep=s["ep"], seat=s["seat"], wave=s["w"], match=s["m"], turn=s["turn"], obs=s["obs"])
            for s in reply["seats"]
        ]
        obs = [s.obs for s in seats]
        by_size: dict[tuple[int, int], list[int]] = {}
        for i, o in enumerate(obs):
            by_size.setdefault(tuple(o["size"]), []).append(i)
        groups = [
            Group(size=size, indices=idx, boards=encode_batch([obs[i] for i in idx]))
            for size, idx in sorted(by_size.items())
        ]
        return Step(
            seats=seats,
            groups=groups,
            scores={r["ep"]: r["scores"] for r in reply["scores"]},
            ended=[
                Ended(
                    ep=e["ep"], seat_count=len(e["scores"]), ranks=e["ranks"], scores=e["scores"],
                    reason=e["reason"], turns=e["turns"], map=e["map"],
                    map_id=e["map_id"], seed=e["seed"],
                )
                for e in reply["ended"]
            ],
            turn=reply["turn"],
            observations=obs,
        )

    # ---- the loop ------------------------------------------------------------------

    def reset(self) -> Step:
        self._write({"op": "observe"})
        return self._pack(self._read())

    def step(self, actions: list[str]) -> Step:
        """`actions[i]` is for `seats[i]`: one character an ant, in `mine`'s order.

        The compact form is the replay's own (`ants/engine/src/replay.rs`), not an invention, and it
        is four times smaller on the wire than an array of strings.
        """
        self._write({"op": "step", "actions": actions})
        return self._pack(self._read())

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        try:
            self._write({"op": "close"})
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()

    def __enter__(self) -> "Env":
        return self

    def __exit__(self, *_) -> None:
        self.close()


def orders_from_indices(indices: np.ndarray, counts: list[int]) -> list[str]:
    """A flat array of move indices, cut per seat, as the compact strings the env wants.

    `counts[i]` is how many ants seat `i` has, which is `len(obs["mine"])` — the action array is
    positionally aligned with `mine` and nothing else addresses an ant.
    """
    table = np.array([ord(m) for m in MOVES], dtype=np.uint8)
    chars = table[np.clip(indices, 0, len(MOVES) - 1)]
    out, at = [], 0
    for n in counts:
        out.append(chars[at : at + n].tobytes().decode("ascii"))
        at += n
    return out
