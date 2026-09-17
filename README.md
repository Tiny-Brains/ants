# ants

Ants is the reference game cartridge for TinyBrains: a deterministic contest between colonies on a
wrapping grid, after the 2011 Google AI Challenge. This repository builds the WebAssembly component
the platform plays, its manifests, the boards, and the replay viewer — and keeps the platform's own
trained entries for the game beside the rules they encode.

## Scope

**It owns**

- The rules: turn resolution, visibility, end conditions, ranks and scores.
- The boards: the catalogue under `maps/`, its validation and player symmetry, and the generator that
  writes it from one recipe a preset ([`mapgen/`](mapgen/recipes)).
- The packed game state, and replay reconstruction from recorded actions.
- The plugin ABI (`engine/plugin.toml`) and the generated registration manifest (`cartridge.json`).
- The replay viewer (`viz/`), which re-simulates through the component.
- The platform's trained entries and the pipeline that trains them ([`baselines/`](baselines/README.md)):
  competitor entries with no special access, kept here so an observation change and the encoding
  that reads it land together. They are not the cartridge, and the component knows nothing of them.

**It does not**

- Schedule or play ladder matches; [Kalam](https://github.com/Tiny-Brains/kalam) runs the cartridge.
- Run models or evaluate adapters; Orion's `models` entity does, and Kalam reads the policy head.
- Admit competitors or keep ratings; [Soma](https://github.com/Tiny-Brains/soma)'s clocks do.
- Document the game for competitors; [the competitor guide](https://github.com/Tiny-Brains/web/tree/main/docs)
  does, and is where the protocol is published.

## Where it sits

```text
[ants] --docker build--> tinybrains/ants:<tag>   /artifacts/ == dist/
                               |
         +---------------------+----------------------+---------------------+
         v                     v                      v                     v
  [Kalam package]       [DevOps loader]          [web + book]        [tinybrains CLI]
   tb-ants.wasm          cartridge.json,          viz/                 a games registry
   plugin.toml/json      reference/, the digest                        `path` at dist/
```

| Direction | Party | Over | What moves |
|---|---|---|---|
| called by | Kalam | Orion plugin ABI | Worlds, observations, actions and results |
| read by | DevOps loader | `cartridge.json` | Presets, seat counts, the board catalogue, limits, the adapter budget |
| read by | Admission | `reference/observations.json` | The payloads an adapter is validated against |
| read by | web, the book, `tinybrains view` | `viz/` | The viewer bundle and the digest it was transpiled from |
| read by | `tinybrains` | a registry `path` | The component, the manifest, `maps/`, `reference/` and `viz/` |

## Interface

The component is `tb-ants.wasm`, plugin id `tb.ants`, ABI `orion:plugin@1.0.0`.
[`engine/plugin.toml`](engine/plugin.toml) declares each function's input fields.

| Export | Input | Result |
|---|---|---|
| `tb.ants.worldgen` | Seeds and a preset; optionally `players`, `max_turns`, and a board by id or inline (`map`, or `maps` per seed) | The initial packed `wave_state` |
| `tb.ants.observe` | `wave_state`, and opaque per-seat `refs` | A view per live seat, its ref echoed |
| `tb.ants.step` | `wave_state` and actions | The next `wave_state`, `done`, `ended` and the replay delta |
| `tb.ants.finish` | `wave_state` | Ranks, scores, the ending reason, and the board each ended match was played on |
| `tb.ants.replay-decode` | A replay envelope, and a `turn` or a `from`/`to` range | One frame, or a range in one pass |

`wave_state` is opaque to every caller. Actions follow the last observation's order, or name
`{m, seat, action}` explicitly. The hyphen in `replay-decode` is part of the name.

What a view contains, what an action is and what a cartridge must honour are published in the
competitor guide: *What your model sees*, *What your model answers*, and *Adding a game*.

### The artifact set

`./build.sh` writes `dist/`, and the image carries the same tree under `/artifacts/`:

| Path | What it is | Made by |
|---|---|---|
| `tb-ants.wasm` | The component; its sha256 is the **engine digest** | `cargo build` + `wasm-tools component new` |
| `plugin.toml`, `plugin.json` | The ABI, authored and as JSON | `engine/plugin.toml`; `tools/package.py` |
| `cartridge.json` | Presets, limits, the adapter budget, the board catalogue, `about` | `engine/src/bin/manifest.rs`; `tools/package.py` adds `maps` and `engine/about.json` |
| `maps/` | The boards, as committed | copied from `maps/` |
| `reference/observations.json` | The observations admission validates an adapter against | `engine/src/bin/reference.rs` |
| `viz/` | The viewer bundle and `engine.json`, the digest it carries | `viz/build.sh` |

## Run it, test it

This is a plugin, so there is no server. The build needs stable Rust with the
`wasm32-unknown-unknown` target, `wasm-tools`, and Python 3.11 or newer; the viewer also needs Node.
No database, object store or running platform is needed.

```sh
./build.sh                              # the gate, then every artifact, into dist/
viz/build.sh                            # the viewer, into dist/viz/ -- after ./build.sh
docker build -t tinybrains/ants:dev .   # the artifact image: what actually ships
tools/deny.sh                           # just the determinism check

cd engine                               # the crate is its own Cargo workspace
cargo test                              # just the host suite
cargo fmt --check && cargo clippy --all-targets -- -D warnings

cd mapgen                               # the board factory, a crate of its own
cargo run -- generate                   # every recipe under recipes/, into ../maps/
cargo run -- generate recipes/maze-2.toml
cargo run -- check                      # every board is what its recipe makes, byte for byte
cargo run --release -- check --play 6   # ...and play each, the same walker in every seat
cargo run -- show ../maps/maze-2-00.json
MAPGEN_DEBUG=1 cargo run -- generate    # say why each refused attempt was refused
```

**A board is a recipe and a seed.** `mapgen/recipes/<preset>.toml` describes a preset's boards in
area knobs — area size, coverage, closure, wall thickness, loops, fill, hills, food — plus windows the
finished board must fall in; `generate` draws each board until it passes, and records its seed and
measurements in the file. Edit a recipe and regenerate; never edit a board. `build.sh` runs
`mapgen`'s tests, which regenerate every committed board and compare bytes.

`build.sh` runs the determinism check and the tests before it compiles anything, and ends by
printing the engine digest. **A local digest is not the platform's.** Only the image pins rustc
exactly and remaps build paths, so only the image's digest is reproducible — `docker build
--no-cache` lands on the same one every time. Build it once and let every consumer take that image.

To play or check against this checkout rather than an image, point a games registry at `dist/`:

```toml
[games.ants]
name = "Ants"
path = "../ants/dist"
```

An image's `/artifacts/` copied out works the same way:

```sh
id=$(docker create tinybrains/ants:dev) && docker cp "$id":/artifacts/. dist && docker rm "$id"
```

**A competitor reads neither.** [ants-starter](https://github.com/Tiny-Brains/ants-starter) pins a **release**: the image's `/artifacts/`
as one `ants-artifacts.tar.gz`, tagged `engine-<12 hex>`, which `tinybrains` downloads once and
refuses unless the archive and the component inside it hash to what their `games.toml` declares.

```sh
tools/release.sh              # build the image, pack /artifacts/, print the registry block
tools/release.sh --publish    # and create the release: a clean tree, HEAD on origin/main
```

The archive is packed deterministically, so the same image packs to the same digest. A tag is never
re-cut: registries pin the archive, and a replaced file would break every one of them.

## What a deployment owes it

- **One image, pinned.** `ANTS_REF` reaches Kalam's package, the loader and web, and they must
  agree: a replica on another digest claims nothing, and a viewer on another digest draws a match
  that never happened.
- **A signature per build.** Every rebuild is a new digest, and a component whose Ed25519
  signature does not match brings the node up `degraded`.
- **A declared digest.** `games.active_engine_digest` and each replica's `engine_digest` move with
  the component — as a patch when the rules did not change, on a new season when they did.

## Layout

One directory per toolchain — `engine/` (Rust), `mapgen/` (Rust, host-only), `viz/` (Node),
`baselines/` (Python) — and at the root only what spans them: the boards, the build, and the image.

```text
engine/                    the cartridge: a Rust crate compiled to the wasm component
  src/lib.rs               plugin dispatch: the five exports and their JSON shapes, no rules
  src/grid.rs              wrapping, distances, symmetry, directions, bitmaps, the seeded RNG
  src/maps.rs              the board as a file: parsing, validation, the catalogue, presets derived from it
  src/state.rs             one match while it is played
  src/turn.rs              a turn in its fixed order, the cutoff counter, the end conditions, ranks
  src/food.rs              food at the hidden rate, in shuffled symmetric sets
  src/observe.rs           what a seat sees, and what it has been shown
  src/codec.rs             the packed, base64 wave_state
  src/replay.rs            the action stream, and re-simulating it into frames
  src/authoring.rs         host-only: the reference observations
  src/bin/                 manifest, reference -- the generators
  src/tests/               the host suite, one file per area; fixtures/ holds a platform-written replay
  build.rs                 compiles ../maps/ into the component
  plugin.toml              the authored Orion ABI
  about.json               the game's introduction, folded into cartridge.json
mapgen/                    the board factory: its own crate, so tuning it moves no engine digest
  recipes/                 one TOML file a preset: the knobs, and the windows a board must fall in
  src/make.rs              one board: areas, homes, coverage, walls, doors, hills, fill, food
  src/measure.rs           a board in numbers, and the rules every board obeys, checked on the board
  src/set.rs               a recipe's whole set, the file format, and the engine's own validation
  src/play.rs              the seat-bias smoke check: one walker in every seat
viz/                       the replay viewer: one bundle for the web app, the book and the CLI
baselines/                 the trained entries, and how they were trained (not in the image)
maps/                      the boards, one JSON file each
tools/deny.sh              the determinism check
tools/package.py           finishes dist/: plugin.json, the catalogue, the boards, the report
build.sh                   the gate, then every artifact, into dist/
Dockerfile                 the artifact image
dist/                      build output, gitignored; the image's /artifacts/
```

## What must stay true

- **Game logic uses integer arithmetic.** `tools/deny.sh` refuses floating point in `engine/src/`, and the
  replay tests check reconstruction. It is what lets a replay be an action stream.
- **Seeds and actions determine the match.** No ambient randomness, no clock. The seed chooses the
  board from the preset's pool when the caller names none, so a competitor cannot train against a
  board they picked.
- **Exploring is remembered.** A model is a pure function of one observation, so the engine folds
  each seat's vision into what it knows every turn, and a view carries known water.
- **A view is observer-relative.** Relabel every seat and ask the same player again, and the bytes
  are identical; `a_view_is_observer_relative` checks the engine rather than a stand-in.
- **Every committed board is fair and whole.** A file cannot be symmetric by construction, so
  `engine/src/maps.rs` asserts that it is congruent under its own shift, that all its land is one
  walkable body and that every hill has a way off, and the corpus test checks every board that
  ships. `mapgen` holds the boards to more — clearings, no enemy hill in view — and proves each is
  what its recipe makes.
- **A replay carries the board it was played on**, and the decode test runs against an envelope the
  platform actually wrote.
- **The viewer re-simulates with the cartridge, never a copy of it.** A JavaScript
  re-implementation of a rule would be a second engine.
- **Nothing generated is committed, and everything generated is in `dist/`.** The registration
  manifest is generated from the boards — a preset exists because boards declare it — so it cannot
  disagree with them.
- **The protocol is published, and moves with the engine.** A change to what a view carries is a
  change to the competitor guide and to `baselines/planes.py` in the same batch.
- **The rules are the 2011 contest's rules.** Where the published specification and the contest
  engine (`aichallenge/ants/ants.py`) disagree, the engine wins. The Focus Battle page's worked
  examples ship as `spec_scenario_*` tests.

## Status

**16 September 2026 — restructured, and the same game.** Every generated file now lands in one
gitignored `dist/`, laid out exactly as the image's `/artifacts/`, so a games registry can point at
a checkout's `dist/` or an extracted image and read the same tree; the CLI's viewer lookup and every
sibling's registry `path` moved with it. The crate moved into `engine/`, beside `viz/` and
`baselines/`, so the root holds only what spans the three. `engine/build.rs` embeds `maps/`,
replacing a generated `src/maps_gen.rs` and the build step that had to run before `cargo test`.
`docs/` and `schema/` are gone: the protocol and the cartridge contract are published in the
competitor guide, and the schema checks of hand-written examples became
`a_view_carries_exactly_the_fields_a_model_is_promised`, which checks engine output. `map.rs` is
`grid.rs`, `mapfile.rs` is `maps.rs` and holds the presets, and food spawning left `state.rs` for
`food.rs`. **No rule moved**: `cartridge.json`, `plugin.json`, `reference/observations.json`,
`mapgen` output and a hash of every `observe`, `step`, `finish` and `replay-decode` output over nine
seeded waves, random and greedy, are byte-identical to the previous commit, as are the image's
`/artifacts/` layout and viewer bundle. **The digest moved**, as any source edit does: the image
builds `sha256:281a84f1…` where it built `sha256:185a2845…`, reproducibly under `--no-cache`. The
ladder plays the old component until it is cut over as a patch, and the book's
`tutorials/replays/real-match.json` has to be re-captured on the new one. `cargo test` is 82.

**16 September 2026 — the baselines live here** ([`baselines/`](baselines/README.md), devops
decision N20). ants-starter installs `tb_baselines` with `#subdirectory=baselines`, so that path is
public.

**Owed.** The 10,000-match cross-host conformance run: the transpiled component decodes a recorded
match and agrees with it, but determinism across the platform's runtime and the browser is checked
in the small only.

**17 September 2026 — the cartridge is released.** `engine-281a84f10d59` carries the image's
`/artifacts/` as one archive, cut by the new `tools/release.sh`, and drill and ants-starter pin it —
so a competitor no longer clones this repository, or builds it, to play or check a model (devops
decision N21). No source changed and the digest did not move.

**17 September 2026 — boards are generated from recipes, and a board carries its shift.** The 24
boards are gone and 32 replace them, eight a preset in four presets: `open-2` (64 × 96, open ground),
`maze-2` (96 × 96, a tight lattice maze), `cave-2` (96 × 96, caverns, two hills a seat) and `rooms-4`
(128 × 128, walled rooms, **four seats**). They come from `mapgen/`, a crate of its own driven by one
recipe a preset, which replaces `authoring::worldgen`, `src/bin/mapgen.rs` and the `PRESETS` table;
the presets in `cartridge.json` are now derived from the boards. The old generator delivered half the
water it was asked for, left every map in a preset with the same hill geometry and no detour at all,
and left sealed pockets food could spawn in on three maze boards. In the engine: the map file's
`symmetry` is read rather than re-derived, so seats may be half the rows, half the columns or both
apart (and it travels in `wave_state`); hills are whole orbits, so a seat may have several; and
validation refuses land that cannot be walked to and a hill with no way off (the old check included
the hill's own square and could not fail). A board named inline no longer needs its preset to be a
pool. **No rule moved**: every `observe`, `step` delta and `finish` output over five inline boards,
random and food-seeking play, four seeds, up to 400 turns, hashes the same on this engine and the
previous commit's. **The digest moved**, and the boards with it: a local build prints
`sha256:f684c0d9…`; the image's is not built yet. `cargo test` is 88, and `mapgen`'s is 5.

**Owed, from the boards.** A release, and ants-starter's `games.toml` and match files moved from
`standard` to the new names on it. Seasons name presets: the deploy's list (devops
`soma.toml.tmpl`) now names the three two-seat presets and not `rooms-4`, because the pairing clock
picks a preset before it seats anyone and spends a version's want when the roster cannot fill four
seats, so on a small roster a four-seat preset starves every version of pairings; that is Soma's to
fix before `rooms-4` is played. Five to eight seats generate and validate (`mapgen`'s tests cover
every count) but Kalam claims at most four, so none ship. The reference observations cover every
preset, but no view in them numbers an opponent past 1: the greedy walker's `rooms-4` colonies never
meet, so an adapter that mishandled owner 2 or 3 would still be admitted. And the greedy walker's
colonies stay at two to four ants on `maze-2`, whose two-square corridors punish ants ordered into
each other — tight by design, and worth watching once models play it.

## More

- [The competitor guide](https://github.com/Tiny-Brains/web/tree/main/docs): the rules, what a model
  sees and answers, and *Adding a game*.
- Related repositories: [Kalam](https://github.com/Tiny-Brains/kalam), [Soma](https://github.com/Tiny-Brains/soma), [DevOps](https://github.com/Tiny-Brains/devops).
- Apache-2.0: see [LICENSE](LICENSE).
