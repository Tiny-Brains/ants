//! The map as a file: a board someone can name, commit, diff and hand to a replay.
//!
//! A map fixes the **board** — its size, its water, its hills, and the food on it at turn zero. It
//! does not fix the match: the hidden food rate and every respawn after turn zero are drawn from
//! the seed, so a board played at two seeds is two matches. Both are carried in the replay.
//!
//! `state::worldgen` builds one fundamental domain and translates it, so symmetry is true by the
//! shape of the code. A file cannot make that promise — someone will hand-author one — so the
//! guarantee moves here, to `validate`, which every map passes through before it is played. Its
//! refusals are `caller_input`: the same map can never succeed, so nothing retries it.

use serde_json::{json, Value};

use crate::map::{Bits, Geom, Symmetry, SPAWN_RADIUS2};
use crate::state::Match;

/// A board, in the shape the JSON file has.
///
/// `water` is `Bits::rle()` — `[value, run, value, run, ...]`, row-major — the same encoding an
/// observation carries, so a map file and a view speak one language about terrain. Positions are
/// `[row, col]` pairs rather than indices because a map is a thing people read.
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
    /// How much food the board carries at turn zero. Board metadata: the engine plays from `food`
    /// itself, and this is what the published catalogue reports per board.
    pub food_target: u16,
}

/// A parse or validation refusal, carried as a code and a message so `lib.rs` can turn it into a
/// `Fault` without inventing wording.
#[derive(Debug)]
pub struct MapError {
    pub code: &'static str,
    pub message: String,
}

fn err(code: &'static str, message: impl Into<String>) -> MapError {
    MapError { code, message: message.into() }
}

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
    /// Parse a map from the JSON a file holds. Validation always follows, so a caller that only
    /// wants to read the header still gets a shape it can trust.
    pub fn from_json(v: &Value) -> Result<MapFile, MapError> {
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
            rows: u8_field(v, "rows")?,
            cols: u8_field(v, "cols")?,
            players: u8_field(v, "players")?,
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
        let sym = Symmetry::for_preset(&self.geom(), self.players);
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

    /// Every hill, and every food, as grid positions.
    fn cells_of(&self, of: &[(i32, i32)]) -> Vec<u16> {
        let g = self.geom();
        of.iter().map(|&(r, c)| g.at(r, c)).collect()
    }

    /// The terrain, expanded from its run lengths.
    fn bits(&self) -> Result<Bits, MapError> {
        let cells = self.geom().cells();
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

        for i in 0..g.cells() {
            let img = sym.image(&g, i as u16, 1) as usize;
            if water.get(i) != water.get(img) {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!("water differs between cell {i} and its image {img}"),
                ));
            }
        }

        if self.hills.len() != self.players as usize {
            return Err(err(
                "MAP_UNPLAYABLE",
                format!(
                    "{} hills for {} seats; each seat needs exactly one",
                    self.hills.len(),
                    self.players
                ),
            ));
        }
        let hills = self.cells_of(&self.hills);
        for (k, &pos) in hills.iter().enumerate() {
            if pos != sym.image(&g, hills[0], k as i32) {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!("seat {k}'s hill is not the {k}th image of seat 0's"),
                ));
            }
        }

        // Nobody starts in a lake, or walled into one.
        let clear = g.disk(SPAWN_RADIUS2);
        for (k, &(r, c)) in self.hills.iter().enumerate() {
            if water.get(g.at(r, c) as usize) {
                return Err(err("MAP_UNPLAYABLE", format!("seat {k}'s hill stands on water")));
            }
            if clear.iter().all(|&(dr, dc)| water.get(g.at(r + dr, c + dc) as usize)) {
                return Err(err("MAP_UNPLAYABLE", format!("seat {k}'s hill is walled in")));
            }
        }

        // Food is whole orbits, on land, named once each.
        let mut have = self.cells_of(&self.food);
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
                if have.binary_search(&sym.image(&g, pos, k)).is_err() {
                    return Err(err(
                        "MAP_NOT_SYMMETRIC",
                        format!("food at [{r}, {c}] has no counterpart for seat {k}"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The match this board opens at, for one seed.
    ///
    /// Everything derivable is derived rather than stored — one ant per hill, and each seat's
    /// `known` folded in from what it can see. A map file that carried them could disagree with the
    /// rules, and there would be no way to tell which was right.
    pub fn build(&self, seed: u64, max_turns: u16) -> Result<Match, MapError> {
        let mut m = Match::new(seed, self.geom(), self.players, max_turns, self.bits()?);
        m.food = self.cells_of(&self.food);
        m.food0 = m.food.clone();
        m.map_id = self.id.clone();
        m.open_on(&self.cells_of(&self.hills));
        Ok(m)
    }

    /// Read the board back off a match, at any turn.
    ///
    /// This is how `mapgen` turns the procedural generator into a file, and how `finish` hands the
    /// board to the replay envelope. It takes the food from `food0`, because by then the board's
    /// food has been eaten and respawned many times over and the current set is the position rather
    /// than the board.
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
            food_target: m.food0.len() as u16,
        }
    }
}

// ---------------------------------------------------------------- the catalogue

/// Every committed board, compiled in — `maps_gen.rs` on why it cannot be read from a file.
///
/// Parsed on demand rather than once: an invocation runs in a fresh instance and nothing is cached
/// across calls, so a lazy table would buy nothing and would be the kind of state the sandbox
/// exists to make impossible.
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
/// The seed chooses, not the caller: a competitor who could name the board could train against it,
/// and a seed is assigned by pairing.
pub fn for_seed(preset: &str, seed: u64) -> Option<MapFile> {
    let p = pool(preset);
    if p.is_empty() {
        return None;
    }
    Some(p[(seed % p.len() as u64) as usize].clone())
}

/// Resolve what a caller passed for one match: a map id, a whole map object, or nothing.
///
/// Nothing is the platform's case — Kalam passes seeds and a preset and lets the seed choose. The
/// other two are the local case, and the reason a match file can pin a board.
pub fn resolve(spec: Option<&Value>, preset: &str, seed: u64) -> Result<MapFile, MapError> {
    match spec {
        None | Some(Value::Null) => for_seed(preset, seed).ok_or_else(|| {
            err("NO_SUCH_MAP", format!("no map in the catalogue is played at preset '{preset}'"))
        }),
        Some(Value::String(id)) => {
            by_id(id).ok_or_else(|| err("NO_SUCH_MAP", format!("no map '{id}' in the catalogue")))
        }
        Some(v @ Value::Object(_)) => MapFile::from_json(v),
        Some(_) => Err(err("MAP_BAD_SHAPE", "a map must be an id, an object, or absent")),
    }
}
