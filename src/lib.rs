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
    let p = map::preset(name)
        .ok_or_else(|| Fault::new("NO_SUCH_PRESET", format!("no preset '{name}'")))?;

    // Decision 14: the preset carries the seat count, and a caller that disagrees is refused
    // rather than quietly seated short. docs/protocol.md §4: "the engine may refuse a mismatch".
    if let Some(players) = input.get("players").and_then(Value::as_u64) {
        if players != p.players as u64 {
            return Err(Fault::new(
                "PLAYER_COUNT",
                format!("preset '{name}' is played at {} seats, not {players}", p.players),
            ));
        }
    }
    let max_turns = input.get("max_turns").and_then(Value::as_u64).unwrap_or(1000) as u16;

    let w = Wave {
        matches: seeds.iter().map(|&s| state::worldgen(s, p, max_turns)).collect(),
    };
    Ok(json!({
        "wave_state": pack(&w),
        "matches": w.matches.len(),
        "seats": p.players,
        "preset": p.name,
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

/// How much food the map is kept stocked with. Read off the preset rather than stored, so it
/// cannot drift from the preset table between a match and its replay.
fn target_food(m: &state::Match) -> usize {
    map::PRESETS
        .iter()
        .find(|p| p.rows == m.g.rows as u8 && p.cols == m.g.cols as u8)
        .map(|p| p.food_per_player as usize * p.players as usize)
        .unwrap_or(m.food.len())
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
        })).collect::<Vec<_>>()
    }))
}

fn f_replay_decode(input: &Value) -> Result<Value, Fault> {
    let turn = input.get("turn").and_then(Value::as_u64).unwrap_or(0) as u16;
    let payload = input.get("payload").cloned().unwrap_or(Value::Null);
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
