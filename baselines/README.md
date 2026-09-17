# ants/baselines

**The platform's trained entries for TinyBrains Ants, and the worked example of how each was
trained.** Every model here is a real submission: an ONNX graph and a declarative adapter, measured
into a weight class by the same code that measures yours.

It exists because a ladder needs opponents before it needs interesting ones, and because the
competition's actual question — *how much play fits in eight kilobytes?* — deserves a measured
answer rather than an argument.

## Scope

Two axes, and the honest thing to say about them is that they do not cross cleanly.

**The class ladder** takes one fixed teacher and distils it into each of the five weight classes.
Same data, same labels, same loss; only the budget changes. That is the size/fidelity curve, and it
is the reason the classes exist.

**The method column** takes one class and one dataset and changes only the learner. That is the
comparison worth making — a per-cell model, a convolutional trunk and a reinforcement learner on
identical data say something about the algorithms; the same three at different budgets say nothing
about either.

The control in that column is worth naming: `percell` is 1x1 convolutions only, matched to the
convolutional trunk's parameter count to within 1%, so it has the same capacity and **no receptive
field at all**. The gap between them is what looking around is worth in this game, measured rather
than asserted.

They do not cross because compute does not scale with bytes. Above `mini` the **turn deadline**
binds long before the byte cap does, and `large`'s cap is out of reach of any dense architecture.
That is the interesting fact about the top of the ladder, not a defect in it — see
[`classes.toml`](classes.toml), which carries every number and the measurement behind it.

## Where it sits

A subdirectory of [ants](../README.md), with its own Python toolchain, holding competitor entries
the platform happens to own. They have no special access: this directory reads the cartridge it
sits in, encodes observations through the published adapter dialect, and is admitted through the
same gate as anyone else. It was `Tiny-Brains/ants-baselines` until 16 September 2026.

```text
tinybrains/
  ants/             <- the cartridge; games.toml resolves its build output, `../dist`
    baselines/      <- you are here
  devops/           <- the `tinybrains` CLI, which is the whole toolchain
```

Nothing here is part of the cartridge's build: `../build.sh` does not run it, and `../.dockerignore`
excludes it, so no edit here moves the engine digest.

## Interface

What this directory produces, and what reads it:

| Artifact | Read by |
|---|---|
| `models/<class>-<method>/model.onnx` + `manifest.json` | a presigned PUT; admission reads both from the bucket |
| `models/<class>-<method>/metrics.json` | `devops/scripts/dev/seed-baselines.sh`, for `size_bytes`, `param_count`, `infer_us` |
| `models/<class>-<method>/card.md` | people |
| `models/nano-bc/model.onnx` + `manifest.json` | ants-starter's `matches/vs-nano-bc.json`, by URL at a pinned commit of this repository — a retrained baseline is a new sha there |
| the `tb_baselines` package | [ants-starter](https://github.com/Tiny-Brains/ants-starter)'s `train.py`, pip-installed from `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines` |

## Run it, test it

```sh
(cd .. && ./build.sh)                    # ../dist, which games.toml resolves: component, manifest, reference set
cargo install --path ../../devops/cli    # or export TINYBRAINS=../../devops/cli/target/release/tinybrains
pip install -e '.[dev]'

pytest tests/ -q                                    # the conformance gate; see below
python -m tb_baselines.collect --seat-turns 250000  # the teacher dataset, ~9 min, ~90 MB
python -m tb_baselines.train.bc --class micro --epochs 6
python -m tb_baselines.export --class micro --weights runs/micro-bc/best.pt --out models/micro-bc
python -m tb_baselines.eval models/micro-bc models/nano-bc --boards 3
```

### The one test that matters

A competitor training in Python encodes each observation twice: once in `manifest.json`, which is
what the ladder runs, and once in numpy, which is what the optimiser sees. When those two disagree,
both halves work and the model simply scores worse in the arena than its training curve promised.
Nothing tells you.

So the encoding is declared once, in [`planes.py`](src/tb_baselines/planes.py), with both renderings
side by side — and `tests/test_adapter_conformance.py` runs **datalogic, the evaluator a node runs an adapter on**, over the
cartridge's own reference observations and asserts they agree element for element. If you take one
idea from this directory, take that one.

## What a deployment owes it

Nothing at run time. These are ordinary submissions.

At seed time, `devops/compose/bootstrap/seed.sql` inserts one entry per baseline, and the seeding
script needs each artifact's `metrics.json` for the values admission would otherwise have measured.

Since the entry split, each baseline is a **model** of its own, named for its directory here.
**They no longer need a special case.** They used to share one repository, `Tiny-Brains/ants-baselines`, which was legal
only because they are seeded by `INSERT`, carry no `owner_github_id`, and the global
`models_repo_uniq` index was partial on that column — and season 1 additionally named
`Tiny-Brains` under `repo.allow_orgs` so they were describable by the rule a competitor is admitted
by, which turned out to be an allowance to everyone. With no repository on an entry there is no
shared value, no partial index and no allowance: a baseline is three rows with three names.

## Layout

```text
classes.toml                    the class table: every number, and its measurement
src/tb_baselines/
  planes.py                     THE encoding, rendered twice
  adapters.py                   generates manifest.json from planes.py
  env.py                        client for `tinybrains env`
  teacher.py                    the scripted bot the class ladder is distilled from
  collect.py                    teacher rollouts to a dataset
  nets.py                       one architecture per class
  train/bc.py                   behaviour cloning
  export.py                     torch -> ONNX -> fp16 -> the platform's verdict
  eval.py                       round robin through `tinybrains <match>`
models/                         the finished artifacts, committed
tests/                          the conformance gate
```

### What cost the most to learn

[`docs/receptive-field.md`](docs/receptive-field.md) is the record of the mistake this directory
existed to catch: the first two models were designed against the byte cap, spent it on width, and
could see one and two cells respectively while the teacher they imitated plans 32 cells deep. Nine
times the parameters bought 2.6 points of agreement and the bigger one lost the round robin.
Dilating the convolutions took micro from 47.2% to 91.1% with fewer parameters.

## What must stay true

- **`manifest.json` is generated, never hand-edited.** It is one of two renderings of the encoding;
  editing it alone reintroduces exactly the skew the conformance test exists to catch.
- **The teacher never ships.** It is a label source. Changing it invalidates the class ladder, which
  is only a comparison because every class distils the same one.
- **The value head is never exported.** A critic may see privileged information precisely because it
  is discarded before anything plays.
- **`tinybrains env` is not the referee.** No deadline, no strikes, no adapter. A result from the env
  is not a result.
- **Every artifact names its engine digest.** An engine change is a rules change, and a model that
  cannot say which engine it was trained against cannot be reproduced.

## Status

**16 September 2026 — moved into `ants/`.** This was `Tiny-Brains/ants-baselines`; it is now
`ants/baselines/`, so the rules, the viewer and the entries trained against them come from one
repository, and an observation change and the encoding that reads it land in one commit (devops
decision N20). No model, no number and no line of the pipeline changed: `games.toml` resolves the
cartridge at `..` instead of `../ants`, the CLI install hints gained a `../`, and the duplicate
`LICENSE` went. `pytest tests/ -q` is 15 passed here as it was there. **ants-starter now installs
`tb_baselines` from `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`.**

**11 September 2026 — PPO runs 2.3x faster, same algorithm.** An iteration (micro, 48 turns, 4 waves
of 8) went from 23 / 34 / 39 s to 13 / 13 / 14 s. The time was not arithmetic: MPS compiles a graph
per tensor shape and the per-ant tensors had a new length on nearly every call, and a blocking copy
to MPS waited out all queued work eight times a minibatch. Per-ant tensors are padded to a multiple
of 256 and masked, a board size's samples go to the device once an update, and moves are drawn on
the host from `log_softmax` instead of `Categorical`. On a fixed batch fp32 agrees with the old loop
to seven significant digits; moves now come from `np.random.default_rng(--seed)`, so a run
reproduces itself but not an older one. `--amp` (fp16 autocast) is another 8% and off by default.
About 10 of the 13 seconds are now the convolutions themselves. The env is no longer a term: on
`tinybrains env`'s parallel waves and the faster engine the same run is 13 / 13 / 13 s, with losses
equal to the digit.

**15 September 2026 — the pipeline is built, three artifacts ship on the manifest contract, and the
class ladder measures something.**

| | class | parameters | S' | reach | agreement | on the ladder |
|---|---|---:|---:|---:|---:|---|
| `micro-bc` | micro | 24,077 | 54,426 (42% of cap) | 15 | 93.8% | yes |
| `nano-bc` | nano | 3,006 | 12,280 (75% of cap) | 15 | 85.8% | yes |
| `micro-percell` | micro | 24,993 | 52,732 (40% of cap) | 0 | 40.7% | the control |

**Every number in that table moved and none of the models did.** `parameters` counts every value
the ONNX document carries now — initializers, node attributes, subgraph bodies — rather than
initializers alone, which is the hole a graph could duck through by exporting its weights as
`Constant` nodes. `S'` is `artifact_bytes + len(manifest)`, raw where the old `S` compressed the
initializers, and the class caps doubled with it so the parameter budget each class was calibrated
for is the one it still has.

micro takes nano two to one over 24 matches, which is the size/fidelity curve the weight classes
exist to measure. Both are distilled from one deterministic teacher over 250,000 seat-turns and
7.4 million ant decisions.

Verified end to end on a live stack: seeded, registered and activated on each replica by its own
`tb-roster` clock, claimed one row at a time by `tb-match`, played with zero strikes, replay
uploaded, ratings folded by count, and visible through `GET /v1/models/{id}` with its measured
`infer_us`.

Settled by measurement: **fp16 initializers give 2.03x the parameters** for the same class with
identical play; **the turn deadline binds before the byte cap above `mini`**; and **receptive field,
not capacity, was the thing worth spending on** — see [`docs/receptive-field.md`](docs/receptive-field.md),
which is the document to read first.

Not built: PPO self-play is written and smoke-tested but has never run long enough to learn
anything; `small` has no trained artifact; `large` has no architecture at all, because nothing dense
reaches its cap inside a seat's deadline share. The teacher forages and rarely razes, so what the
ladder currently distils is a forager.


## More

- [The competitor guide](https://github.com/Tiny-Brains/web/tree/main/docs) — the rules, the model format, the
  adapter dialect, submitting, ranking and seasons.
- [ants-starter](https://github.com/Tiny-Brains/ants-starter) — the competitor's starter kit: a
  trained entry, `train.py` on top of this package, and match files to try a model without a
  database.
- Apache-2.0, as the rest of ants: see [LICENSE](../LICENSE).
