# Ants — the reference cartridge

A cartridge is a game: its rules, its world generation, its scoring and its replay format, shipped
as a signed WebAssembly component the platform loads, versions and executes in a sandbox. This is
the reference one.

The rules are [`design/RULES.md`](../design/RULES.md), the contract is
[`design/cartridge.md`](../design/cartridge.md), and what a model sees is
[`design/PROTOCOL.md`](../design/PROTOCOL.md). Where this repository and those documents disagree,
they are right and this is a bug.

```
./build.sh                                  # deny.sh, the tests, the component, both manifests
cargo test -- --nocapture measure_what_random               # what the rules produce
```

## What is here

| | |
|---|---|
| `src/map.rs` | the grid, its wrapping, squared distances, the presets, and the symmetry a world is built on |
| `src/state.rs` | one match, and worldgen |
| `src/turn.rs` | the six steps of §4, in order, and the five ways a match ends |
| `src/observe.rs` | the fog, and what `water` carries |
| `src/codec.rs` | the packed `wave_state`, and base64 over it |
| `src/replay.rs` | the action stream, and `replay-decode` |
| `src/tests.rs` | **the rules**. 44 tests, each naming the rule it is for |
| `deny.sh` | the determinism law, checked against the source |

`plugin.toml` is the Orion manifest; `cartridge.json` is the registration manifest and is
**generated from the preset table**, so the two cannot disagree about how many seats a map is
played at.

## Three things a reader should know

**`RULES.md` is the specification and `tests.rs` is the only place it is enforced.** The game's own
JSON schemas are documentation the platform never loads (`PROTOCOL.md` §6), so nothing at runtime
will catch a rule implemented wrongly. Every test names its rule.

**No floating point, anywhere in game logic.** That is what makes the platform's build and the
browser's agree bit for bit, which is what lets a replay be an action stream rather than a hundred
times as many frames — and `a_replay_re_simulates_the_match_it_recorded` is that property, tested.
`deny.sh` greps for it rather than trusting that nobody added any, because a single `as f64` in a
loop bound would be invisible until two builds disagreed on a match.

**The whole world is symmetric.** Terrain, hills and food are generated on one fundamental domain
and translated to every player, so no pairing can be unfair because of the map. Rule 46 requires it
of food; a map whose *terrain* were only approximately fair would put a thumb on every rating
computed from it.

## Three departures from `cartridge.md` §1 as written

All three were found by driving the shape in a real Orion before this existed
([`design/v2/03-spike/FINDINGS.md`](../design/v2/03-spike/FINDINGS.md)), and all three are folded
into `cartridge.md`.

- **`replay-decode`, not `replay_decode`** — Orion refuses a plugin function label with an
  underscore, so the five-function set does not load as written.
- **`observe` takes `refs`** — a flat list of opaque per-seat handles, echoed onto the matching view
  and never inspected. Without it the platform cannot attach a seat's identity to the view it must
  send to the loader.
- **`actions` is positionally aligned with the last `observe`** — zipping the loader's reply back
  onto the views needs an index inside a JSONLogic `map`, and there is none.

## What the rules produce

24 matches of random play per preset, after `build.sh`:

| preset | mean turns | mean ants | decisive | how they ended |
|---|---|---|---|---|
| `standard` | 237 | 3 | 41% | idle_food ×13, lone_survivor ×10, domination ×1 |
| `maze` | 184 | 3 | 45% | idle_food ×13, lone_survivor ×11 |
| `cell` | 181 | 4 | 50% | idle_food ×12, lone_survivor ×12 |

Random play is not a strategy, so a food stalemate is a common and correct ending for it. The
number that mattered while building was **decisive**: it was 12–16% until worldgen started seeding
food within reach of each hill, because a lone ant that has to *find* its first food usually will
not, and a colony that never grows plays no game at all. Real Ants seeds the hills the same way.

## What is not built yet

The baselines (`baseline-greedy`, `baseline-strong`), which are ONNX models with adapters rather
than engine work; `viz.js`, the browser half of the visualizer contract; and the offline harness
that plays two ONNX files against each other without Orion. The determinism conformance run —
identical per-turn state hashes in the platform runtime and in the browser across 10,000 seeded
matches (`DESIGN.md` §11) — needs `viz.js` to exist first.
