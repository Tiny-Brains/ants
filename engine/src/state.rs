//! One match while it is played: the board, the colonies, and the counters the rules read.

use crate::grid::{Bits, Geom, Symmetry, VIEW_RADIUS2, disk_rows};

/// A hill. Razed hills are kept: a razed hill is a permanent fact of the match, and the score
/// already charged for it must not be charged twice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hill {
    pub pos: u16,
    pub owner: u8,
    pub razed: bool,
    /// The turn this hill was last **touched** — spawned from, or stood on by an ant of its own
    /// owner (`ants.py:812`). When several of a player's hills are free the least recently touched
    /// spawns first, which is what makes "park an ant on a hill to steer where the next one
    /// appears" work.
    pub last_touched: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ant {
    pub pos: u16,
    pub owner: u8,
}

#[derive(Clone)]
pub struct Match {
    pub seed: u64,
    pub turn: u16,
    pub max_turns: u16,
    pub players: u8,
    pub g: Geom,
    pub sym: Symmetry,
    pub done: bool,
    /// Why it ended, as an index into `END_REASONS`. Kept in the state so `finish` is a read.
    pub reason: u8,

    pub water: Bits,
    /// What each player has ever seen. The expensive field, and deliberate — `observe.rs` says why
    /// `water` cannot be derived without it.
    pub known: Vec<Bits>,

    pub ants: Vec<Ant>,
    pub food: Vec<u16>,
    pub hills: Vec<Hill>,
    /// Food collected and not yet spawned: a private store, not a place on the map.
    pub hive: Vec<u16>,
    pub score: Vec<i16>,

    /// Who the cutoff counter is watching: a seat, `CUTOFF_FOOD`, or `CUTOFF_NONE`.
    ///
    /// One holder and one counter, not two independent ones — the reference engine's "food not
    /// being gathered" and "ants not razing hills" are the same counter reported under different
    /// names (`ants.py:1392`).
    pub cutoff_bot: u8,
    /// Consecutive turns `cutoff_bot` has held at least `CUTOFF_PERCENT` of the population. Zeroed
    /// by a razing, and held still on a turn where an ant died on a contested hill.
    pub cutoff_turns: u16,

    /// The match's hidden food rate, drawn once from the seed. Food accrues at
    /// `food_rate * players / food_turn` per turn. `food_rate == 0` turns spawning off entirely,
    /// which is the reference's `do_food_none` and what the rule tests use.
    ///
    /// Carried in the state rather than recomputed from the preset table, for the same reason the
    /// board is: a replay must re-simulate a match whose preset has since been re-tuned.
    pub food_rate: u16,
    pub food_turn: u16,
    /// The numerator of the accrued-but-unspawned food, over `food_turn`. The reference keeps an
    /// exact `Fraction` here (`ants.py:1464`); an integer numerator is the same number without the
    /// floating point game logic may not use.
    pub food_extra: u32,
    /// Where the shuffled food-set rotation has got to. The order itself is not stored: it is
    /// Fisher-Yates over the board's sets, seeded by `seed` and the rotation number.
    pub food_rotation: u16,
    pub food_cursor: u16,
    /// Food that was owed but could not be placed. The reference queues it and places it "as soon
    /// as the location becomes available" (`ants.py:1105`), so the rate is honoured on a crowded
    /// board.
    pub pending_food: Vec<u16>,

    /// The board this match opened on, kept so `finish` can hand it to the replay envelope.
    ///
    /// Water, the hills and the grid survive the match unchanged; the food does not, so the
    /// turn-zero food is the one part of the board that has to be remembered rather than read off
    /// the current state.
    pub map_id: String,
    pub food0: Vec<u16>,
}

#[rustfmt::skip]
pub const END_REASONS: [&str; 6] = [
    "turn_limit",      // 0
    "lone_survivor",   // 1
    "extermination",   // 2
    "rank_stabilized", // 3
    "domination",      // 4 — ants not razing hills
    "idle_food",       // 5 — food not being gathered
];

pub const STALEMATE_TURNS: u16 = 150;

/// The share of the whole population one holder must hold for the cutoff counter to run, in
/// percent. The specification's prose says 90; the engine says 85 (`ants.py:68`).
pub const CUTOFF_PERCENT: u32 = 85;

/// `cutoff_bot` when nobody holds the share — the reference's `LAND`.
pub const CUTOFF_NONE: u8 = 255;
/// `cutoff_bot` when loose food on the map holds the share — the reference's `FOOD`.
pub const CUTOFF_FOOD: u8 = 254;

impl Match {
    /// An empty match on a board: no ants, no food, no hills. `worldgen` and `MapFile::build` are
    /// the two ways one is filled in, and they must not be able to disagree about the rest.
    pub fn new(seed: u64, g: Geom, players: u8, max_turns: u16, water: Bits) -> Match {
        let (food_rate, food_turn) = crate::food::rate_for(seed);
        Match {
            seed,
            turn: 0,
            max_turns,
            players,
            g,
            sym: Symmetry::for_preset(&g, players),
            done: false,
            reason: 0,
            water,
            known: (0..players).map(|_| Bits::zeros(g.cells())).collect(),
            ants: Vec::new(),
            food: Vec::new(),
            hills: Vec::new(),
            hive: vec![0; players as usize],
            score: vec![0; players as usize],
            cutoff_bot: CUTOFF_NONE,
            cutoff_turns: 0,
            food_rate,
            food_turn,
            food_extra: 0,
            food_rotation: 0,
            food_cursor: 0,
            pending_food: Vec::new(),
            map_id: String::new(),
            food0: Vec::new(),
        }
    }

    /// Seat the hills, one per player in order, and open the match on them.
    ///
    /// Each seat begins with a hill, one ant standing on it, and one point per hill owned — the
    /// last so that a player who loses their hill and razes nothing sits on zero rather than on
    /// minus one (`ants.py:152`).
    pub(crate) fn open_on(&mut self, hills: &[u16]) {
        for (k, &pos) in hills.iter().enumerate() {
            self.hills.push(Hill { pos, owner: k as u8, razed: false, last_touched: 0 });
            self.ants.push(Ant { pos, owner: k as u8 });
            self.score[k] += 1;
        }
        for pl in 0..self.players {
            self.reveal(pl);
        }
    }

    pub fn cells(&self) -> usize {
        self.g.cells()
    }

    pub fn ants_of(&self, owner: u8) -> impl Iterator<Item = &Ant> {
        self.ants.iter().filter(move |a| a.owner == owner)
    }

    pub fn alive(&self, owner: u8) -> bool {
        self.ants.iter().any(|a| a.owner == owner)
    }

    pub fn living_players(&self) -> Vec<u8> {
        (0..self.players).filter(|&p| self.alive(p)).collect()
    }

    /// My ants, row-major. Deterministic, not stable: an ant does not keep its slot across turns,
    /// but the sort is committed, so a determinism audit reproduces without the ordering ever
    /// becoming an identity channel.
    pub fn mine(&self, owner: u8) -> Vec<u16> {
        let mut m: Vec<u16> = self.ants_of(owner).map(|a| a.pos).collect();
        m.sort_unstable();
        m
    }

    /// Every square within view radius of at least one living ant of `owner`.
    ///
    /// Stamped a row of the disk at a time: the same 241 squares an ant at radius² 77, with one
    /// wrap per row instead of two per square. It runs twice per seat per turn — `reveal` folds it
    /// into `known`, and the view reads it again — so it is on every call's path.
    pub fn visible(&self, owner: u8) -> Bits {
        let mut v = Bits::zeros(self.cells());
        let rows = disk_rows(VIEW_RADIUS2);
        let (h, w) = (self.g.rows, self.g.cols);
        for a in self.ants_of(owner) {
            let (r, c) = self.g.rc(a.pos);
            for &(dr, half) in &rows {
                let base = (r + dr).rem_euclid(h) * w;
                let mut col = (c - half).rem_euclid(w);
                for _ in 0..=2 * half {
                    v.set((base + col) as usize);
                    col += 1;
                    if col == w {
                        col = 0;
                    }
                }
            }
        }
        v
    }

    /// Fold this turn's vision into what the player knows. Water never changes, so anything already
    /// seen stays true.
    ///
    /// A byte at a time: `visible` never sets a bit past the board, so OR-ing the bytes is OR-ing
    /// the squares.
    pub fn reveal(&mut self, owner: u8) {
        let vis = self.visible(owner);
        for (k, v) in self.known[owner as usize].bits.iter_mut().zip(&vis.bits) {
            *k |= v;
        }
    }

    /// Whether food may be placed on a square: land, and nothing already standing there.
    pub(crate) fn free_for_food(&self, pos: u16) -> bool {
        !self.water.get(pos as usize)
            && !self.food.contains(&pos)
            && !self.ants.iter().any(|a| a.pos == pos)
            && !self.hills.iter().any(|h| h.pos == pos)
    }
}
