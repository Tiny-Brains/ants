"""The scripted teacher: a potential field over the board, one BFS a turn.

**It is not a baseline and must never be shipped as one.** It is a *label source*. The whole point
of the class ladder is to take one fixed teacher and ask how much of it fits in 8 KiB, in 64 KiB, in
512 KiB — and that question is only meaningful if the teacher is the same for every class and if it
is a player rather than a network. Distilling a heuristic is also the fastest honest way to get a
ladder something to play against; an untrained fixture holds still and teaches nobody anything.

It is a *player*, not a second engine. It reads the observation the protocol defines and answers a
move — the same thing a competitor's model does. It implements no rule: it never decides who dies,
what a hill is worth, or when a match ends. `ants/CLAUDE.md`'s prohibition is on a second
implementation of the *rules*, and this has none.

## How it plays

One multi-source BFS a turn, over the cells this seat knows are not water, from every target at
once: food, enemy hills, and the frontier of what has never been seen. Each ant then steps down the
resulting distance field. That is the classic Ants opening bot and it is deliberately ordinary —
the interesting question here is what a network retains of it, not how strong it is.

Three refinements, each of which changes the games it produces rather than its style:

* **Targets are weighted by a head start**, not by a multiplier. Food seeds the queue at distance 0,
  an enemy hill at `-hill_lead`, so hills win ties out to that many steps. A multiplier on a
  distance field would break the BFS's monotonicity and is the usual bug here.
* **No two ants take one square.** They would both die (`ants/src/turn.rs`: every ant finishing on a
  shared square dies, your own included), which is the single cheapest way to lose a colony.
* **Nothing is random.** Given a seed and an engine the teacher is a pure function of the
  observation, and that is not tidiness — it is what makes it imitable at all. See below.

## Determinism is a requirement, not a nicety

The first version shuffled the four directions before choosing, to break ties without a directional
bias, and sent a quarter of its ants scouting at random. Both looked harmless and neither was:
measured over 1.16 million decisions the labels came out **W 25.6% / E 25.3% / S 24.3% / N 23.8% /
hold 1.0%** — almost exactly uniform, because in open ground two of the four neighbours are usually
equidistant from a far target and the choice between them was a coin flip.

A label that is not a function of the state cannot be learned from. Imitation accuracy has a ceiling
somewhere near the fraction of decisions that were not coin flips, and no amount of capacity gets
past it — so a class ladder built on that dataset would be measuring the teacher's noise floor
rather than the class's capacity.

So ties break in a fixed direction order, and the random scouting is gone: the frontier is already
seeded into the flood, so an ant with nowhere better to go walks toward what it has not seen
*because the field says so*. The directional bias in a genuine tie is real and is the right trade —
it is deterministic, so a network can learn it exactly.
"""

from __future__ import annotations

from collections import deque

import numpy as np

from .planes import MOVES

# Row-major deltas in the order `MOVES` names them, so an index into one indexes the other.
DELTAS = ((-1, 0), (0, 1), (1, 0), (0, -1))
HOLD = MOVES.index("-")

FAR = np.int32(1 << 29)


class Teacher:
    """One instance per process; it holds no state between turns and is safe to reuse."""

    def __init__(self, hill_lead: int = 6, max_depth: int = 32):
        self.hill_lead = hill_lead
        # An ant decides one step, so a target sixty cells away and a target thirty cells away lead
        # to the same move. Capping the flood is therefore free in play quality and is most of the
        # run time: uncapped, the frontier seeding makes almost every cell of the board get visited
        # every seat-turn, in pure Python.
        self.max_depth = max_depth

    # ---- the field ------------------------------------------------------------------

    def field(self, obs: dict, water: np.ndarray, seen: np.ndarray) -> np.ndarray:
        """Distance from the nearest target, over known-passable cells, wrapping.

        Unseen cells are passable *and* are targets: that is what makes one field drive foraging and
        exploration together, and why a colony with nothing in sight still spreads out rather than
        stalling into `idle_food`.

        **A numpy wavefront, not a queue.** The Python BFS this replaces was 93% of the training
        loop after `dilate` was fixed -- 11.6 million deque operations in a two-minute profile,
        because seeding the frontier makes almost every cell of the board a target. Expanding the
        whole ring at once is the same flood in `max_depth` array operations instead of one Python
        iteration per cell visited.

        Levels are offset so the earliest seed is 0: an enemy hill starts at 0, food at `hill_lead`,
        the frontier two later. Only differences are ever compared, so the offset is free -- and a
        head start is the right way to prefer a target, where a multiplier on a distance field
        breaks the flood's monotonicity and is the usual bug here.
        """
        rows, cols = obs["size"]
        dist = np.full((rows, cols), FAR, dtype=np.int32)
        passable = ~water

        at_level: dict[int, np.ndarray] = {}

        def seed(mask: np.ndarray, level: int) -> None:
            if level in at_level:
                at_level[level] |= mask
            else:
                at_level[level] = mask.copy()

        hills = np.zeros((rows, cols), dtype=bool)
        for r, c, owner in obs["hills"]:
            if owner != 0:
                hills[r, c] = True
        if hills.any():
            seed(hills, 0)

        if obs["food"]:
            food = np.zeros((rows, cols), dtype=bool)
            idx = np.asarray(obs["food"], dtype=np.int64)
            food[idx[:, 0], idx[:, 1]] = True
            seed(food, self.hill_lead)

        unseen = ~seen
        if unseen.any():
            # Every unseen cell, not a sample of them: with the flood vectorised there is no reason
            # to approximate the frontier, and a sample gave the colony a lumpy idea of where the
            # map ran out.
            seed(unseen & passable, self.hill_lead + 2)

        if not at_level:
            return dist

        wave = np.zeros((rows, cols), dtype=bool)
        for depth in range(0, self.max_depth + 1):
            step = at_level.pop(depth, None)
            if wave.any():
                spread = (
                    np.roll(wave, 1, 0) | np.roll(wave, -1, 0)
                    | np.roll(wave, 1, 1) | np.roll(wave, -1, 1)
                )
                step = spread if step is None else (spread | step)
            if step is None:
                continue
            step &= passable & (dist == FAR)
            if not step.any() and not at_level:
                break
            dist[step] = depth
            wave = step
        return dist

    # ---- the move -------------------------------------------------------------------

    def orders(self, obs: dict, water: np.ndarray, seen: np.ndarray) -> np.ndarray:
        """One move index an ant, positionally aligned with `mine`."""
        mine = obs["mine"]
        if not mine:
            return np.zeros(0, dtype=np.int64)

        rows, cols = obs["size"]
        dist = self.field(obs, water, seen)
        # Food blocks movement as surely as water does (`ants/src/turn.rs`), so an ant ordered onto
        # food simply stays -- which wastes the turn and, worse, teaches the network that it does not.
        blocked = np.zeros((rows, cols), dtype=bool)
        for r, c in obs["food"]:
            blocked[r, c] = True

        taken = {tuple(a) for a in mine}
        out = np.full(len(mine), HOLD, dtype=np.int64)

        # Nearest-first, so the ant with the best claim on a square gets it. Ties in the sort break
        # on the index, which is `mine`'s order, which the engine fixes row-major -- so this is
        # deterministic too.
        order = sorted(range(len(mine)), key=lambda i: (dist[mine[i][0], mine[i][1]], i))
        for i in order:
            ar, ac = mine[i]
            best, best_d = HOLD, dist[ar, ac]
            # Fixed order, and strictly better only. Two equidistant neighbours are common in open
            # ground and choosing between them at random is what made a quarter of these labels
            # unlearnable.
            for k in range(4):
                dr, dc = DELTAS[k]
                nr, nc = (ar + dr) % rows, (ac + dc) % cols
                if water[nr, nc] or blocked[nr, nc] or (nr, nc) in taken:
                    continue
                if dist[nr, nc] < best_d:
                    best, best_d = k, dist[nr, nc]
            if best != HOLD:
                dr, dc = DELTAS[best]
                nr, nc = (ar + dr) % rows, (ac + dc) % cols
                taken.discard((ar, ac))
                taken.add((nr, nc))
            out[i] = best
        return out


def known(obs: dict) -> tuple[np.ndarray, np.ndarray]:
    """Water, and what has ever been seen, from one observation.

    `water` is the only field with memory, so a 1 is water you have seen and a 0 is *either* known
    land or a cell you have never looked at. The teacher needs those apart, and the honest way to
    get it from a single observation is the visibility disk around your own ants — which is the same
    thing plane 6 computes, and the same reason it exists.
    """
    from .planes import VIEW_RADIUS2, Board

    rows, cols = obs["size"]
    b = Board(rows, cols)
    water = b.rle(obs["water"]["rle"]).astype(bool)
    # A cell is "seen" if it is known water or is visible now. This under-counts -- ground you
    # walked over an hour ago reads as unseen -- and that is the right way to be wrong here: it
    # keeps the colony exploring rather than declaring the map finished.
    visible = b.dilate(b.scatter(obs["mine"]), VIEW_RADIUS2).astype(bool)
    return water, water | visible
