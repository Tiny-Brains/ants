"""Torch to a submittable artifact: ONNX, fp16 initializers, and the platform's own verdict.

The last step is not a check this repository invents. It shells out to `tinybrains check --json`,
which reads the graph the way admission reads it and runs the manifest on the evaluator a node runs
it on — so "does it fit its class" is answered against the same measurements the platform will make,
over the cartridge's own reference observations. What this module adds is the loop around that: build, measure, and refuse an artifact
that misses its class.

    python -m tb_baselines.export --class micro --weights runs/micro-bc/best.pt --out models/micro-bc

Every export writes three files beside the model: `manifest.json` (generated, never edited),
`metrics.json` (what the platform said), and `card.md` (what a person needs to reproduce it). The
card names the **engine digest**, because a baseline that cannot say which engine it was trained
against is a baseline nobody can reproduce. (The evaluator is datalogic, and `tinybrains games`
prints the version this binary links.)
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import tomllib
import warnings
from pathlib import Path

import onnx
import torch
from onnx import numpy_helper

from . import adapters, nets
from .planes import ANT_DTYPE, ANT_MEMORY, MEMORY, N_MEMORY, N_PLANES

ROOT = Path(__file__).resolve().parents[2]
OPSET = 17  # inside the deployment's 13-19, and what the reference fixtures use


# ---- the class table --------------------------------------------------------------------

def classes() -> dict:
    with open(ROOT / "classes.toml", "rb") as f:
        doc = tomllib.load(f)
    return {c["name"]: c for c in doc["class"]} | {"_": doc}


def budget(name: str) -> dict:
    """What one class allows, worked out from `classes.toml` rather than written down twice."""
    table = classes()
    spec = table[name]
    doc = table["_"]
    per_param = doc["metric"][f"{doc['dtype']['default']}_bytes_per_param"]
    # A class may set its own fill: `small` cannot reach the global 70% inside the turn deadline,
    # and pretending otherwise would make every small artifact fail certification for a reason that
    # is not the artifact's fault.
    fraction = spec.get("fraction", doc["target"]["fraction"])
    target = int(spec["max_bytes"] * fraction)
    weights = target - doc["metric"]["manifest_bytes"]
    c = doc["compute"]
    return {
        "name": name,
        "max_bytes": spec["max_bytes"],
        "fraction": fraction,
        "target_bytes": target,
        "weight_bytes": weights,
        "max_share": doc["target"]["max_share"],
        "share_us": c["turn_ms"] * 1000 / c["rows_per_call"],
        "params_at_target": int(weights / per_param),
        "flops_per_seat_turn": c["turn_ms"] / 1000 / c["rows_per_call"] * c["flops_per_second"],
        "spec": spec,
    }


# ---- torch to onnx ----------------------------------------------------------------------

def to_onnx(trunk: torch.nn.Module, path: Path, planes: int = N_PLANES) -> None:
    """The policy half only.

    H and W are dynamic because a season's boards come in many sizes, and the manifest declares
    them as the named axes `"H"` and `"W"` -- one admitted session then serves every board a season
    runs. The leading axis is dynamic too and the manifest pins it at 1: a match is two seats and
    each is its own `model_infer` call, so nothing batches, but leaving the axis symbolic in the
    graph costs nothing and keeps the artifact usable in a trainer that does.
    """
    trunk.eval()
    example = (torch.zeros(1, planes, 128, 128, dtype=torch.int8),)
    inputs, outputs = ["board"], ["policy"]
    axes = {"board": {0: "B", 2: "H", 3: "W"}, "policy": {0: "B", 2: "H", 3: "W"}}
    # A memory model has two ports each way, named as the generated manifest declares them. The
    # memory is `i8` like the board.
    if getattr(trunk, "memory_ports", False):
        example += (torch.zeros(1, N_MEMORY, 128, 128, dtype=torch.int8),)
        inputs, outputs = inputs + ["memory_in"], outputs + ["memory"]
        axes |= {"memory_in": {0: "B", 2: "H", 3: "W"}, "memory": {0: "B", 2: "H", 3: "W"}}
    # A memory per ant adds the adapter's three inputs and one row an ant out; the ant axis is
    # dynamic, since a colony is any size.
    if getattr(trunk, "ant_ports", False):
        n = 8
        k = ANT_MEMORY
        example += (torch.zeros(1, k, 128, 128, dtype=torch.uint8),
                    torch.arange(n, dtype=torch.int64).reshape(1, n),
                    torch.arange(n, dtype=torch.int64).reshape(1, n) * 129)
        inputs, outputs = inputs + ["ant_planes", "ids", "cells"], outputs + ["ant_memory"]
        axes |= {"ant_planes": {0: "B", 2: "H", 3: "W"}, "ids": {0: "B", 1: "N"},
                 "cells": {0: "B", 1: "N"}, "ant_memory": {0: "B", 1: "N"}}
    # The TorchScript exporter is deprecated in favour of dynamo, which does not yet produce the
    # dynamic H/W axes this graph needs on every board size. Pinned deliberately; revisit when it
    # does.
    warnings.filterwarnings("ignore", category=DeprecationWarning, module="torch.onnx")
    torch.onnx.export(
        trunk, example, str(path),
        input_names=inputs, output_names=outputs,
        dynamic_axes=axes,
        opset_version=OPSET, dynamo=False,
    )


def halve(path: Path) -> None:
    """Rewrite every float32 initializer as float16, with a `Cast` back at its use.

    The size metric is the artifact's raw bytes, so this halves what the class is measured on --
    and more directly than it used to, when the metric compressed the initializer data first. The
    graph's compute dtype is untouched: the runtime constant-folds `Cast(initializer)` as it
    optimises the graph, so the fp32 tensor is rebuilt once at session load and never per inference.

    Measured: 2.03x the parameters for the same class, no measurable change in
    inference time, the same three operators, and 396 of 396 per-ant orders identical to the fp32
    graph over three played matches.
    """
    model = onnx.load(str(path))
    g = model.graph
    casts, kept = [], []
    for init in g.initializer:
        array = numpy_helper.to_array(init)
        if array.dtype != "float32":
            kept.append(init)
            continue
        kept.append(numpy_helper.from_array(array.astype("float16"), init.name + "_h"))
        casts.append(
            onnx.helper.make_node(
                "Cast", [init.name + "_h"], [init.name],
                to=onnx.TensorProto.FLOAT, name="cast_" + init.name,
            )
        )
    if not casts:
        return
    del g.initializer[:]
    g.initializer.extend(kept)
    # ONNX requires topological order and a Cast must precede its consumer, so they all go first.
    rest = list(g.node)
    del g.node[:]
    g.node.extend(casts + rest)
    onnx.checker.check_model(model)
    onnx.save(model, str(path))


def shorten(path: Path) -> None:
    """Drop every node's name and rename every intermediate tensor to a short id.

    A name is bytes the size metric weighs and the runtime never reads: the exporter writes
    `/trunk/stage/convs.0/Conv_output_0` for a tensor that could be `t7`, and a node's name is
    optional altogether. The graph's inputs and outputs keep their names, since the manifest
    declares them; an initializer keeps its own, since the `Cast` `halve` added reads it. On a
    graph of a few hundred nodes this is tens of kilobytes, which at nano is most of a class.
    """
    model = onnx.load(str(path))
    g = model.graph
    fixed = {i.name for i in g.input} | {o.name for o in g.output} | {t.name for t in g.initializer}
    names: dict[str, str] = {}

    def short(name: str) -> str:
        if not name or name in fixed:
            return name
        if name not in names:
            names[name] = f"t{len(names)}"
        return names[name]

    # One `Constant` node per distinct value: the exporter writes a fresh one for every slice's
    # starts, ends, axes and steps, and a graph of a few hundred nodes carries hundreds of copies
    # of `[0]`, `[1]` and `[-1]`.
    seen: dict[bytes, str] = {}
    alias: dict[str, str] = {}
    kept = []
    for node in g.node:
        if node.op_type == "Constant" and len(node.output) == 1:
            key = node.attribute[0].SerializeToString()
            if key in seen:
                alias[node.output[0]] = seen[key]
                continue
            seen[key] = node.output[0]
        kept.append(node)
    del g.node[:]
    g.node.extend(kept)
    for node in g.node:
        node.name = ""
        for i, name in enumerate(node.input):
            node.input[i] = short(alias.get(name, name))
        for i, name in enumerate(node.output):
            node.output[i] = short(name)
    for vi in list(g.value_info):
        vi.name = short(vi.name)
    onnx.checker.check_model(model)
    onnx.save(model, str(path))


# ---- the platform's verdict -------------------------------------------------------------

def cli() -> str:
    found = os.environ.get("TINYBRAINS") or shutil.which("tinybrains")
    if not found:
        raise SystemExit(
            "no `tinybrains` on PATH.\n"
            "  brew tap tiny-brains/cli https://github.com/Tiny-Brains/cli\n"
            "  brew install tiny-brains/cli/tinybrains   (or set TINYBRAINS to the binary)"
        )
    return found


def verdict(model: Path, manifest: Path) -> dict:
    """`tinybrains check --json`: the node's own evaluator and runtime, over the reference set."""
    r = subprocess.run(
        [cli(), "check", str(model), str(manifest), "--json"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if not r.stdout.strip():
        raise SystemExit(f"tinybrains check produced nothing:\n{r.stderr}")
    return json.loads(r.stdout)


def certify(name: str, said: dict) -> list[str]:
    """Everything wrong with this artifact, in the order a person would want to hear it."""
    b = budget(name)
    size = said["size_metric_bytes"]
    problems = []

    if not said["ok"]:
        problems.append(
            f"the platform refused it: {said.get('reason') or 'unstated'}".strip()
        )
    if size > b["max_bytes"]:
        problems.append(
            f"S' is {size:,} bytes and {name} caps at {b['max_bytes']:,} -- over by "
            f"{size - b['max_bytes']:,}"
        )
    # The deadline, made a check rather than a paragraph. Above `mini` this is what refuses an
    # artifact, and the byte cap never gets a say.
    #
    # A SEAT'S SHARE IS THE WHOLE TURN: one `model_infer` per seat, each with its own deadline, so
    # there is no shared call to divide. `budget()` keeps the
    # fraction this repository holds itself to, which is a self-imposed margin and not the
    # platform's rule.
    share = said["infer_us_max"] / b["share_us"]
    if share > b["max_share"]:
        problems.append(
            f"it spends {share:.0%} of a seat's {b['share_us'] / 1000:.1f} ms deadline share at the "
            f"worst reference board, over the {b['max_share']:.0%} this repository allows. The class "
            f"has bytes left; the turn does not."
        )
    # A memory is priced from the manifest and judged against a season's class, which this
    # repository does not know, so `check` reports the price without a verdict. What it does judge
    # is the round trip: fed its own memory, the graph has to answer.
    mem = said.get("memory")
    if mem:
        if mem.get("verdict"):
            problems.append(f"the memory is refused: {mem['verdict']} -- {mem.get('reason')}")
        rt = mem.get("round_trip") or {}
        if rt.get("failed"):
            problems.append(
                f"fed its own memory, the graph failed {rt['failed']} of {rt['checked']} chained "
                f"calls (case {rt.get('failing_case')}: {rt.get('reason')})"
            )
    return problems


def report(name: str, said: dict) -> dict:
    b = budget(name)
    g = said["graph"]
    return {
        "class": name,
        # S' = artifact_bytes + len(manifest). Both terms are what a node measures
        # against a digest it re-hashes, so neither can be understated by where the weights sit --
        # which the old zstd-of-initializers metric could be, and was.
        "size_metric_bytes": said["size_metric_bytes"],
        "class_max_bytes": b["max_bytes"],
        "class_target_bytes": b["target_bytes"],
        "fill_of_cap": round(said["size_metric_bytes"] / b["max_bytes"], 4),
        "artifact_bytes": said["artifact_bytes"],
        "manifest_bytes": said["manifest_bytes"],
        "params": g["parameters"],
        "nodes": g["nodes"],
        "opset": g["opset"],
        "operators": g["operators"],
        "adapter_ops_max": said["ops_max"],
        "adapter_ops_budget": said["budget_ops"],
        "infer_us_max": said["infer_us_max"],
        "deadline_share_ms": round(b["share_us"] / 1000, 2),
        "share_used": round(said["infer_us_max"] / b["share_us"], 4),
        "target_fraction": b["fraction"],
        "engine_digest": said["engine_digest"],
        "weights_hash": said["weights_hash"],
        "manifest_hash": said["manifest_hash"],
        # What the memory costs, from the manifest alone, when the model declares one: fixed bytes,
        # bytes a cell, and the total at the smallest and the largest board of the envelope.
        "memory": said.get("memory"),
    }


CARD = """# {name}

{summary}

| | |
|---|---|
| Weight class | **{cls}** — {size:,} of {cap:,} bytes ({fill:.0%} of the cap) |
| Parameters | {params:,} ({dtype} initializers) |
| Architecture | `{arch}`, receptive field **{reach} cells** each way{dil} |
| Method | {method} |
| Adapter | {ops:,} of {opsmax:,} operations at its worst reference case |
| Inference | {infer:.2f} ms at the worst reference case — **{shareuse:.1%}** of the {share:.0f} ms a seat gets |
| Operators | {operators} |
| Engine | `{engine}` |
| Model hash | `{whash}` |
| Manifest hash | `{ahash}` |
{memory}
{notes}

Reproduce with:

```sh
{repro}
```

Inference time is measured on whatever machine ran the check and is **reported, never a gate**:
there is no compute cap. It is here because the turn deadline is what a graph too expensive to
play runs into, and a seat's share of it is the WHOLE turn -- one `model_infer` call per seat,
each with its own deadline.
"""


def memory_row(metrics: dict) -> str:
    """What the model carries between turns, as the card's Memory row: the board memory, the memory
    per ant, or both, and the price `check` read off the manifest for all of it."""
    mem = metrics.get("memory")
    if not mem:
        return ""
    kind, ants = metrics.get("memory_kind"), metrics.get("ant_memory")
    learned = {"learned": "the graph's own, learned"}
    carries = []
    if kind or not ants:
        what = learned.get(kind, ", ".join(m.name for m in MEMORY))
        carries.append(f"`{N_MEMORY}` planes of `i8` ({what})")
    if ants:
        carries.append(f"a row of `{ANT_MEMORY}` `{ANT_DTYPE}` values per ant, kept by id "
                       f"({learned.get(ants, ants)})")
    # A memory `check` could not price -- a shape it refuses (`MEMORY_SHAPE`), or a release
    # declaring no `limits.boards` -- is still an object, with nulls where the numbers go
    # (`cli/src/cmd/check.rs`, `MemoryReport::json`). The card says what it knows and names the
    # gap: `certify` is what turns a refusal into the message that stops the export, and
    # `write_card` runs first, so a format crash here would swallow it.
    cell, most = mem.get("cell_bytes"), mem.get("bytes_at_max")
    rt = mem.get("round_trip") or {}
    priced = (f"{cell} bytes a cell: {most:,} bytes on the largest board"
              if cell is not None and most is not None else "which `check` could not price")
    judged = (f"`check` judged the round trip over {rt['checked']} chained observations"
              if rt.get("checked") is not None else "`check` did not run the round trip")
    return f"| Memory | {' and '.join(carries)}, {priced}. The season's class has to allow it; {judged} |\n"


def write_card(out: Path, name: str, method: str, metrics: dict, summary: str,
               notes: str, repro: str) -> None:
    (out / "card.md").write_text(CARD.format(
        memory=memory_row(metrics),
        name=name, summary=summary, cls=metrics["class"],
        size=metrics["size_metric_bytes"], cap=metrics["class_max_bytes"],
        fill=metrics["fill_of_cap"], params=metrics["params"],
        dtype=classes()["_"]["dtype"]["default"], method=method,
        arch=metrics.get("arch", "?"), reach=metrics.get("reach_cells", 0),
        dil=(f" (dilations {metrics['dilations'][0]})"
             if metrics.get("dilations") else ""),
        ops=metrics["adapter_ops_max"], opsmax=metrics["adapter_ops_budget"],
        infer=metrics["infer_us_max"] / 1000, share=metrics["deadline_share_ms"],
        shareuse=metrics["share_used"],
        operators=", ".join(metrics["operators"]), engine=metrics["engine_digest"],
        whash=metrics["weights_hash"], ahash=metrics["manifest_hash"],
        notes=notes, repro=repro,
    ))


def shape_of(trunk: torch.nn.Module) -> dict:
    """The architecture, as the card and metrics record it.

    **A stale artifact directory is self-consistent**: its model, adapter and metrics all agree with
    each other and every hash check passes, so nothing notices that it was built by a version of
    this repository that no longer exists. It nearly shipped one -- a `nano-bc` from before the
    receptive-field fix, sitting beside a `micro-bc` from after it. Writing the architecture down is
    what makes that visible at a glance rather than by counting parameters.
    """
    stages = [m for m in trunk.modules() if type(m).__name__ == "Stage"]
    inner = getattr(trunk, "trunk", trunk)
    kind = getattr(trunk, "memory_kind", None)
    ants = getattr(trunk, "ant_kind", None) if getattr(trunk, "ant_ports", False) else None
    return {
        "arch": type(inner).__name__ + (f" + {kind} memory" if kind else "")
                + ({"learned": " + a memory per ant"}.get(ants, "")),
        "memory_kind": kind,
        "ant_memory": ants,
        "reach_cells": max((st.reach for st in stages), default=0),
        "dilations": [st.dilations for st in stages] or None,
    }


def command(module: str, argv: list[str]) -> str:
    """A command line as a person would type it from this directory."""
    return shlex.join(["python", "-m", module, *argv])


def recipe(weights: Path | None, argv: list[str]) -> str:
    """The commands that made an artifact, one a line, for the card's Reproduce block.

    Each step writes down its own command line (`collect` in the dataset's `.stats.json`, a
    trainer in the run's `history.json`), so this reads them back rather than guessing flags a run
    may have overridden: the dataset, the training run, and this export. A run from before the
    trainers recorded it contributes nothing, and the card then names the export alone.
    """
    steps = []
    history = weights.parent / "history.json" if weights else None
    if history and history.exists():
        h = json.loads(history.read_text())
        if h.get("data"):
            stats = Path(h["data"]).with_suffix("").with_suffix(".stats.json")
            if stats.exists() and json.loads(stats.read_text()).get("command"):
                steps.append(json.loads(stats.read_text())["command"])
        if h.get("command"):
            steps.append(h["command"])
    steps.append(command("tb_baselines.export", argv))
    return "\n".join(steps)


def export(trunk: torch.nn.Module, name: str, out: Path, method: str, repro: str,
           summary: str = "", notes: str = "") -> dict:
    """The whole pipeline. Raises if the artifact misses its class.

    `repro` is the card's Reproduce block: the commands that make this artifact again.
    """
    out.mkdir(parents=True, exist_ok=True)
    model_path, manifest_path = out / "model.onnx", out / "manifest.json"
    manifest_path.write_text(adapters.dumps(
        f"tb.{out.name}",
        memory=getattr(trunk, "memory_kind", None) or getattr(trunk, "memory_ports", False),
        ants=getattr(trunk, "ant_kind", None) or getattr(trunk, "ant_ports", False)))
    to_onnx(trunk, model_path)
    if classes()["_"]["dtype"]["default"] == "fp16":
        halve(model_path)
    shorten(model_path)

    said = verdict(model_path, manifest_path)
    problems = certify(name, said)
    metrics = report(name, said) | shape_of(trunk)
    (out / "metrics.json").write_text(json.dumps(metrics, indent=2) + "\n")
    write_card(out, out.name, method, metrics,
               summary or f"A {name}-class Ants policy.", notes, repro)

    if problems:
        raise SystemExit(
            f"{out}: this artifact does not belong in {name}\n  "
            + "\n  ".join(problems)
        )
    return metrics


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--class", dest="cls", required=True)
    # A variant is a command line, not a second table: the run's history.json and the model card
    # both record what was actually built, which is the copy that matters.
    ap.add_argument("--arch", default=None, help="override the class's architecture")
    ap.add_argument("--channels", type=int, default=None)
    ap.add_argument("--blocks", type=int, default=None)
    ap.add_argument("--weights", type=Path, help="a .pt state dict; omitted means random init")
    ap.add_argument("--memory", nargs="?", const="max", default=False,
                    help="a model that carries a memory: `max` (planes.MEMORY) or `learned`; "
                         "read off the weights when they say so")
    ap.add_argument("--ants", nargs="?", const="learned", default=False,
                    help="a model that carries a row per ant too: `learned`; read off "
                         "the weights likewise")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--method", default="untrained")
    ap.add_argument("--repro", default=None,
                    help="the card's Reproduce block (default: the dataset's, the run's and this "
                         "export's own recorded command lines)")
    argv = sys.argv[1:] if argv is None else argv
    a = ap.parse_args(argv)

    from .train.bc import override
    spec = override(classes()[a.cls], a)
    state = torch.load(a.weights, map_location="cpu") if a.weights else {}
    memory = a.memory or state.get("memory", False)
    ants = a.ants or state.get("ants", False)
    trunk = nets.build(spec, memory=memory, ants=ants)
    if a.weights:
        trunk.load_state_dict(state.get("trunk", state))

    b = budget(a.cls)
    print(f"{a.cls}: {nets.policy_params(trunk):,} parameters, "
          f"budget about {b['params_at_target']:,} at {b['target_bytes']:,} bytes")
    m = export(trunk, a.cls, a.out, a.method, a.repro or recipe(a.weights, argv))
    print(f"  S = {m['size_metric_bytes']:,} bytes, {m['fill_of_cap']:.0%} of the {a.cls} cap "
          f"(aiming at {m['target_fraction']:.0%})")
    print(f"  adapter {m['adapter_ops_max']:,} ops, inference {m['infer_us_max'] / 1000:.2f} ms "
          f"= {m['share_used']:.0%} of a seat share")
    if m.get("memory"):
        mem = m["memory"]
        rt = mem.get("round_trip") or {}
        # Nulls where the numbers go when `check` could not price it; see `memory_row`.
        if mem.get("cell_bytes") is not None and mem.get("bytes_at_max") is not None:
            print(f"  memory {mem['cell_bytes']} bytes a cell, {mem['bytes_at_max']:,} bytes "
                  f"on the largest board")
        else:
            print("  memory declared, which `check` could not price")
        if rt.get("checked") is not None:
            print(f"  round trip {rt['failed']} of {rt['checked']} chained calls failed")
    print(f"  -> {a.out}")


if __name__ == "__main__":
    main(sys.argv[1:])
