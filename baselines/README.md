# ants/baselines

**How TinyBrains Ants entries are trained**: the observation encoding, a scripted teacher, the
learners, and an export that measures each model into a weight class with the same code that
measures a competitor's. It is the `tb_baselines` Python package, which ants-starter's `train.py`
installs, and it commits no trained model: the models competitors test against live in
[ants-starter](https://github.com/Tiny-Brains/ants-starter)'s `models/`.

It is a subdirectory of [ants](../README.md) with its own toolchain. Nothing here is part of the
cartridge's build: `../build.sh` does not run it and `../tools/pack.py` packs `../dist` alone, so no
edit here moves the engine digest or reaches a release. The `build` workflow does run its tests
against every build.

## Scope

Two axes, which do not cross cleanly:

- **The class ladder** distils one fixed teacher into each of the five weight classes. Same data,
  same labels, same loss; only the budget changes. That is the size/fidelity curve the classes exist
  to measure.
- **The method column** takes one class and one dataset and changes only the learner, so the
  comparison is between algorithms rather than algorithms and budgets at once. Its control,
  `percell`, is 1x1 convolutions only, matched to the convolutional trunk's parameter count, so it
  has the same capacity and no receptive field: the gap between them is what looking around is
  worth.

They do not cross because compute does not scale with bytes. Above `mini` the turn deadline binds
long before the byte cap does, and no dense architecture reaches `large`'s cap.
[`classes.toml`](classes.toml) carries every number and the measurement behind it.

**A memory variant** sits beside the ladder. `--memory` gives a class's network the two planes of
`planes.MEMORY`, food seen and enemy hills seen, kept for the match with `Max` inside the graph and
handed back by the runner each turn (the book's *Memory* page). The teacher reads no memory, so a
memory model distils the same forager; what it proves is the whole path a model with memory takes,
from the manifest's second input through admission's round trip to the carry and `conform`.

## Run it

```sh
(cd .. && ./build.sh)                    # ../dist, which games.toml resolves: component, manifest, reference set
brew tap tiny-brains/cli https://github.com/Tiny-Brains/cli && brew install tiny-brains/cli/tinybrains
                                         # or: export TINYBRAINS=../../cli/target/release/tinybrains
pip install -e '.[dev]'                  # torch, numpy, onnx, pytest

pytest tests/ -q                                    # the conformance gate; see below
python -m tb_baselines.collect --seat-turns 250000  # the teacher dataset, about 9 minutes and 90 MB
python -m tb_baselines.train.bc --class micro --epochs 6
python -m tb_baselines.train.bc --class micro --arch percell --channels 112 --blocks 2   # the control
python -m tb_baselines.train.bc --class nano --memory --epochs 5      # the memory variant; the dataset carries the state
python -m tb_baselines.export --class nano --weights runs/nano-bc-memory/best.pt --out models/nano-bc-memory
python -m tb_baselines.train.ppo --class micro --iters 200                              # self-play
python -m tb_baselines.export --class micro --weights runs/micro-bc/best.pt --out models/micro-bc
python -m tb_baselines.eval models/micro-bc ../../ants-starter/models/nano-bc --boards 3
```

`export` writes `model.onnx`, the generated `manifest.json`, a `metrics.json` of what
`tinybrains check` measured, and a `card.md` a person can read into `models/<class>-<method>/`, which
is gitignored. It refuses an artifact that misses its class. A model worth keeping is committed to
ants-starter's `models/` for competitors to test against, or uploaded into a season as a baseline.

### The one test that matters

A competitor training in Python encodes each observation twice: once in `manifest.json`, which is
what the ladder runs, and once in numpy, which is what the optimiser sees. When those two disagree,
both halves work and the model simply scores worse in the arena than its training curve promised.
Nothing tells you.

So the encoding is declared once, in [`planes.py`](src/tb_baselines/planes.py), with both renderings
side by side, and `tests/test_adapter_conformance.py` runs **datalogic, the evaluator a node runs an
adapter on** (through `tinybrains adapt`), over the cartridge's own reference observations and
asserts they agree element for element. If you take one idea from this directory, take that one.

The test loads a trained graph to check the manifest against: `$TB_CONFORMANCE_ONNX`, else
`ants-starter/models/micro-bc/model.onnx` in a checkout beside `ants`. Without either it skips;
with the variable set and the file missing it fails, which is how CI runs it.

The memory is held to the same standard. Its adapter passes last turn's memory through and builds
zeros on turn 0, and the test runs it through `tinybrains adapt --obs` over views with and without
a memory, against a four-port graph it builds with `onnx`. The trainer's update, `remember`, is
checked against the `Max` the graph takes on the reference observations, and, where torch is
installed, against the graph itself. A CLI without the memory carry fails the adapter test with a
message that says so.

## Design rules

- **Spend a small class on reach, not width.** Aim for a receptive field of at least ~15 cells each
  way (the view radius is about 8.8), which 3x3 convolutions at dilations 1, 2, 4, 8 reach in four
  layers. Dilation is an attribute of `Conv`, so it needs nothing the operator allowlist lacks.
- **Reach is a threshold, not a gradient.** Two cells of context played almost exactly like none,
  so tune reach against the threshold; adding it a layer at a time looks like a dead end.
- **Ship fp16 initializers.** A `Cast` back to float32 at each use, which the runtime constant-folds,
  halves the file the size metric reads: 2.03x the parameters for the same class, with identical play.
- **`Resize` is the upsampler.** `ConvTranspose` is not on the operator allowlist.
- **A memory is a fixed function until a learner needs more.** The two planes of `planes.MEMORY`
  are the 1s of two board planes, kept with `Max`: nothing to train, nothing for numpy and the graph
  to disagree on, and 2 bytes a cell as `i8`. A learned memory is a different trainer (Known gaps).

## Artifacts

| Artifact | Read by |
|---|---|
| `models/<class>-<method>/model.onnx` + `manifest.json` | a presigned upload: a competitor's submission, or an admin's baseline upload into a season; admission reads both from the bucket |
| `models/<class>-<method>/metrics.json` | people and `eval`. Nothing on the platform reads it; admission measures for itself |
| `models/<class>-<method>/card.md` | people |
| the `tb_baselines` package | ants-starter's `train.py`, installed from `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines` |

**What a deployment owes it: nothing.** These are ordinary submissions with no special access. A
season's baselines are uploaded on the season's admin page, admitted like any submission and land
disabled until an admin enables them, on a local stack as in production.

## Layout

```text
classes.toml                    the class table: every number, and its measurement
games.toml                      resolves the cartridge at ../dist
src/tb_baselines/
  planes.py                     THE encoding, and the memory, each rendered for numpy and for the manifest
  adapters.py                   generates manifest.json from planes.py, with the memory ports on request
  env.py                        client for `tinybrains env`
  teacher.py                    the scripted bot the class ladder is distilled from
  collect.py                    teacher rollouts to a dataset, each row carrying what the seat remembered
  nets.py                       one architecture per class
  train/bc.py                   behaviour cloning
  train/ppo.py                  PPO self-play
  export.py                     torch -> ONNX -> fp16 -> the platform's verdict, metrics and card
  eval.py                       round robin through `tinybrains <match>`
tests/                          the conformance gate, and the env client's tests
data/ runs/ models/ replays/    gitignored output
```

## Invariants

- **`manifest.json` is generated, never hand-edited.** It is one of two renderings of the encoding;
  editing it alone reintroduces exactly the skew the conformance test exists to catch.
- **The teacher never ships.** It is a label source. Changing it invalidates the class ladder, which
  is only a comparison because every class distils the same one.
- **The value head is never exported.** A critic may see privileged information precisely because it
  is discarded before anything plays.
- **`tinybrains env` is not the referee.** No deadline, no strikes, no adapter. A result from the env
  is not a result.
- **Every artifact names its engine digest**: the dataset header, `metrics.json` and `card.md`. An
  engine change is a rules change, and a model that cannot say which engine it was trained against
  cannot be reproduced.
- **The directory name and the `tb_baselines` package name are a contract** with every ants-starter
  clone.
- **The memory is one definition, `planes.MEMORY`, rendered three ways**: the manifest's
  pass-through adapter, the graph's `Max` and the trainer's `remember`. A dataset row carries the
  state its seat remembered under `m`, so a change to `MEMORY` is a new dataset, and
  `train/bc.py --memory` refuses one collected under another definition.

## Known gaps

- PPO self-play is written and smoke-tested but has never run long enough to learn anything.
- `small` has no trained artifact, and `large` has no architecture: nothing dense reaches its cap
  inside a seat's deadline share.
- The teacher forages and rarely razes, so what the class ladder distils is a forager.
- The memory baseline learns nothing from its memory: the teacher reads none, so no label depends on
  it. It proves the path, not the idea; a teacher that remembers, or a learned memory, would prove
  the idea.
- PPO self-play carries no memory. `ActorCritic` reads the board alone; `--memory` is behaviour
  cloning's.
- No class table here names memory numbers. A season sets `memory_flat_bytes` and
  `memory_cell_bytes`, 0 and 0 by default; the memory baseline needs 2 bytes a cell, and `export`
  reports the price without a verdict.
- A memory nano sits at 84% of its cap, above the 70% the class aims at: two more input channels of
  weights and a manifest 263 bytes longer.

## License

Apache-2.0, as the rest of ants: see [LICENSE](../LICENSE).
