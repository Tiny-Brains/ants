# Building a cartridge

A cartridge is a game: its rules, its world generation, its scoring and its replay format. It ships
as a **signed WebAssembly component** the platform loads, versions and executes in a sandbox.

This is the document for someone adding a game. What a model sees is [`protocol.md`](protocol.md);
where the cartridge sits in the running system is
[devops/docs/architecture.md](https://github.com/Tiny-Brains/devops/blob/main/docs/architecture.md);
the competitor-facing walkthrough is
[the book](https://github.com/Tiny-Brains/docs), *The platform → Adding a game*.

`ants` is the reference implementation of everything on this page: `src/` is the engine,
`build.sh` generates `plugin.json` and `cartridge.json`, and `deny.sh` is the determinism law
enforced at source level.

**Adding a game deploys nothing.** There is no image to build, no fleet to size, no service to
supervise. A finished cartridge is uploaded, self-tested, activated and promoted between
environments as a package. If shipping your game requires anything else, something is wrong — say so
before working around it, because the seam is the point.

---

## 1. The function set

Five functions, in your own namespace. Each is a **pure JSON-to-JSON transformation**: it receives
one value and returns one value.

```
tb.<game>.worldgen(seed[], preset, players, map?, maps?)  → wave_state
tb.<game>.observe(wave_state, refs)                       → [per-seat views, each echoing its ref]
tb.<game>.step(wave_state, actions)                       → { wave_state, done[], replay_delta }
tb.<game>.finish(wave_state)                              → [{ ranks, scores, reason, map }]
tb.<game>.replay-decode(payload, turn | from, to)         → frame | frames[]
```

**`map` and `maps` are optional and the platform passes neither.** A board may be named by
catalogue id or given inline, for one match or for the whole wave; when nothing is passed the
*seed* chooses from the preset's pool. That is deliberate rather than convenient: pairing assigns
the seed, so a competitor who cannot pass a map cannot train against a board they picked. See §4.1.

**`finish` returns the board each ended match was played on**, which is what makes a replay
self-sufficient — §4.2.

> **Three corrections the wave-turn spike made to an earlier version of this contract.** All three
> are law now, and `ants/plugin.toml` is the proof: its five `[[functions]]` are exactly the labels
> above.
>
> 1. **`replay-decode`, not `replay_decode`.** Orion refuses a plugin function label that is not
>    `[a-z][a-z0-9-]*`, so the underscore spelling does not load at all.
> 2. **`observe` takes a second argument, `refs`** — a **flat list** of opaque per-seat handles,
>    each carrying its own `m` and `seat`, which the engine matches on and echoes onto the
>    corresponding view without ever inspecting. Flat and matched rather than nested and indexed
>    because the caller rebuilds it every turn from the *live* seats, so a nested `refs[m][seat]`
>    would shift the moment a match in the wave ended. Without it the platform cannot
>    attach a seat's identity to the view it must send: an Orion `map` body is evaluated with the
>    element as its data context, so a join back to the caller's own rows evaluates to `null`,
>    silently.
> 3. **`actions` is positionally aligned with the last `observe`**, because a fixed task list
>    cannot zip two arrays. A match ending mid-wave does not break it: both sides derive the order
>    from the same `wave_state`.

One component exports all five and dispatches on the function name.

**`observe` and `step` operate on a wave, not a match.** A wave is K matches advancing
turn-synchronously, and the platform steps all of them in one call. This is not an optimisation you
may opt out of: it is what lets one batched inference serve every match a given model is playing,
which is the economics the whole competition rests on. A cartridge that can only step one match at a
time throws that away at the seam.

**`finish` is the only semantic dependency the platform has on your game.** It answers ranks, scores
and a free-text reason. Ranks are 1-based with ties allowed; scores are **integers**, because game
state carries no floats (§4). What a rank means for a ladder is not your concern.

**`observe` returns nothing for a finished match.** There is no terminal message. A model receives
states while its match runs and nothing afterwards — it is never told that it lost.

---

## 2. `wave_state`

Everything about a wave lives in one value that goes out with every answer and comes back with the
next call. The sandbox keeps no state between invocations, so there is nowhere else for it to live.

Three rules, and the first is the one that matters:

**Make it opaque and compact.** Base64 of a packed binary encoding, not readable JSON. Nothing
outside your cartridge may decode it, and nothing outside your cartridge should be able to — that is
what makes "the platform never parses game state" a property rather than a promise. It also keeps
the round trip small, and the round trip is the design's main cost.

**It must round-trip exactly.** `step` returns the state its next call receives. Any information you
do not encode is information you do not have next turn.

**It is versioned by the digest that produced it.** A wave never spans two cartridge versions, so
the encoding needs no version field. Do not add one.

Size is the binding constraint. State crosses the boundary twice per turn and is bounded by the
platform's plugin request and response ceilings — 1 MiB each by default, and the operator can raise
them. Wave size is chosen against that budget, so a compact state buys throughput directly. Start
small: 8–16 matches per wave, and measure before growing it.

---

## 3. The manifest

Host-owned metadata submitted with the component, as TOML.

```toml
abi = "orion:plugin@1.0.0"
name = "tb.ants"
version = "1.0.0"            # yours; the platform assigns the entity version
component = "cartridge.wasm"

[[functions]]
name = "tb.ants.step"
description = "Advance every live match in the wave by one turn"
category = "transform"
output_default_root = "data"

[[functions.input_fields]]
name = "wave_state"
kind = "string"
required = true
resolvable = true            # {"var": …} nodes are folded before you see it

[[functions.input_fields]]
name = "actions"
kind = "array"
required = true
resolvable = true
```

`name` is a lowercase reverse-domain identifier with at least two labels, and every function must be
`<name>.<label>` — a cartridge's functions live in its own namespace, which is what keeps two games
from colliding.

The field table is a contract the platform checks at workflow-activation time. Renaming a field or
making one newly required between versions is refused, naming the workflow that would break, while
the previous version keeps serving.

**A cartridge never sees key material.** A `{"secret": …}` node anywhere in a task input for one of
your functions is refused when the workflow is written.

---

## 4. Determinism

**Game state must be integer-only. No floating point in game logic.**

This is what makes the platform's build and the browser's agree bit-for-bit, which is what lets the
viewer re-simulate the exact match the referee produced. Without it, replays would have to ship
frames instead of actions and would be a hundred times larger. It reaches into the database too:
`matches.scores` is an integer array because a score is an integer by law.

The sandbox enforces the precondition rather than trusting you with it. Your component's world
**imports nothing** — no clock, no randomness, no filesystem, no sockets. There is no wall-clock to
read and no CSPRNG to reach, so the usual ways integer discipline erodes are not available. All
randomness derives from the seeds `worldgen` is handed.

What conformance still has to prove is that your own arithmetic is stable across the two hosts that
run it:

```
10,000 seeded matches
  → run in the platform runtime and in the browser
  → compare per-turn state hashes
  → any divergence fails the build
```

Run it in CI. The platform also probes every declared function at upload, before a draft version
exists, so a component that will not load never becomes a row.

### 4.1 Boards, if your game has them

Ants generates nothing at match time. Its boards are **files** — `maps/*.json`, one per board,
each carrying its grid, its water as run lengths, its hills, its turn-zero food and how much food
it keeps stocked. `build.sh` validates every one and compiles them in with `include_str!`, because
a cartridge imports nothing and so cannot read a file at run time.

Two consequences are worth taking on purpose rather than discovering:

**A map edit is an engine-digest change**, and therefore refused while a season is live and enters
with the next one — the same rails a rules change already runs on. There is no separate mechanism
for shipping a board, and there should not be.

**Symmetry stops being a construction and becomes an assertion.** Growing a world on one
fundamental domain and translating it makes fairness true by the shape of the code; a file someone
hand-authored cannot make that promise. So the guarantee moves into a validator every board passes
before it is played — water, hills and turn-zero food each closed under the orbit, the grid
dividing by the seat count, no hill on water or walled in. The refusals are `caller_input`: the
same board can never succeed, so nothing retries it. What the corpus buys back is that the property
is now checked against *every* board that ships rather than one generated example.

The generator does not die. It becomes the factory that writes the files — `cargo run --bin
mapgen` — which is the same idiom as generating the manifests: an artifact nobody hand-edits
cannot drift from the thing it was made from.

A game whose configurations are not boards simply has none of this. `cartridge.json`'s `maps` is
optional, and omitting it is what keeps a second cartridge content rather than a platform change.

### 4.2 The replay carries its own board

A replay is the action stream, so re-simulating it needs the position it started from. Ants used to
answer that with `state0`, a packed state the envelope was supposed to carry — and never did, so
for the whole life of the format no stored replay could be decoded by the function that decodes
replays. It went unnoticed because the round-trip test built its own envelope in-process.

The envelope now carries `map`, `seed` and `max_turns`: the board it was played on, the seed that
drove food respawn, and the limit it ran under. That is strictly better than naming a preset and a
seed, because a replay stays viewable when the preset table has been re-tuned or the catalogue has
moved on — it brought its terrain with it. The board comes off `finish`, at the moment the drain
writes the row, so it costs no state carried across turns.

Two rules fall out. **Test against an envelope the platform actually wrote**, not one your tests
built; `tests/fixtures/` holds a real one for exactly this reason. And **give the viewer a range**:
decoding re-simulates from turn zero, so a scrubber asking frame by frame is quadratic. `from`/`to`
walks the match once. An optional field on a declared function is not a sixth function, so it stays
inside what §9 permits.

**A frame carries what the viewer must not work out for itself.** Ants' frames name, per seat, the
squares that turn revealed for the first time (`discovered`), so the viewer can draw what each seat
has explored without drawing vision — which is a rule, and a viewer that computed it would be the
second engine §4 forbids. It is news rather than a mask, so a thousand-frame range costs one entry
per square per seat rather than a thousand copies of a mask that only grows; and it belongs to the
turn rather than the call, so a range and a single frame still agree.

---

## 5. Building it

A Rust `cdylib` on `orion-plugin-sdk`, targeting `wasm32-unknown-unknown` — **not** a WASI target,
whose standard library imports WASI and would be refused by the world.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
orion-plugin-sdk = "1"

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
strip = true
```

```bash
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new target/wasm32-unknown-unknown/release/tb_ants.wasm -o cartridge.wasm
```

Refusals are a `PluginError` with a stable code (`^[A-Z][A-Z0-9_]{0,63}$`) and a class:
`caller_input` when the input was wrong and the same input can never succeed, `internal` when the
cartridge itself failed. Neither is retried. The code is what a workflow can branch on, so choose
codes a caller can act on — `WAVE_STATE_CORRUPT` beats `ERROR`.

---

## 6. The ceilings

Every invocation runs in a fresh instance under limits the operator sets. A manifest requests
nothing, and a per-cartridge override may only *lower* a ceiling.

| Ceiling | Default | What it bounds |
|---|---|---|
| `max_memory_bytes` | 64 MiB | Linear memory per invocation |
| `max_timeout_ms` | 5,000 | Wall clock; the task's own deadline applies too and the shorter wins |
| `max_request_bytes` | 1 MiB | The serialised input — this is what bounds wave size |
| `max_response_bytes` | 1 MiB | The returned JSON |
| `max_concurrency_per_function` | 64 | Invocations of one function at once |
| `fuel_backstop` | — | An instruction budget, sized well above what the clock admits |

A timeout is the one failure class that is **retried**, because a pure function retries for free.
Every other failure — a trap, a panic, a size limit, a returned value that is not JSON — writes
nothing and is not retried.

Compilation happens once per digest per process, never on a request; instantiation is microseconds.
Do not cache anything across calls, because there is no across-calls.

---

## 7. The viewer

The one part of a cartridge that is not the component. It is an ES module the platform loads by
convention at `/cartridges/{slug}/viz.js`:

```js
export const meta = { gameId: "ants", abiVersion: 1 };

export async function mount(target, replay, opts) { /* → { destroy() } */ }
```

`opts` carries `turn`, a `from`/`to` range, `autoplay`, `speed`, `zoom`/`centre`, `theme`, `chrome`,
`height`, `labels` — what the host calls each seat, because an envelope only has the referee's name
for one, which is a weights hash — and an `onTurn` callback. Ants adds `explored`. A replay carries
its own board (§4.2), so the envelope is the whole input.

**`renderFrame` is fed by your own `replay-decode`, running in the browser from the same component
digest the match recorded** — which is why the viewer and the referee cannot disagree. Transpile
the component with `jco` and drive it; a JavaScript re-implementation of your rules would be a
second engine, and §4 exists to prevent exactly that.

### What changed, and what it costs

An earlier version of this section said the platform shell owned the timeline scrubber, playback
speed, seek, the player list and the score chart, that a cartridge **owned pixels**, and that
adding a game required zero changes to the shell.

**The shell now lives in the cartridge.** The viewer has three consumers that are not one
application — the web Replay screen, the book's tutorials, and `tinybrains view` — and a shell
split across three of them is a shell maintained in three places. Ants ships the whole thing, and
exports it twice: `mount(el, replay, opts)` for anything that is not React, and a React component
for the application that is.

**The cost is real and is not hidden: the second cartridge writes its own scrubber.** That is the
trade, taken while there is one cartridge and one viewer to reason from. The extraction point is
the second cartridge (tracker §7): whatever turns out to be genuinely game-independent becomes a
shared package *then*, informed by two real viewers instead of one imagined one. If you are that
second author and this feels like a lot of scrubber to write, say so — that is the signal the
extraction is due, and it is a better signal than a shell designed in advance for one game.

Commit the built bundle the way you commit the component, so a clone with no Node toolchain still
has a viewer, and record the digest it was built against: a viewer re-simulating with an engine
other than the one a replay names looks right and is wrong.

**A viewer never runs a model.** It re-simulates from recorded actions — no ONNX, no adapter, no
competitor code in the browser — which is what lets a viewer be embedded anywhere without
inheriting the evaluator's security surface.

### What a viewer owes the page it is on

Five rules, learned from having three hosts. They are not enforced by the ABI, and each of them was
a bug first.

**Scope every rule to your root class.** A viewer mounts by putting its class on the host element
and injecting one `<style>`; it is not a shadow root. Ants' unscoped `.tb-bar` landed on the web
shell's own header and relaid it out the moment a replay mounted — a site that broke when you opened
a match. `viz/check.mjs` fails the build if a rule escapes.

**Take the chrome's colours from the host, and keep the board's.** Read the platform's tokens with
your own values as the fallback (`var(--ink, …)`), and the player is the colour of the card it sits
in and follows the theme switch for free; hard-code the board, and a match looks like itself in
either theme, the way a video does not change colour with the player around it.

**Give the board the frame.** A host embeds a viewer at whatever height its page can spare — 420
pixels is a realistic one — so anything permanent beside the transport has to earn its height. Ants
keeps one line of it, a title bar of each seat's name and score, because those are read the whole
way through a match; its tools are a tray over the stage that appears on hover, focus or touch.

**Take the layout question yourself.** Before the tray, the web application carried fourteen rules
reaching into Ants' class names to float the seat row over the board. It worked, and it was pinned
to names this repository owns: a release that renamed one would have silently undone it. If a host
needs the viewer laid out differently, that is an option on `mount()`.

**Take the width you are given.** A host places a viewer in a column and expects it to fill that
column; it does not expect the viewer to decide how wide the column is. Anything in the viewer
that does not wrap — a line of names, a canvas sized in pixels — becomes its minimum width, and a
grid column holding it grows to fit: Ants' title bar put the web home page's replay at 871 pixels
in a 410-pixel column. `contain: inline-size` on the root takes the viewer's content out of its
width entirely.

---

## 8. Publishing

A cartridge is a versioned entity with the same lifecycle as a workflow: `draft → active →
archived`, integer versions, one draft at a time, active rows immutable.

```bash
# upload as a draft — validated, hashed, compiled and probed before the row exists
orion-cli plugins create --manifest manifest.toml --component cartridge.wasm

# activate: the previously active version is archived in the same transaction
orion-cli plugins activate tb.ants
```

Two rules are worth knowing before you plan a release:

- **Exactly one version is active at a time**, so a function name resolves to one digest per
  generation. There is no window in which two cartridge versions rate into the same ladder.
- **A cartridge cannot be archived while an active workflow calls one of its functions.** The
  refusal names the workflows. This is the whole of cartridge upgrade safety, and it is why there
  are no blue/green fleets in this design.

**Signing.** Where the operator configures trust keys, an upload must carry a detached Ed25519
signature over the ASCII digest string (`sha256:<64 hex>`), and **every node verifies again at
load**. The digest is what is signed, not the bytes, so a release pipeline signs an identity and
never needs the component in memory.

**Promotion.** Move a cartridge between environments as a package rather than by re-uploading:
export by tag from one instance, plan and apply against the next. The digest is preserved, so what
you tested is what runs.

---

## 9. What your game must not need

The seam is only worth having if it holds. If adding your game requires any of the following, raise
it rather than working around it — the design is wrong and this is the cheapest moment to find out.

| | |
|---|---|
| A change to the five functions | The set is the ABI. A sixth function is a design conversation. |
| Floating-point game state | §4. It breaks replays before it breaks anything else. |
| State held between invocations | There is nowhere to put it. Thread it through `wave_state`. |
| I/O of any kind | Nothing in a cartridge may read a file, a clock, a socket or a secret. |
| Knowledge of ratings, classes, users, quotas or seasons | You score a match and rank its players. The rest is not your business. |
| A per-match step function | It would cost the platform its batching. See §1. |
| Your own tensor layout | There is no platform-mandated one, and there is no cartridge-mandated one either: turning a payload into tensors is the competitor's adapter, not yours. |
