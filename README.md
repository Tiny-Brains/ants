# ants

Ants is the reference game cartridge for TinyBrains: a deterministic contest between colonies on a
wrapping grid, after the 2011 Google AI Challenge. This repository builds the WebAssembly component
the platform plays, its manifests, the five basic boards and the replay viewer, and keeps the
pipeline that trains Ants entries ([`baselines/`](baselines/README.md)) beside the rules it encodes.
One GitHub release of `dist/` feeds every consumer: Soma, Kalam, web and its book, the `tinybrains`
CLI and ants-starter.

The component knows no platform: no ratings, no admission, no models, no scheduling. What a model
sees and answers, and what a cartridge must honour, are published in the competitor guide
([`web/docs`](https://github.com/Tiny-Brains/web/tree/main/docs): *What your model sees*, *What your
model answers*, *Adding a game*, and the rules under *Ants*).

## Build and test

There is no server, database, container or running platform to set up.

| Command | What it does | Needs |
|---|---|---|
| `./build.sh` | The determinism check, the engine's host tests and `mapgen`'s tests, then every artifact into `dist/`; prints the engine digest | rustup (`rust-toolchain.toml` pins rustc and the `wasm32-unknown-unknown` target), `wasm-tools` at `WASM_TOOLS_VERSION` in `build.sh`, Python 3.11+ |
| `viz/build.sh` | Transpiles the component with `jco` into `dist/viz/`, copies the viewer, runs `viz/check.mjs` | Node; `./build.sh` first |
| `tools/pack.py DIR` | Packs `dist/` into `DIR/ants-artifacts.tar.gz`; prints the engine, archive and content digests and the tag a release would take | a complete `dist/`, viewer included |
| `tools/deny.sh` | The determinism check alone: no floating point in `engine/src/` | |
| `(cd engine && cargo test)` | The host suite alone | |
| `(cd engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings)` | Lint; CI runs the same in `mapgen/` | |
| `(cd viz && node check.mjs)` | The viewer's checks: geometry, step counts, territory, labels, CSS scoping | Node, `dist/` |
| `(cd baselines && pytest tests/ -q)` | The encoding conformance gate ([baselines](baselines/README.md#the-one-test-that-matters)) | `dist/`, `tinybrains`, an ants-starter checkout beside this one or `TB_CONFORMANCE_ONNX` |

`engine/` and `mapgen/` are Cargo workspaces of their own; there is no `Cargo.toml` at the root.

**The engine digest is the sha256 of `tb-ants.wasm`**, and it is a function of four things: the
source, the exact rustc, the exact `wasm-tools` (it stamps its version into the component) and the
host rustc runs on. `rust-toolchain.toml` and `build.sh` pin the first three and remap every build
path, so where the checkout lives does not matter. The host cannot be pinned: the same source built
on an Apple-silicon Mac lays functions out in another order. Releases build on
`aarch64-unknown-linux-gnu`, so only a build there lands on a release's digest. Cite the workflow's
digest, never a laptop's.

## Artifacts

`./build.sh` and `viz/build.sh` write `dist/`, which is gitignored, and a release carries exactly
the same tree as one archive.

| Path | What it is | Made by |
|---|---|---|
| `tb-ants.wasm` | The component; its sha256 is the engine digest | `cargo build` + `wasm-tools component new` |
| `plugin.toml`, `plugin.json` | The Orion plugin ABI, authored and as JSON | `engine/plugin.toml`; `tools/package.py` |
| `cartridge.json` | Limits (`limits.boards` is the envelope a season's board must fit), the adapter budget, the basic boards' catalogue, `about` | `engine/src/bin/manifest.rs`; `tools/package.py` adds `maps`, `limits.boards` and `engine/about.json` |
| `maps/` | The five basic boards, as committed | copied from `maps/` |
| `reference/observations.json` | The observations admission validates an adapter against, drawn on the basic boards | `engine/src/bin/reference.rs` |
| `viz/` | The viewer bundle, and `engine.json`: the digest it was transpiled from | `viz/build.sh` ([viz/README.md](viz/README.md)) |

The component is plugin `tb.ants`, ABI `orion:plugin@1.0.0`. `engine/plugin.toml` declares each
function's inputs.

| Export | Input | Result |
|---|---|---|
| `tb.ants.worldgen` | Seeds and the board, whole (`map`, or `maps` one per seed); optionally `players` and `max_turns` | The initial packed `wave_state` |
| `tb.ants.observe` | `wave_state`, and opaque per-seat `refs` | A view per live seat, its ref echoed |
| `tb.ants.step` | `wave_state` and actions | The next `wave_state`, `done`, `ended` and the replay delta |
| `tb.ants.finish` | `wave_state` | Ranks, scores, the ending reason, and the board each ended match was played on |
| `tb.ants.replay-decode` | A replay envelope, and a `turn` or a `from`/`to` range | One frame, or a range in one pass |

`wave_state` is opaque to every caller. Actions follow the last observation's order, or name
`{m, seat, action}` explicitly. The hyphen in `replay-decode` is part of the name.

## Using a local build or a release

Point a games registry at a checkout's `dist/` to play or check against it:

```toml
[games.ants]
name = "Ants"
path = "../ants/dist"
```

A release unpacked is the same tree, and is the authoritative one:

```sh
gh release download --repo Tiny-Brains/ants --pattern ants-artifacts.tar.gz && tar -xzf ants-artifacts.tar.gz -C dist
```

A registry can also pin a release, which `tinybrains` downloads once and refuses unless the archive
and the component inside it hash to the pinned digests. The release notes carry the block:

```toml
[games.ants]
name = "Ants"
repo = "Tiny-Brains/ants"
release = "engine-<12 hex>"
artifacts = { file = "ants-artifacts.tar.gz", sha256 = "sha256:..." }
engine = "sha256:..."
```

To build Soma, Kalam, web or the book against an unreleased engine, pass
`--build-context ants=<this dist/>` to their Docker builds.

## Releasing

The `build` workflow (`.github/workflows/build.yml`) is the only build that ships. Every push to
`main` runs the gate on an `ubuntu-24.04-arm` runner, builds every artifact, packs `dist/`, runs the
baselines' conformance test, plays ants-starter against the build, and reports which release this
build reproduces byte for byte, or the tag publishing it would cut.

```sh
gh workflow run build.yml                   # a rehearsal: all of the above, nothing published
gh workflow run build.yml -f publish=true   # ...and cut the release, from main, if none carries this build
```

A release is `ants-artifacts.tar.gz`, tagged `engine-<first 12 hex of the digest>`. A later build of
the same engine whose archive differs (a viewer fix, new reference observations) is
`engine-<12 hex>-2`. The workflow compares builds by the tar's content, before gzip. A tag is never
re-cut: registries pin the archive's bytes.

**Publishing is deploying.** Soma's, Kalam's, web's and the book's images fetch the latest release
when they build unless `ANTS_RELEASE` names a tag, so the next build of each takes a new engine with
no commit in its repository. A deployment under a live season pins `ANTS_RELEASE`. After a release
with a new engine digest:

1. Rebuild Soma, Kalam and web (with the book) from the same release. A runner on another digest
   claims nothing, and a viewer on another digest draws a match that never happened.
2. Re-sign the plugins (web's `scripts/setup/sign-plugins.sh`). A component whose Ed25519 signature
   does not match brings the node up `degraded`.
3. Declare it. Soma's `bootstrap` declares the digest its image carries as a patch when no rule
   changed (the live season takes it), or with `ENGINE_RELEASE=1` as a rules change, which it
   refuses while a season is live.
4. Bump the release block in ants-starter's `games.toml`; the release notes carry it.
5. Re-capture web's `docs/tutorials/replays/real-match.json` from a match played on the new engine.
   It is a real match captured from a running stack, and the book's `tutorials/build.sh` refuses it
   once it is stale.

## Boards

This repository holds **five basic boards** under `maps/`, one per size class, and no others:

| Board | Size | Seats |
|---|---|---|
| `basic-tiny-2p` | 24 × 24 | 2 |
| `basic-small-3p` | 36 × 36 | 3 |
| `basic-medium-4p` | 48 × 64 | 4 |
| `basic-large-6p` | 80 × 96 | 6 |
| `basic-xlarge-8p` | 120 × 124 | 8 |

**They are the envelope.** `tools/package.py` derives `limits.boards` from what they span (2 to 8
seats, sides 24 to 124, at most 14,880 cells) and the reference set is drawn on them, so a season's
board must fit inside them and Soma refuses one that does not. The component carries no boards:
`worldgen` takes the board whole from its caller and validates it.

**A board is a design; never edit a board.** `mapgen/recipes/<name>.toml` is one board written out
as data: its size, its seat shift, a decorative point group about every seat's centre, every shape
and pattern drawn on it with their seeds, and the food plan. `mapgen/src/design.rs` renders it.

```sh
cd mapgen
cargo run -- generate                    # every recipe under recipes/ into ../maps/, removing any board none makes
cargo run -- generate recipes/basic-tiny-2p.toml
cargo run -- check                       # every committed board is what its recipe makes, byte for byte
cargo run --release -- check --play 6    # ...and play each, the same walker in every seat
cargo run -- show ../maps/basic-tiny-2p.json
cargo run --release -- explore --slots slots.toml --out run --n 500   # propose designs by the hundred
cargo run --release -- playtest run picks.txt                         # play the ones you picked
cargo run -- adopt run picks.txt                                      # write the picks down as recipes
cargo run --release -- sweep             # the fairness proof: many styles, 2-8 seats, played congruently
```

`explore` proposes and a person picks by eye; `adopt` writes the pick down, so a chosen board stays
that board whatever later happens to the sampler. `build.sh` runs `mapgen`'s tests, which re-render
every committed board and compare bytes.

**A season's boards are not here.** They are made with the same tool pointed outside every
repository (`--recipes DIR --maps DIR`), uploaded to the season by an admin, and pushed to a backup
repository only once the season has closed. No season's board is committed here or shipped in a
release.

## Layout

```text
engine/                    the cartridge: a Rust crate compiled to the wasm component
  src/lib.rs               plugin dispatch: the five exports and their JSON shapes, no rules
  src/grid.rs              wrapping, distances, symmetry, directions, bitmaps, the seeded RNG
  src/maps.rs              the board as a file: parsing, validation, and what a caller sent
  src/state.rs             one match while it is played
  src/turn.rs              a turn in its fixed order, the cutoff counter, the end conditions, ranks
  src/food.rs              food at the hidden rate, in shuffled symmetric sets
  src/observe.rs           what a seat sees, and what it has been shown
  src/codec.rs             the packed, base64 wave_state
  src/replay.rs            the action stream, and re-simulating it into frames
  src/authoring.rs         host-only: the reference observations
  src/bin/                 manifest, reference: the generators (args/ is their shared flag parsing)
  src/tests/               the host suite, one file per area; fixtures/ holds a platform-written replay
  plugin.toml              the authored Orion ABI
  about.json               the game's introduction, folded into cartridge.json
mapgen/                    the board factory: its own crate, so tuning it moves no engine digest
  recipes/                 one design per basic board
  src/main.rs              the commands: generate, check, show, explore, playtest, adopt, sweep
  src/design.rs            the renderer: a design's steps under the full group, hills, repair, food
  src/sym.rs               the seat shift, the point group about each centre, orbits, the lattice
  src/sample.rs            the sampler: a slot and a seed in, a design out
  src/explore.rs           explore and playtest: many designs a slot, rendered, validated, scored
  src/grid.rs              the torus, orbits and the RNG the generators share
  src/measure.rs           a board in numbers, and the rules every board obeys
  src/recipe.rs            the area recipes `sweep` draws from
  src/make.rs              the area generator: areas, walls, doors, hills, fill
  src/set.rs               the area generator's sets, the file format, and the engine's validation
  src/play.rs              a board played: the congruence check, and the greedy walker's smoke play
  src/sweep.rs             the fairness sweep over styles and seat counts
  src/tests.rs             mapgen's tests, which build.sh runs
viz/                       the replay viewer: one bundle for the web app, the book and the CLI
baselines/                 the training pipeline for Ants entries (Python); no model, not in a release
maps/                      the five basic boards, one JSON file each
tools/deny.sh              the determinism check
tools/package.py           finishes dist/: plugin.json, the catalogue and limits.boards, the boards, the report
tools/pack.py              dist/ as a release's one archive, packed deterministically, and its digests
build.sh                   the gate, then every artifact, into dist/
rust-toolchain.toml        the exact rustc, which is part of the engine digest
.github/workflows/build.yml  the gate and the build on every push; the release when asked
dist/                      build output, gitignored; exactly what a release archive carries
```

## Invariants

- **Game logic uses integer arithmetic.** `tools/deny.sh` refuses floating point in `engine/src/`
  outside `tests/` and `bin/`. It is what lets the platform and the browser agree bit for bit, and a
  replay be an action stream.
- **Seeds, actions and the board determine a match.** No ambient randomness, no clock. On the
  ladder Soma's pair clock chooses the board and the seed, so a competitor cannot train against a
  board they picked.
- **Any edit under `engine/src/` outside `tests/` and `bin/`, or to `engine/plugin.toml`, is a new
  engine digest**, comment-only edits included, because panic locations carry line numbers. A new
  digest is a release and a ladder event.
- **Exploring is remembered.** A model is a pure function of one observation, so the engine folds
  each seat's vision into what it knows and a view carries known water.
- **A view is observer-relative.** Relabel every seat and ask the same player again, and the bytes
  are identical (`a_view_is_observer_relative`).
- **Every board is fair and whole, whoever sends it.** `engine/src/maps.rs` checks that a board is
  congruent under its own shift, that its land is one walkable body and that every hill has a way
  off, on every board `worldgen` is given, including a season's at upload.
- **A replay carries the board it was played on and its seed**, so it re-simulates whatever became
  of its season. The decode test runs against an envelope the platform actually wrote.
- **The viewer re-simulates through the component, never a copy of it.** A JavaScript
  re-implementation of a rule would be a second engine.
- **Nothing generated is committed, and everything generated is in `dist/`.** Consumers read
  `dist/` by path, so renaming a file in it is a change to every consumer and to the CLI.
- **The protocol is published, and moves with the engine.** A change to what a view carries is a
  change to the competitor guide and to `baselines/src/tb_baselines/planes.py` in the same batch.
- **The rules are the 2011 contest's.** Where the published specification and the contest engine
  (`aichallenge/ants/ants.py`) disagree, the engine wins. The Focus Battle page's worked examples
  ship as `spec_scenario_*` tests.
- **`baselines/` and the `tb_baselines` package are a public path.** ants-starter pip-installs them
  from `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`.

## Known gaps

- The 10,000-match cross-host conformance run is owed: determinism between the platform's runtime
  and the browser is checked in the small only.
- Web's `scripts/check/configs.sh` compares Kalam's engine with the book's viewer, but not with web's
  own viewer or Soma's image, and nothing compares ants-starter's pinned `engine`.
- A view carries no turn number. Whether endgame play needs one is untested.

## License

Apache-2.0: see [LICENSE](LICENSE).
