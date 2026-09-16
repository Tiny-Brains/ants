"""The environment client, against the real cartridge.

These are cheap — a handful of turns on a two-match wave — and they cover the three things that are
easy to get wrong and silent when you do: the positional alignment between seats and actions, the
episode key surviving a wave refill, and the score channel actually carrying live scores.
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path

import numpy as np
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from tb_baselines.env import Env, EnvError, orders_from_indices  # noqa: E402
from tb_baselines.planes import MOVES, N_PLANES  # noqa: E402

pytestmark = pytest.mark.skipif(
    not (os.environ.get("TINYBRAINS") or shutil.which("tinybrains")),
    reason="no `tinybrains` on PATH; set TINYBRAINS to the binary",
)


@pytest.fixture
def env():
    e = Env(waves=1, matches_per_wave=4, max_turns=40, seed=7, preset="standard")
    yield e
    e.close()


def test_hello_names_the_engine_and_the_evaluator(env):
    """A run that cannot say which engine produced its data cannot be reproduced, and an engine
    change is a rules change."""
    assert env.engine_digest.startswith("sha256:")
    assert env.evaluator.startswith("datalogic "), "the evaluator is named by version now"
    assert env.hello["max_turns"] == 40


def test_seats_and_boards_line_up(env):
    step = env.reset()
    assert len(step.seats) == 8, "four matches of two seats"
    assert step.boards.shape == (8, N_PLANES, 64, 96), "standard is 64x96"
    for i, seat in enumerate(step.seats):
        assert tuple(seat.obs["size"]) == (64, 96)
        assert step.groups[0].indices[i] == i


def test_actions_are_positional_and_the_engine_agrees(env):
    """Every ant of every live seat gets an order, in `mine`'s order. If the alignment were wrong
    the engine would still accept it — it would just be playing someone else's moves, which is the
    reason this is a test and not a comment."""
    step = env.reset()
    for _ in range(5):
        counts = [len(s.obs["mine"]) for s in step.seats]
        picks = np.zeros(sum(counts), dtype=np.int64)      # every ant north
        orders = orders_from_indices(picks, counts)
        assert [len(o) for o in orders] == counts
        assert all(set(o) <= {"N"} for o in orders)
        step = env.step(orders)
    assert step.turn == 5


def test_scores_are_live_and_keyed_by_episode(env):
    """`f_finish` answers a running match, which is the whole reason a dense reward needs no
    cartridge change."""
    step = env.reset()
    assert len(step.scores) == 4, "one entry a live match"
    for ep, scores in step.scores.items():
        assert len(scores) == 2
        assert all(isinstance(s, int) for s in scores)
    assert {s.ep for s in step.seats} == set(step.scores)


def test_a_mixed_pool_refuses_to_pretend_it_is_one_batch():
    """Three presets are three board sizes and one tensor cannot hold two. Handing back a fraction
    of the batch would be a silent third of a training step."""
    e = Env(waves=3, matches_per_wave=2, max_turns=20, seed=3)
    try:
        step = e.reset()
        assert len(step.groups) > 1, "three waves cycle the three presets"
        with pytest.raises(EnvError, match="board sizes"):
            _ = step.boards
        assert sum(len(g.indices) for g in step.groups) == len(step.seats)
        for g in step.groups:
            assert g.boards.shape == (len(g.indices), N_PLANES, *g.size)
    finally:
        e.close()


def test_an_empty_seat_is_an_empty_order(env):
    """Zero ants is always valid, and is what a wiped-out seat sends until its match ends."""
    assert orders_from_indices(np.zeros(0, dtype=np.int64), [0]) == [""]


def test_order_characters_are_the_moves_the_adapter_decodes():
    """`orders_from_indices` and the adapter's `tb.at` table must index the same list, or a policy
    channel means one move in training and another at play."""
    picks = np.arange(len(MOVES))
    assert orders_from_indices(picks, [len(MOVES)])[0] == "".join(MOVES)
