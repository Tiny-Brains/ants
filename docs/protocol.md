# TinyBrains Protocol v1

**Send state. Receive actions. When the match ends, stop sending.**

```
while !done {
    for each player p:   s = observe(wave, p)           // opaque JSON, from the cartridge
    for each player p:   a = model_p(s)                 // adapter -> ONNX -> adapter
    wave, done = step(wave, [a_0, a_1, …])              // opaque JSON, to the cartridge
}
results = finish(wave)                                  // ranks, scores, reason
```

There is no envelope, no version field, no message type, no setup handshake and **no terminal
message**. A model receives states while the match is running and nothing afterwards. It is never
told that it lost, or why — that reaches the platform through `finish()`, and reaches the SDK the
same way, since the SDK drives the loop and knows the last state it sent.

| Path | What it is |
|---|---|
| `schema/tb-cartridge.schema.json` | The platform's **only** schema. Read once, at registration. |
| `schema/ants/` | The reference game: `state.schema.json`, `action.schema.json`, three worked examples |
| `schema/examples/{tron,planetwars}.json` | Comparison sketches |
| `schema/validate.py` | 3 cartridges · 19 rejects · 3 state/action pairs · 17 rejects · 12 cross-field · 3 properties |

---

## 1. What a state contains

Only what the model needs to choose this turn's moves. For Ants that is the map size, your ants,
the enemy ants you can see, food, hills, and known water:

```json
{
  "size":  [64, 96],
  "mine":  [[12,30], [13,30], [41,77]],
  "foes":  [[12,33,1], [11,34,1]],
  "food":  [[11,31], [40,80]],
  "hills": [[20,20,0], [44,76,1]],
  "water": { "rle": [0,812, 1,6, 0,4110, 1,12, 0,1204] }
}
```

```json
["N", "-", "E"]
```

**The action array is positionally aligned with `mine`.** `action[i]` is the move for `mine[i]`.
No ids, no coordinates, no command objects — the ordering *is* the addressing. Zero ants means an
empty array, which is always valid.

### Everything removed, and why

| Removed | Why |
|---|---|
| `turn` | Ants has no scoring cliff at the turn limit, so a move rarely turns on how many turns remain. Planet Wars **keeps** it, because it scores at the limit — the principle is *send what the model needs to decide*, not *strip everything*. |
| `wrap` | Ants maps always wrap. A constant of the game, baked into the weights. |
| `view2`, `attack2`, `spawn2` | Same — constants, not state. |
| `vis` (visibility mask) | Derivable: visible cells are the union of view-radius disks around `mine`, and the radius is a constant. Same reasoning that made Planet Wars send coordinates instead of a distance matrix. |
| `scores` | Derivable from hills and history, and not an input to a move. |
| `rejected` | Feedback about last turn's illegal moves. A stateless model cannot act on it anyway. |
| terminal state | The engine stops sending. There is nothing to say. |

The `vis` removal has a real consequence worth stating: with no explored mask, a `0` in `water`
conflates *known empty* with *never seen*. A model that cares about the exploration frontier has to
carry that in its recurrent state. That is the cost of the trade, and it is the model's problem
rather than the protocol's.

---

## 2. The functions

The cartridge is a WebAssembly component, and each of these is a pure JSON-to-JSON function it
exports. Nothing here is a method on a live object: the sandbox keeps no state between calls, so the
wave is threaded through every one of them.

```
worldgen(seed[], preset, players)   → wave_state
observe(wave_state)                 → [per-seat views]     // fog-filtered; empty once a match ends
step(wave_state, actions)           → { wave_state, done[], replay_delta }
finish(wave_state)                  → [{ ranks, scores, reason }]   // the only semantic dependency
replay_decode(payload, turn)        → frame                // for the viewer
```

**`wave_state` is not the protocol.** It is the cartridge's own encoding of every match in the wave,
opaque to everything else and decoded only by the component that produced it — a compact blob rather
than readable JSON, because it crosses the sandbox boundary twice per turn. Nothing in this document
describes it, and nothing outside the cartridge may depend on its shape.

**The per-seat view is the protocol**, and it is the thing §1 specifies. It is what a model sees,
and it is unchanged by any of this.

**Where the view goes.** A view leaves the cartridge, crosses the platform untouched, and is handed
to the model runner together with the model's hash and its adapter's. The runner applies the
adapter, runs the graph and applies the adapter in reverse, and answers an action in this same
protocol. The platform between them reads neither end. That the adapter runs there rather than in
the caller is a placement decision, not a protocol one: what travels is still exactly the payloads
below.

---

## 3. The payload rules

Three, down from four. **The root may be any JSON value** — Ants' action is an array, Tron's is a
bare string. The old "object at the root" rule was defensive rather than necessary; JSONLogic
addresses arrays and scalars perfectly well, and the first concrete game to need an array showed it
up as invented.

1. **Observer-relative.** What a player sees may depend on the game situation but never on which
   seat it occupies. As a property: relabel every seat in the world, ask the same player under its
   new label, and the bytes must be identical.
2. **Canonically serialized** (RFC 8785 JCS) so payloads hash reproducibly for determinism audits.
3. **Fog-filtered by the engine**, and under the platform-wide byte cap.

---

## 4. Registration

Read once, when a cartridge is registered. Six keys, each answering a question the platform must
resolve *before* anything communicates.

**The preset carries its own player count** (decision 14, 7 September 2026). There is no top-level
`players`: the number of seats is a property of the map, not of the game, so a preset names a world
and how many play it. `worldgen(seed, preset, players)` keeps its signature — the platform hands
the engine what the preset declared, and the engine may refuse a mismatch.

```json
{
  "game": "ants", "version": "1.0.0", "abi": 1,
  "presets": [ { "name": "standard", "players": 2 },
               { "name": "maze",     "players": 2 },
               { "name": "cell",     "players": 2 } ],
  "limits":  { "max_turns": 1000, "turn_ms": 1000 },
  "budgets": { "flop_caps": { "nano": 2.5e8, … }, "adapter_ops_max": 1000000 }
}
```

---

## 5. The other two games, given the same pass

**Tron** — `alive` is gone; in a two-player game a dead opponent means the match is over and the
engine simply stops. Heads are split into `me`/`foes` for the same reason Ants splits `mine`/`foes`:
the action refers to my unit, so which one that is must be unambiguous.
```json
state   { "size":[30,30], "wall":"AQEBAgIC", "me":[4,11], "foes":[[25,18,1]] }
action  "E"
```

**Planet Wars** — keeps `turn`, drops `scores` (derivable from owner, ships and fleets), and sends
`xy` rather than a precomputed distance matrix.
```json
state   { "turn":12, "xy":[[0,0],[2.1,1.4],…], "growth":[5,3,5,2,4],
          "owner":[0,-1,1,-1,0], "ships":[34,12,29,8,14],
          "fleets":[[0,0,1,18,2],[1,2,1,22,3]] }
action  [[0,1,20],[4,3,9],[0,3,6]]
```

---

## 6. The game's schemas are documentation

`schema/ants/state.schema.json` and `action.schema.json` are written by the game developer and
published for model developers. **The platform never loads them.** They are checked in CI because
the game developer is the platform owner and a broken example helps nobody — not because anything
at runtime depends on them.

Two invariants they state but cannot enforce, so conformance does:

- `len(action) == len(state.mine)`
- the water run-lengths sum to `rows × cols`

---

## 7. What each round bought

Same Ants position, 180 ants and 40 food:

| Draft | Per message | |
|---|---|---|
| 1 — standardized game vocabulary | 15,208 B | |
| 2 — five-field envelope | 2,162 B | 7.0× smaller |
| 3 — no envelope | 2,127 B | a further 1.6% |
| 4 — no setup | 2,155 B | slightly larger |
| 5 — only what the model needs | 1,985 B | **7.7× smaller than draft 1** |

Draft 5 is the first round since draft 2 to save real bytes, and it does so by deleting
information rather than compressing it — which is the only kind of saving that also makes the
thing simpler.

---

## 8. Open questions

1. **Was dropping `turn` from Ants right?** It is cheap and endgame behaviour might turn on it. The
   counter-argument is that a model can infer lateness from ant counts and map control. Worth an
   A/B once there is a ladder.
2. ~~**Ordering of `mine`.**~~ **DECIDED 7 September 2026: deterministic, not stable.** It stays
   "meaningless across turns", and the engine additionally commits to a *fixed sort* — row-major by
   position — so a determinism audit reproduces without the ordering becoming an identity channel.

   True across-turn stability would mean an ant keeping its slot for its lifetime, so a dead ant
   leaves a hole and `mine` goes sparse — which is the "no ids, the ordering **is** the addressing"
   rule in §1 collapsing. And the capability it buys is smaller than it looks: a grid game invites a
   spatial encoding, where the adapter scatters ant positions onto a plane and position *is* the
   identity. Per-ant-vector models are the less natural architecture here.

   The asymmetry decided it: going from "meaningless" to "stable" later breaks no adapter, and the
   reverse breaks every one. Revisit if a real model architecture ever needs the tracking.
3. **What does `water` actually carry?** *Raised by the wave-turn spike, 7 September 2026, by
   having to implement it* (the wave-turn spike §2.4). §1 calls it
   "known water" and says a model that cares about the frontier "has to carry that in its recurrent
   state" — but a model here is a pure function of one observation, with no channel for state
   between turns, so nothing downstream can accumulate anything. The engine must therefore choose:
   send the whole map (fog is broken for the one thing worth scouting for), send only what is
   visible now (a model can never learn the map), or keep a per-player seen-mask and send
   `water AND seen`.

   Only the third matches what the field is documented to mean, and it is what the spike
   implemented. It costs: the masks are **60% of `wave_state`**, and worse, they make the
   observation grow monotonically — RLE's worst case is a *half*-explored map, so `water`'s run
   count went from 102 at turn 0 to 774 at turn 600 and the views payload grew 65% over a match's
   life. If the reading is kept, a raw base64 bitmap is 2 KiB flat at any fragmentation and is
   probably the better encoding; `{"rle": [...]}` was chosen when the only water was the map's own,
   which is blobby. Layer 05 owns the decision.

4. ~~**The adapter instruction budget.**~~ **DECIDED 8 September 2026: 1,000,000**, raised from
   200,000, and measured rather than argued. The adapter runs under a run-time operation count
   inside the Model Loader, in logic nodes plus tensor elements (review finding 4, option A).

   Deriving visibility from ant positions is what made it concrete, exactly as this entry said it
   would. A reference six-plane Ants adapter costs **197,272** operations against a real worst-case
   observation — it fits 200,000 with 1.01× headroom, which is to say it accommodates the plainest
   adapter anyone would write and nothing else. The visibility-deriving adapter this entry asked to
   fit *with headroom* cost 217,269 and did not fit at all; worse, it was not expressible
   *correctly*, because Ants maps wrap and the modulo needs a map size that is not in scope where
   the ant is. The dialect gained a `tb.dilate` operator for it, at which point the same mask costs
   32,777.

   A million is about three times the richest adapter written so far, and at a measured 1.8 ms per
   million operations it is 1.8 ms a seat — 58 ms for 32 seats run serially, against a `turn_ms` of
   1000. The budget is a fairness rule rather than a performance one, and at that price there is no
   reason to make it tight. See [axon/docs/dialect.md](https://github.com/Tiny-Brains/axon/blob/main/docs/dialect.md) §4 and `axon/tests/ants_adapter.rs`.

   Related and now decided: **a resign value in the action schema — not yet.** A forfeited seat
   plays no-ops until the engine ends the match; Kalam already ranks forfeits last, so a
   timed-out model cannot win on points, and the only cost is wall clock on a match whose result is
   settled. A resign value is a protocol change every future cartridge must honour, for a case
   needing five consecutive missed turn clocks — and it is additive later, since it is a value no
   existing adapter emits. Measure it in the wave-turn spike first.
5. **Sequential games** remain out of scope, and are now purely a trait change.
