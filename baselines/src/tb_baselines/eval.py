"""Play the artifacts against each other, through the real path.

    python -m tb_baselines.eval models/micro-bc models/nano-bc --boards 4

**Nothing here evaluates a network.** It writes match files and runs `tinybrains <match>`, which
hosts the cartridge, evaluates the manifest's adapters through datalogic, runs the graph through
tract and writes the same replay envelope Kalam writes — the same two libraries an Orion node links.
So a result here is a result under the rules — the adapter included, the operation budget included,
the wrap included — and not a number from a training loop that believes its own encoder.

That distinction is the whole reason this module is thin. Three gates, and only the last two are
real:

    tinybrains env       training rollouts    fast; no deadline, no strikes, no adapter
    tinybrains <match>   this module          the real path, minus admission
    tinybrains check     export.py            what the platform will actually decide

## Every pair plays both seats of every board

Ants boards are symmetric by construction but a *match* is not: the seed drives food respawn, and
one seat moves first into any contested square. So a pairing is played twice on each board with the
seats swapped, and a win rate that survives the swap is a win rate about the players.
"""

from __future__ import annotations

import argparse
import itertools
import json
import os
import shutil
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


@dataclass
class Entrant:
    """One thing that can hold a seat: an exported artifact, or a written script."""

    name: str
    weights: Path | None = None
    manifest: Path | None = None
    script: list[str] | None = None

    def seat(self, index: int) -> dict:
        s: dict = {"seat": index, "label": self.name}
        if self.script is not None:
            s["script"] = self.script
        else:
            s["weights"] = str(self.weights.resolve())
            s["manifest"] = str(self.manifest.resolve())
        return s


@dataclass
class Record:
    wins: int = 0
    draws: int = 0
    losses: int = 0
    score_for: int = 0
    score_against: int = 0
    matches: int = 0
    reasons: dict[str, int] = field(default_factory=dict)

    @property
    def points(self) -> float:
        return self.wins + 0.5 * self.draws

    @property
    def win_rate(self) -> float:
        return self.points / self.matches if self.matches else 0.0


def cli() -> str:
    found = os.environ.get("TINYBRAINS") or shutil.which("tinybrains")
    if not found:
        raise SystemExit("no `tinybrains` on PATH; set TINYBRAINS to the binary")
    return found


def boards(maps: str | None, limit: int) -> list[str]:
    """The two-seat boards to play, as a match file names them: an id or a path.

    `maps` is a directory of boards (a season's, say), or comma-separated ids or paths; None is
    every two-seat board the release ships, read off `tinybrains maps` rather than hard-coded,
    because the board list belongs to the cartridge. That means parsing a human-readable table, so
    the parse is CHECKED: an empty result would otherwise play no matches and print a round robin of
    all zeroes, which reads like a field of draws rather than like a bug. A round robin is head to
    head, so a board seating more than two is not one of its boards.
    """
    if maps and Path(maps).is_dir():
        picked = []
        for f in sorted(Path(maps).glob("*.json")):
            if json.loads(f.read_text()).get("players") == 2:
                picked.append(str(f.resolve()))
        if not picked:
            raise SystemExit(f"no two-seat boards in {maps}")
        return picked[:limit]
    if maps:
        return [m.strip() for m in maps.split(",") if m.strip()][:limit]

    out = subprocess.run([cli(), "maps"], cwd=ROOT, capture_output=True, text=True)
    if out.returncode != 0:
        raise SystemExit(f"tinybrains maps failed:\n{out.stderr or out.stdout}")
    rows = []
    for line in out.stdout.splitlines()[1:]:      # line 0 is the "<game> N boards" header
        parts = line.split()
        # id, dimensions, seats -- enough shape to notice if the table changes.
        if len(parts) >= 4 and "x" in parts[1] and parts[2] == "2" and parts[3] == "seats":
            rows.append(parts[0])
    if not rows:
        raise SystemExit(
            "could not read a two-seat board out of `tinybrains maps`; its output format has "
            "changed, or the release ships none:\n" + "\n".join(out.stdout.splitlines()[:4])
        )
    return rows[:limit]


def play(entrants: list[Entrant], board_limit: int, maps: str | None,
         max_turns: int, seed: int, out_dir: Path) -> dict[str, Record]:
    """Every pair, both seat orders, every board -- one match file, since a wave needs one seat
    count and every board here seats two."""
    records = {e.name: Record() for e in entrants}
    pairs = list(itertools.combinations(range(len(entrants)), 2))
    if not pairs:
        raise SystemExit("a round robin needs at least two entrants")

    rows: list[dict] = []
    labels: dict[str, tuple[str, str]] = {}
    for board in boards(maps, board_limit):
        name = Path(board).stem if board.endswith(".json") else board
        for a, b in pairs:
            for flip in (0, 1):
                first, second = (a, b) if not flip else (b, a)
                mid = f"{entrants[first].name}-vs-{entrants[second].name}-{name}-{flip}"
                rows.append({
                    "id": mid,
                    "seed": seed + len(rows),
                    "map": board,
                    "seat_count": 2,
                    "seats": [entrants[first].seat(0), entrants[second].seat(1)],
                })
                labels[mid] = (entrants[first].name, entrants[second].name)

    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        mf = Path(tmp) / "round-robin.json"
        mf.write_text(json.dumps(
            {"game": "ants", "vars": {"max_turns": max_turns}, "rows": rows}))
        r = subprocess.run(
            [cli(), str(mf), "--out", str(out_dir)],
            cwd=ROOT, capture_output=True, text=True,
        )
        if r.returncode != 0:
            raise SystemExit(f"round robin: {r.stderr or r.stdout}")

        for row in rows:
            replay = json.loads((out_dir / f"{row['id']}.json").read_text())
            left, right = labels[row["id"]]
            ranks, scores = replay["engine_ranks"], replay["scores"]
            for who, mine, theirs in ((left, 0, 1), (right, 1, 0)):
                rec = records[who]
                rec.matches += 1
                rec.score_for += scores[mine]
                rec.score_against += scores[theirs]
                rec.reasons[replay["reason"]] = rec.reasons.get(replay["reason"], 0) + 1
                if ranks[mine] < ranks[theirs]:
                    rec.wins += 1
                elif ranks[mine] > ranks[theirs]:
                    rec.losses += 1
                else:
                    rec.draws += 1
    return records


def table(records: dict[str, Record]) -> str:
    lines = [f"{'entrant':22s} {'played':>7s} {'W':>4s} {'D':>4s} {'L':>4s} "
             f"{'rate':>6s} {'score':>7s} {'against':>8s}"]
    for name, r in sorted(records.items(), key=lambda kv: -kv[1].win_rate):
        lines.append(
            f"{name:22s} {r.matches:>7d} {r.wins:>4d} {r.draws:>4d} {r.losses:>4d} "
            f"{r.win_rate:>5.0%} {r.score_for / max(r.matches, 1):>7.2f} "
            f"{r.score_against / max(r.matches, 1):>8.2f}"
        )
    return "\n".join(lines)


def entrant_from(path: str) -> Entrant:
    p = Path(path)
    if p.is_dir():
        return Entrant(name=p.name, weights=p / "model.onnx", manifest=p / "manifest.json")
    raise SystemExit(f"{path} is not an exported model directory")


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("entrants", nargs="+", help="exported model directories")
    ap.add_argument("--boards", type=int, default=3, help="two-seat boards to play, at most")
    ap.add_argument("--maps", default=None,
                    help="board ids, paths, or a directory of boards; default the release's own")
    ap.add_argument("--max-turns", type=int, default=300)
    ap.add_argument("--seed", type=int, default=90000)
    ap.add_argument("--out", type=Path, default=Path("replays"))
    ap.add_argument("--json", dest="as_json", action="store_true")
    a = ap.parse_args(argv)

    entrants = [entrant_from(e) for e in a.entrants]
    records = play(entrants, a.boards, a.maps, a.max_turns, a.seed, a.out)
    if a.as_json:
        print(json.dumps({n: vars(r) for n, r in records.items()}, indent=2))
    else:
        print(table(records))
        print()
        print("Played under the rules -- the manifest, the operation budget and the wrap included.")
        print("It is not the ladder: no ratings, no seasons, and no trial.")


if __name__ == "__main__":
    main()
