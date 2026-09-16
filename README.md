# ants

Ants is the reference game cartridge for TinyBrains: a deterministic contest between colonies on a
wrapping grid, after the 2011 Google AI Challenge. This repository builds the WebAssembly component
the platform plays, its manifests, the boards, and the replay viewer — and keeps the platform's own
trained entries for the game beside the rules they encode.

## Scope

**It owns**

- The rules: turn resolution, visibility, end conditions, ranks and scores.
- The boards: the catalogue under `maps/`, its validation, and player symmetry.
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
```

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

## What a deployment owes it

- **One image, pinned.** `ANTS_REF` reaches Kalam's package, the loader and web, and they must
  agree: a replica on another digest claims nothing, and a viewer on another digest draws a match
  that never happened.
- **A signature per build.** Every rebuild is a new digest, and a component whose Ed25519
  signature does not match brings the node up `degraded`.
- **A declared digest.** `games.active_engine_digest` and each replica's `engine_digest` move with
  the component — as a patch when the rules did not change, on a new season when they did.

## Layout

One directory per toolchain — `engine/` (Rust), `viz/` (Node), `baselines/` (Python) — and at the
root only what spans them: the boards, the build, and the image.

```text
engine/                    the cartridge: a Rust crate compiled to the wasm component
  src/lib.rs               plugin dispatch: the five exports and their JSON shapes, no rules
  src/grid.rs              wrapping, distances, symmetry, directions, bitmaps, the seeded RNG
  src/maps.rs              the board as a file: parsing, validation, the catalogue, the presets
  src/state.rs             one match while it is played
  src/turn.rs              a turn in its fixed order, the cutoff counter, the end conditions, ranks
  src/food.rs              food at the hidden rate, in shuffled symmetric sets
  src/observe.rs           what a seat sees, and what it has been shown
  src/codec.rs             the packed, base64 wave_state
  src/replay.rs            the action stream, and re-simulating it into frames
  src/authoring.rs         host-only: the board factory and the reference observations
  src/bin/                 manifest, mapgen, reference -- the generators
  src/tests/               the host suite, one file per area; fixtures/ holds a platform-written replay
  build.rs                 compiles ../maps/ into the component
  plugin.toml              the authored Orion ABI
  about.json               the game's introduction, folded into cartridge.json
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
- **Every committed board is symmetric.** A file cannot be symmetric by construction, so
  `engine/src/maps.rs` asserts it and the corpus test checks every board that ships.
- **A replay carries the board it was played on**, and the decode test runs against an envelope the
  platform actually wrote.
- **The viewer re-simulates with the cartridge, never a copy of it.** A JavaScript
  re-implementation of a rule would be a second engine.
- **Nothing generated is committed, and everything generated is in `dist/`.** The registration
  manifest is generated from the preset table and the boards, so it cannot disagree with them.
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
in the small only. No release is cut, so drill and ants-starter still resolve a checkout.

## More

- [The competitor guide](https://github.com/Tiny-Brains/web/tree/main/docs): the rules, what a model
  sees and answers, and *Adding a game*.
- Related repositories: [Kalam](https://github.com/Tiny-Brains/kalam), [Soma](https://github.com/Tiny-Brains/soma), [DevOps](https://github.com/Tiny-Brains/devops).
- Apache-2.0: see [LICENSE](LICENSE).
