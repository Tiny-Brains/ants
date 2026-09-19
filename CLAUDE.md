# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

`ants` is the reference **game cartridge** for TinyBrains: a Rust crate compiled to a WebAssembly
component (`tb-ants.wasm`, plugin id `tb.ants`, ABI `orion:plugin@1.0.0`) that Kalam loads and calls.
There is no server, no database, no network, and no clock — the component imports nothing. It owns
the rules of Ants (the 2011 Google AI Challenge game) and knows nothing about the platform: not
ratings, not admission, not models, not matches as scheduling units.

**The repository is everything Ants; the component is only the rules.** Three subdirectories carry
their own toolchains and are not the cartridge: `mapgen/`, the board factory (a host-only Rust crate
of its own), `viz/`, the replay viewer (Node), and `baselines/`, the platform's trained entries and
the pipeline that trains them (Python, with its own [CLAUDE.md](baselines/CLAUDE.md)). The baselines know the platform thoroughly — weight classes, the
adapter dialect, the turn deadline, `tinybrains check` — and that is fine *because* they are not
the component: the rule above is about `src/`, and the determinism check, the build and the release
never see `baselines/`.

The parent `tinybrains/CLAUDE.md` describes the platform this sits in; read it for anything
crossing a repo boundary. `README.md` here is the canonical page, in the platform's standard shape —
update its **Status** when work lands. `DECISIONS.md` is Ants' share of the platform's decision record
(the game and protocol decisions, R5, the baselines' 49/50 and N20–N22, N24). **There are no other
design docs here**: what a model sees and
answers, and what a cartridge must honour, are published in the competitor guide
(`../web/docs/src/models/observation.md`, `actions.md`, `platform/adding-a-game.md`, and the rules
under `games/ants/`). Module doc comments carry the rest of the why.

## Commands

Needs rustup (`rust-toolchain.toml` pins rustc 1.98.1 and the `wasm32-unknown-unknown` target),
`wasm-tools` at the `WASM_TOOLS_VERSION` in `build.sh`, and Python 3.11+ (`tomllib`). Node only for
the viewer. **No Docker**: nothing here builds or ships an image. **The crate is `engine/`, its own
Cargo workspace** — there is no `Cargo.toml` at the root, so every `cargo` command runs from `engine/`.

```sh
./build.sh          # the whole gate, then every artifact, into dist/
viz/build.sh        # the viewer, into dist/viz/ (after ./build.sh)
tools/pack.py DIR   # dist/ as the release archive, its digests, and the tag it would take
gh workflow run build.yml                  # the build that ships, on GitHub: a rehearsal, nothing published
gh workflow run build.yml -f publish=true  # ...and the release, from main
tools/deny.sh       # just the determinism check (no floating point in game logic)
(cd viz && node check.mjs)   # just the viewer checks -- geometry, and CSS scoping

cd engine
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # edition 2024; rustfmt.toml is not the default
cargo test          # just the host suite -- 89 tests, and one ignored diagnostic
cargo test a_replay_re_simulates_the_match_it_recorded    # one test by name
cargo test spec_scenario_                                  # the spec's worked fights
cargo test measure_what_random_play_produces -- --ignored --nocapture  # diagnostic, not a guarantee; minutes
```

`build.sh` runs `tools/deny.sh` and `cargo test`, clears `dist/`, builds the wasm with every build
path remapped and component-izes it, runs the `manifest` and `reference` generators into `dist/`,
then `tools/package.py` writes `plugin.json`, folds the basic boards' catalogue, the `limits.boards`
envelope derived from them, and `engine/about.json` into `cartridge.json`, copies `maps/` and
`engine/plugin.toml`, and prints the **engine digest**. Clearing `dist/` also drops
`dist/viz/`, on purpose: a viewer transpiled from the previous component is a viewer for some other
engine.

The two host binaries are generators (from `engine/`):

```sh
cargo run --bin manifest > ../dist/cartridge.json                  # then tools/package.py folds in maps + about
cargo run --bin reference > ../dist/reference/observations.json    # -- --only basic-small-3p:20260919:400 for one spec
```

The boards come from `mapgen/`, a crate of its own (from `mapgen/`). **This repository holds the five
basic boards and no others**; a season's are made with the same commands pointed outside every
repository (N28):

```sh
cargo run -- generate                    # every recipe under recipes/, into ../maps/ -- and removes any board none makes
cargo run -- generate recipes/basic-tiny-2p.toml
cargo run -- check                       # every committed board is what its recipe makes, byte for byte
cargo run --release -- check --play 6    # ...and play each one, the same walker in every seat
cargo run -- show ../maps/basic-tiny-2p.json   # draw a board and its metrics
cargo test                               # 7 tests; build.sh runs them
cargo run --release -- explore --slots slots.toml --out run --n 500   # propose designs, hundreds a slot
cargo run --release -- playtest run picks.txt                         # play the picked candidates
cargo run -- adopt run picks.txt         # write `<slot> <candidate> [name]` picks as recipes/<name>.toml
cargo run -- generate --recipes ../../maps/recipes --maps ../../maps   # a season's boards, outside the repo
cargo run -- check    --recipes ../../maps/recipes --maps ../../maps
cargo run --release -- sweep             # 11 recipe styles x 2-8 seats: generated, played congruently, replayed
cargo run --release -- sweep --seats 8 --styles cave,rooms --boards 10 --turns 1000 --json out.json --export dir
```

**`sweep` is the fairness proof, not a benchmark.** Every seat plays one policy that reads only its own
view moved into seat 0's frame, so on a fair board under fair rules every view, every turn, is seat 0's
moved by the shift, and every match ends level; a divergence is a bug, reported with the first turn the
referee's board broke symmetry (`MAPGEN_TRACE=1` draws the squares). It found two: spawn ties broken by
square rather than list order, and an open-ended `replay-decode` range over a cut-off recording. Run it
after any change to a rule that could treat seats differently.

**A board is a design; never edit a board.** A recipe (`mapgen/recipes/<name>.toml`, one board a
recipe) is the design written out: size, seat `shift`, decorative point group (`symmetry`), `origin`,
the hills from seat 0's centre, every drawing step (`water`/`land` shapes; `noise`, `ridge`, `maze`,
`rooms`, `dots`, `border`, `smooth` patterns with their seeds) and the food plan. `design.rs` renders
it and is the contract; `sample.rs` only proposes (N27), so improving the sampler redraws nothing that
was picked, while changing the renderer redraws every board. `check` re-renders every recipe and
compares bytes, so a hand edit or a renderer change nobody regenerated for fails `build.sh`. The old
area recipes (`recipe.rs`, `make.rs`, `set.rs`) remain only for `sweep` and their own tests. It is a
separate crate so tuning it is not an engine-digest change -- and since N28 neither is regenerating a
board: **the component carries none**, so `build.rs` is gone and a board reaches `worldgen` whole.
`mapgen` links this crate and validates every board it writes through `tb.ants.worldgen`, the check a
season's upload runs on Soma's node too. `tools/package.py` refuses to ship an empty `maps/`.

The baselines resolve the cartridge at `../dist` (`baselines/games.toml`), so they need `./build.sh`
run here first. Their guide has the training commands:

```sh
cd baselines && pytest tests/ -q   # the adapter conformance gate; needs `tinybrains` on PATH
```

**An observation change is a baselines change.** `planes.py` encodes what `engine/src/observe.rs` sends,
and the conformance test runs over `dist/reference/observations.json`, so a change to either lands
with the encoding and its regenerated manifest in the same commit — and with the competitor guide's
*What your model sees*. A retrained model is not a rebuild, and **no trained model is committed here**
(N29): an export lands in the gitignored `baselines/models/`, the platform's baselines are uploaded into
a season by an admin, and the models a competitor tests against live in `ants-starter/models/` --
which is also where the conformance test finds a graph to load the manifest against.

## Architecture

**Five exports, dispatched by name in `engine/src/lib.rs` and nothing else:**

```
tb.ants.worldgen(seeds[], map | maps, players?, max_turns?)  → wave_state
tb.ants.observe(wave_state, refs)      → per-seat views, each echoing its opaque ref
tb.ants.step(wave_state, actions)      → { wave_state, done[], ended[], replay_delta }
tb.ants.finish(wave_state)             → per-match ranks, scores, reason, and the board it used
tb.ants.replay-decode(payload, turn | from,to)  → one frame, or a range in one pass
```

The hyphen in `replay-decode` is load-bearing (Orion refuses `[^a-z0-9-]` in a function label).
`refs` is a **flat** list carrying its own `m`/`seat`, matched and echoed, never interpreted;
`actions` may be positionally aligned with the last `observe` or use explicit `{m, seat, action}`.
A *wave* is many matches advanced together in one call.

**Module responsibilities** (under `engine/`) — keep the split:

| File | Owns |
|---|---|
| `src/lib.rs` | Plugin dispatch and JSON shapes. No rules |
| `src/grid.rs` | Wrapping geometry, distances, symmetry, directions, the view/attack/spawn radii, `Bits`, `Rng` |
| `src/maps.rs` | The board as a file: parsing, validation (the board's own shift, whole hill orbits, one walkable body of land), and resolving what a caller sent -- a board object, and nothing else (N28) |
| `src/state.rs` | One match while it is played; the cutoff counter's constants |
| `src/turn.rs` | Turn resolution in a fixed order: move → attack → raze → spawn → gather → spawn food; ending; ranks |
| `src/food.rs` | Food at the hidden rate: the rate, the symmetric sets, their shuffled rotation, the pending queue |
| `src/observe.rs` | One seat's view: fog, known water, `vis`, observer-relative owners |
| `src/codec.rs` | The packed, base64 `wave_state`. Opaque outside this file; no version field |
| `src/replay.rs` | Action-stream recording and re-simulation into frames |
| `src/authoring.rs` | Host-only: the reference observations — unreachable from a plugin call, so the linker drops it from the component. The board factory is `../mapgen/` |
| `plugin.toml`, `about.json` | The authored ABI, and the game's introduction folded into `cartridge.json` |

**Four properties that explain most of the design:**

1. **Integer arithmetic only in game logic**, so the platform's build and the browser's agree bit
   for bit — which is what lets a replay be an action stream instead of frames. `tools/deny.sh`
   greps `engine/src/` for float constructs, excluding `tests/` and `bin/`, which are host tooling
   and never reach the component. Nothing in Rust enforces this.
2. **Seeds and actions determine the match**, on the board the caller sends. No ambient randomness,
   no time. The caller chooses the board -- on the ladder that is pair, which also assigns the seed,
   so a competitor still cannot train against a board they chose.
3. **Exploring is remembered.** A model is a pure function of one observation with no state channel,
   so `turn.rs` folds each seat's vision into `known` every turn and observations carry *known
   water*. The per-player seen-masks are ~60% of `wave_state`; that cost is the point.
4. **Boards are files, and the component carries none** (N28). Five basic boards are committed under
   `maps/` at the root (content, not source — `mapgen/` renders them from its recipes and
   `dist/maps/` ships them): `basic-tiny-2p` (24 × 24) to `basic-xlarge-8p` (120 × 124), one a size
   class, two to eight seats. **They are the envelope**: `limits.boards` is what they span, the
   reference set is drawn on them, and a season's board -- uploaded to Soma, never committed here --
   must fit inside it. Every board, whoever sends it, is validated before it is played. A board
   carries its **shift**
   (`symmetry`): seat `k`'s board is seat 0's moved `k` times by it, it travels in `wave_state`, and
   `food::sets` and observer-relative owners both follow it. Hills are listed in orbits, so hill `i`
   is seat `i % players`'s, and a board states its own seat count. A replay envelope carries its own
   board *and* its seed, so it re-simulates correctly whatever became of the board's season.

## What breaks if you forget it

- **Everything generated is in `dist/`, nothing in it is committed, and nothing outside it is
  generated.** `dist/` is laid out exactly as a release's `ants-artifacts.tar.gz`, which is what
  lets a games registry `path` point at a checkout and a `release` at the archive. Consumers read
  that layout by path — the component by extension at the root, `cartridge.json`, `plugin.json`,
  `plugin.toml`, `maps/`, `reference/observations.json`, `viz/viz.js` and `viz/engine.json` — in
  `cli` (`registry.rs`, `serve.rs`, `cmd/mod.rs`), soma's, kalam's and web's Dockerfiles,
  `web/docs/Dockerfile` and `tutorials/build.sh` — soma's takes `cartridge.json` and
  `reference/observations.json` for `bootstrap` to register.
  Rename one and grep the siblings in the same batch; they reach it only through a release.
- **The `build` workflow is the build, and its digest is the platform's.** Four things make the
  component's bytes: the source, rustc (`rust-toolchain.toml`, exact — a patch bump moves the
  bytes), `wasm-tools` (`build.sh`, exact — it stamps its version in), and **the host rustc runs
  on**. `build.sh` remaps the crates.io, standard-library and checkout paths rustc bakes into panic
  locations, so where the checkout is does not matter; the host does, and no flag removes it (the
  same source built on a Mac lays functions out in another order). Every release has been built on
  `aarch64-unknown-linux-gnu` — the old Docker image on an arm64 Mac, now the `ubuntu-24.04-arm`
  runner, which reproduces `engine-df312c0458d9` byte for byte — so moving the runner to x86-64 is
  an engine-digest change no rule made. Cite the workflow's digest, never a laptop's.
- **Publishing is deploying.** soma's, kalam's, web's and web/docs' Dockerfiles fetch the latest
  release unless `ANTS_RELEASE` pins a tag, and their release workflows resolve it the same way, so a
  release is the engine the next image of each of them declares, plays and draws — under a live
  season too.
- **Any source edit is a new engine digest**, comment-only ones included (panic locations carry
  line numbers). `games.active_engine_digest`, each replica's `engine_digest`, the plugin
  signatures and `viz/engine.json` all move with it. Say so in the commit and in README Status. To
  prove a refactor changed no rule, diff `cartridge.json`, `plugin.json`,
  `reference/observations.json` and `mapgen` output against a `git archive HEAD` build, and hash
  every `observe`/`step`/`finish` output turn by turn under a random and a greedy policy.
- **A new engine digest is a new release, or competitors keep playing the old one.** ants-starter's
  `games.toml` pins a release by the archive's digest and the `engine` digest, and has no checkout
  of this repository to fall back on (N21, N22 in `DECISIONS.md`). `gh workflow run build.yml -f publish=true`
  cuts it and its notes carry the block to paste there. Never re-cut a tag: a registry pins the
  archive's bytes. The tag is `engine-<12 hex>`, and `engine-<12 hex>-2` when the same engine ships
  a different archive; the workflow compares builds by the tar's content, so a build that
  reproduces a release says so rather than cutting another.
- **Never hand-edit a generated file.** `engine/plugin.toml` is the authored ABI (`plugin.json` is
  derived); `mapgen/recipes/` are authored and `maps/` is generated from them (`mapgen check` fails on
  a hand edit); `cartridge.json`'s catalogue and `limits.boards` are derived from them; `engine/about.json` is the one
  hand-written input folded into the manifest.
- **The basic boards are a contract with other repositories.** Their ids are named by ants-starter's
  match files, the book's board pages and baselines' tests, and renaming one reaches the starter only
  through a release. Their span IS `limits.boards`: shrink it and Soma refuses season maps that fit
  before; grow it and the reference set, and so every admitted adapter, must cover the new corner.
  Kalam claims at most eight seats, mapgen's ceiling and the envelope's top.
- **No season's board is ever committed here or shipped in a release** (N28). A season's boards are
  made with `mapgen --recipes/--maps` outside every repository, uploaded to the season by an admin, and
  pushed to a backup repository only once the season has closed.
- **Rules changes cite the reference.** Where the published specification and the 2011 contest
  engine (`aichallenge/ants/ants.py`) disagree, **the engine wins** — it is what every bot was
  scored against. Comments carry `ants.py:NNN` line cites; keep that habit. The Focus Battle page's
  worked examples ship as `spec_scenario_*` tests.
- **The protocol is published in the competitor guide, not here.** A change to a view's fields, an
  action's alphabet, or an ABI input is a change to `web/docs` in the same batch.
  `a_view_carries_exactly_the_fields_a_model_is_promised` pins the view's shape against engine
  output, so an accidental field fails a test rather than an adapter.
- **The viewer re-simulates through the cartridge**, via the jco-transpiled component. A JavaScript
  re-implementation of any rule would be a second engine. `viz/check.mjs` also fails the build if a
  stylesheet rule is not scoped to `.tb-viz` (an unscoped selector once relaid out the web app's
  header), and replays `engine/src/tests/fixtures/replay-maze-03.json` through the geometry.
- **`wave_state` stays opaque** to every caller, and turn order in `turn.rs` is a rule, not an
  implementation detail.
- **`baselines/` is a public path.** ants-starter pip-installs `tb_baselines` from
  `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`, so renaming the directory or the
  package breaks every starter clone. It holds the training code and no model: the baseline rosters
  that fetched `baselines/models/<name>/` by pinned URL are gone with the models (N29), and a
  season's baselines are uploaded into it. It stays out of a release: `build.sh` never reads it and
  `tools/pack.py` packs `dist/` alone, so no edit there can move the engine digest.

## Conventions

- Commits go straight to `main`, no feature branches. Messages are a sentence describing the change
  in the platform's voice ("Separate the engine from the tools that generate its artifacts"), not a
  conventional-commit prefix.
- Module and function doc comments carry the *why* — the alternative rejected, the failure it
  prevents, the reference line it implements. Match that density rather than adding bare comments.
- Tests are named as the sentence of the rule they enforce
  (`a_map_that_is_not_symmetric_is_refused_rather_than_played`), one file per area under
  `engine/src/tests/`.
