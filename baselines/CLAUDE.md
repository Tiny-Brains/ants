# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this directory.

`baselines/` is how TinyBrains Ants entries are trained: the `tb_baselines` Python package (the
encoding, a scripted teacher, the learners, the export) that ants-starter pip-installs with
`#subdirectory=baselines`, so **the directory name and the package name are a contract with every
starter clone**. Its entries are ordinary competitor entries with no special access. It keeps its
own toolchain and ships in no release: `../build.sh` never runs it and `../tools/pack.py` packs
`../dist` alone, so an edit here cannot move the engine digest. `README.md` is the human guide
(commands, design rules, artifacts, invariants); `../CLAUDE.md` is the cartridge's guide and
`../../CLAUDE.md` the platform's.

`models/<class>-<method>/` is where an export writes each artifact (`model.onnx`, the generated
`manifest.json`, `metrics.json`, `card.md`). **It is gitignored**: the platform commits no model
here. One worth keeping goes to `ants-starter/models/` for competitors to test against, or is
uploaded into a season as a baseline from the admin page.

## Checks

Needs the cartridge built (`games.toml` resolves `../dist`) and a current `tinybrains`:

```sh
(cd .. && ./build.sh)                              # ../dist: the component, cartridge.json, reference observations
pip install -e '.[dev]'                            # torch, numpy, onnx, pytest
pytest tests/ -q                                   # THE test; needs `tinybrains` on PATH or TINYBRAINS set
python -m tb_baselines.adapters > /tmp/a.json      # the generated adapter
```

The conformance test loads a trained graph: `$TB_CONFORMANCE_ONNX`, else
`../../ants-starter/models/micro-bc/model.onnx`. CI clones the starter and names it, so a missing
file there is a failure rather than a skip.

## Rules

**Three gates, and only two of them are real:**

```
tinybrains env       training rollouts    fast; NO deadline, NO strikes, NO adapter
tinybrains <match>   eval.py              the real path, minus admission
tinybrains check     export.py            what the platform will actually decide
```

A policy that trains happily in the env can still be struck for missing the turn clock or refused
for an adapter over budget. Never report a result from the env as a result.

- **`planes.py` is the only definition of the encoding, and it is rendered twice.** The numpy
  encoder trains the network; `adapters.py` generates the `manifest.json` the ladder runs.
  `tests/test_adapter_conformance.py` runs the real evaluator (`tinybrains adapt`, which is
  **datalogic**, the evaluator an Orion node runs the manifest on) over the cartridge's reference
  observations and asserts they agree element for element. **It is the most important test here.**
- **`manifest.json` is generated and never hand-edited.** Editing one rendering of the encoding
  without the other is exactly the bug the conformance test exists to catch.
- **An observation change lands here in the same commit.** `planes.py` encodes what
  `../engine/src/observe.rs` sends; regenerate the manifest and update `manifest_bytes` in
  `classes.toml` when the plane set changes.
- **Numbers in `classes.toml` are measured, and the measurement is written down beside them.**
  Nothing there is a guess. When something is re-measured, replace the number *and* its note.
- **A variant is a command line, not a second table.** `--arch/--channels/--blocks` override the
  class's entry, and the run's `history.json` and the model card record what was actually built.
  `classes.toml` stays the five classes.
- **The teacher is a label source, not a baseline.** It never ships. The class ladder is only
  meaningful if every class distils the *same* teacher, so changing it means regenerating the whole
  dataset and retraining everything.
- **The value head is never exported.** `export.py` takes the trunk and the policy head only. It is
  a real saving (at nano a critic would be a third of the budget) and it is what makes privileged
  input safe: a critic may see the true score because it is discarded before anything plays.
- **The engine digest belongs on every artifact**: the dataset header, `metrics.json` and `card.md`.
  A model trained against one engine and played under another cannot be reproduced.
- **`data/`, `runs/`, `replays/` and `models/` are gitignored output.** The dataset is 90 MB and
  regenerable from a seed, and a checkpoint is not an artifact.
- **The model card is generated from `export.py`'s `CARD` template**, and ants-starter commits the
  cards of its models. A change to the template's fixed text is a hand edit to those cards too.

## Gotchas / what breaks

- **An old `tinybrains` fails the env tests.** `tests/test_env.py` needs a binary whose `env`
  understands `--maps`; an older one exits and every env test errors with "the environment exited".
  Point `TINYBRAINS` at a current build (`../../cli/target/release/tinybrains`) or the latest release.
- **Receptive field is the binding constraint, not capacity.** A model with too small a window
  trains, exports, passes admission and loses quietly. README §Design rules has the rules.
- **fp16 initializers are free capacity.** A `Cast` back to float32 at each use is constant-folded
  by the runtime, so the *file* halves and inference does not change: 2.03x the parameters for the
  same class, identical play. The size metric is the artifact's raw bytes, so there is no reason to
  ship fp32.
- **Above `mini` the turn deadline binds before the byte cap does.** A seat owns the whole turn (one
  `model_infer` per seat, each with its own `timeout_ms`), and `export.py` still refuses an artifact
  over 70% of that share: a model needing 95% of the clock here has nothing left for a slower host.
- **`ConvTranspose` is not on the operator allowlist.** `Resize` is the upsampler.
- **The board wraps, and padding costs.** Wrapping before every convolution cost 2.3x the FLOP
  model; one wrap per resolution stage is the same arithmetic for a third of the copying.
- **A batch is per board size.** The env plays one board a wave, the basic boards are five sizes and
  a season's are any size inside them, and one tensor cannot hold two. `Step.groups` is that, and
  `Step.boards` raises rather than silently handing back a fraction of the batch.
- **`dilate` scatters from the ants; it does not roll the plane.** Rolling the board by all 241
  disk offsets was about half the training loop; walking the disk out from each set cell is the same
  answer for a tiny fraction of the writes. It is not part of the encoding (the cartridge sends
  `vis`, which `planes.py` reads with `rle_expand`); it is kept as the independent check that the
  engine's mask is the disk the trainer would derive.
