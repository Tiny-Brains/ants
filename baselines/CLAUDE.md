# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this directory.

`../CLAUDE.md` is the cartridge's guide and `../../CLAUDE.md` the platform's. This file covers only
what is specific to the baselines.

## What this directory is

**The platform's own trained entries, and the worked example of how they were trained.** They are
competitor entries that the platform happens to own: they submit ONNX models and declarative
adapters like anyone else, and they have no special access to anything.

It was `Tiny-Brains/ants-baselines`, its own repository, until 16 September 2026 (devops decision
N20). It is a subdirectory of `ants/` because everything Ants — the rules, the viewer and the
entries trained against them — now comes from one repository, and because an observation change
and the encoding that reads it used to be two commits in two repositories on the same day. The
competitor-facing starting point is `Tiny-Brains/ants-starter`, which pip-installs this directory
as a library (`#subdirectory=baselines`), so **the directory name and the `tb_baselines` package
name are a contract with every starter clone**.

**This directory keeps its own toolchain and ships in no image.** Python, torch and the
`tinybrains` binary build it; nothing here is part of the cartridge's gate, `../build.sh` does not
run it, and `../.dockerignore` excludes `baselines/`, so an edit here cannot move the engine digest.

Two axes, and they do not cross cleanly:

- **The class ladder** — one fixed teacher distilled into each of the five weight classes, which is
  how the competition's actual question ("how much play fits in N bytes?") gets a measured answer.
- **The method column** — several learners on one class and one dataset, so the comparison is
  between algorithms rather than between algorithms *and* budgets at once.

`models/<class>-<method>/` holds each finished artifact: `model.onnx`, the generated `manifest.json`,
a `metrics.json` of what the platform said about it, and a `card.md` a person can read.

## Working here needs the cartridge built, and one sibling checkout

```text
tinybrains/
  ants/             <- games.toml resolves the cartridge from `../dist`; run ./build.sh there first
    baselines/      <- you are here
  devops/           <- the `tinybrains` CLI is built from devops/cli
```

```sh
(cd .. && ./build.sh)                    # into ../dist: the component, cartridge.json, reference observations
cargo install --path ../../devops/cli    # or: export TINYBRAINS=../../devops/cli/target/release/tinybrains
pip install -e '.[dev]'                  # torch, numpy, onnx, pytest
```

## Commands

```sh
pytest tests/ -q                                   # THE test. See "What must stay true".
python -m tb_baselines.adapters > /tmp/a.json      # the generated adapter
python -m tb_baselines.collect --seat-turns 250000 # the teacher dataset (about 9 minutes, 90 MB)
python -m tb_baselines.train.bc --class micro --epochs 6
python -m tb_baselines.train.bc --class micro --arch percell --channels 112 --blocks 2  # the control
python -m tb_baselines.export --class micro --weights runs/micro-bc/best.pt --out models/micro-bc
python -m tb_baselines.eval models/micro-bc models/nano-bc --boards 3
```

## The three gates, and only two of them are real

```
tinybrains env       training rollouts    fast; NO deadline, NO strikes, NO adapter
tinybrains <match>   eval.py              the real path, minus admission
tinybrains check     export.py            what the platform will actually decide
```

`tinybrains env` is not the referee. A policy that trains happily can still be struck for missing
the turn clock or refused for an adapter that goes over budget, because none of that exists in the
env. Never report a result from the env as a result.

## What must stay true

- **`planes.py` is the only definition of the encoding, and it is rendered twice.** The numpy
  encoder trains the network; `adapters.py` generates the `manifest.json` the ladder runs. Two
  implementations of one encoding is how a model scores worse in the arena than in training, and it
  fails *silently*. `tests/test_adapter_conformance.py` runs the real evaluator (`tinybrains adapt`,
  which is **datalogic**, the evaluator an Orion node runs the manifest on) over the cartridge's
  reference observations and asserts they agree element for element. **It is the most important
  test here.** It has already caught a plane filter inverted and a dilation radius off by five.
- **`manifest.json` is generated and never hand-edited.** Same rule as `ants/build.sh`'s manifests,
  for a sharper reason: editing one rendering of the encoding without the other is exactly the bug
  the conformance test exists to catch, and a hand-edit is how it gets reintroduced.
- **Numbers in `classes.toml` are measured, and the measurement is written down beside them.**
  Nothing there is a guess. When something is re-measured, replace the number *and* its note.
- **A variant is a command line, not a second table.** `--arch/--channels/--blocks` override the
  class's entry, and the run's `history.json` and the model card record what was actually built.
  `classes.toml` stays the five classes and does not grow a row per experiment.
- **The teacher is a label source, not a baseline.** It never ships. The class ladder is only
  meaningful if every class distils the *same* teacher, so changing it invalidates the comparison —
  regenerate the whole dataset and retrain everything, or don't change it.
- **The value head is never exported.** `export.py` takes the trunk and the policy head only. It is
  a real saving (at nano a critic would be a third of the budget) and it is what makes privileged
  input safe: a critic may see the true score because a critic is discarded before anything plays.
- **The engine digest belongs on every artifact.** A model trained against one engine and played
  under another is a model nobody can reproduce, and an engine change is a rules change. The dataset
  header, `metrics.json` and `card.md` all carry it.
- **`data/`, `runs/` and `replays/` are gitignored output.** The dataset is 90 MB and regenerable
  from a seed; a checkpoint is not an artifact. Only `models/` is committed.

## Things that were measured here, and cost time to find

- **Receptive field was the binding constraint, not capacity** — the long version is
  [`docs/receptive-field.md`](docs/receptive-field.md), and it is the one to read first. Dilation is
  an attribute of `Conv`, not an operator, so it needs nothing the allowlist does not have.

- **fp16 initializers are free capacity.** A `Cast` back to float32 at each use, which the runtime
  constant-folds when it optimises the graph, so the *file* halves and the runtime does not change:
  **2.03x the parameters for the same weight class**, identical play over 396 per-ant orders, same
  three operators. It matters more under `S'` than it did under `S`, because `S'` is the artifact's
  raw bytes rather than a compression of its initializers. There is no reason to ship fp32.
- **Above `mini` the turn deadline binds before the byte cap does.** A seat owns the **whole** turn
  since the wave went — one `model_infer` per seat, each with its own `timeout_ms` (decision R7) —
  so the share is 1000 ms rather than the 31.2 ms a 16-row wave divided out. `export.py` still
  refuses an artifact over 70% of that share, which is the check that turns this from a paragraph
  into a gate, and the margin is deliberate: a model needing 95% of the clock here has nothing left
  for a slower host.
- **`ConvTranspose` is not on the operator allowlist.** `Resize` is the upsampler.
- **The board wraps, and padding costs.** Wrapping before every convolution measured 2.3x the FLOP
  model; one wrap per resolution stage is the same arithmetic for a third of the copying.
- **A batch is per board size.** Three presets are three sizes (64x96, 96x96, 128x128) and one
  tensor cannot hold two. `Step.groups` is that, and `Step.boards` raises rather than silently
  handing back a fraction of the batch.
- **`dilate` scatters from the ants, it does not roll the plane.** The obvious reading — shift the
  board by all 241 disk offsets and OR — was 48% of the training loop at 1.86 million `np.roll`
  calls. Walking the disk out from each set cell is the same answer for 8,700 writes instead of 3.9
  million. **It is no longer part of the encoding**: the cartridge sends `vis` (decision R5), so
  `planes.py` reads it with the same `rle_expand` it uses for `water`. The function is kept because
  it is what proves the engine's mask is the mask the trainer used to derive.
