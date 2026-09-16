"""Run the teacher and write down what it did: the dataset every supervised cell trains on.

    python -m tb_baselines.collect --seat-turns 200000 --out data/teacher.jsonl.gz

**One dataset, reused by every learner.** That is what makes the method column mean anything: a
per-cell linear model, a convolutional trunk and anything else are then the same data and the same
labels, differing only in the learner — which is the comparison the ladder is for. Regenerate it and
the comparison is between two things at once.

## What a row is

One seat on one turn: the observation exactly as the engine emitted it, and one character an ant in
`mine`'s order, exactly as a replay delta writes a turn. Observations rather than tensors, for two
reasons. Tensors are fifty times larger — a 7x128x128 int8 board is 114 KiB against about 2 KiB of
JSON — so 200,000 of them is 23 GiB against 400 MiB. And an encoding stored in the dataset is an
encoding frozen at collection time: `planes.py` should be free to gain a plane without every dataset
becoming stale.

## What it is not

It is not a replay and cannot be viewed. Replays come from `tinybrains <match>`; this is a pile of
independent observations with no match structure, which is all a supervised learner wants.
"""

from __future__ import annotations

import argparse
import gzip
import json
import time
from pathlib import Path

from .env import Env, orders_from_indices
from .teacher import Teacher


def collect(
    out: Path,
    seat_turns: int,
    waves: int = 4,
    matches_per_wave: int = 16,
    max_turns: int = 300,
    seed: int = 1,
    preset: str | None = None,
) -> dict:
    """Play until `seat_turns` rows are written, and report what was collected."""
    teacher = Teacher()
    out.parent.mkdir(parents=True, exist_ok=True)

    written = 0
    ants = 0
    episodes = 0
    reasons: dict[str, int] = {}
    scores: list[int] = []
    started = time.time()

    with Env(waves=waves, matches_per_wave=matches_per_wave, max_turns=max_turns,
             seed=seed, preset=preset) as env, gzip.open(out, "wt") as f:
        # The header is the provenance. A dataset that cannot name the engine that produced it is a
        # dataset nobody can reproduce, and an engine change is a rules change.
        f.write(json.dumps({
            "kind": "teacher-rollout",
            "engine_digest": env.engine_digest,
            # `evaluator_digest` named an axon build. The evaluator is datalogic now and the env
            # names it by version (R10), which is what a version skew between a local pass and a
            # remote refusal would show up in.
            "evaluator": env.evaluator,
            "presets": [p["name"] for p in env.hello["presets"]],
            "max_turns": max_turns,
            "env_seed": seed,
            # The teacher is a pure function of the observation, so the env seed is the whole
            # provenance: this dataset is reproducible from that number and the engine digest.
            "teacher": "deterministic potential field",
        }) + "\n")

        step = env.reset()
        while written < seat_turns:
            actions = []
            for s in step.seats:
                water, seen = _known(s.obs)
                moves = teacher.orders(s.obs, water, seen)
                order = orders_from_indices(moves, [len(s.obs["mine"])])[0]
                actions.append(order)
                if written < seat_turns and s.obs["mine"]:
                    f.write(json.dumps({"o": s.obs, "a": order}, separators=(",", ":")) + "\n")
                    written += 1
                    ants += len(order)

            step = env.step(actions)
            for e in step.ended:
                episodes += 1
                reasons[e.reason] = reasons.get(e.reason, 0) + 1
                scores.append(max(e.scores))
            if not step.seats:
                break

    elapsed = time.time() - started
    return {
        "rows": written,
        "ant_labels": ants,
        "episodes": episodes,
        "reasons": reasons,
        "mean_best_score": round(sum(scores) / len(scores), 2) if scores else None,
        "seconds": round(elapsed, 1),
        "rows_per_second": round(written / max(elapsed, 1e-9)),
        "bytes": out.stat().st_size,
    }


def _known(obs):
    from .teacher import known
    return known(obs)


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description="Collect a teacher rollout dataset.")
    ap.add_argument("--out", type=Path, default=Path("data/teacher.jsonl.gz"))
    ap.add_argument("--seat-turns", type=int, default=200_000)
    ap.add_argument("--waves", type=int, default=4)
    ap.add_argument("--matches-per-wave", type=int, default=16)
    ap.add_argument("--max-turns", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--preset", default=None, help="pin to one preset; default is all three")
    a = ap.parse_args(argv)

    stats = collect(
        a.out, a.seat_turns, waves=a.waves, matches_per_wave=a.matches_per_wave,
        max_turns=a.max_turns, seed=a.seed, preset=a.preset,
    )
    (a.out.with_suffix("").with_suffix(".stats.json")).write_text(json.dumps(stats, indent=2) + "\n")
    print(json.dumps(stats, indent=2))


if __name__ == "__main__":
    main()
