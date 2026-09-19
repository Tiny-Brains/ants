# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`ants` is the reference game cartridge for TinyBrains: a Rust crate in `engine/` compiled to a
WebAssembly component (`tb-ants.wasm`, plugin `tb.ants`, ABI `orion:plugin@1.0.0`) that Kalam plays
and Soma validates boards with. It imports nothing: no server, database, network or clock. It owns
the rules of Ants and knows nothing of the platform. Three directories beside it are not the
component and carry their own toolchains: `mapgen/` (the board factory, a host-only Rust crate),
`viz/` (the replay viewer, Node) and `baselines/` (the training pipeline, Python, with its own
[CLAUDE.md](baselines/CLAUDE.md)). The baselines know the platform thoroughly, and that is fine
because the determinism check, the build and the release never see them. `README.md` is the human
guide (build, artifacts, releasing, boards, layout, invariants); the parent `../CLAUDE.md` covers
everything that crosses a repo boundary. The protocol is published in the competitor guide
(`../web/docs/src/models/observation.md`, `actions.md`, `platform/adding-a-game.md`, and the rules
under `games/ants/`), not here.

## Checks

Needs rustup (`rust-toolchain.toml` pins rustc and the wasm32 target), `wasm-tools` at the
`WASM_TOOLS_VERSION` in `build.sh`, Python 3.11+ and, for the viewer, Node. No Docker. `engine/` and
`mapgen/` are separate Cargo workspaces, so every `cargo` command runs from one of them.

```sh
./build.sh                  # deny.sh, engine tests, mapgen tests, then every artifact into dist/; prints the digest
viz/build.sh                # the viewer into dist/viz/, then check.mjs (after ./build.sh)
(cd engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings)
(cd mapgen && cargo fmt --check && cargo clippy --all-targets -- -D warnings)
(cd viz && node check.mjs)  # the viewer checks alone
(cd baselines && pytest tests/ -q)   # the encoding conformance gate; see baselines/CLAUDE.md
tools/pack.py DIR           # dist/ as the release archive, its digests, and the tag it would take

cd engine
cargo test a_replay_re_simulates_the_match_it_recorded    # one test by name
cargo test spec_scenario_                                  # the spec's worked fights
cargo test measure_what_random_play_produces -- --ignored --nocapture   # a diagnostic, not a guarantee; minutes
cargo run --bin reference -- --only basic-small-3p:20260919:400          # one reference spec
```

`build.sh` clears `dist/` first, which also drops `dist/viz/` on purpose: a viewer transpiled from
the previous component is a viewer for some other engine. Rerun `viz/build.sh` after it.

The mapgen commands are in README §Boards. A season's boards are made with the same commands
pointed outside every repository:

```sh
cd mapgen
cargo run -- generate --recipes ../../maps/recipes --maps ../../maps
cargo run -- check    --recipes ../../maps/recipes --maps ../../maps
cargo run --release -- sweep --seats 8 --styles cave,rooms --boards 10 --turns 1000 --json out.json --export dir
```

## Rules

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

**Module responsibilities** (under `engine/`). Keep the split:

| File | Owns |
|---|---|
| `src/lib.rs` | Plugin dispatch and JSON shapes. No rules |
| `src/grid.rs` | Wrapping geometry, distances, symmetry, directions, the view/attack/spawn radii, `Bits`, `Rng` |
| `src/maps.rs` | The board as a file: parsing, validation (the board's own shift, whole hill orbits, one walkable body of land), and resolving what a caller sent: a board object, and nothing else |
| `src/state.rs` | One match while it is played; the cutoff counter's constants |
| `src/turn.rs` | Turn resolution in a fixed order: move → attack → raze → spawn → gather → spawn food; ending; ranks |
| `src/food.rs` | Food at the hidden rate: the rate, the symmetric sets, their shuffled rotation, the pending queue |
| `src/observe.rs` | One seat's view: fog, known water, `vis`, observer-relative owners |
| `src/codec.rs` | The packed, base64 `wave_state`. Opaque outside this file; no version field |
| `src/replay.rs` | Action-stream recording and re-simulation into frames |
| `src/authoring.rs` | Host-only: the reference observations. Unreachable from a plugin call, so the linker drops it from the component |
| `plugin.toml`, `about.json` | The authored ABI, and the game's introduction folded into `cartridge.json` |

**Four properties that explain most of the design:**

1. **Integer arithmetic only in game logic**, so the platform's build and the browser's agree bit
   for bit, which is what lets a replay be an action stream instead of frames. `tools/deny.sh`
   greps `engine/src/` for float constructs, excluding `tests/` and `bin/`, which are host tooling
   and never reach the component. Nothing in Rust enforces this.
2. **Seeds, actions and the board determine the match.** No ambient randomness, no time. The
   caller chooses the board; on the ladder that is Soma's pair clock, which also assigns the seed,
   so a competitor cannot train against a board they chose.
3. **Exploring is remembered.** A model is a pure function of one observation with no state
   channel, so `turn.rs` folds each seat's vision into `known` every turn and observations carry
   *known water*. The per-player seen-masks are most of `wave_state`; that cost is the point.
4. **Boards are files, and the component carries none.** Five basic boards live under `maps/`
   (content, not source: `mapgen/` renders them and `dist/maps/` ships them). **They are the
   envelope**: `limits.boards` is what they span, the reference set is drawn on them, and a season's
   board, uploaded to Soma and never committed here, must fit inside it. Every board is validated
   before it is played, whoever sends it. A board carries its **shift** (`symmetry`): seat `k`'s
   board is seat 0's moved `k` times by it, it travels in `wave_state`, and `food::sets` and
   observer-relative owners both follow it. Hills are listed in orbits, so hill `i` is seat
   `i % players`'s, and a board states its own seat count. A replay envelope carries its own board
   *and* its seed, so it re-simulates whatever became of the board's season.

**Other rules:**

- **Turn order in `turn.rs` is a rule**, not an implementation detail, and `wave_state` stays
  opaque to every caller.
- **One engine.** The viewer re-simulates through the jco-transpiled component. A JavaScript
  re-implementation of any rule, or a workflow that interprets `wave_state`, would be a second
  engine.
- **Rules changes cite the reference.** Where the published specification and the 2011 contest
  engine (`aichallenge/ants/ants.py`) disagree, **the engine wins**: it is what every bot was scored
  against. Comments carry `ants.py:NNN` line cites; keep that habit. The Focus Battle page's worked
  examples ship as `spec_scenario_*` tests.
- **The protocol is published in the competitor guide, not here.** A change to a view's fields, an
  action's alphabet, or an ABI input is a change to `web/docs` in the same batch.
  `a_view_carries_exactly_the_fields_a_model_is_promised` pins the view's shape against engine
  output, so an accidental field fails a test rather than an adapter.
- **An observation change is a baselines change.** `planes.py` encodes what `engine/src/observe.rs`
  sends and the conformance test runs over `dist/reference/observations.json`, so a change to either
  lands with the encoding and its regenerated manifest in the same commit.
- **A board is a design; never edit a board.** `mapgen/recipes/<name>.toml` is the design written
  out; `design.rs` renders it and is the contract; `sample.rs` only proposes, so improving the
  sampler redraws nothing that was picked, while changing the renderer redraws every board. `check`
  re-renders every recipe and compares bytes, so a hand edit, or a renderer change nobody
  regenerated for, fails `build.sh`. `recipe.rs`, `make.rs` and `set.rs` (the area generator) remain
  only for `sweep` and their own tests. `mapgen` validates every board it writes through
  `tb.ants.worldgen`, the same check a season's upload runs on Soma's node.
- **`sweep` is the fairness proof, not a benchmark.** Every seat plays one policy that reads only
  its own view moved into seat 0's frame, so on a fair board under fair rules every view, every
  turn, is seat 0's moved by the shift and every match ends level. A divergence is a bug, reported
  with the first turn the referee's board broke symmetry (`MAPGEN_TRACE=1` draws the squares). Run
  it after any change to a rule that could treat seats differently.
- **Commits go straight to `main`.** A message is a sentence in the platform's voice ("Separate the
  engine from the tools that generate its artifacts"), not a conventional-commit prefix.
- **Doc comments carry the why**: the alternative rejected, the failure it prevents, the reference
  line it implements. Tests are named as the sentence of the rule they enforce
  (`a_map_that_is_not_symmetric_is_refused_rather_than_played`), one file per area under
  `engine/src/tests/`.

## Gotchas / what breaks

- **Any edit under `engine/src/` outside `tests/` and `bin/`, or to `engine/plugin.toml`, is a new
  engine digest**, comment-only edits included (panic locations carry line numbers).
  `games.active_engine_digest`, each replica's `engine_digest`, the plugin signatures and
  `viz/engine.json` all move with it, and it is a release and a ladder event. Say so in the commit.
  To prove a refactor changed no rule, diff `cartridge.json`, `plugin.json`,
  `reference/observations.json` and `mapgen` output against a `git archive HEAD` build, and hash
  every `observe`/`step`/`finish` output turn by turn under a random and a greedy policy.
- **The `build` workflow's digest is the platform's.** Four things make the component's bytes: the
  source, rustc (exact; a patch bump moves the bytes), `wasm-tools` (exact; it stamps its version
  in) and **the host rustc runs on**. `build.sh` remaps the crates.io, standard-library and checkout
  paths, so where the checkout is does not matter; the host does, and no flag removes it. Releases
  build on `aarch64-unknown-linux-gnu` (`ubuntu-24.04-arm`), so moving the runner to x86-64 is an
  engine-digest change no rule made. A laptop's digest is never the ladder's.
- **Publishing is deploying.** Soma's, Kalam's, web's and web/docs' Dockerfiles fetch the latest
  release unless `ANTS_RELEASE` pins a tag, so a release is the engine the next image of each of
  them declares, plays and draws, under a live season too.
- **A new engine digest is a new release, or competitors keep playing the old one.** ants-starter's
  `games.toml` pins a release by the archive's digest and the `engine` digest, and has no checkout
  to fall back on. `gh workflow run build.yml -f publish=true` cuts it and its notes carry the block
  to paste there. Never re-cut a tag. The tag is `engine-<12 hex>`, and `engine-<12 hex>-2` when the
  same engine ships a different archive; the workflow compares builds by the tar's content, so a
  build that reproduces a release says so rather than cutting another.
- **Nothing checks that the consumers agree.** Web's `scripts/check/configs.sh` compares Kalam's
  engine with the book's viewer, but not web's own viewer, Soma's image or the starter's pinned
  `engine`. Images built either side of a release carry two engines.
- **Everything generated is in `dist/`, nothing in it is committed, and nothing outside it is
  generated.** `dist/` is laid out exactly as a release's `ants-artifacts.tar.gz`, which is what lets
  a registry `path` point at a checkout and a `release` at the archive. Consumers read it by path
  (the component at the root, `cartridge.json`, `plugin.json`, `plugin.toml`, `maps/`,
  `reference/observations.json`, `viz/viz.js`, `viz/engine.json`): the CLI (`registry.rs`,
  `serve.rs`, `cmd/mod.rs`), Soma's, Kalam's and web's Dockerfiles, `web/docs/Dockerfile` and
  `web/docs/tutorials/build.sh`. Rename one and grep the siblings in the same batch; they reach it
  only through a release.
- **Never hand-edit a generated file.** `engine/plugin.toml` is the authored ABI (`plugin.json` is
  derived); `mapgen/recipes/` are authored and `maps/` is generated from them; `cartridge.json`'s
  catalogue and `limits.boards` are derived from the boards; `engine/about.json` is the one
  hand-written input folded into the manifest. `tools/package.py` refuses an empty `maps/`.
- **The basic boards are a contract with other repositories.** Their ids are named by
  ants-starter's match files, the book's board pages and the baselines' tests, and a rename reaches
  the starter only through a release. Their span IS `limits.boards`: shrink it and Soma refuses
  season boards that fit before; grow it and the reference set, and so every admitted adapter, must
  cover the new corner. Kalam claims at most eight seats, the envelope's top.
- **No season's board is ever committed here or shipped in a release.** Season boards are made
  outside every repository, uploaded to the season by an admin, and pushed to a backup repository
  only once the season has closed.
- **`viz/check.mjs` fails the build if a stylesheet rule is not scoped to `.tb-viz`**: the viewer's
  one `<style>` goes into the host document, and an unscoped selector relays out the host page.
  Web copies the viewer's modules by name, so a new static import in `viz.js` is a change to web's
  list too (`map.js` is loaded on first use for that reason).
- **`baselines/` is a public path.** ants-starter pip-installs `tb_baselines` from
  `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`, so renaming the directory or the
  package breaks every starter clone. It stays out of a release: `build.sh` never reads it and
  `tools/pack.py` packs `dist/` alone, so no edit there moves the engine digest.
