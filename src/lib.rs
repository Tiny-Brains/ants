//! `tb.ants` — the reference cartridge.
//!
//! Five functions in one component, dispatched on the function name — docs/docs/cartridge.md §1:
//!
//! ```text
//! tb.ants.worldgen(seeds[], preset, players)  → wave_state
//! tb.ants.observe(wave_state, refs)           → [ { m, seat, ref, view } ]
//! tb.ants.step(wave_state, actions)           → { wave_state, done[], ended[], replay_delta }
//! tb.ants.finish(wave_state)                  → [ { m, ranks, scores, reason } ]
//! tb.ants.replay-decode(payload, turn)        → frame
//! ```
//!
//! # Three departures from docs/docs/cartridge.md §1 as written, each found by running it
//!
//! The wave-turn spike (`the wave-turn spikeFINDINGS.md`) drove this shape in a real Orion before
//! the cartridge existed. All three are folded into docs/cartridge.md.
//!
//! **`replay-decode`, not `replay_decode`.** Orion refuses a plugin function label that is not
//! `[a-z][a-z0-9-]*`, so the five-function set does not load as written.
//!
//! **`observe` takes `refs`.** A flat list, each entry carrying its own `m` and `seat`, echoed onto
//! the matching view and never inspected. Without it the platform cannot attach a seat's identity
//! to the view it must send to the loader: an Orion `map` body is evaluated with the element as
//! its data context, and a join back to the caller's own rows evaluates to `null` — silently. The
//! list is flat and matched rather than indexed because the caller rebuilds it every turn from the
//! live seats, so a nested `refs[m][seat]` would shift the moment a match in the wave ended.
//!
//! **`actions` is positionally aligned with the last `observe`.** Zipping the loader's reply back
//! onto the views needs an index inside a `map`, and JSONLogic has none. The alignment is safe
//! because both sides derive the order from the same `wave_state`, and a match ending mid-wave is
//! tested. The explicit `{m, seat, action}` form is accepted too, for a caller that can build it.

mod codec;
mod map;
mod mapfile;
mod maps_gen;
mod observe;
mod replay;
mod state;
mod turn;

use codec::{pack, unpack, Wave};
use serde_json::{json, Value};

#[derive(Debug, PartialEq)]
pub struct Fault {
    pub code: &'static str,
    pub message: String,
}

impl Fault {
    fn new(code: &'static str, message: impl Into<String>) -> Fault {
        Fault { code, message: message.into() }
    }
}

/// The presets, for the registration manifest. `src/bin/manifest.rs` generates `cartridge.json`
/// from this, so the manifest and the engine cannot disagree about how many seats a map is played
/// at.
pub fn presets() -> &'static [map::Preset] {
    &map::PRESETS
}

/// The turn limit the manifest publishes — the rules of Ants in the book §12.
pub const MAX_TURNS: u16 = 1000;

/// Run the procedural generator once and hand back the board it produced, as a map file.
///
/// This is the whole of `src/bin/mapgen.rs`, and it is the reason the generator survives maps
/// becoming files: it is no longer a step in a match, it is the factory that writes the boards a
/// match is played on. `build.sh` calls it, the output is committed, and nothing at runtime grows
/// a world any more.
pub fn generate_map(preset_name: &str, seed: u64, id: &str) -> Option<Value> {
    let p = map::preset(preset_name)?;
    let m = state::worldgen(seed, p, MAX_TURNS);
    Some(mapfile::MapFile::from_match(&m, id, p.name).to_json())
}

pub const FUNCTIONS: [&str; 5] = [
    "tb.ants.worldgen",
    "tb.ants.observe",
    "tb.ants.step",
    "tb.ants.finish",
    "tb.ants.replay-decode",
];

pub fn invoke(function: &str, input: Value) -> Result<Value, Fault> {
    match function {
        "tb.ants.worldgen" => f_worldgen(&input),
        "tb.ants.observe" => f_observe(&input),
        "tb.ants.step" => f_step(&input),
        "tb.ants.finish" => f_finish(&input),
        "tb.ants.replay-decode" => f_replay_decode(&input),
        other => Err(Fault::new(
            "UNKNOWN_FUNCTION",
            format!("this component exports no '{other}'"),
        )),
    }
}

fn state_of(input: &Value) -> Result<Wave, Fault> {
    let s = input
        .get("wave_state")
        .and_then(Value::as_str)
        .ok_or_else(|| Fault::new("NO_STATE", "wave_state is required and must be a string"))?;
    unpack(s).ok_or_else(|| Fault::new("BAD_STATE", "wave_state did not decode"))
}

fn f_worldgen(input: &Value) -> Result<Value, Fault> {
    let seeds: Vec<u64> = input
        .get("seeds")
        .and_then(Value::as_array)
        .ok_or_else(|| Fault::new("NO_SEEDS", "seeds is required and must be an array"))?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0))
        .collect();
    if seeds.is_empty() {
        return Err(Fault::new("NO_SEEDS", "a wave of no matches has nothing to play"));
    }
    let name = input.get("preset").and_then(Value::as_str).unwrap_or("standard");
    // The preset table still names the presets: it is what `cartridge.json` publishes and what
    // `mapgen` generates a pool from. A board is a file now, so the table no longer *builds* the
    // world -- but a caller naming a preset nothing is played at should hear that, rather than
    // "no map", which would send them looking in the catalogue for a fault in the request.
    if map::preset(name).is_none() {
        return Err(Fault::new("NO_SUCH_PRESET", format!("no preset '{name}'")));
    }
    let max_turns = input.get("max_turns").and_then(Value::as_u64).unwrap_or(1000) as u16;

    // What board each match is played on. Three forms, and the platform uses the third:
    //
    //   "maps": [<id or object or null>, ...]   one per seed, positionally
    //   "map":  <id or object>                   the same board for every seed
    //   absent                                   the preset's pool, chosen by the seed
    //
    // The last is what Kalam sends, and it is deliberate that a caller who does not ask cannot
    // influence the board: pairing assigns the seed, so the seed assigning the map keeps a
    // competitor from training against a board they chose. `mapfile::for_seed` is the whole rule.
    let per_seed = input.get("maps").and_then(Value::as_array);
    let one = input.get("map");
    let mut matches = Vec::with_capacity(seeds.len());
    for (i, &seed) in seeds.iter().enumerate() {
        let spec = per_seed.map(|a| a.get(i).unwrap_or(&Value::Null)).or(one);
        let mf = mapfile::resolve(spec, name, seed).map_err(|e| Fault::new(e.code, e.message))?;

        // Decision 14: seats are a property of the map, so the map is what a caller's `players` is
        // checked against -- refused rather than quietly seated short (docs/protocol.md §4).
        if let Some(players) = input.get("players").and_then(Value::as_u64) {
            if players != mf.players as u64 {
                return Err(Fault::new(
                    "PLAYER_COUNT",
                    format!("map '{}' is played at {} seats, not {players}", mf.id, mf.players),
                ));
            }
        }
        matches.push(mf.build(seed, max_turns).map_err(|e| Fault::new(e.code, e.message))?);
    }

    let seats = matches[0].players;
    let w = Wave { matches };
    Ok(json!({
        "wave_state": pack(&w),
        "matches": w.matches.len(),
        "seats": seats,
        "preset": name,
        "map_ids": w.matches.iter().map(|m| m.map_id.clone()).collect::<Vec<_>>(),
    }))
}

fn f_observe(input: &Value) -> Result<Value, Fault> {
    let w = state_of(input)?;
    let empty = Vec::new();
    let refs = input.get("refs").and_then(Value::as_array).unwrap_or(&empty);
    let find = |mi: usize, seat: u8| -> Option<&Value> {
        refs.iter().find(|r| {
            r.get("m").and_then(Value::as_u64) == Some(mi as u64)
                && r.get("seat").and_then(Value::as_u64) == Some(seat as u64)
        })
    };

    let mut views = Vec::new();
    for (mi, m) in w.matches.iter().enumerate() {
        // `observe` returns nothing for a finished match — docs/docs/cartridge.md §1. There is no terminal
        // message: a model receives states while its match runs and nothing afterwards, and is
        // never told that it lost.
        if m.done {
            continue;
        }
        for seat in 0..m.players {
            let mut v = json!({ "m": mi, "seat": seat, "view": observe::view(m, seat) });
            if let Some(r) = find(mi, seat) {
                v["ref"] = r.clone();
            }
            views.push(v);
        }
    }
    Ok(json!({ "views": views }))
}

fn f_step(input: &Value) -> Result<Value, Fault> {
    let mut w = state_of(input)?;
    let actions = input
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| Fault::new("NO_ACTIONS", "actions is required and must be an array"))?;

    let mut moves: Vec<Vec<Vec<String>>> =
        w.matches.iter().map(|m| vec![Vec::new(); m.players as usize]).collect();

    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|v| v.iter().map(|x| x.as_str().unwrap_or("-").to_string()).collect())
            .unwrap_or_default()
    };

    if actions.iter().all(|a| !a.is_object()) {
        // Positional, over the live view order — which is a pure function of `wave_state`, so both
        // sides derive it from the same place.
        let mut k = 0usize;
        for (mi, m) in w.matches.iter().enumerate() {
            if m.done {
                continue;
            }
            for slot in moves[mi].iter_mut() {
                *slot = strings(actions.get(k));
                k += 1;
            }
        }
        if k != actions.len() {
            return Err(Fault::new(
                "BAD_ACTION",
                format!("{} actions for {k} live seats", actions.len()),
            ));
        }
    } else {
        for a in actions {
            let mi = a.get("m").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
            let seat = a.get("seat").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
            if mi >= moves.len() || seat >= moves[mi].len() {
                return Err(Fault::new("BAD_ACTION", format!("no seat {seat} of match {mi}")));
            }
            moves[mi][seat] = strings(a.get("action"));
        }
    }

    let mut deltas = Vec::new();
    let mut ended = Vec::new();
    for (mi, m) in w.matches.iter_mut().enumerate() {
        if m.done {
            continue;
        }
        let turn = m.turn;
        let food_target = target_food(m);
        deltas.push({
            let mut d = replay::delta(m, turn, &moves[mi]);
            d["m"] = json!(mi);
            d
        });
        turn::step(m, &moves[mi], food_target);
        if m.done {
            ended.push(mi);
        }
    }

    Ok(json!({
        "wave_state": pack(&w),
        "done": w.matches.iter().map(|m| m.done).collect::<Vec<_>>(),
        "ended": ended,
        "replay_delta": deltas,
    }))
}

/// How much food the map is kept stocked with.
///
/// Read off the state, which read it off the map. It used to be recovered by finding the preset
/// whose `rows` and `cols` matched the board -- which was unambiguous only while every board of a
/// size came from one preset, and stopped being true the moment a board became a file.
fn target_food(m: &state::Match) -> usize {
    m.food_target as usize
}

fn f_finish(input: &Value) -> Result<Value, Fault> {
    let w = state_of(input)?;
    Ok(json!({
        "results": w.matches.iter().enumerate().map(|(mi, m)| json!({
            "m": mi,
            "ranks":  turn::ranks(m),
            "scores": m.score,
            "reason": state::END_REASONS[m.reason as usize % state::END_REASONS.len()],
            "turns":  m.turn,
            "done":   m.done,
            // THE BOARD, so the replay envelope is self-sufficient -- a replay carries what it was
            // played on and needs no catalogue, no preset table and no second lookup to be viewed,
            // however far the game has moved on since. It is emitted here rather than from
            // `worldgen` because this is the call the drain makes at the moment it writes the
            // envelope, so the map arrives exactly when it is needed and is carried across no turns.
            //
            // Only for a match that has ENDED. The drain calls `finish` on every sweep while
            // anything is queued and reads only the head, so sending every live match's board as
            // well would repeat tens of kilobytes a turn to be thrown away. A board is written
            // once, into the envelope of the match that was played on it.
            "map_id": m.map_id,
            "map": if m.done {
                mapfile::MapFile::from_match(m, &m.map_id, "").to_json()
            } else {
                Value::Null
            },
        })).collect::<Vec<_>>()
    }))
}

fn f_replay_decode(input: &Value) -> Result<Value, Fault> {
    let payload = input.get("payload").cloned().unwrap_or(Value::Null);
    // A range if the caller asked for one, a single frame otherwise. `to` alone means "from the
    // turn you asked for, to there"; `from` alone means "to the end of what the deltas hold".
    let from = input.get("from").and_then(Value::as_u64);
    let to = input.get("to").and_then(Value::as_u64);
    if from.is_some() || to.is_some() {
        let a = from.unwrap_or(0) as u16;
        let b = to.unwrap_or(u16::MAX as u64) as u16;
        return replay::decode_range(&payload, a, b).map_err(|e| Fault::new("BAD_REPLAY", e));
    }
    let turn = input.get("turn").and_then(Value::as_u64).unwrap_or(0) as u16;
    replay::decode(&payload, turn)
        .map(|frame| json!({ "frame": frame }))
        .map_err(|e| Fault::new("BAD_REPLAY", e))
}

#[cfg(target_arch = "wasm32")]
mod exported {
    use orion_plugin_sdk::{export_plugin, serde_json::Value, Plugin, PluginError};

    struct TbAnts;

    impl Plugin for TbAnts {
        fn invoke(function: &str, input: Value) -> Result<Value, PluginError> {
            super::invoke(function, input)
                .map_err(|f| PluginError::caller_input(f.code, f.message))
        }
    }

    export_plugin!(TbAnts);
}

#[cfg(test)]
mod tests;
