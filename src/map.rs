//! The grid: wrapping, distances, presets and the symmetry a world is built on.
//!
//! Integer arithmetic throughout — distances are squared so no square root is ever taken, and the
//! map wraps in both directions so nothing can be defended by putting its back to a wall.

/// The contest engine's own settings (`ants.py:1799`). The specification's prose says the view
/// radius is 55; the engine says 77, and the engine is what every bot was scored against.
pub const VIEW_RADIUS2: i32 = 77;
pub const ATTACK_RADIUS2: i32 = 5;
pub const SPAWN_RADIUS2: i32 = 1;

/// splitmix64: seeded, portable and integer-only, because `docs/cartridge.md` §4 forbids floating
/// point in game logic.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, n)`. The biased tail is rejected rather than folded: modulo bias would be
    /// invisible in one match and visible in a thousand.
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

    #[inline]
    pub fn clear(&mut self, i: usize) {
        if i < self.len {
            self.bits[i >> 3] &= !(1 << (i & 7));
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn count(&self) -> usize {
        (0..self.len).filter(|&i| self.get(i)).count()
    }

    /// The protocol's `water` field: `[value, run, value, run, ...]`, row-major. The run lengths
    /// must sum to `rows * cols`, which conformance checks.
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
    /// Seats are a property of the map, not of the game (decision 14), so there is no game-level
    /// constant and a four-seat map is content rather than code.
    pub players: u8,
    /// Roughly what fraction of the map is water, in percent, before symmetry.
    pub water_pct: u32,
    /// How much food the board is stocked with at turn zero, per player.
    pub food_per_player: u32,
    /// The blob size water is grown in. A maze wants long thin walls; a cave wants fat ones.
    pub blob: u32,
    /// Food placed within reach of each hill at turn zero.
    ///
    /// Without it a colony cannot bootstrap: a player starts with one ant, an ant collects only
    /// from an adjacent square, and a lone ant that has to *find* its first food usually will not.
    /// Measured before it existed: random play finished with an average of two ants and ended as a
    /// food stalemate twenty times in twenty-four.
    pub hill_food: u32,
}

#[rustfmt::skip]
pub const PRESETS: [Preset; 3] = [
    Preset { name: "standard", rows: 64,  cols: 96,  players: 2, water_pct: 12, food_per_player: 12, blob: 4, hill_food: 5 },
    Preset { name: "maze",     rows: 96,  cols: 96,  players: 2, water_pct: 28, food_per_player: 10, blob: 2, hill_food: 5 },
    Preset { name: "cell",     rows: 128, cols: 128, players: 2, water_pct: 18, food_per_player: 16, blob: 7, hill_food: 5 },
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

    /// Wrapping, in both directions.
    #[inline]
    pub fn at(&self, r: i32, c: i32) -> u16 {
        (r.rem_euclid(self.rows) * self.cols + c.rem_euclid(self.cols)) as u16
    }

    /// Squared distance, taking the shorter way around the wrap.
    #[inline]
    pub fn dist2(&self, a: u16, b: u16) -> i32 {
        let (ar, ac) = self.rc(a);
        let (br, bc) = self.rc(b);
        let dr = (ar - br).abs().min(self.rows - (ar - br).abs());
        let dc = (ac - bc).abs().min(self.cols - (ac - bc).abs());
        dr * dr + dc * dc
    }

    /// Every offset within a squared radius. The disk is the hot shape in this game: vision,
    /// combat and gathering are all "everything within r²".
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

/// The four moves, in the order the protocol names them: N, E, S, W.
pub const DIRS: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
pub const DIR_NAMES: [&str; 4] = ["N", "E", "S", "W"];

pub fn dir_of(a: &str) -> Option<(i32, i32)> {
    DIR_NAMES.iter().position(|&d| d == a).map(|i| DIRS[i])
}

/// The symmetry a map is built with.
///
/// The whole world is built on one fundamental domain and translated: for two players, by half the
/// rows and half the columns, which on a wrapping torus is a fixed-point-free symmetry of order
/// two. Every player's surroundings are congruent to every other's, so a pairing can never be
/// unfair because of the map.
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
