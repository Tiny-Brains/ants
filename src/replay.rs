//! The replay: the action stream, not frames.
//!
//! Game state is integer-only, so a replay stores what everyone *did* and the viewer re-simulates
//! the match rather than being shipped a hundred times the bytes. The platform never decodes a
//! delta; `replay-decode` does, in the browser, against the same digest that produced it.
//!
//! A turn's delta is one string per seat, one character per ant, in `mine`'s order — the same
//! ordering the actions arrived in, so a delta and an action array are the same shape.
//!
//! The envelope carries its own board. `decode` rebuilds from `map`, which `finish` emits, so a
//! replay stays viewable when the preset table has been re-tuned or the catalogue has moved on.
//! `seed` is still required: the map fixes the board, but the seed drives the hidden food rate and
//! every respawn, so a replay missing its seed re-simulates a different match on the same terrain.

use serde_json::{json, Value};

use crate::mapfile::MapFile;
use crate::observe::{rc, rc_owned};
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

/// A match rebuilt at turn zero, and the deltas that drive it.
struct Tape<'a> {
    m: Match,
    deltas: &'a [Value],
    /// How far through `deltas` the match has been played. The stream is in order, so a scan never
    /// needs to start again from the front — which is what keeps `decode_range` linear.
    at: usize,
}

impl<'a> Tape<'a> {
    fn open(payload: &'a Value) -> Result<Tape<'a>, String> {
        let map = payload.get("map").ok_or("the envelope carries no map")?;
        let mf = MapFile::from_json(map).map_err(|e| format!("{}: {}", e.code, e.message))?;
        let seed = payload.get("seed").and_then(Value::as_u64).unwrap_or(0);
        let max_turns = payload.get("max_turns").and_then(Value::as_u64).unwrap_or(1000) as u16;
        Ok(Tape {
            m: mf.build(seed, max_turns).map_err(|e| format!("{}: {}", e.code, e.message))?,
            deltas: payload.get("deltas").and_then(Value::as_array).map_or(&[], Vec::as_slice),
            at: 0,
        })
    }

    /// Play forward until the match reaches `turn`.
    fn advance(&mut self, turn: u16) {
        while self.at < self.deltas.len() && self.m.turn < turn && !self.m.done {
            let d = &self.deltas[self.at];
            self.at += 1;
            // A delta is played only if it is the one for the turn the match is on. The stream is
            // written in order and a match's deltas are filtered out of the wave's when it
            // finishes, so this guards a hand-edited envelope rather than a normal case.
            if d.get("t").and_then(Value::as_u64) != Some(self.m.turn as u64) {
                continue;
            }
            let moves: Vec<Vec<String>> = d
                .get("a")
                .and_then(Value::as_array)
                .map(|seats| {
                    seats
                        .iter()
                        .map(|s| s.as_str().unwrap_or("").chars().map(String::from).collect())
                        .collect()
                })
                .unwrap_or_default();
            crate::turn::step(&mut self.m, &moves);
        }
    }

    /// One frame, for the viewer.
    fn frame(&self) -> Value {
        let m = &self.m;
        json!({
            "turn":  m.turn,
            "size":  [m.g.rows, m.g.cols],
            "water": { "rle": m.water.rle() },
            "ants":  m.ants.iter().map(|a| rc_owned(&m.g, a.pos, a.owner)).collect::<Vec<_>>(),
            "food":  m.food.iter().map(|&f| rc(&m.g, f)).collect::<Vec<_>>(),
            "hills": m.hills.iter().filter(|h| !h.razed)
                       .map(|h| rc_owned(&m.g, h.pos, h.owner)).collect::<Vec<_>>(),
            "score": m.score,
            "ranks": ranks(m),
            "done":  m.done,
        })
    }
}

/// `tb.ants.replay-decode(payload, turn)` — one frame.
///
/// Note the name: Orion refuses a plugin function label that is not `[a-z][a-z0-9-]*`, so the
/// `replay_decode` spelling does not load at all.
pub fn decode(payload: &Value, turn: u16) -> Result<Value, String> {
    let mut tape = Tape::open(payload)?;
    tape.advance(turn);
    Ok(tape.frame())
}

/// Every frame from `from` to `to` inclusive, in one call.
///
/// The scrubber is why this exists. `decode` re-simulates from turn zero, so a viewer asking for
/// each frame in turn would replay the match once per frame — quadratic in exactly the interaction
/// a timeline is made of. This walks the match once and photographs it on the way past.
pub fn decode_range(payload: &Value, from: u16, to: u16) -> Result<Value, String> {
    let mut tape = Tape::open(payload)?;
    let (from, to) = (from.min(to), from.max(to));

    tape.advance(from);
    let mut frames = vec![tape.frame()];
    let mut t = from;
    while t < to && !tape.m.done {
        t += 1;
        tape.advance(t);
        frames.push(tape.frame());
    }
    Ok(json!({ "from": from, "to": tape.m.turn, "frames": frames }))
}
