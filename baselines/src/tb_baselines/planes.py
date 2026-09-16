"""The observation encoding: one spec, rendered twice.

**This module exists to make train/serve skew impossible to have quietly.** A competitor who trains
in Python encodes each observation twice — once in `manifest.json`, which is what the ladder runs,
and once in numpy, which is what the optimiser sees. Two implementations of one encoding is the
classic way to ship a model that scores worse in the arena than it did in training, and the failure
is silent: both halves work, they just disagree.

So the planes are declared once, here, and each declaration carries **both** renderings side by
side: the JSONLogic fragment that goes into `manifest.json`, and the numpy function the trainer
calls. `tests/test_adapter_conformance.py` then runs the real evaluator — `tinybrains adapt`, which
is datalogic, the evaluator an Orion node runs the adapter on — over the cartridge's reference
observations and asserts the two agree element for element. Proximity makes them easy to keep in
step; the test is what proves it.

## The planes

Six of these are the reference adapter's, unchanged, because they are proven and cheap. The seventh
is the visibility mask — and since 14 September 2026 **the engine sends it**. It used to be derived
from `mine` with a `tb.dilate` operator that existed for exactly this plane; the expression language
a node evaluates an adapter on cannot address an enclosing iterator's element, so the per-ant disk
was a 241-fold unrolled kernel or nothing. The radius is a rule of the game, so the cartridge now
computes it once and `vis` arrives beside `water` (`ants/docs/protocol.md` §1, decision R5). This
plane is an `rle_expand` like any other.

Owners are relative to the observer — you are always 0 — which is what makes `hills` splittable at
all. That was fixed in engine `sha256:f17b51b6c92b`; under an older engine these two planes are
swapped for seat 1 of every match.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable

import numpy as np

# The observation's own vocabulary, so a rename is one edit rather than a search.
VIEW_RADIUS2 = 77

# The datavalue dtype spelling, which is what a manifest declares and what the tensor operators
# take. The old dialect spelled it `int8`.
DTYPE = "i8"
NP_DTYPE = np.int8


@dataclass(frozen=True)
class Plane:
    """One channel of the board tensor.

    `logic` is the JSONLogic that computes it inside `manifest.json`, as a function of the size
    expression (the adapter cannot hard-code a board size: three presets mean three sizes).
    `numpy` computes the same plane from the same observation, for the trainer.
    """

    name: str
    why: str
    logic: Callable[[Any], Any]
    numpy: Callable[[dict, "Board"], np.ndarray]


_DISKS: dict[int, np.ndarray] = {}


def _disk(radius2: int) -> np.ndarray:
    """The offsets within euclidean radius², computed once per radius and kept.

    Integer arithmetic throughout, and `<=` on the squared radius, because that is exactly what
    `tb.dilate` tests -- a float radius with a `<` would drop the ring of cells at exactly the
    boundary, and at radius² 77 that is a visible fraction of the mask.
    """
    if radius2 not in _DISKS:
        r = int(np.floor(np.sqrt(radius2)))
        _DISKS[radius2] = np.array(
            [(dr, dc) for dr in range(-r, r + 1) for dc in range(-r, r + 1)
             if dr * dr + dc * dc <= radius2],
            dtype=np.int64,
        )
    return _DISKS[radius2]


class Board:
    """Scratch space for the numpy renderings: the size, and a zeroed plane on demand."""

    def __init__(self, rows: int, cols: int):
        self.rows = rows
        self.cols = cols

    def zeros(self) -> np.ndarray:
        return np.zeros((self.rows, self.cols), dtype=NP_DTYPE)

    def scatter(self, points) -> np.ndarray:
        """Points onto a plane. Out of bounds is dropped rather than refused, which is what
        datalogic's `scatter` does: "the usual producer is a detector emitting boxes in source
        coordinates that may fall outside the target grid"."""
        g = self.zeros()
        if len(points) == 0:
            return g
        p = np.asarray(points, dtype=np.int64)
        r, c = p[:, 0], p[:, 1]
        keep = (r >= 0) & (r < self.rows) & (c >= 0) & (c < self.cols)
        g[r[keep], c[keep]] = 1
        return g

    def rle(self, runs) -> np.ndarray:
        """`[v0, n0, v1, n1, …]`, row-major, as `rle_expand` reads it."""
        flat = np.zeros(self.rows * self.cols, dtype=NP_DTYPE)
        at = 0
        for i in range(0, len(runs) - 1, 2):
            v, n = int(runs[i]), int(runs[i + 1])
            if v:
                flat[at : at + n] = 1
            at += n
        return flat.reshape(self.rows, self.cols)

    def dilate(self, g: np.ndarray, radius2: int) -> np.ndarray:
        """Every cell within euclidean radius² of a non-zero one, wrapping.

        **No longer part of the encoding**: `vis` arrives in the observation. It is kept because it
        is the independent second opinion — `tests/test_adapter_conformance.py` asserts the
        engine's mask equals this, which is what makes trusting the sent one safe.

        **Scattered from the non-zero cells, not rolled over the plane.** The obvious reading of the
        operator is "shift the whole board by every offset in the disk and OR them together", and
        that is what this was: 241 offsets at radius² 77, two `np.roll`s each, over 16,384 cells —
        1.86 million roll calls in a two-minute profile, 48% of the whole training loop.

        The equivalent form is to walk the disk out from each set cell instead. Same answer by
        definition, and the cost goes from the size of the board to the number of ants: 36 ants
        times 241 offsets is 8,700 writes against 3.9 million. `tests/test_adapter_conformance.py`
        is what keeps "equivalent by definition" honest.
        """
        out = self.zeros()
        points = np.argwhere(g != 0)
        if len(points) == 0:
            return out
        disk = _disk(radius2)
        r = (points[:, None, 0] + disk[None, :, 0]) % self.rows
        c = (points[:, None, 1] + disk[None, :, 1]) % self.cols
        out[r.ravel(), c.ravel()] = 1
        return out


# ---- the JSONLogic side ------------------------------------------------------------------

def var(path: str) -> dict:
    return {"var": path}


SIZE = var("size")


def _scatter(points) -> Callable[[Any], Any]:
    return lambda size: {"scatter": [points, size, DTYPE]}


def _rc_only(source) -> dict:
    """`[r, c, owner]` triples down to `[r, c]` pairs: `scatter` takes either, but a third element
    is a *value* to write, and an owner id written as a value is not a mask."""
    return {"map": [source, [var("0"), var("1")]]}


def _hills(mine: bool) -> Callable[[Any], Any]:
    test = {"==": [var("2"), 0]} if mine else {"!=": [var("2"), 0]}
    return lambda size: {
        "scatter": [_rc_only({"filter": [var("hills"), test]}), size, DTYPE]
    }


PLANES: tuple[Plane, ...] = (
    Plane(
        "mine",
        "your ants; the only positions you are told in full, fog or no fog",
        _scatter(var("mine")),
        lambda o, b: b.scatter(o["mine"]),
    ),
    Plane(
        "foes",
        "enemy ants you can see this turn -- never remembered, so this plane blinks",
        lambda size: {"scatter": [_rc_only(var("foes")), size, DTYPE]},
        lambda o, b: b.scatter([f[:2] for f in o["foes"]]),
    ),
    Plane(
        "food",
        "food you can see; the whole economy, since ants come from food",
        _scatter(var("food")),
        lambda o, b: b.scatter(o["food"]),
    ),
    Plane(
        "water",
        "known water: the one field with memory, and so the only map you accumulate",
        lambda size: {"rle_expand": [var("water.rle"), size, DTYPE]},
        lambda o, b: b.rle(o["water"]["rle"]),
    ),
    Plane(
        "hill_mine",
        "your hills, owner 0 -- what you lose 1 point each for",
        _hills(mine=True),
        lambda o, b: b.scatter([h[:2] for h in o["hills"] if h[2] == 0]),
    ),
    Plane(
        "hill_foe",
        "enemy hills -- what you gain 2 points each for razing",
        _hills(mine=False),
        lambda o, b: b.scatter([h[:2] for h in o["hills"] if h[2] != 0]),
    ),
    Plane(
        "visible",
        "what you can see RIGHT NOW, so a 0 in `water` stops meaning both known-empty and "
        "never-seen. SENT BY THE ENGINE since 14 September 2026 -- the radius is a rule of the "
        "game and an adapter cannot build the disk union (ants/docs/protocol.md §1).",
        lambda size: {"rle_expand": [var("vis.rle"), size, DTYPE]},
        lambda o, b: b.rle(o["vis"]["rle"]),
    ),
)

N_PLANES = len(PLANES)

# The five moves, in the order the policy's channels mean them. `-` is the hold, and it is last
# because `drill/models/README.md` is right that a model whose last channel wins everywhere looks
# passive by choice and is not.
MOVES = ("N", "E", "S", "W", "-")
N_MOVES = len(MOVES)


def encode(obs: dict) -> np.ndarray:
    """One observation to `[1, planes, rows, cols]`, exactly as the manifest's adapter produces it.

    The leading 1 is the batch dimension, and the graph declares it dynamic so a trainer can stack
    a batch through the same graph the ladder runs one seat at a time.
    """
    rows, cols = obs["size"]
    b = Board(rows, cols)
    planes = np.stack([p.numpy(obs, b) for p in PLANES], axis=0)
    return planes.reshape(1, N_PLANES, rows, cols)


def encode_batch(observations: list[dict]) -> np.ndarray:
    """A wave's worth. Every board in one wave is one preset and so one size — `worldgen` takes a
    single preset per call — so these always stack."""
    return np.concatenate([encode(o) for o in observations], axis=0)
