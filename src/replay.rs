//! The replay: the action stream, not frames.
//!
//! `DESIGN.md` §8 and §3.3. Game state is integer-only, so a replay can store what everyone *did*
//! and the viewer re-simulates the match rather than being shipped a hundred times the bytes. The
//! platform never decodes a delta; `replay-decode` does, in the browser, against the same digest
//! that produced it.
//!
//! A turn's delta is one string per seat, one character per ant, in `mine`'s order — which is the
//! same ordering the actions arrived in, so a delta and an action array are the same shape.

use serde_json::{json, Value};

use crate::codec::unpack;
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

/// `tb.ants.replay-decode(payload, turn)` — one frame, for the viewer.
///
/// **Note the name.** Orion refuses a plugin function label that is not `[a-z][a-z0-9-]*`, so the
/// `replay_decode` spelling in `cartridge.md` §1 does not load at all; it is `replay-decode`.
/// Found by the wave-turn spike (`design/v2/03-spike/FINDINGS.md` §2.1).
///
/// The envelope carries the seed and the preset, so a frame is produced by replaying the action
/// stream from turn zero rather than by storing one. That is the determinism law paying for
/// itself: the viewer needs the same component the referee used, and nothing else.
pub fn decode(payload: &Value, turn: u16) -> Result<Value, String> {
    let state = payload
        .get("state0")
        .and_then(Value::as_str)
        .ok_or("the envelope has no state0")?;
    let wave = unpack(state).ok_or("state0 did not decode")?;
    let mut m = wave.matches.into_iter().next().ok_or("state0 holds no match")?;

    let deltas = payload.get("deltas").and_then(Value::as_array).cloned().unwrap_or_default();
    let food_target = m.food.len();

    for d in &deltas {
        if m.turn >= turn || m.done {
            break;
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
        crate::turn::step(&mut m, &moves, food_target);
    }

    let (rows, cols) = (m.g.rows, m.g.cols);
    let rc = |p: u16| json!([p as i32 / cols, p as i32 % cols]);
    Ok(json!({
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
        "ranks": ranks(&m),
        "done":  m.done,
    }))
}
