"""`manifest.json`, generated from `planes.py` — never hand-written.

An artifact nobody hand-edits cannot drift from the thing it was made from. That is the same rule
`ants/build.sh` applies to `cartridge.json` and `plugin.json`, and it applies here for a sharper
reason: the adapter is one of the two renderings of the encoding, and the whole anti-skew argument
collapses if someone can edit one rendering without the other.

    python -m tb_baselines.adapters > manifest.json

The bytes are what the platform hashes and half of what it weighs (`S' = artifact_bytes +
len(manifest)`), so this must be **deterministic**: compact separators, sorted keys, no trailing
newline. Change the spacing and `manifest_hash` moves, which is a new submission for a document
that means the same thing.

## What a manifest is

Orion's `orion:model@1.0.0`: the model's name, its inputs and outputs with their dtypes and shapes,
and one JSONLogic **adapter** per input that turns the observation into that input's tensor. It is
the whole competitor-authored surface — there is no second document.

`in` became the `board` input's adapter and `out` is **gone**. The platform reads the head now
(decision R3): a manifest's `result` expression is evaluated against the output tensors alone, so it
cannot see the observation and cannot gather at the ants' cells. `tb-match` does the gather, and
this manifest declares the head shape it will find — `[1, moves, H, W]`, per cell.

## Why the shapes are named

`H` and `W` are variable axes (Orion 1.8.1). A season runs several board sizes and a name binds to
what the call brings, so one manifest and one loaded session serve 64x96, 96x96 and 128x128 alike.
`probe_dims` is what admission's five zero-filled inferences run at, and it is set to the largest
board the catalogue ships: probing the smallest would gate a board nobody plays.
"""

from __future__ import annotations

import json
import sys

from .planes import DTYPE, N_MOVES, N_PLANES, PLANES

ABI = "orion:model@1.0.0"

# What the admission probe binds each named axis to. The largest board the cartridge ships, because
# `probe_ms` is only as representative as the size it was measured at.
PROBE_DIMS = {"H": 128, "W": 128}


def board_adapter() -> dict:
    """Observation to the `board` tensor, `[1, planes, H, W]`."""
    size = {"var": "size"}
    stacked = {"stack": [[p.logic(size) for p in PLANES], 0]}
    # `merge` builds the shape list at run time: [1, planes] ++ size. `reshape` is metadata only
    # and costs 1, so the leading batch axis is free.
    return {"reshape": [stacked, {"merge": [[1, N_PLANES], size]}]}


def manifest(name: str = "tb.baseline", version: str = "1") -> dict:
    return {
        "abi": ABI,
        "name": name,
        "version": version,
        "format": "onnx",
        "description": "A TinyBrains Ants entry: seven planes in, a per-cell policy out.",
        "inputs": [
            {
                "name": "board",
                "dtype": DTYPE,
                "shape": [1, N_PLANES, "H", "W"],
                "adapter": board_adapter(),
            }
        ],
        # The head the platform gathers from. `f32` because a policy is scores, not classes, and
        # the channel order is the game's (`planes.MOVES`).
        "outputs": [{"name": "policy", "dtype": "f32", "shape": [1, N_MOVES, "H", "W"]}],
        "probe_dims": PROBE_DIMS,
    }


def dumps(name: str = "tb.baseline", version: str = "1") -> str:
    """The exact bytes. Sorted and compact, so the same spec always hashes the same."""
    return json.dumps(manifest(name, version), separators=(",", ":"), sort_keys=True)


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "tb.baseline"
    sys.stdout.write(dumps(name))


if __name__ == "__main__":
    main()
