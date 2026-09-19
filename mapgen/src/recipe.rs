//! An area recipe: one set of boards, described by the knobs a person tunes rather than by the
//! algorithm that honours them.
//!
//! **Every terrain is the same few area knobs.** The board is cut into areas; `coverage_pct` says how
//! many are carved out of solid water, `wall` and `closure_pct` how enclosed each carved one is,
//! `loops_pct` how many routes join them, and `fill` what clutters their insides. An open field is
//! full coverage with no walls; a tight maze is small areas, closed, joined by a tree; a cave is
//! warped areas, partly covered, cellular inside. A new style is a new recipe, not new code.
//!
//! **What is not here cannot be tuned.** The rules every board obeys whatever its recipe — whole
//! orbits, one body of land, a hill with a way off it, no enemy hill in view at turn zero, a clearing
//! of at least two squares — are checked in `check()` and `measure`, and refuse the recipe or the
//! board rather than bending.

use serde::Deserialize;

use crate::grid::{Torus, isqrt, shifts};

/// How many seats a board may seat. The engine's orbit buffer holds sixteen; the platform's runners
/// hold four today, and eight is what the design commits to.
pub const MAX_SEATS: u8 = 8;

/// The smallest clearing a hill may have: an ant spawned on a hill must be able to step off it and
/// out of the way of the next one, whichever way the terrain lies.
pub const MIN_CLEARING: u32 = 2;

/// The engine's vision radius², which no enemy hill may lie within at turn zero: a board that shows
/// you your opponent's home before you have moved has already decided who scouts.
pub const VIEW_RADIUS2: i64 = tb_ants::VIEW_RADIUS2 as i64;

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub set: SetSpec,
    pub board: BoardSpec,
    pub areas: AreaSpec,
    #[serde(default)]
    pub connectivity: ConnectivitySpec,
    #[serde(default)]
    pub fill: FillSpec,
    #[serde(default)]
    pub hills: HillSpec,
    pub food: FoodSpec,
    #[serde(default)]
    pub accept: AcceptSpec,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    /// The set's name, and the prefix of every board's id.
    pub name: String,
    pub seats: u8,
    /// How many boards the set draws.
    pub count: u32,
    /// The set's seed: board `i` is drawn from `mix(seed, i + 1)`.
    pub seed: u64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BoardSpec {
    pub rows: u8,
    pub cols: u8,
    /// The shifts the set cycles through, board by board: `diagonal`, `rows`, `cols` or `any`.
    pub shifts: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AreaSpec {
    /// Cells per area, `[min, max]`. Against the vision disk (241 cells), it is how much of the
    /// board one ant standing in an area can see of it.
    pub size: [u32; 2],
    /// Seeds on a square lattice, one every `isqrt(size)` squares, instead of scattered: square
    /// areas, which closed and joined by a tree are a corridor maze. The side must divide the board
    /// and every shift.
    #[serde(default)]
    pub grid: bool,
    /// How far area boundaries are pushed around, in cells. 0 is straight-edged; more is organic.
    #[serde(default)]
    pub warp: u32,
    /// The share of areas that grow to about four times the size of the rest.
    #[serde(default)]
    pub arenas_pct: u32,
    /// The share of the board carved into areas; the rest stays solid water.
    #[serde(default = "hundred")]
    pub coverage_pct: u32,
    /// The walled share of each carved area's boundary with a neighbour it has a door to. A boundary
    /// with no door is always walled whole, or "one route" would leak.
    #[serde(default)]
    pub closure_pct: u32,
    /// Wall thickness between carved areas, in cells. 0 means no walls at all, and then every area
    /// is open to every neighbour. Vision and attacks reach through water: a wall under two cells
    /// thick stops ants walking, not ants fighting.
    #[serde(default)]
    pub wall: u32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ConnectivitySpec {
    /// Doors beyond the fewest that join every area, as a share of the boundaries that could take
    /// one. 0 is a tree inside each seat's share; 100 is every neighbour joined.
    #[serde(default = "hundred")]
    pub loops_pct: u32,
    /// The narrowest a door may be, in cells.
    #[serde(default = "one")]
    pub door_min: u32,
}

impl Default for ConnectivitySpec {
    fn default() -> Self {
        ConnectivitySpec { loops_pct: 100, door_min: 1 }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FillStyle {
    #[default]
    None,
    /// Rectangles of water dropped into areas, each kept only if the land stays one body.
    Scatter,
    /// Noise smoothed by a cellular automaton, then reconnected: caverns rather than rocks.
    Cellular,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FillSpec {
    #[serde(default)]
    pub style: FillStyle,
    /// Scatter: the share of open land to fill. Cellular: the starting density of the noise.
    #[serde(default)]
    pub pct: u32,
    /// Scatter: a rectangle's side, `[min, max]`.
    #[serde(default = "blob")]
    pub blob: [u32; 2],
    /// Cellular: smoothing passes.
    #[serde(default = "four")]
    pub smooth: u32,
}

impl Default for FillSpec {
    fn default() -> Self {
        FillSpec { style: FillStyle::None, pct: 0, blob: blob(), smooth: four() }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct HillSpec {
    #[serde(default = "one")]
    pub per_seat: u32,
    /// The square around a hill, radius in cells, that is kept open.
    #[serde(default = "two")]
    pub clearing: u32,
    /// Give each home area a square room around its hill, whatever size its area came out. What a
    /// maze of small areas needs to have anywhere to put a hill.
    #[serde(default)]
    pub room: bool,
    /// Closure for the doors of a home area, when it should differ from the rest.
    #[serde(default)]
    pub home_closure_pct: Option<u32>,
    /// The fewest doors a home area has.
    #[serde(default = "one")]
    pub home_doors_min: u32,
}

impl Default for HillSpec {
    fn default() -> Self {
        HillSpec {
            per_seat: 1,
            clearing: 2,
            room: false,
            home_closure_pct: None,
            home_doors_min: 1,
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Bias {
    #[default]
    Uniform,
    /// Where a seat's walk and its nearest opponent's are within `band` of each other.
    Contested,
    /// Nearer home than any opponent's by more than `band`.
    Home,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FoodSpec {
    /// Food each seat has on the board at turn zero, bootstrap included.
    pub per_seat: u32,
    /// Food within a short walk of each hill, so a lone first ant can grow a colony.
    #[serde(default)]
    pub bootstrap: u32,
    /// How far that walk is, `[min, max]`.
    #[serde(default = "walk")]
    pub bootstrap_walk: [u32; 2],
    #[serde(default)]
    pub bias: Bias,
    #[serde(default = "eight")]
    pub band: u32,
}

/// Windows a finished board must fall inside, or the attempt is drawn again.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AcceptSpec {
    pub water_pct: Option<[u32; 2]>,
    /// Square-disjoint routes between a seat's home and its nearest opponent's.
    pub routes: Option<[u32; 2]>,
    pub enemy_walk: Option<[u32; 2]>,
    /// The walk to the nearest enemy hill against the same walk on an empty board, in percent.
    pub detour_pct: Option<[u32; 2]>,
    /// Land squares with one way in or none, per thousand.
    pub dead_end_pm: Option<[u32; 2]>,
    /// Run-length pairs in the whole water mask: a ceiling on what an observation can cost.
    pub water_runs_max: Option<u32>,
    #[serde(default = "attempts")]
    pub attempts: u32,
}

impl Default for AcceptSpec {
    fn default() -> Self {
        AcceptSpec {
            water_pct: None,
            routes: None,
            enemy_walk: None,
            detour_pct: None,
            dead_end_pm: None,
            water_runs_max: None,
            attempts: attempts(),
        }
    }
}

fn hundred() -> u32 {
    100
}
fn one() -> u32 {
    1
}
fn two() -> u32 {
    2
}
fn four() -> u32 {
    4
}
fn eight() -> u32 {
    8
}
fn blob() -> [u32; 2] {
    [1, 3]
}
fn walk() -> [u32; 2] {
    [2, 8]
}
fn attempts() -> u32 {
    200
}

impl Recipe {
    pub fn parse(text: &str) -> Result<Recipe, String> {
        let r: Recipe = toml::from_str(text).map_err(|e| e.to_string())?;
        r.check()?;
        Ok(r)
    }

    pub fn torus(&self) -> Torus {
        Torus { rows: self.board.rows as i32, cols: self.board.cols as i32 }
    }

    pub fn seats(&self) -> usize {
        self.set.seats as usize
    }

    /// Everything that makes a recipe impossible rather than unlucky, refused before a board is drawn.
    pub fn check(&self) -> Result<(), String> {
        let bad = |m: String| Err(m);
        let p = &self.set;
        if p.name.is_empty()
            || !p.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return bad(format!("set '{}': a name is lowercase letters, digits and '-'", p.name));
        }
        if !(2..=MAX_SEATS).contains(&p.seats) {
            return bad(format!("{} seats: a board seats 2 to {MAX_SEATS}", p.seats));
        }
        if !(1..=100).contains(&p.count) {
            return bad(format!("count {}: a set has 1 to 100 boards", p.count));
        }
        let t = self.torus();
        if t.rows < 8 || t.cols < 8 {
            return bad(format!("a {}x{} board is too small to hold a hill", t.rows, t.cols));
        }

        // No enemy hill in view at turn zero. With one hill a seat the offsets between seats are the
        // shift's multiples wherever the hill lands, so this is a property of the shift alone.
        let shifts = shifts(&self.board.shifts, &t, self.seats())?;
        for s in &shifts {
            let nearest = (1..s.seats).map(|k| t.dist2(0, s.image(&t, 0, k))).min().unwrap_or(0);
            if nearest <= VIEW_RADIUS2 {
                return bad(format!(
                    "shift ({}, {}) puts an enemy hill {} squares² away, inside the view radius² \
                     of {VIEW_RADIUS2}",
                    s.dr, s.dc, nearest
                ));
            }
        }

        let a = &self.areas;
        if a.size[0] < 4 || a.size[0] > a.size[1] {
            return bad(format!("areas.size {:?}: [min, max] with min at least 4", a.size));
        }
        if a.grid {
            let side = isqrt(((a.size[0] + a.size[1]) / 2) as i64) as i32;
            let fits = |n: i32| n % side == 0;
            if !fits(t.rows) || !fits(t.cols) || !shifts.iter().all(|s| fits(s.dr) && fits(s.dc)) {
                return bad(format!(
                    "areas.grid: a lattice of side {side} must divide the {}x{} board and every shift",
                    t.rows, t.cols
                ));
            }
        }
        let share = t.cells() as u32 / self.seats() as u32;
        if share / a.size[1] < 2 {
            return bad(format!(
                "areas of up to {} cells leave fewer than two in a seat's {share}; nothing to connect",
                a.size[1]
            ));
        }
        for (what, v) in [
            ("areas.arenas_pct", a.arenas_pct),
            ("areas.closure_pct", a.closure_pct),
            ("connectivity.loops_pct", self.connectivity.loops_pct),
        ] {
            if v > 100 {
                return bad(format!("{what} is {v}; a share is 0 to 100"));
            }
        }
        if !(1..=100).contains(&a.coverage_pct) {
            return bad(format!("areas.coverage_pct is {}; 1 to 100", a.coverage_pct));
        }
        if a.wall > 4 {
            return bad(format!("areas.wall is {}; 0 to 4", a.wall));
        }
        if a.wall == 0 && self.connectivity.loops_pct != 100 {
            return bad("areas.wall = 0 leaves nothing to close a boundary with, so connectivity \
                        .loops_pct must be 100"
                .into());
        }
        if self.connectivity.door_min == 0 {
            return bad("connectivity.door_min must be at least 1".into());
        }

        let h = &self.hills;
        if h.clearing < MIN_CLEARING {
            return bad(format!("hills.clearing is {}; at least {MIN_CLEARING}", h.clearing));
        }
        if !(1..=4).contains(&h.per_seat) {
            return bad(format!("hills.per_seat is {}; 1 to 4", h.per_seat));
        }
        if h.home_closure_pct.is_some_and(|v| v > 100) {
            return bad("hills.home_closure_pct is a share, 0 to 100".into());
        }
        if h.room {
            let room = (h.clearing + a.wall.div_ceil(2)) as i64;
            if isqrt(VIEW_RADIUS2) < room {
                return bad(format!("hills.room of radius {room} is wider than a seat can see"));
            }
        }

        let f = &self.food;
        if f.bootstrap * h.per_seat > f.per_seat {
            return bad(format!(
                "food.bootstrap {} for each of {} hills is more than food.per_seat {}",
                f.bootstrap, h.per_seat, f.per_seat
            ));
        }
        if f.bootstrap_walk[0] > f.bootstrap_walk[1] || f.bootstrap_walk[0] == 0 {
            return bad(format!(
                "food.bootstrap_walk {:?}: [min, max], min at least 1",
                f.bootstrap_walk
            ));
        }

        let fill = &self.fill;
        if fill.pct > 60 {
            return bad(format!("fill.pct is {}; above 60 there is no board left", fill.pct));
        }
        if fill.blob[0] == 0 || fill.blob[0] > fill.blob[1] {
            return bad(format!("fill.blob {:?}: [min, max], min at least 1", fill.blob));
        }
        if self.accept.attempts == 0 {
            return bad("accept.attempts must be at least 1".into());
        }
        Ok(())
    }
}
