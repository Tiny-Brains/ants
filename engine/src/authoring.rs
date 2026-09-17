//! The reference observations — what `src/bin/reference.rs` needs, and nothing a match runs.
//!
//! Boards are not grown here any more: `mapgen/`, a crate of its own beside this one, writes them
//! into `maps/`, so a change to how boards are made is not an edit to the component's source. None
//! of this is reachable from a plugin call, so the linker leaves it out of the component.

use serde_json::{Value, json};

use crate::codec::{Wave, pack};
use crate::grid::{DIR_NAMES, DIRS, Rng};
use crate::maps;
use crate::state::Match;
use crate::{MAX_TURNS, turn};

/// Play one match to a busy turn and hand back what every seat could see.
///
/// **The gate is only as good as its worst case.** Admission validates an adapter against these
/// observations and nothing else, so a set drawn from turn zero — two ants, no contact, almost
/// nothing known — would admit adapters that are struck every turn of a real match. This plays to a
/// turn where colonies have grown, explored and met, and takes the views from there.
///
/// `observe` returns nothing for a finished match and a greedy field ends one, so the last live
/// state is kept: asking for turn 250 of a match that ended on turn 190 gives the observations from
/// turn 189 rather than an empty file.
pub fn reference_observations(preset_name: &str, seed: u64, until_turn: u16) -> Option<Value> {
    let mf = maps::for_seed(preset_name, seed)?;
    let mut m = mf.build(seed, MAX_TURNS).ok()?;

    let mut last_live = m.clone();
    let mut rng = Rng(seed ^ 0x5EED_0B5E_2A17_C0DE);
    while m.turn < until_turn && !m.done {
        let moves: Vec<Vec<String>> =
            (0..m.players).map(|seat| greedy_orders(&m, seat, &mut rng)).collect();
        last_live = m.clone();
        turn::step(&mut m, &moves);
    }

    let m = if m.done { last_live } else { m };
    let reached = m.turn;
    let w = Wave { matches: vec![m] };
    let views = crate::f_observe(&json!({ "wave_state": pack(&w) })).ok()?;
    Some(json!({
        "generated_from": {
            "preset": preset_name, "seed": seed,
            "asked_for_turn": until_turn, "turn": reached,
            "map": mf.id,
        },
        "observations": views.get("views")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v["view"].clone()).collect::<Vec<_>>())
            .unwrap_or_default(),
    }))
}

/// A greedy walker: each ant steps toward the nearest food no other ant has claimed, and wanders
/// when there is none in sight.
///
/// Not play, and not meant to be — what the reference set has to contain is a board in a demanding
/// state, and a random walk will not produce one: it collects food by accident, so the colony never
/// grows and after two hundred turns a seat still has four ants. The claim matters as much as the
/// greed: without it the whole colony converges on one square and every ant dies there.
fn greedy_orders(m: &Match, seat: u8, rng: &mut Rng) -> Vec<String> {
    let mine = m.mine(seat);
    let mut claimed: Vec<u16> = Vec::new();
    let mut orders = Vec::with_capacity(mine.len());
    for &pos in &mine {
        let target = m
            .food
            .iter()
            .copied()
            .filter(|f| !claimed.contains(f))
            .min_by_key(|&f| m.g.dist2(pos, f));
        if let Some(f) = target {
            claimed.push(f);
        }
        orders.push(
            match target {
                Some(f) if m.g.dist2(pos, f) > 0 => {
                    // Whichever of the four steps ends up closest; ties fall to the first, which is
                    // stable and therefore reproducible.
                    let (r, c) = m.g.rc(pos);
                    let best = DIRS
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, (dr, dc))| m.g.dist2(m.g.at(r + dr, c + dc), f))
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    DIR_NAMES[best]
                }
                // Nothing visible: wander, so the colony covers ground rather than sitting on a hill
                // and knowing nothing about the board.
                _ => DIR_NAMES[rng.below(4) as usize],
            }
            .to_string(),
        );
    }
    orders
}
