"""The teacher's memory: what a remembered hill or food cell does to the field, and what it does
not. Synthetic boards, since the question is the rule and not a real match."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from tb_baselines.teacher import FAR, Teacher, known  # noqa: E402


def board(size: int = 24, ant=(12, 12), hills=(), food=()) -> dict:
    """An open board, one ant, no water, and whatever else the test names."""
    return {
        "size": [size, size], "mine": [list(ant)], "ids": [0], "foes": [],
        "food": [list(f) for f in food], "hills": [list(h) for h in hills],
        "water": {"rle": [0, size * size]}, "vis": {"rle": [0, size * size]},
    }


def test_a_remembered_enemy_hill_out_of_sight_is_the_nearest_target():
    """Twelve cells away is past the view radius (8.8), so the plain teacher sees only the
    frontier there; a remembering one seeds the hill at level 0, ahead of everything."""
    t = Teacher()
    obs = board()
    water, seen = known(obs)
    plain = t.field(obs, water, seen)
    remembering = t.field(obs, water, seen, {"hill_foe_seen": [[0, 12]], "food_seen": []})
    assert plain[0, 12] == t.hill_lead + 2, "unseen, so the frontier's level"
    assert remembering[0, 12] == 0, "an enemy hill seen once is a target until seen gone"
    assert remembering[12, 12] < plain[12, 12], "and the ant's own square is nearer to a target"


def test_a_remembered_hill_whose_square_is_seen_empty_is_a_hill_razed():
    """Four cells away is in view. The view lists no hill there, so the memory is stale and the
    remembering teacher reads it against the view rather than walking to a hill that is gone."""
    t = Teacher()
    obs = board()
    water, seen = known(obs)
    remembering = t.field(obs, water, seen, {"hill_foe_seen": [[12, 16]], "food_seen": []})
    assert remembering[12, 16] != 0
    assert np.array_equal(remembering, t.field(obs, water, seen)), "a razed hill changes nothing"


def test_remembered_food_out_of_sight_sits_between_food_in_view_and_the_frontier():
    t = Teacher()
    obs = board(food=[(12, 14)])
    water, seen = known(obs)
    f = t.field(obs, water, seen, {"hill_foe_seen": [], "food_seen": [[0, 12], [12, 14]]})
    assert f[12, 14] == t.hill_lead, "food in view keeps its level"
    assert f[0, 12] == t.hill_lead + 1, "food remembered out of view is one level behind it"
    assert t.field(obs, water, seen)[0, 12] == t.hill_lead + 2, "and the frontier one behind that"


def test_without_a_memory_the_remembering_teacher_is_the_plain_one():
    t = Teacher()
    obs = board(hills=[(12, 2, 1)], food=[(3, 20)])
    water, seen = known(obs)
    assert np.array_equal(t.orders(obs, water, seen), t.orders(obs, water, seen, None))
    assert np.array_equal(t.orders(obs, water, seen), t.orders(obs, water, seen, {}))
    assert (t.field(obs, water, seen) != FAR).any()
