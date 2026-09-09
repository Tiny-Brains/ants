//! The map as a file: a board someone can name, commit, diff and hand to a replay.
//!
//! Decision 14 already said where this was going — "seats are a property of the map, not of the
//! game, so a 4-seat map is content rather than code". A `Preset` in `map.rs` is a *generator*, so
//! until now a board existed only as the output of `worldgen` and could be referred to only as a
//! seed. That is enough to play a match and not enough to do any of the things a competitor needs:
//! pin the board while changing the model, ship the board beside a sample match, or open a replay
//! a year later and still see the same terrain.
//!
//! # What a map fixes, and what it does not
//!
//! A map fixes the **board**: its size, its water, its hills, and the food on it at turn zero.
//! It does not fix the match. `turn.rs` calls `spawn_food` every turn from `turn_rng(seed, turn)`,
//! so food respawn is still drawn from the seed and a map played at two seeds is two matches.
//! Both are inputs and both are carried in the replay.
//!
//! # Symmetry stops being a construction and becomes an assertion
//!
//! `state::worldgen` builds one fundamental domain and translates it, so symmetry is true by the
//! shape of the code. A file cannot make that promise — someone will hand-author one — so the
//! guarantee moves here, to `validate`, which every map passes through before it is played:
//! water, hills and turn-zero food must each be closed under the orbit `(rows/p, cols/p)·k`.
//!
//! The failures are `caller_input`: the same map can never succeed, so nothing retries it.

use serde_json::{json, Value};

use crate::map::{Bits, Geom, Symmetry, SPAWN_RADIUS2};
use crate::state::{Ant, Hill, Match};

/// A board, in the shape the JSON file has.
///
/// `water` is `Bits::rle()` — `[value, run, value, run, ...]`, row-major — which is the same
/// encoding an observation carries, so a map file and a view speak one language about terrain.
/// Positions are `[row, col]` pairs rather than indices because a map is a thing people read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFile {
    pub id: String,
    pub preset: String,
    pub rows: u8,
    pub cols: u8,
    pub players: u8,
    pub water: Vec<u32>,
    pub hills: Vec<(i32, i32)>,
    pub food: Vec<(i32, i32)>,
    /// How much food the board is kept stocked with, for the whole match.
    ///
    /// It lives on the map because it is a property of the board, and it is packed into the state
    /// (`codec.rs`) because `step` needs it every turn. Before maps it was recovered by looking a
    /// preset up by its `rows` and `cols`, which two boards of one size would have made ambiguous.
    pub food_target: u16,
}

/// A parse or validation refusal. Carried as a code and a message so `lib.rs` can turn it into a
/// `Fault` without inventing wording.
#[derive(Debug)]
pub struct MapError {
    pub code: &'static str,
    pub message: String,
}

fn err(code: &'static str, message: impl Into<String>) -> MapError {
    MapError { code, message: message.into() }
}

// ---------------------------------------------------------------- reading

fn u8_field(v: &Value, k: &str) -> Result<u8, MapError> {
    let n = v
        .get(k)
        .and_then(Value::as_u64)
        .ok_or_else(|| err("MAP_BAD_SHAPE", format!("the map has no numeric '{k}'")))?;
    if n == 0 || n > 255 {
        // Positions are u16 and a cell is `row * cols + col`, so the grid caps at 256x256.
        return Err(err("MAP_BAD_SHAPE", format!("'{k}' is {n}; it must be 1..=255")));
    }
    Ok(n as u8)
}

fn pairs(v: &Value, k: &str) -> Result<Vec<(i32, i32)>, MapError> {
    let a = match v.get(k) {
        None => return Ok(Vec::new()),
        Some(x) => x
            .as_array()
            .ok_or_else(|| err("MAP_BAD_SHAPE", format!("'{k}' must be an array")))?,
    };
    let mut out = Vec::with_capacity(a.len());
    for (i, e) in a.iter().enumerate() {
        let p = e
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| err("MAP_BAD_SHAPE", format!("{k}[{i}] must be [row, col]")))?;
        let r = p[0]
            .as_i64()
            .ok_or_else(|| err("MAP_BAD_SHAPE", format!("{k}[{i}] row is not a number")))?;
        let c = p[1]
            .as_i64()
            .ok_or_else(|| err("MAP_BAD_SHAPE", format!("{k}[{i}] col is not a number")))?;
        out.push((r as i32, c as i32));
    }
    Ok(out)
}

impl MapFile {
    /// Parse a map from the JSON a file holds. Validation is separate and always follows: a caller
    /// that only wants to read the header still gets a shape it can trust.
    pub fn from_json(v: &Value) -> Result<MapFile, MapError> {
        let rows = u8_field(v, "rows")?;
        let cols = u8_field(v, "cols")?;
        let players = u8_field(v, "players")?;
        let water = v
            .get("water")
            .and_then(Value::as_array)
            .ok_or_else(|| err("MAP_BAD_SHAPE", "the map has no 'water' run-length array"))?
            .iter()
            .map(|n| n.as_u64().unwrap_or(u64::MAX))
            .collect::<Vec<u64>>();
        if water.iter().any(|&n| n > u32::MAX as u64) {
            return Err(err("MAP_BAD_SHAPE", "'water' holds a value that is not a run length"));
        }
        let m = MapFile {
            id: v.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            preset: v.get("preset").and_then(Value::as_str).unwrap_or("").to_string(),
            rows,
            cols,
            players,
            water: water.into_iter().map(|n| n as u32).collect(),
            hills: pairs(v, "hills")?,
            food: pairs(v, "food")?,
            food_target: v
                .get("food_target")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u16::MAX as u64) as u16,
        };
        m.validate()?;
        Ok(m)
    }

    pub fn to_json(&self) -> Value {
        let g = self.geom();
        let sym = Symmetry::for_preset(&g, self.players);
        json!({
            "id": self.id,
            "preset": self.preset,
            "rows": self.rows,
            "cols": self.cols,
            "players": self.players,
            "water": self.water,
            "hills": self.hills.iter().map(|&(r, c)| json!([r, c])).collect::<Vec<_>>(),
            "food":  self.food.iter().map(|&(r, c)| json!([r, c])).collect::<Vec<_>>(),
            "food_target": self.food_target,
            "symmetry": { "dr": sym.dr, "dc": sym.dc },
        })
    }

    pub fn geom(&self) -> Geom {
        Geom::new(self.rows, self.cols)
    }

    /// The terrain, expanded from its run lengths.
    fn bits(&self) -> Result<Bits, MapError> {
        let g = self.geom();
        let cells = g.cells();
        let mut b = Bits::zeros(cells);
        let mut i = 0usize;
        for pair in self.water.chunks(2) {
            if pair.len() != 2 {
                return Err(err("MAP_BAD_SHAPE", "'water' is not a sequence of value/run pairs"));
            }
            let (value, run) = (pair[0], pair[1] as usize);
            if i + run > cells {
                return Err(err(
                    "MAP_BAD_SHAPE",
                    format!("'water' runs past the end of a {}x{} board", self.rows, self.cols),
                ));
            }
            if value == 1 {
                for j in i..i + run {
                    b.set(j);
                }
            }
            i += run;
        }
        if i != cells {
            return Err(err(
                "MAP_BAD_SHAPE",
                format!("'water' covers {i} cells; a {}x{} board has {cells}", self.rows, self.cols),
            ));
        }
        Ok(b)
    }

    // ---------------------------------------------------------------- validation

    /// Everything `worldgen`'s construction used to guarantee, asserted instead.
    pub fn validate(&self) -> Result<(), MapError> {
        let g = self.geom();
        let (rows, cols, players) = (g.rows, g.cols, self.players as i32);

        // The translation is by whole cells, so the board must divide by the seat count. Without
        // this the orbit of a cell is not a partition and no map of this size is symmetric.
        if rows % players != 0 || cols % players != 0 {
            return Err(err(
                "MAP_BAD_SHAPE",
                format!("a {rows}x{cols} board does not divide by {players} seats"),
            ));
        }

        let water = self.bits()?;
        let sym = Symmetry::for_preset(&g, self.players);

        // ---- water is closed under the orbit
        for i in 0..g.cells() {
            let img = sym.image(&g, i as u16, 1) as usize;
            if water.get(i) != water.get(img) {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!("water differs between cell {i} and its image {img}"),
                ));
            }
        }

        // ---- one hill per seat, and every hill is the orbit of the first
        if self.hills.len() != self.players as usize {
            return Err(err(
                "MAP_UNPLAYABLE",
                format!("{} hills for {} seats; each seat needs exactly one", self.hills.len(), self.players),
            ));
        }
        let hill0 = g.at(self.hills[0].0, self.hills[0].1);
        for (k, &(r, c)) in self.hills.iter().enumerate() {
            let want = sym.image(&g, hill0, k as i32);
            if g.at(r, c) != want {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!("seat {k}'s hill is not the {k}th image of seat 0's"),
                ));
            }
        }

        // ---- nobody starts in a lake, or walled into one
        let clear = g.disk(SPAWN_RADIUS2);
        for (k, &(r, c)) in self.hills.iter().enumerate() {
            let pos = g.at(r, c);
            if water.get(pos as usize) {
                return Err(err("MAP_UNPLAYABLE", format!("seat {k}'s hill stands on water")));
            }
            if clear.iter().all(|&(dr, dc)| water.get(g.at(r + dr, c + dc) as usize)) {
                return Err(err("MAP_UNPLAYABLE", format!("seat {k}'s hill is walled in")));
            }
        }

        // ---- food is whole orbits, on land, and off the hills
        let mut have: Vec<u16> = self.food.iter().map(|&(r, c)| g.at(r, c)).collect();
        have.sort_unstable();
        have.dedup();
        if have.len() != self.food.len() {
            return Err(err("MAP_BAD_SHAPE", "'food' names the same square twice"));
        }
        for &(r, c) in &self.food {
            let pos = g.at(r, c);
            if water.get(pos as usize) {
                return Err(err("MAP_UNPLAYABLE", format!("food at [{r}, {c}] is under water")));
            }
            for k in 0..players {
                let img = sym.image(&g, pos, k);
                if have.binary_search(&img).is_err() {
                    return Err(err(
                        "MAP_NOT_SYMMETRIC",
                        format!("food at [{r}, {c}] has no counterpart for seat {k}"),
                    ));
                }
            }
        }

        // `food_target` is deliberately unconstrained against the turn-zero food.
        //
        // An earlier version refused a target BELOW what the board starts with, reasoning that it
        // would make `spawn_food` a no-op for the rest of the match. It does -- and that is a board
        // someone may want: one that starts stocked and never restocks, which is how a teaching
        // board shows food being consumed rather than replaced the same turn. `spawn_food` fills
        // *up to* the target and does nothing when there is already more, so the case was always
        // handled; the refusal was a guess about intent, and it cost a lesson.
        Ok(())
    }

    // ---------------------------------------------------------------- playing

    /// The match this board opens at, for one seed.
    ///
    /// Everything derivable is derived rather than stored: one ant per hill (*Map Format*), and
    /// each seat's `known` folded in from what it can see. A map file that carried them could
    /// disagree with the rules, and there would be no way to tell which was right.
    pub fn build(&self, seed: u64, max_turns: u16) -> Result<Match, MapError> {
        let (rate, turn_len) = crate::state::food_rate_for(seed);
        let g = self.geom();
        let sym = Symmetry::for_preset(&g, self.players);
        let water = self.bits()?;
        let mut m = Match {
            seed,
            turn: 0,
            max_turns,
            players: self.players,
            g,
            sym,
            done: false,
            reason: 0,
            water,
            known: (0..self.players).map(|_| Bits::zeros(g.cells())).collect(),
            ants: Vec::new(),
            food: self.food.iter().map(|&(r, c)| g.at(r, c)).collect(),
            hills: Vec::new(),
            hive: vec![0; self.players as usize],
            score: vec![0; self.players as usize],
            cutoff_bot: crate::state::CUTOFF_NONE,
            cutoff_turns: 0,
            // The hidden rate is drawn from the seed, so two matches on the same board are not
            // the same match -- `state::food_rate_for`.
            food_rate: rate,
            food_turn: turn_len,
            food_extra: 0,
            food_rotation: 0,
            food_cursor: 0,
            pending_food: Vec::new(),
            map_id: self.id.clone(),
            food0: self.food.iter().map(|&(r, c)| g.at(r, c)).collect(),
        };
        for (k, &(r, c)) in self.hills.iter().enumerate() {
            let pos = g.at(r, c);
            m.hills.push(Hill { pos, owner: k as u8, razed: false, last_touched: 0 });
            m.ants.push(Ant { pos, owner: k as u8 });
        }
        // One point per hill owned, before anything is razed — see `state::worldgen`.
        for h in &m.hills {
            m.score[h.owner as usize] += 1;
        }
        for pl in 0..self.players {
            m.reveal(pl);
        }
        Ok(m)
    }

    /// Read the board back off a match, at any turn.
    ///
    /// This is how `mapgen` turns the procedural generator into a file, and how `finish` hands the
    /// board to the replay envelope while the match that used it is ending. It takes the food from
    /// `food0` rather than from `food`, because by then the board's food has been eaten and
    /// respawned many times over and the current set is not the board — it is the position.
    pub fn from_match(m: &Match, id: &str, preset: &str) -> MapFile {
        let rc = |p: u16| m.g.rc(p);
        MapFile {
            id: id.to_string(),
            preset: preset.to_string(),
            rows: m.g.rows as u8,
            cols: m.g.cols as u8,
            players: m.players,
            water: m.water.rle(),
            hills: m.hills.iter().map(|h| rc(h.pos)).collect(),
            food: m.food0.iter().map(|&f| rc(f)).collect(),
            // Every committed board carries as many food as it says it does, and `from_match`
            // takes its food from `food0`, so the count is the board's own.
            food_target: m.food0.len() as u16,
        }
    }
}

// ---------------------------------------------------------------- the catalogue

/// Every committed board, compiled in — `maps_gen.rs` on why it cannot be read from a file.
///
/// Parsed on demand rather than once: an invocation runs in a fresh instance and nothing is
/// cached across calls (`docs/cartridge.md` §6), so a lazy table would buy nothing and would be
/// the kind of state the sandbox exists to make impossible.
pub fn catalogue() -> Vec<MapFile> {
    crate::maps_gen::MAPS
        .iter()
        .filter_map(|(_, src)| serde_json::from_str::<Value>(src).ok())
        .filter_map(|v| MapFile::from_json(&v).ok())
        .collect()
}

/// The boards a preset is played on, in catalogue order.
pub fn pool(preset: &str) -> Vec<MapFile> {
    catalogue().into_iter().filter(|m| m.preset == preset).collect()
}

/// One board by id, whatever preset it belongs to.
pub fn by_id(id: &str) -> Option<MapFile> {
    catalogue().into_iter().find(|m| m.id == id)
}

/// The board a match is played on, chosen by its seed.
///
/// **The seed chooses, not the caller.** A competitor who could name the board could train against
/// it; a seed is assigned by pairing and is not theirs to pick. Selection is `seed % len` so it is
/// deterministic, and a replay that carries its seed and its preset can say which board it was
/// even before it reads the map it carries.
pub fn for_seed(preset: &str, seed: u64) -> Option<MapFile> {
    let p = pool(preset);
    if p.is_empty() {
        return None;
    }
    let i = (seed % p.len() as u64) as usize;
    Some(p[i].clone())
}

/// Resolve what a caller passed for one match: a map id, a whole map object, or nothing.
///
/// Nothing is the platform's case — Kalam passes seeds and a preset and lets the seed choose.
/// The other two are the local case, and the reason `match.json` can pin a board.
pub fn resolve(spec: Option<&Value>, preset: &str, seed: u64) -> Result<MapFile, MapError> {
    match spec {
        None | Some(Value::Null) => for_seed(preset, seed).ok_or_else(|| {
            err("NO_SUCH_MAP", format!("no map in the catalogue is played at preset '{preset}'"))
        }),
        Some(Value::String(id)) => by_id(id)
            .ok_or_else(|| err("NO_SUCH_MAP", format!("no map '{id}' in the catalogue"))),
        Some(v @ Value::Object(_)) => MapFile::from_json(v),
        Some(_) => Err(err("MAP_BAD_SHAPE", "a map must be an id, an object, or absent")),
    }
}
