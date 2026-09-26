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
is the visibility mask, and **the engine sends it**: the expression language a node evaluates an
adapter on cannot address an enclosing iterator's element, so the per-ant disk would be a 241-fold
unrolled kernel or nothing. The radius is a rule of the game, so the cartridge computes it once and
`vis` arrives beside `water` (*What your model sees* in the competitor guide). This plane is an
`rle_expand` like any other.

Owners are relative to the observer — you are always 0 — which is what makes `hills` splittable at
all. That was fixed in engine `sha256:f17b51b6c92b`; under an older engine these two planes are
swapped for seat 1 of every match.

## The memory

A seat may carry a memory from one turn to the next (the book's *Memory* page): an output the graph
writes, which the runner hands back on the seat's next view under the same key. The one here is
basic: two planes the view forgets between turns, kept with `Max`, so a cell reads 1 once food, or
an enemy hill, has been seen on it. It is a fixed function of what the seat has seen, so nothing in
it is learned; the policy that reads it is. `MEMORY` declares it, rendered three ways, and
`tests/test_adapter_conformance.py` holds them together:

- the adapter (`memory_adapter`): last turn's memory passes through, zeros on turn 0
- the graph (`nets.WithMemory`): `memory = Max(memory_in, board[sources])`
- numpy (`remember`, `encode_memory`): the union of the cells seen so far, as planes
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
    expression (the adapter cannot hard-code a board size: a season's boards come in many).
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
        "never-seen. SENT BY THE ENGINE -- the radius is a rule of the "
        "game and an adapter cannot build the disk union.",
        lambda size: {"rle_expand": [var("vis.rle"), size, DTYPE]},
        lambda o, b: b.rle(o["vis"]["rle"]),
    ),
)

N_PLANES = len(PLANES)

# The five moves, in the order the policy's channels mean them. `-` is the hold, and it is last
# because a model whose last channel wins everywhere looks passive by choice and is not -- the
# book's *Testing* chapter shows that colony standing still.
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
    """A wave's worth. Every match of a wave is played on one board and so one size — the env
    hands `worldgen` one board a wave — so these always stack."""
    return np.concatenate([encode(o) for o in observations], axis=0)


# ---- the memory -------------------------------------------------------------------------

@dataclass(frozen=True)
class MemoryPlane:
    """One plane of the memory tensor: the 1s of a board plane, kept for the rest of the match."""

    name: str
    why: str
    source: str


MEMORY: tuple[MemoryPlane, ...] = (
    MemoryPlane(
        "food_seen",
        "every cell food has been seen on: the map feeds a colony from fixed places",
        "food",
    ),
    MemoryPlane(
        "hill_foe_seen",
        "every enemy hill ever seen, which the view forgets the turn no ant is near it",
        "hill_foe",
    ),
)
N_MEMORY = len(MEMORY)
# Channel indices into `PLANES`, which is what the graph gathers to update the memory.
MEMORY_SOURCES: tuple[int, ...] = tuple(
    next(i for i, p in enumerate(PLANES) if p.name == m.source) for m in MEMORY
)


def memory_adapter(size) -> dict:
    """The `memory_in` adapter: the memory the graph wrote last turn, or zeros on turn 0.

    `{"tensor": [{"var": "memory"}]}` passes the carried tensor through with the dtype and shape the
    output declared, on a node and in `tinybrains` alike; the adapter never looks inside it.
    """
    return {
        "if": [
            var("memory"),
            {"tensor": [var("memory")]},
            {"zeros": [{"merge": [[1, N_MEMORY], size]}, DTYPE]},
        ]
    }


def remember(state: dict | None, obs: dict) -> dict:
    """What a seat has seen once this observation is counted: `state`, plus this turn's cells.

    `state` is `{name: [[r, c], ...]}`, sorted and without repeats -- the shape `collect.py` writes
    into a dataset row -- and None on turn 0. A union of 0/1 planes is their `Max`, which is the
    update the graph makes; the conformance test asserts the two agree.
    """
    rows, cols = obs["size"]
    b = Board(rows, cols)
    out = {}
    for m, k in zip(MEMORY, MEMORY_SOURCES):
        seen = {tuple(p) for p in (state or {}).get(m.name, [])}
        seen |= {(int(r), int(c)) for r, c in np.argwhere(PLANES[k].numpy(obs, b) != 0)}
        out[m.name] = [list(p) for p in sorted(seen)]
    return out


def encode_memory(state: dict | None, size) -> np.ndarray:
    """A remembered state to `[1, N_MEMORY, rows, cols]`: the graph's `memory_in`."""
    rows, cols = size
    b = Board(rows, cols)
    planes = np.stack([b.scatter((state or {}).get(m.name, [])) for m in MEMORY], axis=0)
    return planes.reshape(1, N_MEMORY, rows, cols)


def memory_in_view(obs: dict) -> np.ndarray:
    """The numpy rendering of `memory_adapter`: the memory a view carries, or zeros without one."""
    if obs.get("memory") is None:
        return encode_memory(None, obs["size"])
    return np.asarray(obs["memory"], dtype=NP_DTYPE).reshape(1, N_MEMORY, *obs["size"])


# ---- a memory per ant -------------------------------------------------------------------
#
# `ant_memory` is one row per ant, `[id, s_1 .. s_K]` as `u8`, that the graph writes each turn and
# the runner hands back on the next view (the book's *Memory* page, *A memory per ant*). The rows
# follow the ants by id, never by position: `mine` is re-sorted every turn, so the adapter turns
# last turn's rows into a table indexed by id and reads it back in this turn's `ids` order, with
# zeros for an ant born since. The state values `s_k` are the graph's own to learn
# (`nets.PerAnt`); the ids are the view's, kept modulo `ANT_TABLE` so a row fits a byte.
#
# Rendered twice, as the board is: the adapter below builds the graph's three inputs from the view,
# and the numpy functions build the same tensors for the trainer; `tests/test_adapter_conformance.py`
# runs the adapter through `tinybrains adapt --obs` over views carrying `ant_memory` and holds them
# equal. The graph's inputs are
#
#   ant_planes  u8 [1, K, H, W]   each ant's remembered values written at its own cell
#   ids         i64 [1, N]        this turn's ids modulo ANT_TABLE, which the graph writes back
#   cells       i64 [1, N]        each ant's cell as row × cols + col, which the graph gathers at

ANT_MEMORY = 2
ANT_TABLE = 256
ANT_DTYPE = "u8"
ANT_NP_DTYPE = np.uint8


def _ant_rows_logic() -> dict:
    """Last turn's rows re-keyed to this turn's ants: `[N, ANT_MEMORY]` u8, the book's join."""
    # `to_list` of `[1, P, K+1]` is one list holding the rows; the reduce projects it out, which is
    # the language's way to index a value the program computed.
    rows = {"reduce": [[{"to_list": [var("ant_memory")]}], var("current.0"), None]}
    cols = [
        {"scatter": [{"map": [rows, [var("0"), 0, var(str(k + 1))]]}, [ANT_TABLE, 1], ANT_DTYPE]}
        for k in range(ANT_MEMORY)
    ]
    keys = {"map": [var("ids"), {"%": [var(""), ANT_TABLE]}]}
    return {"gather": [{"concat": [cols, 1]}, keys, 0]}


def ant_planes_adapter(size) -> dict:
    """The `ant_planes` input: each ant's re-keyed row scattered at its cell, zeros on turn 0 or
    with no ants."""
    shape = {"merge": [[1, ANT_MEMORY], size]}
    joined = {"to_list": [{"concat": [[{"tensor": [var("mine"), ANT_DTYPE]}, _ant_rows_logic()], 1]}]}
    planes = [
        {"scatter": [{"map": [joined, [var("0"), var("1"), var(str(2 + k))]]}, size, ANT_DTYPE]}
        for k in range(ANT_MEMORY)
    ]
    return {
        "if": [
            {"and": [var("ant_memory"), var("mine")]},
            {"reshape": [{"stack": [planes, 0]}, shape]},
            {"zeros": [shape, ANT_DTYPE]},
        ]
    }


def ant_ids_adapter() -> dict:
    """The `ids` input, `[1, N]` i64: this turn's ids modulo the table, as the graph writes them."""
    keys = {"map": [var("ids"), {"%": [var(""), ANT_TABLE]}]}
    return {"reshape": [{"tensor": [keys, "i64"]}, {"merge": [[1], [{"length": [var("ids")]}]]}]}


def ant_cells_adapter() -> dict:
    """The `cells` input, `[1, N]` i64: each ant's cell in row-major order, for the graph's gather.
    Inside the map the document is one ant, so the width is read from the root with `val`."""
    cell = {"+": [{"*": [var("0"), {"val": [[1], "size", 1]}]}, var("1")]}
    flat = {"map": [var("mine"), cell]}
    return {"reshape": [{"tensor": [flat, "i64"]}, {"merge": [[1], [{"length": [var("mine")]}]]}]}


def ant_rows_in_view(obs: dict) -> np.ndarray:
    """numpy's `_ant_rows_logic`: `[N, ANT_MEMORY]` u8, last turn's rows by this turn's ids."""
    ids = obs.get("ids") or []
    out = np.zeros((len(ids), ANT_MEMORY), dtype=ANT_NP_DTYPE)
    prev = obs.get("ant_memory")
    if prev is None or not ids:
        return out
    table = np.zeros((ANT_TABLE, ANT_MEMORY), dtype=ANT_NP_DTYPE)
    for row in np.asarray(prev, dtype=np.int64).reshape(-1, ANT_MEMORY + 1):
        # A later row on the same id overwrites an earlier one, as `scatter` writes.
        table[int(row[0]) % ANT_TABLE] = np.clip(row[1:], 0, 255)
    return table[[int(i) % ANT_TABLE for i in ids]]


def ant_planes_in_view(obs: dict) -> np.ndarray:
    """numpy's `ant_planes_adapter`: `[1, ANT_MEMORY, rows, cols]` u8."""
    rows, cols = obs["size"]
    out = np.zeros((1, ANT_MEMORY, rows, cols), dtype=ANT_NP_DTYPE)
    if obs.get("ant_memory") is None or not obs["mine"]:
        return out
    values = ant_rows_in_view(obs)
    for (r, c), v in zip(obs["mine"], values):
        out[0, :, r, c] = v
    return out


def ant_ids_in_view(obs: dict) -> np.ndarray:
    return np.asarray([int(i) % ANT_TABLE for i in obs.get("ids") or []], dtype=np.int64).reshape(1, -1)


def ant_cells_in_view(obs: dict) -> np.ndarray:
    cols = obs["size"][1]
    return np.asarray([r * cols + c for r, c in obs["mine"]], dtype=np.int64).reshape(1, -1)


# ---- the 2011 winner's memory -------------------------------------------------------------
#
# `xathis.py` keeps three things between turns, and a model built on it carries the same three
# (the book's *Memory* page): two board planes under `memory`, and a mission per ant under
# `ant_memory`. The two planes are a fixed function of the turns before, so the graph computes the
# update (`nets.XathisMemory`), numpy computes the same for the conformance test
# (`xathis_remember`), and the adapter passes last turn's memory through, building the turn-0
# values itself (`xathis_memory_adapter`):
#
#   explore  u8   the bot's `exploreValue`: 100 at the start, +1 a turn, 0 on every tile within
#                 ten steps' walk of an own ant or an own hill, capped at 255. The bot also zeroes
#                 tiles an ant claims when it picks a direction to explore, which is a decision,
#                 not a function of the view: the plane carries the function and not the claims.
#   stay     u8   the bot's stillness detector, packed 128 + mask + 16 x count on a tile with an
#                 enemy on it and 0 elsewhere: `mask` is which of its N, E, S, W neighbours hold an
#                 enemy, `count` how many turns running that mask has not changed (capped at 7),
#                 and an enemy `willStay` once count is 5 -- the bot's `stayValue`/`stayTurnCount`.
#
# A mission is `[has, target_row, target_col]`; the runner carries `[id, has, tr, tc]` an ant as
# `u8`, and the adapter hands the graph each ant's `[has, dr + 128, dc + 128]` at its cell, the
# wrapped offset to its target, which is what deciding a step needs (`mission_planes_adapter`).

XATHIS_MEMORY = 2
XATHIS_START = 100
XATHIS_REACH = 10
XATHIS_DTYPE = "u8"
XATHIS_STAY_CAP = 7
MISSION = 3          # has, target_row, target_col


def xathis_flood(sources: np.ndarray, water: np.ndarray, steps: int = XATHIS_REACH) -> np.ndarray:
    """Every cell within `steps` of a source walking N, E, S, W through land, wrapping: the bot's
    breadth-first `initExplore`, as a plane."""
    reach = (sources != 0) & ~water
    for _ in range(steps):
        grown = reach | np.roll(reach, 1, 0) | np.roll(reach, -1, 0) | np.roll(reach, 1, 1) | np.roll(reach, -1, 1)
        reach = grown & ~water
    return reach


def xathis_remember(state: np.ndarray | None, obs: dict) -> np.ndarray:
    """The two planes after this observation, `[1, 2, rows, cols]` u8, from the two before it
    (`None` on turn 0: explore 100, stay 0). numpy's `nets.XathisMemory.write`."""
    rows, cols = obs["size"]
    b = Board(rows, cols)
    before = xathis_memory_in_view({"size": obs["size"]}) if state is None else np.asarray(state, dtype=np.int64)
    explore, stay = before[0, 0].astype(np.int64), before[0, 1].astype(np.int64)
    water = b.rle(obs["water"]["rle"]).astype(bool)
    mine = b.scatter(obs["mine"])
    hill_mine = b.scatter([h[:2] for h in obs["hills"] if h[2] == 0])
    reach = xathis_flood(mine | hill_mine, water)
    explore = np.where(reach, 0, np.minimum(explore + 1, 255))

    foes = b.scatter([f[:2] for f in obs["foes"]]).astype(np.int64)
    mask = (np.roll(foes, 1, 0) + 2 * np.roll(foes, -1, 1) + 4 * np.roll(foes, -1, 0) + 8 * np.roll(foes, 1, 1))
    has_prev = stay >= 128
    count_prev = np.where(has_prev, (stay - 128) // 16, 0)
    mask_prev = np.where(has_prev, stay - 128 - 16 * count_prev, 0)
    same = has_prev & (mask == mask_prev)
    count = np.where((foes != 0) & same, np.minimum(count_prev + 1, XATHIS_STAY_CAP), 0)
    stay = np.where(foes != 0, 128 + mask + 16 * count, 0)
    return np.stack([explore, stay], axis=0).reshape(1, XATHIS_MEMORY, rows, cols).astype(np.uint8)


def xathis_memory_in_view(obs: dict) -> np.ndarray:
    """numpy's `xathis_memory_adapter`: the memory a view carries, or turn 0's start values."""
    rows, cols = obs["size"]
    if obs.get("memory") is None:
        out = np.zeros((1, XATHIS_MEMORY, rows, cols), dtype=np.uint8)
        out[0, 0] = XATHIS_START
        return out
    return np.asarray(obs["memory"], dtype=np.uint8).reshape(1, XATHIS_MEMORY, rows, cols)


def xathis_memory_adapter(size) -> dict:
    """The `memory_in` adapter: last turn's planes through, or on turn 0 the bot's start values,
    100 on every tile of `explore` and nothing on `stay`."""
    plane = {"merge": [[1, 1], size]}
    return {
        "if": [
            var("memory"),
            {"tensor": [var("memory")]},
            {"concat": [[{"full": [plane, XATHIS_DTYPE, XATHIS_START]}, {"zeros": [plane, XATHIS_DTYPE]}], 1]},
        ]
    }


def _mission_rows_logic() -> dict:
    """Last turn's mission rows re-keyed to this turn's ants, `[N, MISSION]` u8: the per-ant join
    with three columns."""
    rows = {"reduce": [[{"to_list": [var("ant_memory")]}], var("current.0"), None]}
    cols = [
        {"scatter": [{"map": [rows, [var("0"), 0, var(str(k + 1))]]}, [ANT_TABLE, 1], ANT_DTYPE]}
        for k in range(MISSION)
    ]
    keys = {"map": [var("ids"), {"%": [var(""), ANT_TABLE]}]}
    return {"gather": [{"concat": [cols, 1]}, keys, 0]}


def _wrapped(delta: dict, extent: dict) -> dict:
    """`delta` brought into (-extent/2, extent/2], as the board wraps."""
    half = {"/": [extent, 2]}
    return {"if": [{">": [delta, half]}, {"-": [delta, extent]},
                   {"<": [delta, {"-": [0, half]}]}, {"+": [delta, extent]}, delta]}


def mission_planes_adapter(size) -> dict:
    """The `ant_planes` input for a memory of missions: at each ant's cell, whether it has one and
    the wrapped offset to its target, `[has, dr + 128, dc + 128]`; zeros on turn 0 or with no ants.
    Inside the map the document is one joined row `[r, c, has, tr, tc]`; the board's size is read
    from the root."""
    shape = {"merge": [[1, MISSION], size]}
    joined = {"to_list": [{"concat": [[{"tensor": [var("mine"), ANT_DTYPE]}, _mission_rows_logic()], 1]}]}
    rows_, cols_ = {"val": [[1], "size", 0]}, {"val": [[1], "size", 1]}
    columns = [
        var("2"),
        {"+": [128, _wrapped({"-": [var("3"), var("0")]}, rows_)]},
        {"+": [128, _wrapped({"-": [var("4"), var("1")]}, cols_)]},
    ]
    planes = [
        {"scatter": [{"map": [joined, [var("0"), var("1"), value]]}, size, ANT_DTYPE]}
        for value in columns
    ]
    return {
        "if": [
            {"and": [var("ant_memory"), var("mine")]},
            {"reshape": [{"stack": [planes, 0]}, shape]},
            {"zeros": [shape, ANT_DTYPE]},
        ]
    }


def mission_rows_in_view(obs: dict) -> np.ndarray:
    """numpy's `_mission_rows_logic`: `[N, MISSION]` u8."""
    ids = obs.get("ids") or []
    out = np.zeros((len(ids), MISSION), dtype=ANT_NP_DTYPE)
    prev = obs.get("ant_memory")
    if prev is None or not ids:
        return out
    table = np.zeros((ANT_TABLE, MISSION), dtype=ANT_NP_DTYPE)
    for row in np.asarray(prev, dtype=np.int64).reshape(-1, MISSION + 1):
        table[int(row[0]) % ANT_TABLE] = np.clip(row[1:], 0, 255)
    return table[[int(i) % ANT_TABLE for i in ids]]


def wrapped_delta(target: int, at: int, extent: int) -> int:
    d = target - at
    if d > extent / 2:
        return d - extent
    if d < -extent / 2:
        return d + extent
    return d


def mission_planes_in_view(obs: dict) -> np.ndarray:
    """numpy's `mission_planes_adapter`: `[1, MISSION, rows, cols]` u8."""
    rows, cols = obs["size"]
    out = np.zeros((1, MISSION, rows, cols), dtype=ANT_NP_DTYPE)
    if obs.get("ant_memory") is None or not obs["mine"]:
        return out
    values = mission_rows_in_view(obs)
    for (r, c), (has, tr, tc) in zip(obs["mine"], values):
        out[0, 0, r, c] = has
        out[0, 1, r, c] = np.clip(128 + wrapped_delta(int(tr), r, rows), 0, 255)
        out[0, 2, r, c] = np.clip(128 + wrapped_delta(int(tc), c, cols), 0, 255)
    return out
