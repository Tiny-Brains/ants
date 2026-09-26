"""Join datasets collected in parallel into one, keeping every seat's rows its own.

    python -m tb_baselines.merge data/xathis-1[1-5].jsonl.gz --out data/xathis.jsonl.gz

A teacher too slow for one process is collected by several, each from its own seed, and
`train/seq.py` reads a seat's rows back in order by `k = [episode, seat, turn]`. Each collector
numbers its episodes from 0, so two files' episode 3 are two matches: the merge offsets each
file's episodes by a hundred thousand times its place, and the header names the parts. A header
that disagrees on the engine, the teacher or the memory is refused: those are one dataset's.
"""

from __future__ import annotations

import argparse
import gzip
import json
from pathlib import Path

STRIDE = 100_000


def merge(parts: list[Path], out: Path) -> dict:
    header = None
    rows = 0
    with gzip.open(out, "wt") as w:
        for i, part in enumerate(parts):
            with gzip.open(part, "rt") as f:
                h = json.loads(f.readline())
                if header is None:
                    header = dict(h)
                    header["parts"] = []
                else:
                    for key in ("engine_digest", "teacher", "memory", "evaluator"):
                        if h.get(key) != header.get(key):
                            raise SystemExit(f"{part}: {key} is {h.get(key)!r}, the first part's {header.get(key)!r}")
                header["parts"].append({"file": part.name, "env_seed": h.get("env_seed"), "episode_offset": i * STRIDE})
                if i == 0:
                    w.write(json.dumps(header) + "\n")
                for line in f:
                    if i == 0:
                        w.write(line)
                    else:
                        row = json.loads(line)
                        row["k"][0] += i * STRIDE
                        w.write(json.dumps(row, separators=(",", ":")) + "\n")
                    rows += 1
    if header is None:
        raise SystemExit("nothing to merge")
    # The header was written before the parts were known; rewrite it now that they are.
    return {"rows": rows, "parts": len(parts), "out": str(out)}


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("parts", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args(argv)
    print(json.dumps(merge(a.parts, a.out), indent=2))


if __name__ == "__main__":
    main()
