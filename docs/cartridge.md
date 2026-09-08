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
tb.<game>.worldgen(seed[], preset, players)   → wave_state
tb.<game>.observe(wave_state, refs)           → [per-seat views, each echoing its ref]
tb.<game>.step(wave_state, actions)           → { wave_state, done[], replay_delta }
tb.<game>.finish(wave_state)                  → [{ ranks, scores, reason }]
tb.<game>.replay-decode(payload, turn)        → frame
```

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

## 7. The visualizer

The one part of a cartridge that is not the component. `viz.js` is an ES module the web shell loads
by convention at `/cartridges/{slug}/viz.js`:

```js
export const meta = { gameId: "ants", abiVersion: 1 };

export function createRenderer(canvas, manifest) {
  return {
    resize(w, h),
    renderFrame(state, opts),   // state comes from replay-decode, in the browser
    controls(),                 // game-specific toggles, surfaced by the shell
    destroy(),
  };
}
```

The shell owns the timeline scrubber, playback speed, seek, the player list, the score chart, share
links and keyboard handling. **You own pixels.** Adding a game requires zero changes to the shell.

`renderFrame` is fed by your own `replay-decode`, running in the browser from the same component
digest the match recorded — which is why the viewer and the referee cannot disagree.

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
