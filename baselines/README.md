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

**A memory column** sits beside the ladder, and it is a column because it has a teacher of its
own: `collect.py --remember` has the teacher read each seat's memory (`teacher.py`, *A teacher that
remembers*: an enemy hill seen once stays a target, food seen out of sight is a weak one), so its
labels depend on what the seat remembered and a model with a memory has something to learn from
it. On that dataset three models are trained the same way and differ only in what they carry:

- `train/bc.py` on the board alone -- what forgetting costs, the control;
- `train/bc.py --memory` with the two planes of `planes.MEMORY`, food seen and enemy hills seen,
  kept for the match with `Max` inside the graph and handed back by the runner each turn (the
  book's *Memory* page): a memory that is a fixed function, nothing in it learned;
- `train/seq.py` with `nets.LearnedMemory`, the same two `i8` planes but written by a gated update
  the optimiser shapes, trained by replaying each seat's rows in order with truncated
  backpropagation through time. `--ants` adds a memory per ant (`planes.ANT_MEMORY`), one row an
  ant that follows it by id, which the smallest class it fits is `micro`.

The class ladder's own dataset is still collected without a memory, so its teacher is unchanged.

**The 2011 winner's column** is a second teacher: `xathis.py`, a port of the bot that won the
contest this game comes from (`Strategy.java`, method for method, tie-break for tie-break; its
docstring names the four things the Java left to its runtime and what this port does instead,
the wall-clock timeouts first of all). It keeps three things between turns, and a model built on
it carries the same three: two `u8` board planes (`planes.py`, *the 2011 winner's memory*) whose
update the graph computes exactly, the bot's ten-step walk from every own ant and hill included,
and a mission per ant as `[id, has, target_row, target_col]`, which the graph is taught to write
from the dataset's `x` and reads back as the offset to its target. `collect.py --xathis` collects
it (70 ms a seat-turn in Python, so `merge.py` joins what several collectors wrote), and the two
models fill their classes to the byte: nano at 10 channels with the two planes, 16,368 of 16,384
bytes, and micro at 43 with the missions too, 129,672 of 131,072. A graph is weighed whole, so
`export.py` strips every name the runtime never reads and shares its repeated constants, which is
a third of nano back for every model here.

**What the column measured**, all at nano, five epochs each, then a round robin through
`tinybrains <match>` over the nine two-seat boards of a season, both seats of every pairing,
1000 turns (`eval.py`; the model cards carry each artifact's own numbers):

| Carries | Labels | Held-out agreement | Round-robin rate | Points a match, for / against |
|---|---|---|---|---|
| the fixed memory | remembering | 81.8% | 72% | 4.44 / 2.32 |
| the board alone | remembering | 81.0% | 49% | 3.46 / 3.38 |
| the learned memory | remembering | 77.0% | 47% | 3.15 / 3.53 |
| the fixed memory | forgetting | 82.8% | 45% | 3.24 / 3.69 |
| the board alone (the starter's `nano-bc`) | forgetting | 81.9% | 37% | 2.75 / 4.12 |

Three things to read off it. **A memory is worth a great deal once the labels use it**: the same
class, data and method with and without the two planes is 72% against 49%, eleven wins to two
head to head, and the difference is points, a remembered hill razed. **Agreement does not say
so**: the same two models are 0.8 points apart on the held-out set, and the fixed memory on the
forgetting teacher's labels agrees best of all and plays worst but one. **The learned memory does
not reach the fixed one in five epochs**: it plays like the board alone, four points of agreement
below it, which is where the trainer's step count leaves it (`train/seq.py`); the fixed memory is
the baseline until a longer run says otherwise.

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
python -m tb_baselines.collect --remember --seat-turns 250000 --out data/teacher-memory.jsonl.gz  # the memory column's teacher
python -m tb_baselines.train.bc --class nano --memory --data data/teacher-memory.jsonl.gz --epochs 5    # a fixed memory
python -m tb_baselines.train.seq --class nano --memory learned --data data/teacher-memory.jsonl.gz     # a learned one
python -m tb_baselines.train.seq --class micro --memory learned --ants --data data/teacher-memory.jsonl.gz   # and one per ant
python -m tb_baselines.export --class nano --weights runs/nano-bc-memory/best.pt --out models/nano-bc-memory
for s in 11 12 13 14 15; do python -m tb_baselines.collect --xathis --seat-turns 50000 --seed $s --out data/xathis-$s.jsonl.gz & done; wait
python -m tb_baselines.merge data/xathis-1?.jsonl.gz --out data/xathis.jsonl.gz                  # the 2011 winner's column
python -m tb_baselines.train.seq --class nano --channels 10 --memory xathis --bptt 1 --batch 32 --data data/xathis.jsonl.gz
python -m tb_baselines.train.seq --class micro --channels 43 --memory xathis --ants mission --bptt 1 --batch 32 --data data/xathis.jsonl.gz
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
installed, against the graph itself. The memory per ant is held to it too: the adapter's join by
id (`to_list`, `scatter`, `concat`, `gather`, then a scatter at each ant's cell) is run through
`tinybrains adapt --obs` over views carrying rows for the previous view's ants, and its three
inputs are compared with the trainer's numpy join element for element. A CLI without the memory
carry fails these tests with a message that says so.

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
  to disagree on, and 2 bytes a cell as `i8`. A learned memory (`nets.LearnedMemory`) is the same
  two planes written by a gated update, and it costs a trainer that replays a seat in turn order
  (`train/seq.py`), because the memory is the graph's own output on the row before.
- **What the wire carries is what training carries.** A learned memory is rounded to its `i8` (a
  memory per ant to its `u8`) inside the graph with `Floor` and `Cast`, and the trainer carries the
  rounded value too, through a straight-through estimator: the net never learns to read a precision
  the runner cannot hand back. `Round` is not on the operator allowlist; `Floor` is.
- **Rows follow ants by id.** `mine` is re-sorted every turn, so a memory per ant is a table
  indexed by id that the adapter reads back in this turn's `ids` order (the book's join), with the
  id kept modulo `planes.ANT_TABLE` so a row fits a byte. The graph writes each ant's id in front
  of its row; nothing else keeps the rows and the ants together.

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
  teacher.py                    the scripted bot the class ladder is distilled from; remembering, the memory column's
  xathis.py                     the 2011 winner's bot, ported: the second column's teacher, and what it remembers
  collect.py                    teacher rollouts to a dataset, each row carrying what the seat remembered and its place in its match
  merge.py                      datasets collected in parallel joined into one, their episodes kept apart
  nets.py                       one architecture per class, and the three memories a graph can carry
  train/bc.py                   behaviour cloning
  train/seq.py                  behaviour cloning over a seat's turns in order, for a memory the graph computes
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
- **The memory per ant is one join, `planes.py`'s, rendered twice**: the adapter's, through
  datalogic, and the trainer's, in numpy and in torch. The conformance test holds the first two
  equal, and `train/seq.py` carries the third.

## Known gaps

- PPO self-play is written and smoke-tested but has never run long enough to learn anything.
- `small` has no trained artifact, and `large` has no architecture: nothing dense reaches its cap
  inside a seat's deadline share.
- The teacher forages and rarely razes, so what the class ladder distils is a forager.
- PPO self-play carries no memory. `ActorCritic` reads the board alone; the memories are behaviour
  cloning's, fixed or learned.
- No class table here names memory numbers. A season sets `memory_flat_bytes` and
  `memory_cell_bytes`, 0 and 0 by default; the memory baseline needs 2 bytes a cell, and `export`
  reports the price without a verdict.
- A memory nano sits at 84% of its cap, above the 70% the class aims at: two more input channels of
  weights and a manifest 263 bytes longer.

## License

Apache-2.0, as the rest of ants: see [LICENSE](../LICENSE).
