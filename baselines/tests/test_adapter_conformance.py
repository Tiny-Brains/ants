"""The trainer's encoder and the ladder's adapter must produce the same tensor. Byte for byte.

**This is the most important test in the repository.** Everything else here is a training script
that can be wrong in ways a metric will show you. This one guards a failure that shows you nothing:
an encoder that disagrees with the adapter trains a model on a distribution the arena never serves,
and the only symptom is a rating lower than the training curve promised.

It is not a re-implementation checking itself. `tinybrains adapt` evaluates the manifest through
**datalogic** — the evaluator an Orion node compiles adapters on, at the version the fleet runs —
over the cartridge's own reference observations, which is the only set on which agreement decides
anything. If this passes, `planes.encode` and `manifest.json` are the same function on the inputs
the platform actually validates against.

    pytest tests/ -q                    # needs `tinybrains` on PATH, or TINYBRAINS set

The memory (`planes.MEMORY`) is held to the same standard, in three parts: the `memory_in` adapter
against `planes.memory_in_view` through `tinybrains adapt --obs` (a CLI that carries memory), the
numpy `remember` against the `Max` the graph takes, and, where torch is installed, the graph itself.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import numpy as np
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from tb_baselines import adapters, planes  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
CLI = os.environ.get("TINYBRAINS") or shutil.which("tinybrains")


@pytest.fixture(scope="session")
def dumped(tmp_path_factory):
    """The ladder's own answer: the manifest's adapters through datalogic -- the evaluator an Orion
    node runs them on -- over the cartridge's reference observations.

    `adapt` loads the graph too, so a manifest whose declared shapes the graph refuses fails here
    rather than at admission. Any trained model serves: the encoding is the manifest's, not the
    weights'. This repository commits none -- the platform's trained models live in
    ants-starter -- so the graph is `$TB_CONFORMANCE_ONNX`, else the starter's micro-bc beside this
    checkout. Named by the variable and missing is a FAILURE, not a skip: CI names it, and a gate
    that skips in CI is no gate."""
    if not CLI:
        pytest.skip("no `tinybrains` on PATH; set TINYBRAINS to the binary")
    named = os.environ.get("TB_CONFORMANCE_ONNX")
    onnx = Path(named) if named else ROOT.parents[1] / "ants-starter" / "models" / "micro-bc" / "model.onnx"
    if not onnx.exists():
        if named:
            pytest.fail(f"TB_CONFORMANCE_ONNX names {onnx}, which does not exist")
        pytest.skip(f"no trained graph to load the manifest against: set TB_CONFORMANCE_ONNX, or check out ants-starter beside ants ({onnx})")
    out = tmp_path_factory.mktemp("tensors")
    manifest = out / "manifest.json"
    manifest.write_text(adapters.dumps())
    r = subprocess.run(
        [CLI, "adapt", str(onnx), str(manifest), "--out", str(out)],
        cwd=ROOT, capture_output=True, text=True,
    )
    assert r.returncode == 0, f"tinybrains adapt failed:\n{r.stdout}\n{r.stderr}"
    return json.loads((out / "index.json").read_text()), out


def test_every_reference_observation_encodes_identically(dumped):
    index, out = dumped
    assert index["cases"], "the cartridge shipped no reference observations"

    for case in index["cases"]:
        i = case["case"]
        obs = json.loads((out / f"case-{i}" / "observation.json").read_text())
        theirs = np.load(out / f"case-{i}" / "board.npy")
        ours = planes.encode(obs)

        assert ours.shape == theirs.shape, (
            f"case {i}: the trainer builds {ours.shape}, the adapter builds {theirs.shape}"
        )
        assert ours.dtype == theirs.dtype, f"case {i}: {ours.dtype} vs {theirs.dtype}"

        if not np.array_equal(ours, theirs):
            # Name the plane rather than the cell. A whole plane is almost always what is wrong,
            # and "plane 4 (hill_mine) differs in 2 cells" is a diagnosis where a flat index is a
            # puzzle.
            bad = [
                f"{k} ({planes.PLANES[k].name}): {int((ours[0, k] != theirs[0, k]).sum())} cells"
                for k in range(planes.N_PLANES)
                if not np.array_equal(ours[0, k], theirs[0, k])
            ]
            pytest.fail(f"case {i} disagrees on plane(s) -- " + "; ".join(bad))


def test_the_adapter_fits_the_operation_budget(dumped):
    index, _ = dumped
    budget = index["budget_ops"]
    worst = max(c["ops_in"] for c in index["cases"])
    # The worst case is what admission refuses, so the mean is not the number to watch. A comfortable
    # margin, not a passing grade: an observation busier than any of these still has to fit.
    assert worst < budget * 0.6, (
        f"the adapter costs {worst} of {budget} at its worst -- too little headroom for a "
        f"board busier than the reference set"
    )


def test_the_generated_manifest_is_byte_stable():
    """The platform hashes these bytes -- and weighs them, since `S'` is
    `artifact_bytes + len(manifest)` -- so the same spec must always produce the same document."""
    assert adapters.dumps() == adapters.dumps()
    doc = json.loads(adapters.dumps())
    assert doc["abi"] == adapters.ABI
    assert [i["name"] for i in doc["inputs"]] == ["board"], "the graph declares one input"
    assert [o["name"] for o in doc["outputs"]] == ["policy"]
    assert "result" not in doc, "the platform reads the head; a manifest may not decode it"
    # The axes are NAMED, which is what lets one session serve every board the season runs.
    assert doc["inputs"][0]["shape"][2:] == ["H", "W"]
    assert doc["outputs"][0]["shape"][2:] == ["H", "W"]
    assert doc["probe_dims"] == adapters.PROBE_DIMS


def test_the_encoder_handles_an_empty_colony():
    """Zero ants is always valid (*What your model sees*) and is what a wiped-out seat sends
    right up until its match ends. An encoder that indexes into an empty list dies there."""
    obs = {
        "size": [64, 96],
        "mine": [], "foes": [], "food": [], "hills": [],
        "water": {"rle": [0, 64 * 96]},
        # No ants, no vision: the engine sends an all-zero mask, which is what `ants` asserts in
        # `the_view_carries_the_mask_it_filtered_through`.
        "vis": {"rle": [0, 64 * 96]},
    }
    board = planes.encode(obs)
    assert board.shape == (1, planes.N_PLANES, 64, 96)
    assert board.sum() == 0, "nothing seen, nothing set -- including the visibility mask"


def test_the_action_table_is_the_channel_order_the_platform_decodes():
    """Channel `i` of the policy head means `MOVES[i]`.

    That index is what the cross-entropy label uses (`train/bc.py`'s `MOVE_INDEX`) and what
    `orders_from_indices` writes back. **The platform closes the loop, not the manifest**:
    `kalam-match` argmaxes the channels and indexes its own table, which is
    `["N","E","S","W","-"]` in `kalam/scripts/gen-kalam.py` and `cli/src/model.rs`. So this
    table is a contract between the trainer and the platform, with no adapter in between — and if
    the two disagreed, every move would be systematically wrong while the model, the loss, the
    replay and the match all continued to work. Nothing else would notice.
    """
    assert list(planes.MOVES) == ["N", "E", "S", "W", "-"], (
        "the platform's decode table is fixed in kalam and the CLI; a reorder here is a silent "
        "relabelling of every move the ladder plays"
    )
    assert planes.N_MOVES == json.loads(adapters.dumps())["outputs"][0]["shape"][1], (
        "the manifest declares a head whose channel count is not the move count"
    )


def test_the_manifest_reads_the_output_the_export_names():
    """`export.to_onnx` names the graph's output `policy`, and the manifest declares an output of
    that name. A rename on one side is a refusal at admission naming a tensor the graph does not
    have, which is a clear failure — but only if someone runs admission, and this is cheaper."""
    doc = json.loads(adapters.dumps())
    assert [o["name"] for o in doc["outputs"]] == ["policy"]


def test_the_visibility_plane_reads_the_engines_mask():
    """`vis` is SENT by the engine, not derived.

    The old plane was `tb.dilate(scatter(mine), 77)` and existed because a model cannot tell *known
    empty* from *never seen* without it. The expression language cannot address an enclosing
    iterator's element, so the per-ant disk was a 241-fold unrolled kernel or nothing — and the
    radius is a rule of the game, which the cartridge owns. This asserts the encoder reads the
    engine's mask rather than rebuilding one."""
    doc = json.loads(adapters.dumps())
    text = json.dumps(doc)
    assert "vis.rle" in text, "the visibility plane must read the observation's `vis`"
    assert "dilate" not in text, "the dilate operator is withdrawn; the engine sends the mask"


def test_the_engines_mask_is_the_disk_it_claims_to_be(dumped):
    """The independent second opinion, and the reason trusting a sent mask is safe.

    `Board.dilate` is kept for exactly this: it computes the disk union from `mine` the way the
    trainer always did, and the engine's `vis` must equal it on every reference observation. If the
    cartridge ever changes what it means by visible, this fails here rather than in a rating."""
    index, out = dumped
    for case in index["cases"]:
        i = case["case"]
        obs = json.loads((out / f"case-{i}" / "observation.json").read_text())
        rows, cols = obs["size"]
        b = planes.Board(rows, cols)
        sent = b.rle(obs["vis"]["rle"])
        derived = b.dilate(b.scatter(obs["mine"]), planes.VIEW_RADIUS2)
        assert np.array_equal(sent, derived), (
            f"case {i}: the engine's `vis` is not the radius-{planes.VIEW_RADIUS2} disk union of "
            f"`mine` -- {int((sent != derived).sum())} cells differ"
        )


# ---- the memory -------------------------------------------------------------------------

def cases_of(dumped) -> list[dict]:
    index, out = dumped
    return [json.loads((out / f"case-{c['case']}" / "observation.json").read_text())
            for c in index["cases"]]


def chained(observations: list[dict], per_size: int = 12):
    """`(state before, observation)` pairs, chained per board size the way a match chains them: a
    memory is a board's, so a seat's state never crosses a size."""
    by_size: dict[tuple[int, int], list[dict]] = {}
    for o in observations:
        by_size.setdefault(tuple(o["size"]), []).append(o)
    for group in by_size.values():
        state = None
        for o in group[:per_size]:
            yield state, o
            state = planes.remember(state, o)


def test_the_memory_manifest_declares_the_two_ports_beside_the_board():
    """`memory_in` is the second input and `memory` the second output, both `i8` at
    `[1, N_MEMORY, H, W]`; the board-only manifest is untouched by the option."""
    doc = json.loads(adapters.dumps(memory=True))
    assert [i["name"] for i in doc["inputs"]] == ["board", "memory_in"]
    assert [o["name"] for o in doc["outputs"]] == ["policy", "memory"]
    for port in (doc["inputs"][1], doc["outputs"][1]):
        assert port["dtype"] == planes.DTYPE
        assert port["shape"] == [1, planes.N_MEMORY, "H", "W"]
    text = json.dumps(doc["inputs"][1]["adapter"])
    assert '{"tensor": [{"var": "memory"}]}' in text, "the memory passes through as the tensor it is"
    assert '"zeros"' in text, "turn 0 has no memory, so the adapter builds zeros"
    assert adapters.dumps() == adapters.dumps(memory=False)
    assert json.loads(adapters.dumps())["inputs"][0] == doc["inputs"][0], "the board adapter is the same"


def test_remember_is_the_max_the_graph_takes(dumped):
    """The trainer's update (`remember`, a union of cells) and the graph's (`Max` over the memory
    and the source planes) must be one function. On 0/1 planes they are, and this says so on the
    reference observations, chained per board size; turn 0 is all zeros."""
    for state, obs in chained(cases_of(dumped)):
        size = obs["size"]
        before = planes.encode_memory(state, size)
        if state is None:
            assert before.sum() == 0 and before.dtype == planes.NP_DTYPE
        after = planes.encode_memory(planes.remember(state, obs), size)
        sources = planes.encode(obs)[:, list(planes.MEMORY_SOURCES)]
        assert after.shape == (1, planes.N_MEMORY, *size) and after.dtype == planes.NP_DTYPE
        assert np.array_equal(after, np.maximum(before, sources)), f"remember is not Max at {size}"


@pytest.fixture(scope="session")
def memory_graph(tmp_path_factory) -> Path:
    """A graph with the memory manifest's four ports, for `adapt` to load the manifest against.

    `adapt` runs nothing through the graph, but it type-checks the manifest's declared shapes
    against it, so the ports have to exist. No trained memory model is committed anywhere, so this
    is a fourteen-line `onnx.helper` graph: the policy is five planes of the board, cast; the
    memory is its input, unchanged. What the test measures is the adapter, not the graph."""
    onnx = pytest.importorskip("onnx", reason="the memory adapter test builds its graph with onnx")
    from onnx import TensorProto as T, helper as h, numpy_helper as nh
    b, m = planes.N_PLANES, planes.N_MEMORY
    g = h.make_graph(
        [h.make_node("Cast", ["board"], ["bf"], to=T.FLOAT),
         h.make_node("Slice", ["bf", "s0", "e5", "ax1"], ["policy"]),
         h.make_node("Identity", ["memory_in"], ["memory"])],
        "memory-ports",
        [h.make_tensor_value_info("board", T.INT8, [1, b, "H", "W"]),
         h.make_tensor_value_info("memory_in", T.INT8, [1, m, "H", "W"])],
        [h.make_tensor_value_info("policy", T.FLOAT, [1, planes.N_MOVES, "H", "W"]),
         h.make_tensor_value_info("memory", T.INT8, [1, m, "H", "W"])],
        [nh.from_array(np.array([0], dtype=np.int64), "s0"),
         nh.from_array(np.array([planes.N_MOVES], dtype=np.int64), "e5"),
         nh.from_array(np.array([1], dtype=np.int64), "ax1")],
    )
    model = h.make_model(g, opset_imports=[h.make_opsetid("", 17)], ir_version=8)
    onnx.checker.check_model(model)
    path = tmp_path_factory.mktemp("graph") / "memory-ports.onnx"
    onnx.save(model, str(path))
    return path


def test_the_memory_adapter_hands_back_the_memory_it_is_given_and_zeros_without(dumped, memory_graph, tmp_path):
    """The ladder's own answer for `memory_in`: through datalogic, a view carrying a memory yields
    that memory as the tensor the graph will read, and a view without one yields zeros, and the
    board adapter is unchanged beside it. The observation file carries the memory as nested arrays,
    which `adapt` decodes into the dtype the manifest declares for `memory`, as the book's *Memory*
    page says."""
    views = []
    for i, (state, obs) in enumerate(chained(cases_of(dumped), per_size=3)):
        view = dict(obs)
        if i % 2:
            view["memory"] = planes.encode_memory(state or planes.remember(None, obs), obs["size"]).tolist()
        views.append(view)
    assert any("memory" in v for v in views) and any("memory" not in v for v in views)

    out = tmp_path / "tensors"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(adapters.dumps(memory=True))
    (tmp_path / "views.json").write_text(json.dumps({"observations": views}))
    r = subprocess.run(
        [CLI, "adapt", str(memory_graph), str(manifest), "--obs", str(tmp_path / "views.json"),
         "--out", str(out)],
        cwd=ROOT, capture_output=True, text=True,
    )
    assert r.returncode == 0, (
        "tinybrains adapt failed on a view carrying `memory`; a CLI without the memory carry "
        f"(before 0.3.0's successor) cannot run this test:\n{r.stdout}\n{r.stderr}"
    )
    index = json.loads((out / "index.json").read_text())
    assert len(index["cases"]) == len(views)
    for case in index["cases"]:
        i = case["case"]
        view = json.loads((out / f"case-{i}" / "observation.json").read_text())
        theirs = np.load(out / f"case-{i}" / "memory_in.npy")
        ours = planes.memory_in_view(view)
        assert theirs.shape == ours.shape and theirs.dtype == ours.dtype, (
            f"case {i}: the adapter builds {theirs.shape} {theirs.dtype}, the trainer {ours.shape} {ours.dtype}"
        )
        assert np.array_equal(theirs, ours), (
            f"case {i} ({'with' if 'memory' in view else 'without'} a memory): memory_in differs in "
            f"{int((theirs != ours).sum())} cells"
        )
        assert np.array_equal(np.load(out / f"case-{i}" / "board.npy"), planes.encode(view)), (
            f"case {i}: the board adapter changed beside the memory input"
        )


def test_the_graphs_memory_update_matches_the_encoder(dumped):
    """`nets.WithMemory` writes `Max(memory_in, board[sources])` as `i8`; the trainer's `remember`
    must produce the same planes for the next turn. Torch is a dev dependency and CI installs
    none, so CI relies on `test_remember_is_the_max_the_graph_takes`; run this before an export."""
    torch = pytest.importorskip("torch")
    from tb_baselines import nets
    net = nets.build({"arch": "trunk", "channels": 4, "blocks": 1}, memory=True).eval()
    for state, obs in chained(cases_of(dumped), per_size=6):
        size = obs["size"]
        mem_in = planes.encode_memory(state, size)
        with torch.no_grad():
            policy, mem_out = net(torch.from_numpy(planes.encode(obs)), torch.from_numpy(mem_in))
        assert policy.shape == (1, planes.N_MOVES, *size)
        assert mem_out.dtype == torch.int8
        want = planes.encode_memory(planes.remember(state, obs), size)
        assert np.array_equal(mem_out.numpy(), want), f"the graph and the encoder disagree at {size}"


# ---- a memory per ant -------------------------------------------------------------------

@pytest.fixture(scope="session")
def ant_graph(tmp_path_factory) -> Path:
    """A graph with the per-ant manifest's ports (`board`, `ant_planes`, `ids`, `cells` in;
    `policy`, `ant_memory` out), for `adapt` to type-check the manifest against. The row it
    writes is the id and two copies of the cell, which is enough for the ports; what the test
    measures is the adapter's join, not the graph."""
    onnx = pytest.importorskip("onnx", reason="the per-ant adapter test builds its graph with onnx")
    from onnx import TensorProto as T, helper as h, numpy_helper as nh
    b, k = planes.N_PLANES, planes.ANT_MEMORY
    g = h.make_graph(
        [h.make_node("Cast", ["board"], ["bf"], to=T.FLOAT),
         h.make_node("Slice", ["bf", "s0", "e5", "ax1"], ["policy"]),
         h.make_node("Cast", ["ids"], ["idu"], to=T.UINT8),
         h.make_node("Cast", ["cells"], ["cu"], to=T.UINT8),
         h.make_node("Unsqueeze", ["idu", "ax2"], ["id3"]),
         h.make_node("Unsqueeze", ["cu", "ax2"], ["c3"]),
         h.make_node("Concat", ["id3", "c3", "c3"], ["ant_memory"], axis=2)],
        "ant-ports",
        [h.make_tensor_value_info("board", T.INT8, [1, b, "H", "W"]),
         h.make_tensor_value_info("ant_planes", T.UINT8, [1, k, "H", "W"]),
         h.make_tensor_value_info("ids", T.INT64, [1, "N"]),
         h.make_tensor_value_info("cells", T.INT64, [1, "N"])],
        [h.make_tensor_value_info("policy", T.FLOAT, [1, planes.N_MOVES, "H", "W"]),
         h.make_tensor_value_info("ant_memory", T.UINT8, [1, "N", k + 1])],
        [nh.from_array(np.array([0], dtype=np.int64), "s0"),
         nh.from_array(np.array([planes.N_MOVES], dtype=np.int64), "e5"),
         nh.from_array(np.array([1], dtype=np.int64), "ax1"),
         nh.from_array(np.array([2], dtype=np.int64), "ax2")],
    )
    model = h.make_model(g, opset_imports=[h.make_opsetid("", 17)], ir_version=8)
    onnx.checker.check_model(model)
    path = tmp_path_factory.mktemp("graph") / "ant-ports.onnx"
    onnx.save(model, str(path))
    return path


def test_the_per_ant_manifest_declares_the_three_inputs_and_one_row_an_ant():
    doc = json.loads(adapters.dumps(ants=True))
    assert [i["name"] for i in doc["inputs"]] == ["board", "ant_planes", "ids", "cells"]
    assert [o["name"] for o in doc["outputs"]] == ["policy", "ant_memory"]
    assert doc["outputs"][1] == {"name": "ant_memory", "dtype": planes.ANT_DTYPE,
                                 "shape": [1, "N", planes.ANT_MEMORY + 1]}
    assert doc["probe_dims"]["N"], "the probe binds the ant axis"
    assert adapters.dumps() == adapters.dumps(ants=False), "the plain manifest is untouched"
    text = json.dumps(doc["inputs"][1]["adapter"])
    assert '"gather"' in text and '"scatter"' in text, "rows re-keyed by id, then written at cells"


def test_the_ant_join_reads_last_turns_rows_back_by_id_and_zeros_for_a_new_ant():
    """The numpy rendering alone, on a hand-made turn: last turn's ants were 7, 3 and 12, this
    turn's are 3, 12 and 20 (the book's example), so the rows come back as 3's, 12's and zeros."""
    prev = [[[7, 10, 11], [3, 30, 31], [12, 120, 121]]]
    obs = {"size": [8, 8], "mine": [[1, 1], [2, 5], [6, 3]], "ids": [3, 12, 20], "ant_memory": prev}
    rows = planes.ant_rows_in_view(obs)
    assert rows.tolist() == [[30, 31], [120, 121], [0, 0]] and rows.dtype == planes.ANT_NP_DTYPE
    p = planes.ant_planes_in_view(obs)
    assert p.shape == (1, planes.ANT_MEMORY, 8, 8)
    assert p[0, :, 1, 1].tolist() == [30, 31] and p[0, :, 2, 5].tolist() == [120, 121]
    assert p[0, :, 6, 3].tolist() == [0, 0] and int(p.sum()) == 30 + 31 + 120 + 121
    assert planes.ant_ids_in_view({"ids": [3, 300]}).tolist() == [[3, 300 % planes.ANT_TABLE]]
    assert planes.ant_cells_in_view(obs).tolist() == [[9, 21, 51]]


def test_the_per_ant_adapter_joins_by_id_as_the_trainer_does(dumped, ant_graph, tmp_path):
    """The ladder's own answer, through datalogic: views carrying last turn's `ant_memory` -- rows
    written for the ids of the previous observation on the same board size, so some of this
    turn's ants find a row and some are new -- yield the same `ant_planes`, `ids` and `cells` the
    trainer builds, and a view without one yields zeros."""
    views = []
    prev_ids: dict[tuple[int, int], list[int]] = {}
    for obs in cases_of(dumped):
        view = dict(obs)
        key = tuple(obs["size"])
        before = prev_ids.get(key)
        if before:
            # One row an ant of the last view on this size, valued by its id, and a row for an id
            # long dead, which nothing this turn should read.
            view["ant_memory"] = [[[i % planes.ANT_TABLE, (i * 7) % 256, (i * 13) % 256] for i in before]
                                  + [[199, 5, 6]]]
        views.append(view)
        prev_ids[key] = list(obs["ids"])
    assert any("ant_memory" in v for v in views) and any("ant_memory" not in v for v in views)

    out = tmp_path / "tensors"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(adapters.dumps(ants=True))
    (tmp_path / "views.json").write_text(json.dumps({"observations": views}))
    r = subprocess.run(
        [CLI, "adapt", str(ant_graph), str(manifest), "--obs", str(tmp_path / "views.json"),
         "--out", str(out)],
        cwd=ROOT, capture_output=True, text=True,
    )
    assert r.returncode == 0, f"tinybrains adapt failed on a view carrying `ant_memory`:\n{r.stdout}\n{r.stderr}"
    index = json.loads((out / "index.json").read_text())
    assert len(index["cases"]) == len(views)
    joined = 0
    for case in index["cases"]:
        i = case["case"]
        view = json.loads((out / f"case-{i}" / "observation.json").read_text())
        for name, ours in (("ant_planes", planes.ant_planes_in_view(view)),
                           ("ids", planes.ant_ids_in_view(view)),
                           ("cells", planes.ant_cells_in_view(view))):
            theirs = np.load(out / f"case-{i}" / f"{name}.npy")
            assert theirs.shape == ours.shape and theirs.dtype == ours.dtype, (
                f"case {i} {name}: the adapter builds {theirs.shape} {theirs.dtype}, the trainer {ours.shape} {ours.dtype}")
            assert np.array_equal(theirs, ours), f"case {i} {name}: {int((theirs != ours).sum())} elements differ"
        joined += int(planes.ant_planes_in_view(view).any())
        assert np.array_equal(np.load(out / f"case-{i}" / "board.npy"), planes.encode(view))
    assert joined, "at least one view read a row back through the join"


# ---- the 2011 winner's memory -------------------------------------------------------------

@pytest.fixture(scope="session")
def xathis_graph(tmp_path_factory) -> Path:
    """A graph with the xathis manifest's ports: `memory_in` and `memory` are `u8`, and with the
    mission ports beside them (`ant_planes` of three, `ant_memory` rows of four)."""
    onnx = pytest.importorskip("onnx", reason="the xathis adapter tests build their graph with onnx")
    from onnx import TensorProto as T, helper as h, numpy_helper as nh
    b, m, k = planes.N_PLANES, planes.XATHIS_MEMORY, planes.MISSION
    g = h.make_graph(
        [h.make_node("Cast", ["board"], ["bf"], to=T.FLOAT),
         h.make_node("Slice", ["bf", "s0", "e5", "ax1"], ["policy"]),
         h.make_node("Identity", ["memory_in"], ["memory"]),
         h.make_node("Cast", ["ids"], ["idu"], to=T.UINT8),
         h.make_node("Cast", ["cells"], ["cu"], to=T.UINT8),
         h.make_node("Unsqueeze", ["idu", "ax2"], ["id3"]),
         h.make_node("Unsqueeze", ["cu", "ax2"], ["c3"]),
         h.make_node("Concat", ["id3", "c3", "c3", "c3"], ["ant_memory"], axis=2)],
        "xathis-ports",
        [h.make_tensor_value_info("board", T.INT8, [1, b, "H", "W"]),
         h.make_tensor_value_info("memory_in", T.UINT8, [1, m, "H", "W"]),
         h.make_tensor_value_info("ant_planes", T.UINT8, [1, k, "H", "W"]),
         h.make_tensor_value_info("ids", T.INT64, [1, "N"]),
         h.make_tensor_value_info("cells", T.INT64, [1, "N"])],
        [h.make_tensor_value_info("policy", T.FLOAT, [1, planes.N_MOVES, "H", "W"]),
         h.make_tensor_value_info("memory", T.UINT8, [1, m, "H", "W"]),
         h.make_tensor_value_info("ant_memory", T.UINT8, [1, "N", k + 1])],
        [nh.from_array(np.array([0], dtype=np.int64), "s0"),
         nh.from_array(np.array([planes.N_MOVES], dtype=np.int64), "e5"),
         nh.from_array(np.array([1], dtype=np.int64), "ax1"),
         nh.from_array(np.array([2], dtype=np.int64), "ax2")],
    )
    model = h.make_model(g, opset_imports=[h.make_opsetid("", 17)], ir_version=8)
    onnx.checker.check_model(model)
    path = tmp_path_factory.mktemp("graph") / "xathis-ports.onnx"
    onnx.save(model, str(path))
    return path


def test_the_xathis_manifest_declares_u8_memory_and_the_mission_rows():
    doc = json.loads(adapters.dumps(memory="xathis", ants="mission"))
    assert [i["name"] for i in doc["inputs"]] == ["board", "memory_in", "ant_planes", "ids", "cells"]
    assert [o["name"] for o in doc["outputs"]] == ["policy", "memory", "ant_memory"]
    assert doc["inputs"][1]["dtype"] == "u8" and doc["outputs"][1]["dtype"] == "u8"
    assert doc["inputs"][1]["shape"] == [1, planes.XATHIS_MEMORY, "H", "W"]
    assert doc["outputs"][2]["shape"] == [1, "N", planes.MISSION + 1]
    assert '"full"' in json.dumps(doc["inputs"][1]["adapter"]), "turn 0 starts explore at 100"
    assert adapters.dumps() == adapters.dumps(memory=False, ants=False)


def test_the_xathis_update_is_the_same_in_numpy_and_in_the_graph(dumped):
    """`planes.xathis_remember` against `nets.XathisMemory.write`, chained per board size over the
    reference observations, both reaches. Torch only; numpy's rendering is what CI checks the
    adapter against."""
    torch = pytest.importorskip("torch")
    from tb_baselines import nets
    from tb_baselines.export import classes
    by_size: dict[tuple[int, int], list[dict]] = {}
    for o in cases_of(dumped):
        by_size.setdefault(tuple(o["size"]), []).append(o)
    for reach in ("walk", "sight"):
        net = nets.build(classes()["nano"], memory="xathis" if reach == "walk" else "xathis-sight")
        net.eval()
        for group in by_size.values():
            state = None
            for obs in group[:4]:
                before = planes.xathis_memory_in_view({"size": obs["size"]}) if state is None else state
                with torch.no_grad():
                    _, after = net(torch.from_numpy(planes.encode(obs)), torch.from_numpy(before))
                if reach == "walk":
                    assert np.array_equal(after.numpy(), planes.xathis_remember(state, obs)), f"{reach} at {obs['size']}"
                assert after.dtype == torch.uint8 and after.shape == (1, planes.XATHIS_MEMORY, *obs["size"])
                state = after.numpy()


def test_the_xathis_adapters_start_the_bot_and_pass_its_memory_through(dumped, xathis_graph, tmp_path):
    """Through datalogic: a view without a memory yields explore 100 and stay 0, one carrying a
    memory yields it unchanged as `u8`, and the mission planes are the trainer's, with rows for
    the previous view's ants on the same board size."""
    views = []
    prev: dict[tuple[int, int], dict] = {}
    for i, obs in enumerate(cases_of(dumped)):
        view = dict(obs)
        key = tuple(obs["size"])
        if key in prev:
            view["memory"] = planes.xathis_remember(None, prev[key]).tolist()
            view["ant_memory"] = [[[j % planes.ANT_TABLE, 1, (j * 3) % obs["size"][0], (j * 5) % obs["size"][1]]
                                   for j in prev[key]["ids"]] + [[250, 1, 1, 1]]]
        views.append(view)
        prev[key] = obs
    assert any("memory" in v for v in views) and any("memory" not in v for v in views)

    out = tmp_path / "tensors"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(adapters.dumps(memory="xathis", ants="mission"))
    (tmp_path / "views.json").write_text(json.dumps({"observations": views}))
    r = subprocess.run(
        [CLI, "adapt", str(xathis_graph), str(manifest), "--obs", str(tmp_path / "views.json"),
         "--out", str(out)],
        cwd=ROOT, capture_output=True, text=True,
    )
    assert r.returncode == 0, f"tinybrains adapt failed on the xathis manifest:\n{r.stdout}\n{r.stderr}"
    index = json.loads((out / "index.json").read_text())
    joined = 0
    for case in index["cases"]:
        i = case["case"]
        view = json.loads((out / f"case-{i}" / "observation.json").read_text())
        for name, ours in (("memory_in", planes.xathis_memory_in_view(view)),
                           ("ant_planes", planes.mission_planes_in_view(view)),
                           ("ids", planes.ant_ids_in_view(view)),
                           ("cells", planes.ant_cells_in_view(view))):
            theirs = np.load(out / f"case-{i}" / f"{name}.npy")
            assert theirs.shape == ours.shape and theirs.dtype == ours.dtype, (
                f"case {i} {name}: the adapter builds {theirs.shape} {theirs.dtype}, the trainer {ours.shape} {ours.dtype}")
            assert np.array_equal(theirs, ours), f"case {i} {name}: {int((theirs != ours).sum())} elements differ"
        if "memory" not in view:
            assert int(np.load(out / f"case-{i}" / "memory_in.npy")[0, 0].min()) == planes.XATHIS_START
        joined += int(planes.mission_planes_in_view(view)[0, 0].any())
    assert joined, "at least one view read a mission back through the join"
