# ants

Ants is the reference game cartridge for TinyBrains: a deterministic contest between colonies on a
wrapping grid, after the 2011 Google AI Challenge. This repository builds the WebAssembly component
the platform plays, its manifests, the boards, and the replay viewer — and keeps the platform's own
trained entries for the game beside the rules they encode.

## Scope

**It owns**

- The rules: turn resolution, visibility, end conditions, ranks and scores.
- The boards: their validation and player symmetry, the five basic boards under `maps/` that define
  what a season's board may be (`limits.boards`), and the generator that renders a board from its
  design ([`mapgen/`](mapgen/recipes)) and proposes the designs. **A season's boards are not here**
  (N28): they are made with the same generator, uploaded to the season, and committed nowhere.
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
[ants] --build workflow--> release engine-<12 hex>: ants-artifacts.tar.gz  == dist/
                               |
         +---------------------+----------------------+---------------------+
         v                     v                      v                     v
  [Kalam package]        [web + book]           [ants-starter]       [tinybrains CLI]
   tb-ants.wasm,          viz/                   games.toml pins      a registry `release`,
   plugin.toml/json,                             the release          or a `path` at dist/
   cartridge.json,
   reference/
```

| Direction | Party | Over | What moves |
|---|---|---|---|
| called by | Kalam | Orion plugin ABI | Worlds, observations, actions and results |
| fetched by | Kalam, web, the book | the latest release, when their images build | The component and its manifests; the viewer |
| fetched by | Soma | the latest release, when its image builds | the component -- its map upload runs `worldgen` on every board (N28) -- `cartridge.json` (limits, `limits.boards`, the adapter budget), `reference/observations.json` and the engine digest |
| read by | web, the book, `tinybrains view` | `viz/` | The viewer bundle and the digest it was transpiled from |
| read by | `tinybrains` | a registry `release` or `path` | The component, the manifest, `maps/`, `reference/` and `viz/` |

## Interface

The component is `tb-ants.wasm`, plugin id `tb.ants`, ABI `orion:plugin@1.0.0`.
[`engine/plugin.toml`](engine/plugin.toml) declares each function's input fields.

| Export | Input | Result |
|---|---|---|
| `tb.ants.worldgen` | Seeds and the board, whole: `map`, or `maps` one per seed; optionally `players` and `max_turns`. The component carries no boards (N28) | The initial packed `wave_state` |
| `tb.ants.observe` | `wave_state`, and opaque per-seat `refs` | A view per live seat, its ref echoed |
| `tb.ants.step` | `wave_state` and actions | The next `wave_state`, `done`, `ended` and the replay delta |
| `tb.ants.finish` | `wave_state` | Ranks, scores, the ending reason, and the board each ended match was played on |
| `tb.ants.replay-decode` | A replay envelope, and a `turn` or a `from`/`to` range | One frame, or a range in one pass |

`wave_state` is opaque to every caller. Actions follow the last observation's order, or name
`{m, seat, action}` explicitly. The hyphen in `replay-decode` is part of the name.

What a view contains, what an action is and what a cartridge must honour are published in the
competitor guide: *What your model sees*, *What your model answers*, and *Adding a game*.

### The artifact set

`./build.sh` and `viz/build.sh` write `dist/`, and a release carries the same tree as one archive:

| Path | What it is | Made by |
|---|---|---|
| `tb-ants.wasm` | The component; its sha256 is the **engine digest** | `cargo build` + `wasm-tools component new` |
| `plugin.toml`, `plugin.json` | The ABI, authored and as JSON | `engine/plugin.toml`; `tools/package.py` |
| `cartridge.json` | Limits -- `limits.boards`, the envelope a season's board must fit -- the adapter budget, the basic boards' catalogue, `about` | `engine/src/bin/manifest.rs`; `tools/package.py` adds `maps`, `limits.boards` and `engine/about.json` |
| `maps/` | The five basic boards, as committed | copied from `maps/` |
| `reference/observations.json` | The observations admission validates an adapter against | `engine/src/bin/reference.rs` |
| `viz/` | The viewer bundle and `engine.json`, the digest it carries | `viz/build.sh` |

## Run it, test it

This is a plugin, so there is no server. The build needs rustup (`rust-toolchain.toml` names the
exact compiler and the `wasm32-unknown-unknown` target), `wasm-tools` at the version `build.sh`
names, and Python 3.11 or newer; the viewer also needs Node. No database, object store, container
or running platform is needed.

```sh
./build.sh                              # the gate, then every artifact, into dist/
viz/build.sh                            # the viewer, into dist/viz/ -- after ./build.sh
tools/pack.py /tmp/out                  # dist/ as the one archive a release carries, and its digests
tools/deny.sh                           # just the determinism check

cd engine                               # the crate is its own Cargo workspace
cargo test                              # just the host suite
cargo fmt --check && cargo clippy --all-targets -- -D warnings

cd mapgen                               # the board factory, a crate of its own
cargo run -- generate                   # every recipe under recipes/, into ../maps/ -- and only those
cargo run -- generate recipes/basic-tiny-2p.toml
cargo run -- check                      # every board is what its recipe makes, byte for byte
cargo run --release -- check --play 6   # ...and play each, the same walker in every seat
cargo run -- show ../maps/basic-tiny-2p.json
cargo run --release -- explore --slots slots.toml --out run --n 500   # hundreds of designs a slot
cargo run --release -- playtest run picks.txt                         # play the ones you like
cargo run -- adopt run picks.txt                                      # write them down as recipes
```

**A board is a design.** `mapgen/recipes/<name>.toml` is one board written out as data — its size,
its seat shift, a decorative point group about every seat's centre, every shape drawn on it (disks,
rings, segments, boxes) and every pattern (noise, ridges, mazes, rooms) with its seed — and
`generate` renders it. `explore` proposes designs by the hundred for a slot of the season, a person
picks by eye, and `adopt` writes the picks down, so a board somebody chose stays that board whatever
later happens to the sampler (N27). Edit a recipe and regenerate; never edit a board. `build.sh` runs
`mapgen`'s tests, which re-render every committed board and compare bytes.

`build.sh` runs the determinism check and the tests before it compiles anything, and ends by
printing the engine digest. **The digest is reproducible, on one host.** `rust-toolchain.toml` pins
rustc exactly, `build.sh` pins `wasm-tools` and remaps every build path, so the component is a
function of the source — and of the machine rustc runs on, which no flag removes. Releases build on
arm64 Linux, so a build there lands on a release's digest wherever the checkout is, and a build on a
Mac is the same game with other bytes. Cite the workflow's digest, not a laptop's.

To play or check against this checkout rather than a release, point a games registry at `dist/`:

```toml
[games.ants]
name = "Ants"
path = "../ants/dist"
```

A release unpacked works the same way, and is the authoritative tree:

```sh
gh release download --repo Tiny-Brains/ants --pattern ants-artifacts.tar.gz && tar -xzf ants-artifacts.tar.gz -C dist
```

**A release is made by the `build` workflow**, and nowhere else. Every push to `main` runs the whole
gate on an arm64 Linux runner, builds every artifact, packs `dist/` with `tools/pack.py`, checks the
baselines' encoding and plays [ants-starter](https://github.com/Tiny-Brains/ants-starter) against
the build, and then says in its summary which release this build reproduces byte for byte — or
the tag publishing it would cut.

```sh
gh workflow run build.yml                   # a rehearsal: all of the above, nothing published
gh workflow run build.yml -f publish=true   # ...and cut the release, from main, if none carries this build
```

A release is `ants-artifacts.tar.gz`, tagged `engine-<12 hex>` after the component it carries; a
later build of the same engine whose archive differs (a viewer fix, new reference observations) is
`engine-<12 hex>-2`. The archive is packed deterministically, and a tag is never re-cut: registries
pin the archive, and a replaced file would break every one of them.

**Publishing is deploying.** Soma, Kalam, web and the book fetch the **latest** release whenever
their images build, so the next build of each takes a new engine with no change in their repositories.
ants-starter pins a release instead, and moves when its `games.toml` does.

## What a deployment owes it

- **One release.** Soma's node, Kalam's runner and web (and the book inside it) take the latest
  release when they build, or the one `ANTS_RELEASE` names, and they must agree: a replica on another digest
  claims nothing, and a viewer on another digest draws a match that never happened. A deployment
  that builds them at different times, across a release, has built two engines.
- **A signature per build.** Every rebuild is a new digest, and a component whose Ed25519
  signature does not match brings the node up `degraded`.
- **A declared digest.** `games.active_engine_digest` and each replica's `engine_digest` move with
  the component — as a patch when the rules did not change, on a new season when they did.

## Layout

One directory per toolchain — `engine/` (Rust), `mapgen/` (Rust, host-only), `viz/` (Node),
`baselines/` (Python) — and at the root only what spans them: the boards, the build, the toolchain
pin, and the workflow that releases it.

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
  src/bin/                 manifest, reference -- the generators
  src/tests/               the host suite, one file per area; fixtures/ holds a platform-written replay
  build.rs                 compiles ../maps/ into the component
  plugin.toml              the authored Orion ABI
  about.json               the game's introduction, folded into cartridge.json
mapgen/                    the board factory: its own crate, so tuning it moves no engine digest
  recipes/                 one design a board: <size>-<terrain>-<N>p-<H>h.toml
  src/design.rs            the renderer: a design's steps under the full group, hills, repair, food
  src/sym.rs               the seat shift, the point group about each centre, orbits, the lattice
  src/sample.rs            the sampler: a slot and a seed in, a design out -- taste, not contract
  src/explore.rs           explore and playtest: many designs a slot, rendered, validated, scored
  src/make.rs              the area generator `sweep` draws on: areas, walls, doors, hills, fill
  src/measure.rs           a board in numbers, and the rules every board obeys, checked on the board
  src/set.rs               the area generator's sets, the file format, and the engine's validation
  src/play.rs              the seat-bias smoke check: one walker in every seat
viz/                       the replay viewer: one bundle for the web app, the book and the CLI
baselines/                 the trained entries, and how they were trained (not in a release)
maps/                      the five basic boards, one JSON file each -- never a season's
tools/deny.sh              the determinism check
tools/package.py           finishes dist/: plugin.json, the catalogue and limits.boards, the boards, the report
tools/pack.py              dist/ as a release's one archive, packed deterministically, and its digests
build.sh                   the gate, then every artifact, into dist/
rust-toolchain.toml        the exact rustc, which is part of the engine digest
.github/workflows/build.yml  the gate and the build on every push; the release when asked
dist/                      build output, gitignored; exactly what a release archive carries
```

## What must stay true

- **Game logic uses integer arithmetic.** `tools/deny.sh` refuses floating point in `engine/src/`, and the
  replay tests check reconstruction. It is what lets a replay be an action stream.
- **Seeds and actions determine the match**, on the board the caller sends. No ambient randomness,
  no clock. On the ladder pair chooses the board and the seed, so a competitor cannot train against
  a board they picked.
- **Exploring is remembered.** A model is a pure function of one observation, so the engine folds
  each seat's vision into what it knows every turn, and a view carries known water.
- **A view is observer-relative.** Relabel every seat and ask the same player again, and the bytes
  are identical; `a_view_is_observer_relative` checks the engine rather than a stand-in.
- **Every board is fair and whole, whoever sends it.** A file cannot be symmetric by construction, so
  `engine/src/maps.rs` asserts that it is congruent under its own shift, that all its land is one
  walkable body and that every hill has a way off -- on every board `worldgen` is given, including a
  season's at upload -- and the corpus test checks every basic board. `mapgen` holds the boards to more — clearings, no enemy hill in view, a seat's hills spread
  apart — and proves each is what its recipe makes.
- **A replay carries the board it was played on**, and the decode test runs against an envelope the
  platform actually wrote.
- **The viewer re-simulates with the cartridge, never a copy of it.** A JavaScript
  re-implementation of a rule would be a second engine.
- **Nothing generated is committed, and everything generated is in `dist/`.** The registration
  manifest's catalogue and `limits.boards` are generated from the basic boards, so they cannot
  disagree with them, and the reference set is drawn on the same boards, so the envelope an upload
  must fit is the one admission proved adapters against.
- **No season's board is committed or released** (N28). Season boards live outside every repository
  until their season closes, reach the platform by an admin's upload, and are pushed to a backup
  repository only then.
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

**17 September 2026 (later) — tested across 2 to 8 seats, and two rules that treated seats unequally.**
`mapgen sweep` draws boards in eleven recipe styles (open ground, boulders, scattered and lattice
mazes from a perfect tree to half-looped, rooms, caves, arenas, islands, walls four thick, four hills
a seat) at every seat count from 2 to 8 and every shift of that order, then plays each with one
frame-relative policy in every seat: on a fair board under fair rules every seat's view is seat 0's
moved by the shift on every turn, and every match ends level, so any difference is a bug. It found
two. **Spawn ties were broken by square**, which a shift does not preserve: on a board with two hills
a seat — `cave-2` ships one — seats spawned from hills that were not images of each other; ties now
go to the hill listed first, and the map file lists hills in orbits. **An open-ended `replay-decode`
range over a recording that stopped before its match did** photographed the last turn once for every
turn up to 65,535; it now ends where the recording does. Both have a test that fails without the fix.
On single-hill boards every `observe`, `step` and `finish` output hashes as it did; on `cave-2` it
does not, which is the fix. After them: 770 boards (10 a style and seat count, 1,000 turns) all
generated, validated, stayed congruent to the end and replayed; independent waves took first place
evenly by seat at every count (8 seats: 369 to 382 first places each, ties shared). Through
`tinybrains` with the trained baselines, 24 matches at 3, 6 and 8 seats on sweep boards ran with no
refusal, and the ten with one model in every seat ended with every seat level — razes included — so
the network, its adapter, the loader and the engine together are shift-fair. A 192 x 192 board costs
an adapter 516,000 operations (51% of the budget) and 8 seats' inference used at most 17.8% of the
turn deadline; 255 x 255, the grid's ceiling, would cost about 91%. Contact is the gap: maze styles
rarely meet an enemy before the idle-food cutoff, so their fights are proved on paper more than in
play. The local digest is now `sha256:5d034eaa...`. `cargo test` is 90, and `mapgen`'s is 6.

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

**17 September 2026 (evening) — sixteen presets, two seats to eight, chosen by design.** The four
presets are replaced by sixteen, and they are the runs of an **L16(4⁵) orthogonal array**: five
factors at four levels each, laid out so every pair of levels of any two factors lands on exactly
one preset. A full factorial would be 1,024 presets; this is the smallest set in which, for example,
every seat band meets every board size and every hill count meets every terrain.

| Factor | Levels |
|---|---|
| seats | 2 · 3–4 · 5–6 · 7–8 |
| terrain | open (open, boulders) · maze (loopy, voronoi, tree, wide) · rooms (rooms, fortress, arenas) · cave (cave, islands) |
| board | small ~80 · medium ~104 · large ~128 · huge ~152 a side, never over 156 |
| hills a seat | 1 · 2 · 3 · 4 |
| food | lean · rich · contested · home |

Which label each level of the board and hill columns gets was chosen to maximise the fewest squares a
hill has anywhere in the design, and within a seat band which count a run takes by the same measure;
the result is two presets of every count from three to eight and four of two. A preset is named
`<terrain>-<seats>`, which the array keeps unique, and `open-2`, `maze-2`, `cave-2` and `rooms-4`
keep their names with new recipes, so match files and tests naming them still resolve.

| Preset | Seats | Board | Style | Hills | Food |
|---|---|---|---|---|---|
| `open-2` | 2 | 104² | open | 3 | lean |
| `maze-2` | 2 | 128² | loopy maze | 2 | home |
| `rooms-2` | 2 | 80² | rooms of 10 | 4 | rich |
| `cave-2` | 2 | 152² | cave | 1 | contested |
| `maze-3` | 3 | 105² | voronoi maze | 4 | contested |
| `cave-3` | 3 | 81² | islands | 3 | home |
| `open-4` | 4 | 128² | boulders | 1 | rich |
| `rooms-4` | 4 | 152² | fortress | 2 | lean |
| `open-5` | 5 | 80² | open | 2 | contested |
| `cave-5` | 5 | 130² | cave | 4 | lean |
| `maze-6` | 6 | 144² | tree maze | 3 | rich |
| `rooms-6` | 6 | 102² | arenas | 1 | home |
| `rooms-7` | 7 | 126² | rooms of 18 | 3 | contested |
| `cave-7` | 7 | 105² | islands | 2 | rich |
| `open-8` | 8 | 152² | boulders | 4 | home |
| `maze-8` | 8 | 80² | wide maze | 1 | lean |

Four boards a preset, 64 in all (840 KB), rather than eight: the boards are compiled into the
component the viewer downloads. Every one generated on the first draw, `mapgen check` reproduces
all 64, and every board was played **congruently** — one frame-relative policy in every seat, two
seeds, 600 turns — with no divergence in any seat's view on any turn. `check --play 6` raised one
smoke alarm, on `open-4-03`, that 24 seeds do not reproduce: the greedy walker shares one random
stream across seats. On `open-4-01` that walker's colonies die by turn 7 without contact, walking
their own two ants into each other; it is the walker, and the board plays fair.

The reference observations cover every preset at turn 20 and turn 400 (148 views, 355 KB), and
views now number opponents up to 7 — the old set never numbered past 1, so an adapter mishandling
owner 2 was admitted. `each_player_starts_with_one_point_per_hill` now checks every preset rather
than naming two boards whose hill counts changed. **A new engine digest**: every board changed.

**Released** as `engine-df312c0458d9` (`sha256:df312c04…`, the digest the local ladder already
played; archive `sha256:6a2a11aa…`, 430 KB), and ants-starter's `games.toml` pins it. The starter's
nano entry passes `tinybrains check` on the new set — 148 observations, worst adapter run 32% of the
budget — and both its match files still play `open-2`.

**17 September 2026 (night) — no Docker; a workflow builds and releases the cartridge.** The
`Dockerfile`, `.dockerignore` and `tools/release.sh` are gone, and `.github/workflows/build.yml`
is the build that ships: every push runs the gate, builds every artifact, packs them with the new
`tools/pack.py`, runs the baselines' conformance test, plays ants-starter against the build and
reports which release it reproduces; `gh workflow run build.yml -f publish=true` cuts one. What the
image did for reproducibility moved into the repository: `rust-toolchain.toml` pins rustc 1.98.1,
and `build.sh` pins `wasm-tools` and remaps the crates.io, standard-library and checkout paths. **The
host is part of the digest**: the same source, compiler, flags and embedded paths build
`sha256:df312c04…` on arm64 Linux, in a plain Debian container with rustup, exactly as the image did
on an arm64 Mac, and `sha256:04b8b8b6…` natively on the Mac, where only the component and the
viewer transpiled from it differ. So the workflow runs on `ubuntu-24.04-arm`. **Kalam, web and the
book fetch the latest release when they build** (`ANTS_RELEASE` pins one), so publishing is
deploying. No source changed and the digest did not move.

**19 September 2026 — boards are designs (N27), and the component carries none (N28).** A board is
a **design** (`mapgen/src/design.rs`) drawn under the seat shift and a decorative point group about
every seat's centre, from families of motifs put where the lattice puts them -- keeps, rings and
plazas round every hill, rivers along the borders, lakes and outposts where territories meet,
symmetric caves, braided and ring mazes, room grids -- proposed by `mapgen explore`, picked by eye and
written down by `mapgen adopt`; **a seat's hills are spread**, each in its own part of the territory.
Then **N28**: `build.rs`, the catalogue and presets are gone, and `worldgen` requires the board whole,
because a season's boards are uploaded to it -- judged by this engine's `worldgen` on Soma's node --
rather than compiled in. This repository keeps **five basic boards**, drawn the same way, one a size
class: `basic-tiny-2p` 24 × 24, `basic-small-3p` 36 × 36, `basic-medium-4p` 48 × 64, `basic-large-6p`
80 × 96, `basic-xlarge-8p` 120 × 124. They define `limits.boards` (2-8 seats, sides 24-124, at most
14,880 cells), which `tools/package.py` derives and Soma checks every upload against, and the
reference set is drawn on them: five boards, three seeds, turns 20, 150 and 400, 207 views.
**Proved to change no rule**: all 32 of season 1's boards, from outside the repository, played under a
random and a greedy policy on two seeds to the end, hashed every `worldgen`, `observe`, `step` and
`finish` output identically through the old engine by preset and the new one by board -- 128 matches.
Engine tests 88, mapgen 7 (its area recipes' `[preset]` table is `[set]`), `build.sh` green. **A new
engine digest, and not yet released**: this Mac builds `sha256:21a694b8…`, which the local stack
plays; a release from the workflow carries another, and needs a CLI released first (the new CLI
passes boards whole and plays old and new engines alike; the released 0.1.1 sends presets).

## More

- [The competitor guide](https://github.com/Tiny-Brains/web/tree/main/docs): the rules, what a model
  sees and answers, and *Adding a game*.
- Related repositories: [Kalam](https://github.com/Tiny-Brains/kalam), [Soma](https://github.com/Tiny-Brains/soma).
- Apache-2.0: see [LICENSE](LICENSE).
