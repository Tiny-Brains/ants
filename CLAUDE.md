# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

`ants` is the reference **game cartridge** for TinyBrains: a Rust crate compiled to a WebAssembly
component (`tb-ants.wasm`, plugin id `tb.ants`, ABI `orion:plugin@1.0.0`) that Kalam loads and calls.
There is no server, no database, no network, and no clock — the component imports nothing. It owns
the rules of Ants (the 2011 Google AI Challenge game) and knows nothing about the platform: not
ratings, not admission, not models, not matches as scheduling units.

The parent `tinybrains/CLAUDE.md` describes the nine-repo platform this sits in; read it for
anything crossing a repo boundary. `README.md` here is the canonical page and is maintained in the
platform's standard shape (Scope / Where it sits / Interface / Run it, test it / Layout / What must
stay true / Status) — update **Status** and `../design/tracker.md` when work lands.

## Commands

All from this repository's root. Needs stable Rust, the `wasm32-unknown-unknown` target,
`wasm-tools`, and Python 3.11+ (`tomllib`). `jsonschema` and Node are optional (schema check, viz).

```sh
./build.sh          # the whole gate, then every committed artifact
./deny.sh           # just the determinism check (no floating point in game logic)
cargo test          # just the host suite -- 74 tests, ~20s
cargo test a_replay_re_simulates_the_match_it_recorded    # one test by name
cargo test spec_scenario_                                  # the spec's worked fights
cargo test measure_what_random_play_produces -- --nocapture  # diagnostic, not a guarantee
```

`build.sh` runs `deny.sh`, regenerates `src/maps_gen.rs`, runs `cargo test`, builds and
component-izes the wasm, then writes `plugin.json`, `cartridge.json`, `reference/observations.json`,
validates the schemas if `jsonschema` is importable, and prints the **engine digest**.

The three host binaries are generators, and `build.sh` calls each:

```sh
cargo run --bin manifest > cartridge.json                  # then tools/cartridge.py folds in the catalogue + about.json
cargo run --bin reference > reference/observations.json    # -- --only cell:20260908:600 for one spec
cargo run --bin mapgen -- --preset cell --seed 7 --id cell-07 > maps/cell-07.json
```

The viewer (`viz/`) has its own toolchain and its own committed `dist/`:

```sh
cd viz && ./build.sh   # jco transpile + copy + geometry checks; writes dist/ and dist/engine.json
cd viz && node check.mjs   # just the checks -- geometry, and CSS scoping
```

## Architecture

**Five exports, dispatched by name in `src/lib.rs` and nothing else:**

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

**Module responsibilities** — the split is deliberate, keep it:

| File | Owns |
|---|---|
| `src/lib.rs` | Plugin dispatch and JSON shapes. No rules |
| `src/map.rs` | Wrapping grid, `PRESETS` (standard/maze/cell), symmetry, distances, RNG, bitmaps |
| `src/state.rs` | One match while it is played; the hidden food rate; the cutoff counter |
| `src/turn.rs` | Turn resolution in a fixed order: move → attack → raze → spawn → gather → spawn food |
| `src/observe.rs` | Visibility, and folding what a seat sees into what it knows |
| `src/mapfile.rs` | The board as a file: parsing, validation, symmetry assertion, catalogue |
| `src/maps_gen.rs` | **Generated** by `tools/embed-maps.py`: every board under `maps/`, compiled in |
| `src/codec.rs` | The packed, base64 `wave_state`. Opaque outside this file; no version field |
| `src/replay.rs` | Action-stream recording and re-simulation |
| `src/authoring.rs` | Host-only: procedural worldgen (the board factory) and artifact generators — unreachable from a plugin call, so the linker drops it from the component |

**Four properties that explain most of the design:**

1. **Integer arithmetic only in game logic**, so the platform's build and the browser's agree bit
   for bit — which is what lets a replay be an action stream instead of frames. `deny.sh` greps
   `src/` for float constructs, excluding `src/tests/` and `src/bin/`, which are host tooling and
   never reach the component. Nothing in Rust enforces this.
2. **Seeds and actions determine the match.** No ambient randomness, no time. The seed also picks
   the board from the preset's pool when the caller passes none — deliberately, so a competitor
   cannot train against a board they chose.
3. **Exploring is remembered.** A model is a pure function of one observation with no state channel,
   so `turn.rs` folds each seat's vision into `known` every turn and observations carry *known
   water*. The per-player seen-masks are ~60% of `wave_state`; that cost is the point.
4. **Boards are files.** 24 committed under `maps/`, eight per preset, validated and asserted
   symmetric at build time, then compiled in. A replay envelope carries its own board *and* its
   seed, so it re-simulates correctly after the catalogue or preset table moves on.

## What breaks if you forget it

- **Generated artifacts are the shipped artifacts, and they are committed**: `tb-ants.wasm`,
  `plugin.json`, `cartridge.json`, `src/maps_gen.rs`, `reference/observations.json`, and
  `viz/dist/`. A source change without its regenerated output ships a stale package and nothing at
  runtime notices. Run `./build.sh` (and `viz/build.sh` if the component or viewer changed) and
  commit the output with the change that caused it.
- **A rebuilt component is a new engine digest.** Kalam's vendored copy,
  `games.active_engine_digest`, the season, the plugin signatures, and `viz/dist/engine.json` all
  have to move with it. Say so in the commit and in README Status.
- **Never hand-edit a generated file.** `plugin.toml` is the authored ABI (`plugin.json` is derived);
  the preset table in `map.rs` is authored (`cartridge.json`'s presets are derived); `about.json` is
  the one hand-written input folded into the manifest.
- **`embed-maps.py` runs before `cargo test`** in `build.sh` — the corpus test can only see the maps
  that step embedded. After adding a board under `maps/`, regenerate before testing.
- **Rules changes cite the reference.** Where the published specification and the 2011 contest
  engine (`aichallenge/ants/ants.py`) disagree, **the engine wins** — it is what every bot was
  scored against. Comments carry `ants.py:NNN` line cites; keep that habit. The Focus Battle page's
  worked examples ship as `spec_scenario_*` tests.
- **`docs/cartridge.md` and `docs/protocol.md` are normative.** Where this crate and those documents
  disagree, they are right and the code is a bug. `schema/` documents what the build actually
  writes; `schema/validate.py` runs in the build so the two cannot drift unnoticed.
- **The viewer re-simulates through the cartridge**, via the jco-transpiled component. A JavaScript
  re-implementation of any rule would be a second engine. `viz/check.mjs` also fails the build if a
  stylesheet rule is not scoped to `.tb-viz` (an unscoped selector once relaid out the web app's
  header).
- **`wave_state` stays opaque** to every caller, and turn order in `turn.rs` is a rule, not an
  implementation detail.

## Conventions

- Commits go straight to `main`, no feature branches. Messages are a sentence describing the change
  in the platform's voice ("Separate the engine from the tools that generate its artifacts"), not a
  conventional-commit prefix.
- Module and function doc comments carry the *why* — the alternative rejected, the failure it
  prevents, the document section it implements. Match that density rather than adding bare comments.
- Tests are named as the sentence of the rule they enforce
  (`a_map_that_is_not_symmetric_is_refused_rather_than_played`), one file per area under
  `src/tests/`.
