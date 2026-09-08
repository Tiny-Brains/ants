//! The replay: the action stream, not frames.
//!
//! the platform design §8 and §3.3. Game state is integer-only, so a replay can store what everyone *did*
//! and the viewer re-simulates the match rather than being shipped a hundred times the bytes. The
//! platform never decodes a delta; `replay-decode` does, in the browser, against the same digest
//! that produced it.
//!
//! A turn's delta is one string per seat, one character per ant, in `mine`'s order — which is the
//! same ordering the actions arrived in, so a delta and an action array are the same shape.
//!
//! # The envelope carries its own board
//!
//! An earlier version of this file rebuilt the match from a `state0` the envelope was supposed to
//! carry and never did — Kalam's `put` wrote `seed`, `preset` and `deltas`, so `decode` refused
//! every replay the platform had ever stored. It now rebuilds from `map`, the board `finish`
//! emits, which is better than either: a replay stays viewable when the preset table has been
//! re-tuned or the map catalogue has moved on, because it brought its terrain with it.
//!
//! `seed` is still required and still matters. The map fixes the board; the seed drives food
//! respawn every turn through `turn_rng`, so a replay missing its seed re-simulates a different
//! match on the same terrain.

use serde_json::{json, Value};

use crate::mapfile::MapFile;
use crate::state::Match;
use crate::turn::ranks;

/// The per-turn delta `step` returns for one match.
pub fn delta(m: &Match, turn: u16, moves: &[Vec<String>]) -> Value {
    let acts: Vec<String> = (0..m.players)
        .map(|seat| {
            moves
                .get(seat as usize)
                .map(|v| v.iter().map(|a| first_char(a)).collect::<String>())
                .unwrap_or_default()
        })
        .collect();
    json!({ "t": turn, "a": acts })
}

fn first_char(a: &str) -> char {
    match a {
        "N" | "E" | "S" | "W" => a.chars().next().unwrap(),
        _ => '-',
    }
}

/// Rebuild the match an envelope describes, at turn zero.
fn open(payload: &Value) -> Result<Match, String> {
    let map = payload.get("map").ok_or("the envelope carries no map")?;
    let mf = MapFile::from_json(map).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let seed = payload.get("seed").and_then(Value::as_u64).unwrap_or(0);
    let max_turns = payload.get("max_turns").and_then(Value::as_u64).unwrap_or(1000) as u16;
    mf.build(seed, max_turns).map_err(|e| format!("{}: {}", e.code, e.message))
}

/// Advance `m` through the envelope's deltas until it reaches `turn`.
fn advance(m: &mut Match, deltas: &[Value], turn: u16) {
    let food_target = m.food_target as usize;
    for d in deltas {
        if m.turn >= turn || m.done {
            break;
        }
        // A delta is played only if it is the one for the turn the match is actually on. The
        // stream is written in order and a match's deltas are filtered out of the wave's when it
        // finishes, so this is a guard against a hand-edited envelope rather than a normal case.
        if d.get("t").and_then(Value::as_u64) != Some(m.turn as u64) {
            continue;
        }
        let moves: Vec<Vec<String>> = d
            .get("a")
            .and_then(Value::as_array)
            .map(|seats| {
                seats
                    .iter()
                    .map(|s| {
                        s.as_str()
                            .unwrap_or("")
                            .chars()
                            .map(|c| c.to_string())
                            .collect::<Vec<String>>()
                    })
                    .collect()
            })
            .unwrap_or_default();
        crate::turn::step(m, &moves, food_target);
    }
}

/// One frame, for the viewer.
fn frame(m: &Match) -> Value {
    let (rows, cols) = (m.g.rows, m.g.cols);
    let rc = |p: u16| json!([p as i32 / cols, p as i32 % cols]);
    json!({
        "turn":  m.turn,
        "size":  [rows, cols],
        "water": { "rle": m.water.rle() },
        "ants":  m.ants.iter().map(|a| {
                    let (r, c) = m.g.rc(a.pos);
                    json!([r, c, a.owner])
                 }).collect::<Vec<_>>(),
        "food":  m.food.iter().copied().map(rc).collect::<Vec<_>>(),
        "hills": m.hills.iter().filter(|h| !h.razed).map(|h| {
                    let (r, c) = m.g.rc(h.pos);
                    json!([r, c, h.owner])
                 }).collect::<Vec<_>>(),
        "score": m.score,
        "ranks": ranks(m),
        "done":  m.done,
    })
}

/// `tb.ants.replay-decode(payload, turn)` — one frame, for the viewer.
///
/// **Note the name.** Orion refuses a plugin function label that is not `[a-z][a-z0-9-]*`, so the
/// `replay_decode` spelling in docs/docs/cartridge.md §1 does not load at all; it is `replay-decode`.
/// Found by the wave-turn spike (`the wave-turn spikeFINDINGS.md` §2.1).
pub fn decode(payload: &Value, turn: u16) -> Result<Value, String> {
    let mut m = open(payload)?;
    let empty = Vec::new();
    let deltas = payload.get("deltas").and_then(Value::as_array).unwrap_or(&empty);
    advance(&mut m, deltas, turn);
    Ok(frame(&m))
}

/// Every frame from `from` to `to` inclusive, in one call.
///
/// The scrubber is why this exists. `decode` re-simulates from turn zero, so a viewer that asked
/// for each frame in turn would replay the match once per frame — half a million turn-steps to
/// scrub a thousand-turn match, quadratic in exactly the interaction a timeline is made of. This
/// walks the match once and photographs it on the way past.
///
/// It is an optional range on a declared function rather than a sixth one, which is what keeps it
/// inside what `docs/cartridge.md` §9 permits without a design conversation.
pub fn decode_range(payload: &Value, from: u16, to: u16) -> Result<Value, String> {
    let mut m = open(payload)?;
    let empty = Vec::new();
    let deltas = payload.get("deltas").and_then(Value::as_array).unwrap_or(&empty);
    let (from, to) = (from.min(to), from.max(to));

    advance(&mut m, deltas, from);
    let mut frames = vec![frame(&m)];
    let mut t = from;
    while t < to && !m.done {
        t += 1;
        advance(&mut m, deltas, t);
        frames.push(frame(&m));
    }
    Ok(json!({ "from": from, "to": m.turn, "frames": frames }))
}
