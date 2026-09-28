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

There is one adapter, the `board` input's, and no output decoder. The platform reads the head: a
manifest's `result` expression is evaluated against the output tensors alone, so it cannot see the
observation and cannot gather at the ants' cells. `kalam-match` does the gather, and this manifest
declares the head shape it will find — `[1, moves, H, W]`, per cell.

With `memory=True` the manifest declares a second input, `memory_in`, whose adapter passes last
turn's memory through (zeros on turn 0), and a second output, `memory`, which the runner hands back
on the next view (`planes.MEMORY`, and the book's *Memory* page).

## Why the shapes are named

`H` and `W` are variable axes (Orion 1.8.1). A season runs several board sizes and a name binds to
what the call brings, so one manifest and one loaded session serve every board from 24x24 to
120x124. `probe_dims` is what admission's five zero-filled inferences run at, and 128x128 covers the
longest side any board may have (124, `limits.boards` in cartridge.json): probing the smallest would
gate a board nobody plays.
"""

from __future__ import annotations

import json
import sys

from .planes import (ANT_DTYPE, ANT_MEMORY, DTYPE, N_MEMORY, N_MOVES, N_PLANES, PLANES,
                     ant_cells_adapter, ant_ids_adapter, ant_planes_adapter, memory_adapter)

ABI = "orion:model@1.0.0"

# What the admission probe binds each named axis to. At least the longest side any board may have
# (124, `limits.boards`), because `probe_ms` is only as representative as the size it ran at.
PROBE_DIMS = {"H": 128, "W": 128}


def board_adapter() -> dict:
    """Observation to the `board` tensor, `[1, planes, H, W]`."""
    size = {"var": "size"}
    stacked = {"stack": [[p.logic(size) for p in PLANES], 0]}
    # `merge` builds the shape list at run time: [1, planes] ++ size. `reshape` is metadata only
    # and costs 1, so the leading batch axis is free.
    return {"reshape": [stacked, {"merge": [[1, N_PLANES], size]}]}


def manifest(name: str = "tb.baseline", version: str = "1", memory: bool | str = False,
             ants: bool | str = False) -> dict:
    """`memory` is False, True or `"max"` (`planes.MEMORY`, `i8`) or `"learned"` (the same ports);
    `ants` is False, True or `"learned"` (`planes.ANT_MEMORY` values an ant)."""
    mkind = ("max" if memory is True else memory) if memory else None
    akind = ("learned" if ants is True else ants) if ants else None
    inputs = [
        {
            "name": "board",
            "dtype": DTYPE,
            "shape": [1, N_PLANES, "H", "W"],
            "adapter": board_adapter(),
        }
    ]
    # The head the platform gathers from. `f32` because a policy is scores, not classes, and the
    # channel order is the game's (`planes.MOVES`).
    outputs = [{"name": "policy", "dtype": "f32", "shape": [1, N_MOVES, "H", "W"]}]
    description = "A TinyBrains Ants entry: seven planes in, a per-cell policy out."
    if mkind in ("max", "learned"):
        # The memory is `i8` like the board: two 0/1 planes, priced at 2 bytes a cell.
        shape = [1, N_MEMORY, "H", "W"]
        inputs.append({"name": "memory_in", "dtype": DTYPE, "shape": shape,
                       "adapter": memory_adapter({"var": "size"})})
        outputs.append({"name": "memory", "dtype": DTYPE, "shape": shape})
        description = ("A TinyBrains Ants entry: seven planes and a two-plane memory in, "
                       "a per-cell policy and the memory out.")
    elif mkind:
        raise ValueError(f"no memory '{mkind}'")
    if akind:
        # A memory per ant: the graph reads each ant's remembered values at its cell and writes
        # one row an ant, its id first, which the runner hands back.
        size = {"var": "size"}
        k = ANT_MEMORY
        inputs += [
            {"name": "ant_planes", "dtype": ANT_DTYPE, "shape": [1, k, "H", "W"], "adapter": ant_planes_adapter(size)},
            {"name": "ids", "dtype": "i64", "shape": [1, "N"], "adapter": ant_ids_adapter()},
            {"name": "cells", "dtype": "i64", "shape": [1, "N"], "adapter": ant_cells_adapter()},
        ]
        outputs.append({"name": "ant_memory", "dtype": ANT_DTYPE, "shape": [1, "N", k + 1]})
        description += " Each ant carries a row of its own."
    return {
        "abi": ABI,
        "name": name,
        "version": version,
        "format": "onnx",
        "description": description,
        "inputs": inputs,
        "outputs": outputs,
        # A memory per ant names an ant axis, and the probe has to bind it to something.
        "probe_dims": PROBE_DIMS | ({"N": 8} if akind else {}),
    }


def dumps(name: str = "tb.baseline", version: str = "1", memory: bool | str = False,
          ants: bool | str = False) -> str:
    """The exact bytes. Sorted and compact, so the same spec always hashes the same."""
    return json.dumps(manifest(name, version, memory, ants), separators=(",", ":"), sort_keys=True)


def main() -> None:
    """`python -m tb_baselines.adapters [NAME] [--memory[=KIND]] [--ants[=KIND]]`"""
    memory: bool | str = False
    ants: bool | str = False
    args = []
    for a in sys.argv[1:]:
        if a.startswith("--memory"):
            memory = a.split("=", 1)[1] if "=" in a else True
        elif a.startswith("--ants"):
            ants = a.split("=", 1)[1] if "=" in a else True
        else:
            args.append(a)
    sys.stdout.write(dumps(args[0] if args else "tb.baseline", memory=memory, ants=ants))


if __name__ == "__main__":
    main()
