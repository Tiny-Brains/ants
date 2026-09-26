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
python -m tb_baselines.adapters --memory > /tmp/m.json   # the same, with the memory ports
python -m tb_baselines.adapters --memory --ants > /tmp/a.json   # and with a memory per ant's three inputs
```

The conformance test loads a trained graph: `$TB_CONFORMANCE_ONNX`, else
`../../ants-starter/models/micro-bc/model.onnx`. CI clones the starter and names it, so a missing
file there is a failure rather than a skip. The memory adapter test builds its own four-port graph
with `onnx` and needs a `tinybrains` that carries memory (cli after 0.3.0); the released 0.3.0
fails it with a message that says so.

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
- **`planes.MEMORY` is rendered three ways, and the same test holds them.** The pass-through
  adapter (`memory_adapter`), the graph's `Max` (`nets.WithMemory`) and the trainer's union
  (`remember`, `encode_memory`). The memory is `i8` like the board, 2 bytes a cell. A dataset row
  carries the state its seat remembered under `m` (`collect.py`), so a change to `MEMORY` is a new
  dataset, and `train/bc.py --memory` refuses one whose header names other planes.
- **The memory per ant is one join, rendered twice, and the same test holds it.** `planes.py`'s
  `_ant_rows_logic` (the adapter: last turn's rows into a table by id, read back in this turn's
  `ids` order, scattered at each ant's cell) and `ant_planes_in_view` (numpy) are compared through
  `tinybrains adapt --obs`; `train/seq.py`'s `AntCarry` is the torch copy a gradient flows
  through. Ids are kept modulo `ANT_TABLE`, and the graph writes each ant's id in front of its row.
- **A memory the graph computes is trained in turn order.** `train/seq.py` replays each seat's rows
  by `k` (`collect.py`) with truncated backpropagation through time; the memory carried between
  turns is the rounded integer the runner carries, through a straight-through estimator in
  `nets.LearnedMemory` and `nets.PerAnt`. `train/bc.py` is for a memory that is a fixed function
  of the rows before (`planes.MEMORY`), which numpy can carry.
- **`xathis.py` is a port, and stays one.** A change to what the bot does is a change to the
  Java it cites, so none is made here; the four things the port decides for itself (evaluation
  budgets for the clock, `java.util.Random` bit for bit, sorted keys for the two random
  comparators, the observation's iteration order) are named in its docstring and nowhere else.
  Its memory for a model is the pure part of what it keeps: `planes.xathis_remember` and
  `nets.XathisMemory.write` are one function (the conformance test holds them equal, walk and
  sight), and the explore claims an ant makes when it picks a direction are the bot's decisions,
  which the plane does not carry.
- **A graph is weighed whole, so `export.py` shortens it.** `shorten` drops node names, renames
  every intermediate tensor to a short id and shares repeated `Constant` nodes; the ports and the
  initializers keep their names. The size a class is filled to is measured after it, and a change
  to what it strips moves every artifact's bytes.
- **The teacher reads a memory only when asked.** `Teacher.orders(..., memory)` reads the seat's
  remembered state (an enemy hill seen once stays a target until seen gone; food seen out of sight
  is a weak one), and `collect.py --remember` gives it one. The class ladder's dataset is collected
  without, so its teacher is the same function move for move (`tests/test_teacher.py`).
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
- **An old `tinybrains` fails the memory adapter tests too.** `adapt --obs` on a view carrying
  `memory` or `ant_memory` needs the carry; 0.3.0 refuses the nested arrays and the test names the
  cause. Ants' CI builds the CLI from cli's `main`, so these pass there only once the carry is on
  `main`.
- **The 2011 winner's walk is written as convolutions.** Ten steps of a 4-neighbour flood as
  shifts were 180 nodes and 60 KB of graph; as one wrap of eleven cells and ten padded 3x3 cross
  convolutions it is 30, and fits nano beside the trunk. Do the same for any other fixed update:
  a shift a side is twelve nodes, a kernel is one.
- **`tinybrains check` passing is not the allowlist passing, until cli `b7e1625`.** A graph with a
  `GreaterOrEqual` ran through `check` and was `OP_NOT_ALLOWED` at admission; the runtime executes
  more than the deployment allows. Write comparisons as `Greater` on half-integer thresholds, and
  read `metrics.json`'s `operators` against the book's *Format* page before an upload.
- **A memory per ant does not fit nano.** Its graph (the gather, the id column, the two gates)
  and its 2,674-byte manifest pass the cap before a weight is counted; `micro` is its smallest
  class, and a season's class has to allow its price (3 bytes a cell alone, 5 with the board
  memory).
- **Four trainings on one MPS is one too many.** A sequence trainer holds `bptt` turns of
  activations; two of them beside two `bc.py` runs on a 16 GB machine ended in a Metal
  out-of-memory that the process survives with garbage. Run the sequence trainers one at a time,
  and start background jobs with `setopt nobgnice`: zsh nices a `&` job by default and the
  trainer lands on the efficiency cores.
- **CI installs `numpy`, `onnx` and `pytest`, not torch.** The graph-side memory test
  (`test_the_graphs_memory_update_matches_the_encoder`) skips without torch and runs on a dev
  install; its numpy twin is what CI runs.
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
