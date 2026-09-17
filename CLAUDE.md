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
the component: the rule above is about `src/`, and the determinism check, the build and the image
never see `baselines/`.

The parent `tinybrains/CLAUDE.md` describes the platform this sits in; read it for anything
crossing a repo boundary. `README.md` here is the canonical page, in the platform's standard shape —
update its **Status** when work lands. **There are no design docs here**: what a model sees and
answers, and what a cartridge must honour, are published in the competitor guide
(`../web/docs/src/models/observation.md`, `actions.md`, `platform/adding-a-game.md`, and the rules
under `games/ants/`). Module doc comments carry the rest of the why.

## Commands

Needs stable Rust, the `wasm32-unknown-unknown` target, `wasm-tools`, and Python 3.11+
(`tomllib`). Node only for the viewer. **The crate is `engine/`, its own Cargo workspace** — there is
no `Cargo.toml` at the root, so every `cargo` command runs from `engine/`.

```sh
./build.sh          # the whole gate, then every artifact, into dist/
viz/build.sh        # the viewer, into dist/viz/ (after ./build.sh)
docker build -t tinybrains/ants:dev .   # the artifact image -- the build that ships
tools/release.sh    # pack the image's /artifacts/ for a release; --publish creates it
tools/deny.sh       # just the determinism check (no floating point in game logic)
(cd viz && node check.mjs)   # just the viewer checks -- geometry, and CSS scoping

cd engine
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # edition 2024; rustfmt.toml is not the default
cargo test          # just the host suite -- 90 tests
cargo test a_replay_re_simulates_the_match_it_recorded    # one test by name
cargo test spec_scenario_                                  # the spec's worked fights
cargo test measure_what_random_play_produces -- --nocapture  # diagnostic, not a guarantee
```

`build.sh` runs `tools/deny.sh` and `cargo test`, clears `dist/`, builds and component-izes the
wasm, runs the `manifest` and `reference` generators into `dist/`, then `tools/package.py` writes
`plugin.json`, folds the board catalogue and `engine/about.json` into `cartridge.json`, copies
`maps/` and `engine/plugin.toml`, and prints the **engine digest**. Clearing `dist/` also drops
`dist/viz/`, on purpose: a viewer transpiled from the previous component is a viewer for some other
engine.

The two host binaries are generators (from `engine/`):

```sh
cargo run --bin manifest > ../dist/cartridge.json                  # then tools/package.py folds in maps + about
cargo run --bin reference > ../dist/reference/observations.json    # -- --only rooms-4:20260918:400 for one spec
```

The boards come from `mapgen/`, a crate of its own (from `mapgen/`):

```sh
cargo run -- generate                    # every recipe under recipes/, into ../maps/ -- replaces a preset's boards whole
cargo run -- generate recipes/maze-2.toml
cargo run -- check                       # every committed board is what its recipe makes, byte for byte
cargo run --release -- check --play 6    # ...and play each one, the same walker in every seat
cargo run -- show ../maps/maze-2-00.json # draw a board and its metrics
cargo test                               # 6 tests; build.sh runs them. MAPGEN_DEBUG=1 says why attempts fail
cargo run --release -- sweep             # 11 recipe styles x 2-8 seats: generated, played congruently, replayed
cargo run --release -- sweep --seats 8 --styles cave,rooms --boards 10 --turns 1000 --json out.json --export dir
```

**`sweep` is the fairness proof, not a benchmark.** Every seat plays one policy that reads only its own
view moved into seat 0's frame, so on a fair board under fair rules every view, every turn, is seat 0's
moved by the shift, and every match ends level; a divergence is a bug, reported with the first turn the
referee's board broke symmetry (`MAPGEN_TRACE=1` draws the squares). It found two: spawn ties broken by
square rather than list order, and an open-ended `replay-decode` range over a cut-off recording. Run it
after any change to a rule that could treat seats differently.

**A board is a recipe and a seed; never edit one.** A recipe (`mapgen/recipes/<preset>.toml`) is
area knobs — size, `grid`, `warp`, `coverage_pct`, `closure_pct`, `wall`, `loops_pct`, `fill`, hills,
food — plus `accept` windows a finished board must fall in. `check` regenerates every set and
compares bytes, so a hand edit or a generator change nobody regenerated for fails `build.sh`. It is a
separate crate so tuning it is not an engine-digest change; regenerating the boards is one, because
`build.rs` compiles them in. `mapgen` links this crate and validates through `tb.ants.worldgen`, so
`build.rs` must keep building with an empty `maps/` (it warns; `tools/package.py` refuses to ship
one).

The baselines resolve the cartridge at `../dist` (`baselines/games.toml`), so they need `./build.sh`
run here first. Their guide has the training commands:

```sh
cd baselines && pytest tests/ -q   # the adapter conformance gate; needs `tinybrains` on PATH
```

**An observation change is a baselines change.** `planes.py` encodes what `engine/src/observe.rs` sends,
and the conformance test runs over `dist/reference/observations.json`, so a change to either lands
with the encoding and its regenerated manifest in the same commit — and with the competitor guide's
*What your model sees*. A retrained model is not a rebuild: `baselines/models/` is committed, like
`maps/`, because training is neither cheap nor bit-reproducible.

## Architecture

**Five exports, dispatched by name in `engine/src/lib.rs` and nothing else:**

```
tb.ants.worldgen(seeds[], preset, players?, max_turns?, map?/maps?)  → wave_state
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
| `src/maps.rs` | The board as a file: parsing, validation (the board's own shift, whole hill orbits, one walkable body of land); the catalogue (`MAPS`, embedded by `build.rs`); presets, derived from it |
| `src/state.rs` | One match while it is played; the cutoff counter's constants |
| `src/turn.rs` | Turn resolution in a fixed order: move → attack → raze → spawn → gather → spawn food; ending; ranks |
| `src/food.rs` | Food at the hidden rate: the rate, the symmetric sets, their shuffled rotation, the pending queue |
| `src/observe.rs` | One seat's view: fog, known water, `vis`, observer-relative owners |
| `src/codec.rs` | The packed, base64 `wave_state`. Opaque outside this file; no version field |
| `src/replay.rs` | Action-stream recording and re-simulation into frames |
| `src/authoring.rs` | Host-only: the reference observations — unreachable from a plugin call, so the linker drops it from the component. The board factory is `../mapgen/` |
| `build.rs` | Embeds every board under `../maps/`; Cargo re-runs it when they change |
| `plugin.toml`, `about.json` | The authored ABI, and the game's introduction folded into `cartridge.json` |

**Four properties that explain most of the design:**

1. **Integer arithmetic only in game logic**, so the platform's build and the browser's agree bit
   for bit — which is what lets a replay be an action stream instead of frames. `tools/deny.sh`
   greps `engine/src/` for float constructs, excluding `tests/` and `bin/`, which are host tooling
   and never reach the component. Nothing in Rust enforces this.
2. **Seeds and actions determine the match.** No ambient randomness, no time. The seed also picks
   the board from the preset's pool when the caller passes none — deliberately, so a competitor
   cannot train against a board they chose.
3. **Exploring is remembered.** A model is a pure function of one observation with no state channel,
   so `turn.rs` folds each seat's vision into `known` every turn and observations carry *known
   water*. The per-player seen-masks are ~60% of `wave_state`; that cost is the point.
4. **Boards are files.** 64 committed under `maps/` at the root (content, not source — `mapgen/`
   writes them from its recipes and `dist/maps/` ships them), four per preset across sixteen
   presets from `open-2` to `maze-8` — the runs of one L16 orthogonal array over seats (2 to 8),
   terrain, board size (80 to 152 a side), hills a seat and food, each recipe's header naming its
   run — validated and compiled in. A board carries its **shift**
   (`symmetry`): seat `k`'s board is seat 0's moved `k` times by it, it travels in `wave_state`, and
   `food::sets` and observer-relative owners both follow it. Hills are listed in orbits, so hill `i`
   is seat `i % players`'s. A preset exists because boards declare it, and has one seat count. A
   replay envelope carries its own board *and* its seed, so it re-simulates correctly after the
   catalogue moves on.

## What breaks if you forget it

- **Everything generated is in `dist/`, nothing in it is committed, and nothing outside it is
  generated.** `dist/` is laid out exactly as the image's `/artifacts/`, which is what lets a games
  registry `path` point at either. Consumers read that layout by path — the component by extension
  at the root, `cartridge.json`, `maps/`, `reference/observations.json`, `viz/viz.js` and
  `viz/engine.json` — in `devops/cli` (`registry.rs`, `serve.rs`, `cmd/mod.rs`), kalam's and web's
  Dockerfiles, `web/docs/Dockerfile` and `tutorials/build.sh`, and the devops loader. Rename one
  and grep the siblings in the same batch.
- **The image is the build, and the digest is reproducible.** `Dockerfile` pins rustc *exactly*
  (`rust:1.98-trixie` floats to the newest patch, and a patch bump moves the component's bytes) and
  passes `--remap-path-prefix`, because rustc bakes the absolute path of every source file a panic
  can name into the binary. So a local `./build.sh` lands on a different digest than the image;
  cite the image's. Build it **once** and have every consumer take that image.
- **Any source edit is a new engine digest**, comment-only ones included (panic locations carry
  line numbers). `games.active_engine_digest`, each replica's `engine_digest`, the plugin
  signatures and `viz/engine.json` all move with it. Say so in the commit and in README Status. To
  prove a refactor changed no rule, diff `cartridge.json`, `plugin.json`,
  `reference/observations.json` and `mapgen` output against a `git archive HEAD` build, and hash
  every `observe`/`step`/`finish` output turn by turn under a random and a greedy policy.
- **A new engine digest is a new release, or competitors keep playing the old one.** ants-starter's
  `games.toml` pins a release by the archive's digest and the `engine` digest, and has no checkout
  of this repository to fall back on (devops N21, N22). Once the ladder plays a new digest,
  `tools/release.sh --publish` and paste the block it prints there. Never re-cut a tag:
  a registry pins the archive's bytes. The archive is the image's `/artifacts/`, never `dist/` —
  only the image's digest is the ladder's.
- **Never hand-edit a generated file.** `engine/plugin.toml` is the authored ABI (`plugin.json` is
  derived); `mapgen/recipes/` are authored and `maps/` is generated from them (`mapgen check` fails on
  a hand edit); `cartridge.json`'s presets are derived from the boards; `engine/about.json` is the one
  hand-written input folded into the manifest.
- **A preset name is a contract with other repositories.** Seasons and the deploy's `[vars]` list
  presets by name (devops `compose/orion/soma.toml.tmpl`, soma `docs/config.md`), match files name
  them (ants-starter, the book's tutorials and *Testing*), and baselines' tests pin one. Renaming or
  retiring a preset is a change to each, and the starter's only through a release. A preset with more
  seats than a small roster can fill is never paired until one can (soma's `choose` and trial query
  filter by it), and Kalam claims at most eight seats, mapgen's ceiling.
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
  `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`, and `devops`'
  `seed-baselines.sh` reads `../ants/baselines/models`. Renaming the directory or the package
  breaks every starter clone. It stays out of the image: `.dockerignore` excludes it, so no edit
  there can move the engine digest.

## Conventions

- Commits go straight to `main`, no feature branches. Messages are a sentence describing the change
  in the platform's voice ("Separate the engine from the tools that generate its artifacts"), not a
  conventional-commit prefix.
- Module and function doc comments carry the *why* — the alternative rejected, the failure it
  prevents, the reference line it implements. Match that density rather than adding bare comments.
- Tests are named as the sentence of the rule they enforce
  (`a_map_that_is_not_symmetric_is_refused_rather_than_played`), one file per area under
  `engine/src/tests/`.
