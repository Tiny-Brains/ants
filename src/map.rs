//! The grid, its wrapping, its distances, and how a world is made.
//!
//! The specification's *Map Format* and *Distance*. Everything here is integer arithmetic:
//! distances are squared so no
//! square root is ever needed (*Distance*), and the map wraps in both directions so there are no
//! edges and nothing can be defended by putting its back to a wall (*Map Format*).

/// The contest's own settings (`ants.py:1799`). The specification's prose says the view radius is
/// 55; the engine says 77, and the engine is what every bot was scored against.
pub const VIEW_RADIUS2: i32 = 77;
pub const ATTACK_RADIUS2: i32 = 5;
pub const SPAWN_RADIUS2: i32 = 1;

/// splitmix64. Seeded, portable, integer-only — `cartridge.md` §4 forbids floating point in game
/// logic, and a generator that used it would make two builds disagree.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[0, n)`, without modulo bias: the biased tail is rejected rather than folded.
    /// Bias would be invisible in a match and visible in a thousand of them.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let limit = u64::MAX - (u64::MAX % n as u64);
        loop {
            let v = self.next();
            if v < limit {
                return (v % n as u64) as u32;
            }
        }
    }
}

/// A fixed-length bitmap over the grid: one bit per cell, row-major, LSB first.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bits {
    pub bits: Vec<u8>,
    pub len: usize,
}

impl Bits {
    pub fn zeros(len: usize) -> Bits {
        Bits { bits: vec![0u8; len.div_ceil(8)], len }
    }
    #[inline]
    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.bits[i >> 3] & (1 << (i & 7)) != 0
    }
    #[inline]
    pub fn set(&mut self, i: usize) {
        if i < self.len {
            self.bits[i >> 3] |= 1 << (i & 7);
        }
    }
    /// How many cells are set. Used by the vision tests, which are the only place the size of a
    /// mask is the thing under test.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn count(&self) -> usize {
        (0..self.len).filter(|&i| self.get(i)).count()
    }
    /// The protocol's `water` field: `[value, run, value, run, ...]`, row-major.
    /// `PROTOCOL.md` §6: the run lengths must sum to rows x cols, which conformance checks.
    pub fn rle(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if self.len == 0 {
            return out;
        }
        let mut cur = self.get(0) as u32;
        let mut run = 0u32;
        for i in 0..self.len {
            let v = self.get(i) as u32;
            if v == cur {
                run += 1;
            } else {
                out.push(cur);
                out.push(run);
                cur = v;
                run = 1;
            }
        }
        out.push(cur);
        out.push(run);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub rows: u8,
    pub cols: u8,
    /// **The preset carries its own player count** — decision 14, 7 September 2026. Seats are a
    /// property of the map, not of the game, so there is no game-level constant and a 4-seat map
    /// is content rather than code.
    pub players: u8,
    /// Roughly what fraction of the map is water, in percent, before symmetry.
    pub water_pct: u32,
    /// How much food is kept on the map, per player.
    pub food_per_player: u32,
    /// The blob size water is grown in. A maze wants long thin walls; a cave wants fat ones.
    pub blob: u32,
    /// Food placed within reach of each hill at turn zero.
    ///
    /// Without it a colony cannot bootstrap: a player starts with one ant (*Map Format*), an ant
    /// collects only from an adjacent square (*Food Harvesting*), and a lone ant that has to *find*
    /// its first food before it can grow will usually not. Measured before it existed: random play
    /// finished with an average of two ants and ended as a food stalemate twenty times in
    /// twenty-four. Real Ants seeds the hills the same way and for the same reason.
    pub hill_food: u32,
}

pub const PRESETS: [Preset; 3] = [
    Preset { name: "standard", rows: 64, cols: 96, players: 2, water_pct: 12, food_per_player: 12, blob: 4, hill_food: 5 },
    Preset { name: "maze", rows: 96, cols: 96, players: 2, water_pct: 28, food_per_player: 10, blob: 2, hill_food: 5 },
    Preset { name: "cell", rows: 128, cols: 128, players: 2, water_pct: 18, food_per_player: 16, blob: 7, hill_food: 5 },
];

pub fn preset(name: &str) -> Option<Preset> {
    PRESETS.iter().find(|p| p.name == name).copied()
}

/// The board's geometry. Positions are `row * cols + col` and fit in a `u16`, which caps a map at
/// 256x256 — four times the largest preset.
#[derive(Clone, Copy)]
pub struct Geom {
    pub rows: i32,
    pub cols: i32,
}

impl Geom {
    pub fn new(rows: u8, cols: u8) -> Geom {
        Geom { rows: rows as i32, cols: cols as i32 }
    }
    pub fn cells(&self) -> usize {
        (self.rows * self.cols) as usize
    }
    #[inline]
    pub fn rc(&self, pos: u16) -> (i32, i32) {
        (pos as i32 / self.cols, pos as i32 % self.cols)
    }
    /// Wrapping, in both directions — *Map Format*.
    #[inline]
    pub fn at(&self, r: i32, c: i32) -> u16 {
        (r.rem_euclid(self.rows) * self.cols + c.rem_euclid(self.cols)) as u16
    }
    /// Squared distance, taking the shorter way around the wrap — *Distance*.
    #[inline]
    pub fn dist2(&self, a: u16, b: u16) -> i32 {
        let (ar, ac) = self.rc(a);
        let (br, bc) = self.rc(b);
        let dr = (ar - br).abs().min(self.rows - (ar - br).abs());
        let dc = (ac - bc).abs().min(self.cols - (ac - bc).abs());
        dr * dr + dc * dc
    }
    /// Every offset within a squared radius, computed once and reused. The disk is the hot shape
    /// in this game: vision, combat and gathering are all "everything within r²".
    pub fn disk(&self, r2: i32) -> Vec<(i32, i32)> {
        let reach = isqrt(r2);
        let mut out = Vec::new();
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                if dr * dr + dc * dc <= r2 {
                    out.push((dr, dc));
                }
            }
        }
        out
    }
}

/// Integer square root. A loop bound derived from a float square root is exactly the kind of thing
/// that makes two builds disagree.
pub const fn isqrt(n: i32) -> i32 {
    let mut r = 0;
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}

/// The four moves, in the order the protocol names them, plus the hold.
pub const DIRS: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)]; // N E S W

pub fn dir_of(a: &str) -> Option<(i32, i32)> {
    match a {
        "N" => Some(DIRS[0]),
        "E" => Some(DIRS[1]),
        "S" => Some(DIRS[2]),
        "W" => Some(DIRS[3]),
        _ => None,
    }
}

/// The symmetry a map is built with.
///
/// The specification's *Food spawning* requires food to be placed symmetrically so every player is
/// offered the same opportunities in the same shape — and a map whose *terrain* were not symmetric
/// would make that meaningless. So the whole world is built on one fundamental domain and
/// translated: for two players, by half the rows and half the columns, which on a wrapping torus is
/// a fixed-point-free symmetry of order two. Every player's surroundings are congruent to every
/// other's, and a pairing can never be unfair because of the map.
#[derive(Clone, Copy)]
pub struct Symmetry {
    pub players: i32,
    pub dr: i32,
    pub dc: i32,
}

impl Symmetry {
    pub fn for_preset(g: &Geom, players: u8) -> Symmetry {
        let p = players as i32;
        Symmetry { players: p, dr: g.rows / p, dc: g.cols / p }
    }
    /// The `k`-th image of a position: player 0's square, as player `k` sees the same square.
    pub fn image(&self, g: &Geom, pos: u16, k: i32) -> u16 {
        let (r, c) = g.rc(pos);
        g.at(r + self.dr * k, c + self.dc * k)
    }
    /// Every image of a position, one per player, in player order.
    pub fn orbit(&self, g: &Geom, pos: u16) -> Vec<u16> {
        (0..self.players).map(|k| self.image(g, pos, k)).collect()
    }
}
