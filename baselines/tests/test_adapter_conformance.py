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
    rather than at admission. Any shipped model serves: the encoding is the manifest's, not the
    weights'."""
    if not CLI:
        pytest.skip("no `tinybrains` on PATH; set TINYBRAINS to the binary")
    onnx = ROOT / "models" / "micro-bc" / "model.onnx"
    if not onnx.exists():
        pytest.skip("no exported model to load the manifest against")
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
    assert "result" not in doc, "the platform reads the head; a manifest may not decode it (R3)"
    # The axes are NAMED, which is what lets one session serve every board the season runs.
    assert doc["inputs"][0]["shape"][2:] == ["H", "W"]
    assert doc["outputs"][0]["shape"][2:] == ["H", "W"]
    assert doc["probe_dims"] == adapters.PROBE_DIMS


def test_the_encoder_handles_an_empty_colony():
    """Zero ants is always valid (`ants/docs/protocol.md` §1) and is what a wiped-out seat sends
    right up until its match ends. An encoder that indexes into an empty list dies there."""
    obs = {
        "size": [64, 96],
        "mine": [], "foes": [], "food": [], "hills": [],
        "water": {"rle": [0, 64 * 96]},
        # No ants, no vision: the engine sends an all-zero mask, which is what the wiped example in
        # `ants/schema/examples/` shows and what `ants` asserts in
        # `the_view_carries_the_mask_it_filtered_through`.
        "vis": {"rle": [0, 64 * 96]},
    }
    board = planes.encode(obs)
    assert board.shape == (1, planes.N_PLANES, 64, 96)
    assert board.sum() == 0, "nothing seen, nothing set -- including the visibility mask"


def test_the_action_table_is_the_channel_order_the_platform_decodes():
    """Channel `i` of the policy head means `MOVES[i]`.

    That index is what the cross-entropy label uses (`train/bc.py`'s `MOVE_INDEX`) and what
    `orders_from_indices` writes back. **The platform closes the loop now, not the manifest**
    (decision R3): `tb-match` argmaxes the channels and indexes its own table, which is
    `["N","E","S","W","-"]` in `kalam/scripts/gen-kalam.py` and `devops/cli/src/model.rs`. So this
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
    """`vis` is SENT now (ants/docs/protocol.md §1, decision R5), not derived.

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
