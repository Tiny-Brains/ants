//! The boards: the map file, its validation, the catalogue that ships, and the presets that pool it.
//!
//! A map fixes the **board** — its size, its shift, its water, its hills, and the food on it at
//! turn zero. It does not fix the match: the hidden food rate and every respawn after turn zero are
//! drawn from the seed, so a board played at two seeds is two matches. Both are carried in the
//! replay.
//!
//! The generator (`mapgen/`, beside this crate) writes every orbit whole, so its boards are
//! symmetric by construction. A file cannot make that promise — someone will hand-author one — so
//! the guarantee lives here, in `validate`, which every map passes through before it is played. Its
//! refusals are `caller_input`: the same map can never succeed, so nothing retries it.

use serde_json::{Value, json};

use crate::grid::{Bits, DIRS, Geom, Symmetry};
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
    /// The shift seat `k`'s board is moved by, `k` times: `(dr, dc)`. A file that names none gets
    /// `Symmetry::diagonal`, which is what every board was before the shift was the board's.
    pub symmetry: (i32, i32),
    pub water: Vec<u32>,
    /// In orbits: seat 0's hill, then its image for each other seat, then the next orbit. So hill
    /// `i` is seat `i % players`'s, and a board with two hills a seat lists `2 · players`.
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
        Some(x) => {
            x.as_array().ok_or_else(|| err("MAP_BAD_SHAPE", format!("'{k}' must be an array")))?
        }
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
        let (rows, cols, players) =
            (u8_field(v, "rows")?, u8_field(v, "cols")?, u8_field(v, "players")?);
        let symmetry = match v.get("symmetry") {
            None | Some(Value::Null) => {
                let s = Symmetry::diagonal(&Geom::new(rows, cols), players);
                (s.dr, s.dc)
            }
            Some(s) => {
                let axis = |k: &str| {
                    s.get(k).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).ok_or_else(
                        || err("MAP_BAD_SHAPE", format!("'symmetry' has no numeric '{k}'")),
                    )
                };
                (axis("dr")?, axis("dc")?)
            }
        };
        let m = MapFile {
            id: v.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            preset: v.get("preset").and_then(Value::as_str).unwrap_or("").to_string(),
            rows,
            cols,
            players,
            symmetry,
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
        let (dr, dc) = self.symmetry;
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
            "symmetry": { "dr": dr, "dc": dc },
        })
    }

    pub fn geom(&self) -> Geom {
        Geom::new(self.rows, self.cols)
    }

    pub fn sym(&self) -> Symmetry {
        Symmetry::new(self.players, self.symmetry.0, self.symmetry.1)
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
                format!(
                    "'water' covers {i} cells; a {}x{} board has {cells}",
                    self.rows, self.cols
                ),
            ));
        }
        Ok(b)
    }

    /// Everything a board must be before it is played, asserted.
    ///
    /// These are the rules no tuning may relax, because breaking any of them makes a match unfair or
    /// unplayable rather than merely different: the board is congruent for every seat, every seat's
    /// hills are whole orbits on land with a way off them, and every square of land can be walked to
    /// from every other. The last is what keeps food honest — `food::sets` draws from every land
    /// square, and food spawned in a sealed pocket can never be gathered while it still counts as
    /// loose food for the idle-food ending. The generator holds itself to more (hill spacing, vision,
    /// door widths); those are design, and they live with it.
    pub fn validate(&self) -> Result<(), MapError> {
        let g = self.geom();
        let (rows, cols, players) = (g.rows, g.cols, self.players as i32);
        let sym = self.sym();

        // A shift of any other order does not partition the board into orbits of one size, so no
        // assignment of water to it can be symmetric.
        if !sym.is_exact(&g) {
            let (dr, dc) = self.symmetry;
            return Err(err(
                "MAP_BAD_SHAPE",
                format!(
                    "a shift of ({dr}, {dc}) on a {rows}x{cols} board does not come back to the start \
                     after exactly {players} steps, so it cannot seat {players} players"
                ),
            ));
        }

        let water = self.bits()?;
        for i in 0..g.cells() {
            let img = sym.image(&g, i as u16, 1) as usize;
            if water.get(i) != water.get(img) {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!("water differs between cell {i} and its image {img}"),
                ));
            }
        }

        // Hills, in whole orbits: `open_on` hands hill `i` to seat `i % players`.
        let n = self.hills.len();
        if n == 0 || !n.is_multiple_of(self.players as usize) || n > u8::MAX as usize {
            return Err(err(
                "MAP_UNPLAYABLE",
                format!(
                    "{n} hills for {players} seats; each seat needs the same number, at least one"
                ),
            ));
        }
        let hills = self.cells_of(&self.hills);
        for (i, &pos) in hills.iter().enumerate() {
            let (orbit, k) = (i / self.players as usize, (i % self.players as usize) as i32);
            let first = hills[orbit * self.players as usize];
            if pos != sym.image(&g, first, k) {
                return Err(err(
                    "MAP_NOT_SYMMETRIC",
                    format!(
                        "hill {i} (seat {k}) is not the {k}th image of hill {}",
                        i - k as usize
                    ),
                ));
            }
        }
        let mut distinct = hills.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != hills.len() {
            return Err(err("MAP_BAD_SHAPE", "'hills' names the same square twice"));
        }

        // Nobody starts in a lake, or walled into one. A hill's ant leaves it by one of the four
        // moves, so a hill with water on all four sides spawns ants that can never go anywhere.
        for (i, &(r, c)) in self.hills.iter().enumerate() {
            let seat = i % self.players as usize;
            if water.get(g.at(r, c) as usize) {
                return Err(err(
                    "MAP_UNPLAYABLE",
                    format!("seat {seat}'s hill {i} stands on water"),
                ));
            }
            if DIRS.iter().all(|&(dr, dc)| water.get(g.at(r + dr, c + dc) as usize)) {
                return Err(err("MAP_UNPLAYABLE", format!("seat {seat}'s hill {i} is walled in")));
            }
        }

        // One body of land. Ants move in four directions, so two squares touching only at a corner
        // are not connected, and the wrap is.
        if let Some((r, c)) = unreachable_land(&g, &water, hills[0]) {
            return Err(err(
                "MAP_UNPLAYABLE",
                format!("land at [{r}, {c}] cannot be walked to from seat 0's hill"),
            ));
        }

        // Food is whole orbits, on open land, named once each.
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
            if distinct.binary_search(&pos).is_ok() {
                return Err(err("MAP_UNPLAYABLE", format!("food at [{r}, {c}] is on a hill")));
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
        let mut m = Match::new(seed, self.geom(), self.sym(), max_turns, self.bits()?);
        m.food = self.cells_of(&self.food);
        m.food0 = m.food.clone();
        m.map_id = self.id.clone();
        m.open_on(&self.cells_of(&self.hills));
        Ok(m)
    }

    /// Read the board back off a match, at any turn.
    ///
    /// This is how `finish` hands the board to the replay envelope. It takes the food from `food0`,
    /// because by then the board's food has been eaten and respawned many times over and the
    /// current set is the position rather than the board.
    pub fn from_match(m: &Match, id: &str, preset: &str) -> MapFile {
        let rc = |p: u16| m.g.rc(p);
        MapFile {
            id: id.to_string(),
            preset: preset.to_string(),
            rows: m.g.rows as u8,
            cols: m.g.cols as u8,
            players: m.players,
            symmetry: (m.sym.dr, m.sym.dc),
            water: m.water.rle(),
            hills: m.hills.iter().map(|h| rc(h.pos)).collect(),
            food: m.food0.iter().map(|&f| rc(f)).collect(),
            food_target: m.food0.len() as u16,
        }
    }
}

/// The first square of land that cannot be walked to from `from`, if there is one.
fn unreachable_land(g: &Geom, water: &Bits, from: u16) -> Option<(i32, i32)> {
    let mut seen = Bits::zeros(g.cells());
    let mut queue = vec![from];
    seen.set(from as usize);
    while let Some(pos) = queue.pop() {
        let (r, c) = g.rc(pos);
        for &(dr, dc) in &DIRS {
            let next = g.at(r + dr, c + dc);
            if !water.get(next as usize) && !seen.get(next as usize) {
                seen.set(next as usize);
                queue.push(next);
            }
        }
    }
    (0..g.cells()).find(|&i| !water.get(i) && !seen.get(i)).map(|i| g.rc(i as u16))
}

// ---------------------------------------------------------------- the presets

/// A preset names a pool of boards and how many seats play them.
///
/// **Derived from the catalogue, never authored.** A preset exists because boards declare it, so a
/// preset played on no board, or a board naming a preset nobody lists, cannot be written down.
/// Seats are a property of the map, not of the game (decision 14), and pairing reads one seat count
/// per preset: every board in a pool must agree, which `tools/package.py` and the tests check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub players: u8,
}

/// Every preset, in the order its first board appears in the catalogue.
pub fn presets() -> Vec<Preset> {
    let mut out: Vec<Preset> = Vec::new();
    for m in catalogue() {
        if !out.iter().any(|p| p.name == m.preset) {
            out.push(Preset { name: m.preset, players: m.players });
        }
    }
    out
}

// ---------------------------------------------------------------- the catalogue

/// Every committed board, as `(id, file)`, compiled in by `build.rs`: a cartridge imports nothing,
/// so a board cannot be read from a file at run time.
pub(crate) const MAPS: &[(&str, &str)] = &include!(concat!(env!("OUT_DIR"), "/maps.rs"));

/// Every committed board, parsed.
///
/// Parsed on demand rather than once: an invocation runs in a fresh instance and nothing is cached
/// across calls, so a lazy table would buy nothing and would be the kind of state the sandbox
/// exists to make impossible.
pub fn catalogue() -> Vec<MapFile> {
    MAPS.iter()
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
/// other two are the local case, and the reason a match file can pin a board. **Only the first
/// needs the preset to be a pool**: a board named outright is played whatever label the call
/// carries, which is how a lesson board or a freshly generated family is played before any
/// catalogue lists it.
pub fn resolve(spec: Option<&Value>, preset: &str, seed: u64) -> Result<MapFile, MapError> {
    match spec {
        None | Some(Value::Null) => for_seed(preset, seed).ok_or_else(|| {
            err(
                "NO_SUCH_PRESET",
                format!("no preset '{preset}': no board in the catalogue is played at it"),
            )
        }),
        Some(Value::String(id)) => {
            by_id(id).ok_or_else(|| err("NO_SUCH_MAP", format!("no map '{id}' in the catalogue")))
        }
        Some(v @ Value::Object(_)) => MapFile::from_json(v),
        Some(_) => Err(err("MAP_BAD_SHAPE", "a map must be an id, an object, or absent")),
    }
}
