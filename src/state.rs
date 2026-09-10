//! One match's state, and how a world is made.

use crate::map::{Bits, Geom, Rng, Symmetry, VIEW_RADIUS2};

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
    /// floating point `docs/cartridge.md` §4 forbids.
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

/// The hidden food rate's range, drawn per match exactly as the reference draws it
/// (`ants.py:54-59`). The specification only says "each game has a hidden food rate", so the engine
/// is the only statement of what the rate actually is.
pub const FOOD_RATE: (u32, u32) = (5, 11);
pub const FOOD_TURN: (u32, u32) = (19, 37);

/// The share of the whole population one holder must hold for the cutoff counter to run, in
/// percent. The specification's prose says 90; the engine says 85 (`ants.py:68`).
pub const CUTOFF_PERCENT: u32 = 85;

/// `cutoff_bot` when nobody holds the share — the reference's `LAND`.
pub const CUTOFF_NONE: u8 = 255;
/// `cutoff_bot` when loose food on the map holds the share — the reference's `FOOD`.
pub const CUTOFF_FOOD: u8 = 254;

/// Draw a match's hidden food rate from its seed.
pub fn food_rate_for(seed: u64) -> (u16, u16) {
    let mut r = Rng(seed ^ 0xF00D_5EED_A11E_2C1D);
    let rate = FOOD_RATE.0 + r.below(FOOD_RATE.1 - FOOD_RATE.0 + 1);
    let turn = FOOD_TURN.0 + r.below(FOOD_TURN.1 - FOOD_TURN.0 + 1);
    (rate as u16, turn as u16)
}

impl Match {
    /// An empty match on a board: no ants, no food, no hills. `worldgen` and `MapFile::build` are
    /// the two ways one is filled in, and they must not be able to disagree about the rest.
    pub fn new(seed: u64, g: Geom, players: u8, max_turns: u16, water: Bits) -> Match {
        let (food_rate, food_turn) = food_rate_for(seed);
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
    pub fn visible(&self, owner: u8) -> Bits {
        let mut v = Bits::zeros(self.cells());
        let disk = self.g.disk(VIEW_RADIUS2);
        for a in self.ants_of(owner) {
            let (r, c) = self.g.rc(a.pos);
            for (dr, dc) in &disk {
                v.set(self.g.at(r + dr, c + dc) as usize);
            }
        }
        v
    }

    /// Fold this turn's vision into what the player knows. Water never changes, so anything already
    /// seen stays true.
    pub fn reveal(&mut self, owner: u8) {
        let vis = self.visible(owner);
        let k = &mut self.known[owner as usize];
        for i in 0..vis.len {
            if vis.get(i) {
                k.set(i);
            }
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

// ---------------------------------------------------------------- food, at the hidden rate
//
// The reference model, from `ants.py`'s `do_food_symmetric`. Three parts:
//
//   1. A hidden rate. `food_rate * players / food_turn` food accrues per turn, kept exactly as a
//      rational (`ants.py:1464`). Whole food is spawned; the remainder carries.
//   2. Symmetric sets, shuffled, each used once per rotation. That is what makes food fair *and*
//      unpredictable: you cannot camp a square, but you also cannot be starved while your opponent
//      is fed.
//   3. A queue. Food owed to an occupied square is not lost; it is placed when the square frees.

/// One representative position per symmetric food set on this board, in canonical order.
///
/// A set is the orbit of a square under the map's symmetry and its representative is its smallest
/// member, so the list is a function of the board alone and every host computes the same one.
///
/// Three exclusions, all the reference's: hills (`ants.py:1306`); sets whose members touch, because
/// "it would be unfair to spawn so much food in one place"; and water — the reference's comment
/// says it starts "with only land squares" and then does not filter, so food aimed at water sits in
/// its pending queue for ever. Following the comment rather than the code is the one deliberate
/// departure here, and it is the difference between a maze board's food rate meaning what it says
/// and being quietly cut by the water fraction.
pub fn food_sets(m: &Match) -> Vec<u16> {
    let mut out = Vec::new();
    let mut buf = [0u16; 16];
    for pos in 0..m.cells() as u16 {
        let k = orbit_into(m, pos, &mut buf);
        let orbit = &buf[..k];
        let usable = orbit[0] == pos
            && !orbit.iter().any(|&p| m.water.get(p as usize))
            && !orbit.iter().any(|&p| m.hills.iter().any(|h| h.pos == p))
            && !orbit[1..].iter().any(|&p| m.g.dist2(orbit[0], p) == 1);
        if usable {
            out.push(pos);
        }
    }
    out
}

/// A square's orbit under the map symmetry, deduplicated and sorted, written into `buf`.
///
/// Deduplicated because a square can be the same distance from two players, which makes a set
/// smaller than normal. The rate takes that into account for free here, because it is spent per
/// location rather than per set.
fn orbit_into(m: &Match, pos: u16, buf: &mut [u16; 16]) -> usize {
    let mut k = 0usize;
    for i in 0..m.players as i32 {
        let p = m.sym.image(&m.g, pos, i);
        if !buf[..k].contains(&p) {
            buf[k] = p;
            k += 1;
        }
    }
    buf[..k].sort_unstable();
    k
}

/// The sets in the order rotation `rotation` uses them. Fisher-Yates, seeded by the match and the
/// rotation number, so the whole order is reproducible without ever being stored.
pub fn shuffled_sets(sets: &[u16], seed: u64, rotation: u16) -> Vec<u16> {
    let mut v = sets.to_vec();
    let mut r = Rng(seed ^ 0x5E75_C0DE_0000_0000 ^ rotation as u64);
    for i in (1..v.len()).rev() {
        v.swap(i, r.below(i as u32 + 1) as usize);
    }
    v
}

/// One turn's food, at the hidden rate.
pub fn spawn_food(m: &mut Match) {
    if m.food_rate > 0 && m.food_turn > 0 {
        m.food_extra += m.food_rate as u32 * m.players as u32;
        let due = m.food_extra / m.food_turn as u32;
        if due > 0 {
            let mut amount = due;
            let sets = food_sets(m);
            if !sets.is_empty() {
                let mut order = shuffled_sets(&sets, m.seed, m.food_rotation);
                let mut buf = [0u16; 16];
                // Whole sets only: a set is spawned when the accrued food covers all of it, and
                // what is left over stays accrued. Spawning half a set would be an asymmetric map.
                loop {
                    if m.food_cursor as usize >= order.len() {
                        m.food_rotation = m.food_rotation.wrapping_add(1);
                        m.food_cursor = 0;
                        order = shuffled_sets(&sets, m.seed, m.food_rotation);
                    }
                    let k = orbit_into(m, order[m.food_cursor as usize], &mut buf) as u32;
                    if k > amount {
                        break;
                    }
                    amount -= k;
                    m.food_cursor += 1;
                    m.pending_food.extend_from_slice(&buf[..k as usize]);
                }
            }
            m.food_extra -= (due - amount) * m.food_turn as u32;
        }
    }
    place_pending(m);
}

/// Place whatever is owed and can be placed. The rest waits, which is what keeps the rate honest.
fn place_pending(m: &mut Match) {
    if m.pending_food.is_empty() {
        return;
    }
    let mut still = Vec::new();
    for p in std::mem::take(&mut m.pending_food) {
        if m.free_for_food(p) {
            m.food.push(p);
        } else {
            still.push(p);
        }
    }
    m.pending_food = still;
}
