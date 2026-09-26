#!/usr/bin/env python3
"""Two engines, one game: prove that a rebuilt cartridge plays the released one's matches.

    tools/equivalence.py OLD_DIST NEW_DIST [--allow ids] [--turns 300] [--seed 1] [--maps DIR|IDS]

A change to `engine/` is a new digest whatever it does, so "nothing else moved" has to be shown
rather than said. This plays the same seeded matches through `tinybrains env` on two dists at
once -- the released archive unpacked, and this checkout's `dist/` -- under two policies, a seeded
random walker and the baselines' greedy teacher, always choosing the actions on NEW's views and
sending the same actions to both. Every view, every score and every ending is then compared turn
by turn, and the only differences allowed are the keys `--allow` names: a field one engine sends
and the other does not, such as `ids`. Anything else -- an ant one cell off, a food that fell a
turn late, a match that ended for another reason -- fails the run on the turn it happens, with
the seat and the key.

It is `observe`, `step` and `finish` compared on real play: the env is those three calls and a
pool, and a scores line every turn is `finish` answering for a live match. It is not a proof of
the codec, which `engine/src/tests/equivalence.rs` holds, and it says nothing about the digest.
The sweep (`mapgen sweep --seats 8`) is the other half, the fairness proof across seats.

Needs a `tinybrains` that plays both dists (`TINYBRAINS`, else on PATH), numpy, and
`baselines/src` beside this file for the teacher.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "baselines" / "src"))

from tb_baselines.env import orders_from_indices  # noqa: E402
from tb_baselines.planes import MOVES  # noqa: E402
from tb_baselines.teacher import Teacher, known  # noqa: E402


class Engine:
    """One `tinybrains env` over one dist, spoken to in JSON Lines."""

    def __init__(self, name: str, dist: Path, cli: str, work: Path, args: list[str]):
        self.name = name
        registry = work / name
        registry.mkdir(parents=True)
        (registry / "games.toml").write_text(
            f'[games.ants]\nname = "Ants"\npath = "{dist.resolve()}"\n'
        )
        env = dict(os.environ, TINYBRAINS_REGISTRY=str(registry / "games.toml"))
        self.proc = subprocess.Popen(
            [cli, "env", *args], cwd=str(registry), env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
        )
        self.hello = self.read()["hello"]

    def read(self) -> dict:
        line = self.proc.stdout.readline()
        if not line:
            err = self.proc.stderr.read() if self.proc.stderr else ""
            raise SystemExit(f"{self.name}: the environment exited{': ' + err if err else ''}")
        reply = json.loads(line)
        if not reply.get("ok"):
            raise SystemExit(f"{self.name}: {reply.get('error')}")
        return reply

    def send(self, request: dict) -> dict:
        self.proc.stdin.write(json.dumps(request) + "\n")
        self.proc.stdin.flush()
        return self.read()

    def close(self) -> None:
        try:
            self.proc.stdin.write('{"op":"close"}\n')
            self.proc.stdin.flush()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()


def strip(reply: dict, allow: set[str]) -> dict:
    """The reply with the allowed keys removed from every view, so the two can be compared."""
    out = {"turn": reply["turn"], "scores": reply["scores"], "ended": reply["ended"], "seats": []}
    for s in reply["seats"]:
        obs = {k: v for k, v in s["obs"].items() if k not in allow}
        out["seats"].append({k: v for k, v in s.items() if k != "obs"} | {"obs": obs})
    return out


def first_difference(a, b, path="") -> str | None:
    """Where two JSON values first differ, as a path, or None."""
    if type(a) is not type(b):
        return f"{path or '/'}: {type(a).__name__} vs {type(b).__name__}"
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                return f"{path}/{k}: only in {'NEW' if k in b else 'OLD'}"
            d = first_difference(a[k], b[k], f"{path}/{k}")
            if d:
                return d
        return None
    if isinstance(a, list):
        if len(a) != len(b):
            return f"{path}: {len(a)} vs {len(b)} entries"
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_difference(x, y, f"{path}[{i}]")
            if d:
                return d
        return None
    return None if a == b else f"{path}: {a!r} vs {b!r}"


class RandomWalker:
    name = "random"

    def __init__(self, seed: int):
        self.rng = np.random.default_rng(seed)

    def act(self, seats: list[dict]) -> list[str]:
        table = np.array([ord(m) for m in MOVES], dtype=np.uint8)
        return [
            table[self.rng.integers(0, len(MOVES), size=len(s["obs"]["mine"]))].tobytes().decode()
            for s in seats
        ]


class Greedy:
    name = "greedy"

    def __init__(self):
        self.teacher = Teacher()

    def act(self, seats: list[dict]) -> list[str]:
        out = []
        for s in seats:
            water, seen = known(s["obs"])
            moves = self.teacher.orders(s["obs"], water, seen)
            out.append(orders_from_indices(moves, [len(s["obs"]["mine"])])[0])
        return out


def play(policy, old: Engine, new: Engine, turns: int, allow: set[str]) -> dict:
    digest = hashlib.sha256()
    a, b = old.send({"op": "observe"}), new.send({"op": "observe"})
    seats = views = endings = 0
    only_new: set[str] = set()
    only_old: set[str] = set()
    for turn in range(turns + 1):
        for s in b["seats"]:
            only_new |= set(s["obs"]) - set(a["seats"][0]["obs"]) if a["seats"] else set()
        for s in a["seats"]:
            only_old |= set(s["obs"]) - set(b["seats"][0]["obs"]) if b["seats"] else set()
        sa, sb = strip(a, allow), strip(b, allow)
        d = first_difference(sa, sb)
        if d:
            raise SystemExit(
                f"{policy.name}: the engines differ on turn {turn} at {d}\n"
                f"  OLD {old.hello['engine_digest']}\n  NEW {new.hello['engine_digest']}"
            )
        digest.update(json.dumps(sb, sort_keys=True, separators=(",", ":")).encode())
        seats += len(b["seats"])
        views += sum(len(s["obs"]["mine"]) for s in b["seats"])
        endings += len(b["ended"])
        if not b["seats"] or turn == turns:
            break
        actions = policy.act(b["seats"])
        a, b = old.send({"op": "step", "actions": actions}), new.send({"op": "step", "actions": actions})
    return {
        "policy": policy.name, "turns": turn, "seat_turns": seats, "ant_turns": views,
        "endings": endings, "sha256": digest.hexdigest()[:16],
        "only_new": sorted(only_new), "only_old": sorted(only_old),
    }


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("old", type=Path, help="the released dist, unpacked")
    ap.add_argument("new", type=Path, help="this checkout's dist/")
    ap.add_argument("--allow", default="ids",
                    help="view keys one engine may send and the other not (comma-separated)")
    ap.add_argument("--turns", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--waves", type=int, default=4)
    ap.add_argument("--matches-per-wave", type=int, default=8)
    ap.add_argument("--maps", default=None, help="board ids, paths or a directory; default the dist's basic boards")
    a = ap.parse_args(argv)

    cli = os.environ.get("TINYBRAINS") or shutil.which("tinybrains")
    if not cli:
        raise SystemExit("no `tinybrains` on PATH; set TINYBRAINS to the binary")
    allow = {k for k in a.allow.split(",") if k}
    args = ["--waves", str(a.waves), "--matches-per-wave", str(a.matches_per_wave),
            "--max-turns", str(a.turns), "--seed", str(a.seed), "--scores", "every"]
    if a.maps:
        args += ["--maps", a.maps]

    results = []
    with tempfile.TemporaryDirectory() as tmp:
        for policy in (RandomWalker(a.seed), Greedy()):
            old = Engine("old", a.old, cli, Path(tmp) / policy.name, args)
            new = Engine("new", a.new, cli, Path(tmp) / policy.name, args)
            if old.hello["engine_digest"] == new.hello["engine_digest"]:
                print(f"note: both dists are engine {old.hello['engine_digest']}", file=sys.stderr)
            try:
                results.append(play(policy, old, new, a.turns, allow))
            finally:
                old.close()
                new.close()
            r = results[-1]
            print(f"{r['policy']:7s} {r['turns']} turns, {r['seat_turns']} seat-turns, "
                  f"{r['ant_turns']} ant-turns, {r['endings']} endings identical  "
                  f"(sha256 {r['sha256']})")
            if r["only_new"] or r["only_old"]:
                print(f"        keys only NEW sends: {r['only_new'] or '-'}; only OLD: {r['only_old'] or '-'}")
    print(f"OLD {a.old}: {old.hello['engine_digest']}")
    print(f"NEW {a.new}: {new.hello['engine_digest']}")
    print(f"identical but for {sorted(allow) or 'nothing'}")


if __name__ == "__main__":
    main()
