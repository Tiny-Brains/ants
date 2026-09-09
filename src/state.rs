//! One match's state, and how a world is made.

use crate::map::{Bits, Geom, Preset, Rng, Symmetry, SPAWN_RADIUS2, VIEW_RADIUS2};

/// A hill. Razed hills are kept, because a razed hill is a permanent fact of the match (*Hill
/// Razing*) and the score already charged for it must not be charged twice (*Scoring*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hill {
    pub pos: u16,
    pub owner: u8,
    pub razed: bool,
    /// The turn this hill was last **touched** — spawned from, or stood on by an ant of its own
    /// owner. When several of a player's hills are free the least recently touched spawns first,
    /// so a colony spreads across its hills rather than piling up on one.
    ///
    /// Touched, not spawned-from. The reference engine stamps this in its raze phase whenever the
    /// owner's own ant is standing on its own hill (`ants.py:812`), which is what makes "park an
    /// ant on a hill to steer where the next one appears" work at all.
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
    /// What each player has ever seen. **This is the expensive field and it is deliberate** — see
    /// `observe.rs` on why `water` cannot be derived without it.
    pub known: Vec<Bits>,

    pub ants: Vec<Ant>,
    pub food: Vec<u16>,
    pub hills: Vec<Hill>,
    /// Food collected and not yet spawned. A private store, not a place on the map (*Food
    /// Harvesting*).
    pub hive: Vec<u16>,
    pub score: Vec<i16>,

    /// Who the cutoff counter is currently watching: a seat, `CUTOFF_FOOD`, or `CUTOFF_NONE`.
    ///
    /// **One holder and one counter, not two independent ones.** The reference engine's "food not
    /// being gathered" and "ants not razing hills" are the same counter reported under different
    /// names (`ants.py:1392`), which is why they cannot both be running.
    pub cutoff_bot: u8,
    /// Consecutive turns `cutoff_bot` has held at least `CUTOFF_PERCENT` of the population. Zeroed
    /// by a razing, and held still on a turn where an ant died on a contested hill.
    pub cutoff_turns: u16,

    /// The match's **hidden food rate**, drawn once from the seed. Food accrues at
    /// `food_rate * players / food_turn` per turn — see `spawn_food`. `food_rate == 0` turns food
    /// spawning off entirely, which is the reference's `do_food_none` and what the rule tests use.
    ///
    /// Carried in the state rather than recomputed from the preset table, for the same reason the
    /// board is: a replay must re-simulate a match whose preset has since been re-tuned.
    pub food_rate: u16,
    pub food_turn: u16,
    /// The numerator of the accrued-but-unspawned food, over `food_turn`. The reference keeps an
    /// exact `Fraction` here (`ants.py:1464`); an integer numerator is the same number without the
    /// floating point that `docs/cartridge.md` §4 forbids.
    pub food_extra: u32,
    /// Where the shuffled food-set rotation has got to. The order itself is **not** stored: it is
    /// Fisher-Yates over the board's sets, seeded by `seed` and the rotation number, so it is
    /// reproducible from four bytes rather than from a list of every set on the map.
    pub food_rotation: u16,
    pub food_cursor: u16,
    /// Food that was owed but could not be placed, because an ant was standing there or food was
    /// there already. The reference queues it and places it "as soon as the location becomes
    /// available" (`ants.py:1105`), so the rate is honoured even on a crowded board.
    pub pending_food: Vec<u16>,

    /// The board this match opened on, kept so `finish` can hand it to the replay envelope.
    ///
    /// Water, the hills and the grid survive the match unchanged, but the food does not — it is
    /// eaten and respawned from turn one. So the turn-zero food is the one part of the board that
    /// has to be *remembered* rather than read off the current state, and `map_id` rides with it
    /// so a replay can name its board as well as carry it. Together they cost about seventy bytes
    /// a match, which buys a replay that needs no catalogue to be viewable.
    pub map_id: String,
    pub food0: Vec<u16>,
}

pub const END_REASONS: [&str; 6] = [
    "turn_limit",       // 0 — *Cutoff Rules*, turn limit reached
    "lone_survivor",    // 1 — *Cutoff Rules*, lone survivor
    "extermination",    // 2 — *Endbot Conditions*
    "rank_stabilized",  // 3 — *Cutoff Rules*, rank stabilized
    "domination",       // 4 — *Cutoff Rules*, ants not razing hills
    "idle_food",        // 5 — *Cutoff Rules*, food not being gathered
];

pub const STALEMATE_TURNS: u16 = 150;

/// The hidden food rate's range, drawn per match exactly as the reference draws it
/// (`ants.py:54-59`): `food_rate` from [5, 11] total food, over a `food_turn` from [19, 37] turns.
/// The specification only says "Each game has a hidden food rate that will increase the amount of
/// food in the game", so the engine is the only statement of what the rate actually is.
pub const FOOD_RATE: (u32, u32) = (5, 11);
pub const FOOD_TURN: (u32, u32) = (19, 37);

/// Draw a match's hidden food rate from its seed.
pub fn food_rate_for(seed: u64) -> (u16, u16) {
    let mut r = Rng(seed ^ 0xF00D_5EED_A11E_2C1D);
    let rate = FOOD_RATE.0 + r.below(FOOD_RATE.1 - FOOD_RATE.0 + 1);
    let turn = FOOD_TURN.0 + r.below(FOOD_TURN.1 - FOOD_TURN.0 + 1);
    (rate as u16, turn as u16)
}

/// The share of the whole population one holder must hold for the cutoff counter to run, in
/// percent. The specification prose says 90; the engine says 85 (`ants.py:68`), and the engine is
/// what every bot was actually scored against.
pub const CUTOFF_PERCENT: u32 = 85;

/// `cutoff_bot` when nobody holds the share — the reference's `LAND`.
pub const CUTOFF_NONE: u8 = 255;
/// `cutoff_bot` when loose food on the map holds the share — the reference's `FOOD`.
pub const CUTOFF_FOOD: u8 = 254;

impl Match {
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

    /// My ants, row-major. **Decision 15: deterministic, not stable.** The ordering is meaningless
    /// across turns — an ant does not keep its slot — but it is a committed sort, so a determinism
    /// audit reproduces without the ordering ever becoming an identity channel.
    pub fn mine(&self, owner: u8) -> Vec<u16> {
        let mut m: Vec<u16> = self.ants_of(owner).map(|a| a.pos).collect();
        m.sort_unstable();
        m
    }

    /// Every square within view radius of at least one living ant of `owner` — *Fog of War*.
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

    /// Fold this turn's vision into what the player knows. *Bot Input*: water never changes, so
    /// anything already seen stays true.
    pub fn reveal(&mut self, owner: u8) {
        let vis = self.visible(owner);
        let k = &mut self.known[owner as usize];
        for i in 0..vis.len {
            if vis.get(i) {
                k.set(i);
            }
        }
    }

    pub fn hill_at(&self, pos: u16) -> Option<&Hill> {
        self.hills.iter().find(|h| !h.razed && h.pos == pos)
    }

    pub fn ant_at(&self, pos: u16) -> Option<&Ant> {
        self.ants.iter().find(|a| a.pos == pos)
    }
}

// ---------------------------------------------------------------- worldgen

/// Build one match from one seed.
///
/// The whole world is generated on a fundamental domain and translated to every player, so the
/// terrain, the hills and the food are congruent for everyone (*Food spawning*, and see
/// `map::Symmetry`). A map that were only *approximately* fair would put a thumb on every rating
/// computed from it.
pub fn worldgen(seed: u64, p: Preset, max_turns: u16) -> Match {
    let g = Geom::new(p.rows, p.cols);
    let sym = Symmetry::for_preset(&g, p.players);
    let cells = g.cells();
    let (rate, turn_len) = food_rate_for(seed);
    let mut rng = Rng(seed);

    let mut m = Match {
        seed,
        turn: 0,
        max_turns,
        players: p.players,
        g,
        sym,
        done: false,
        reason: 0,
        water: Bits::zeros(cells),
        known: (0..p.players).map(|_| Bits::zeros(cells)).collect(),
        ants: Vec::new(),
        food: Vec::new(),
        hills: Vec::new(),
        hive: vec![0; p.players as usize],
        score: vec![0; p.players as usize],
        cutoff_bot: CUTOFF_NONE,
        cutoff_turns: 0,
        // Drawn from the seed, exactly as `MapFile::build` draws it, so a board generated here and
        // the same board loaded from its file are the same match.
        food_rate: rate,
        food_turn: turn_len,
        food_extra: 0,
        food_rotation: 0,
        food_cursor: 0,
        pending_food: Vec::new(),
        map_id: String::new(),
        food0: Vec::new(),
    };

    // ---- water, in blobs on the fundamental domain, then translated.
    //
    // Blobs rather than per-cell noise for two reasons: a map of speckles is not a map anyone can
    // play, and `water` is sent run-length encoded, so speckles would make every observation
    // several times larger for a worse game.
    let domain = cells / p.players as usize;
    let target = domain * p.water_pct as usize / 100;
    let mut placed = 0usize;
    let mut guard = 0;
    while placed < target && guard < 100_000 {
        guard += 1;
        let r0 = rng.below(g.rows as u32) as i32;
        let c0 = rng.below(g.cols as u32) as i32;
        let h = 1 + rng.below(p.blob) as i32;
        let w = 1 + rng.below(p.blob) as i32;
        for dr in 0..h {
            for dc in 0..w {
                let pos = g.at(r0 + dr, c0 + dc);
                for img in sym.orbit(&g, pos) {
                    if !m.water.get(img as usize) {
                        m.water.set(img as usize);
                        placed += 1;
                    }
                }
            }
        }
    }

    // ---- one hill per player, on land, with room around it.
    //
    // The hill's square and its neighbourhood are cleared of water, so no player starts walled in
    // by a blob that happened to land on them.
    let hill0 = loop {
        let pos = g.at(rng.below(g.rows as u32) as i32, rng.below(g.cols as u32) as i32);
        // Far enough from its own image that the two colonies do not start on top of each other.
        if g.dist2(pos, sym.image(&g, pos, 1)) >= 400 {
            break pos;
        }
    };
    let clear = g.disk(SPAWN_RADIUS2 * 8);
    let (hr, hc) = g.rc(hill0);
    for (dr, dc) in &clear {
        let pos = g.at(hr + dr, hc + dc);
        for img in sym.orbit(&g, pos) {
            m.water.bits[img as usize >> 3] &= !(1 << (img as usize & 7));
        }
    }
    for (k, img) in sym.orbit(&g, hill0).into_iter().enumerate() {
        m.hills.push(Hill { pos: img, owner: k as u8, razed: false, last_touched: 0 });
        // *Map Format*: each player begins with a hill, and one ant on it.
        m.ants.push(Ant { pos: img, owner: k as u8 });
    }
    // One point per hill owned, before anything is razed. This is not decoration: it is what keeps
    // a player who loses their hill and razes nothing on zero rather than on minus one
    // (`ants.py:152`, "points start at # of hills to prevent negative scores").
    for h in &m.hills {
        m.score[h.owner as usize] += 1;
    }

    // ---- food beside the hills, so a colony can bootstrap (see `Preset::hill_food`), and then
    // the rest of the map's food, symmetric.
    let near = m.g.disk(36); // within six squares of the hill
    let mut placed_near = 0u32;
    let mut tries = 0;
    while placed_near < p.hill_food && tries < 500 {
        tries += 1;
        let (dr, dc) = near[rng.below(near.len() as u32) as usize];
        let pos = m.g.at(hr + dr, hc + dc);
        let orbit = m.sym.orbit(&m.g, pos);
        let ok = orbit.iter().all(|&i| {
            !m.water.get(i as usize) && !m.food.contains(&i) && m.hill_at(i).is_none()
                && m.ant_at(i).is_none()
        });
        if ok {
            m.food.extend(orbit);
            placed_near += 1;
        }
    }
    let target = p.food_per_player as usize * p.players as usize;
    fill_food(&mut m, &mut rng, target);

    m.food0 = m.food.clone();
    for pl in 0..p.players {
        m.reveal(pl);
    }
    m
}

/// Place food symmetrically until the map holds `target`.
///
/// **This builds a board, it does not run a match.** It is what `mapgen` uses to write a `maps/*`
/// file's turn-zero food, and nothing calls it after turn zero — food during a match accrues at the
/// hidden rate instead, in `spawn_food`. Placed symmetrically, so every player is offered the same
/// opportunities in the same shape.
pub fn fill_food(m: &mut Match, rng: &mut Rng, target: usize) {
    let mut guard = 0;
    while m.food.len() < target && guard < 10_000 {
        guard += 1;
        let pos = m.g.at(rng.below(m.g.rows as u32) as i32, rng.below(m.g.cols as u32) as i32);
        let orbit = m.sym.orbit(&m.g, pos);
        // All or nothing: a partially placeable orbit would be an asymmetric map.
        let ok = orbit.iter().all(|&i| {
            !m.water.get(i as usize)
                && !m.food.contains(&i)
                && m.hill_at(i).is_none()
                && m.ant_at(i).is_none()
        });
        if ok {
            m.food.extend(orbit);
        }
    }
}

// ---------------------------------------------------------------- food, at the hidden rate
//
// The reference model, from `ants.py`'s `do_food_symmetric` and the specification's "Food spawning"
// section. Three parts, and the middle one is the interesting one:
//
// 1. **A hidden rate.** `food_rate * players / food_turn` food accrues per turn, kept exactly as a
//    rational (`ants.py:1464`). Whole food is spawned; the remainder carries.
// 2. **Symmetric sets, shuffled, each used once per rotation.** "The entire map is divided into
//    sets of squares that are symmetric. The sets are shuffled into a random order. When food is
//    spawned, the next set is chosen. When all the sets have been chosen, they are shuffled again."
//    That is what makes food fair *and* unpredictable: you cannot camp a square, but you also
//    cannot be starved while your opponent is fed.
// 3. **A queue.** Food owed to an occupied square is not lost; it is placed when the square frees.
//
// What this replaced was a fixed `food_target` the board was topped back up to every turn. That
// kept food density constant, so food was never scarce and the board never changed character
// between the opening and the endgame — a different game from the one the rules describe.

/// One representative position per symmetric food set on this board, in a fixed canonical order.
///
/// A set is the orbit of a square under the map's symmetry, and its representative is its smallest
/// member, so the list is a function of the board alone and every host computes the same one.
///
/// Three exclusions, all the reference's:
/// - **Hills.** `ants.py:1306` skips them, so food never spawns on a hill.
/// - **Squares whose set members touch.** "Some maps have mirror symmetry so that a set of
///   symmetric squares are touching. It would be unfair to spawn so much food in one place, so
///   these sets are not used." The engine tests squared distance 1 against the first member.
/// - **Water.** The reference's own comment says it starts "with only land squares" and then does
///   not filter, so food aimed at water sits in its pending queue forever and is silently lost.
///   Following the comment rather than the code is the one deliberate departure here, and it is
///   the difference between a maze board's food rate meaning what it says and being quietly cut by
///   the water fraction.
pub fn food_sets(m: &Match) -> Vec<u16> {
    let mut out = Vec::new();
    let mut buf = [0u16; 16];
    for pos in 0..m.cells() as u16 {
        let k = orbit_into(m, pos, &mut buf);
        let orbit = &buf[..k];
        if orbit[0] != pos {
            continue; // some other member represents this set
        }
        if orbit.iter().any(|&p| m.water.get(p as usize)) {
            continue;
        }
        if orbit.iter().any(|&p| m.hills.iter().any(|h| h.pos == p)) {
            continue;
        }
        if orbit[1..].iter().any(|&p| m.g.dist2(orbit[0], p) == 1) {
            continue;
        }
        out.push(pos);
    }
    out
}

/// A square's orbit under the map symmetry, deduplicated and sorted, written into `buf`.
///
/// Deduplicated because a square can be the same distance from two players — "This makes for a set
/// that is smaller than normal. The food rate takes this into account when spawning food", which
/// falls out for free here because the rate is spent per *location*, not per set.
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
fn shuffled_sets(sets: &[u16], seed: u64, rotation: u16) -> Vec<u16> {
    let mut v = sets.to_vec();
    let mut r = Rng(seed ^ 0x5E75_C0DE_0000_0000 ^ rotation as u64);
    for i in (1..v.len()).rev() {
        v.swap(i, r.below(i as u32 + 1) as usize);
    }
    v
}

/// The set that rotation `rotation` uses at position `cursor` — the same order `spawn_food` walks,
/// one element at a time, for anything that needs to check the rotation property without replaying
/// a match.
pub fn nth_set(sets: &[u16], seed: u64, rotation: u16, cursor: usize) -> u16 {
    shuffled_sets(sets, seed, rotation)[cursor]
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
                    let rep = order[m.food_cursor as usize];
                    let k = orbit_into(m, rep, &mut buf) as u32;
                    if k > amount {
                        break;
                    }
                    amount -= k;
                    m.food_cursor += 1;
                    for &p in &buf[..k as usize] {
                        m.pending_food.push(p);
                    }
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
    let owed = std::mem::take(&mut m.pending_food);
    let mut still = Vec::new();
    for p in owed {
        let free = !m.water.get(p as usize)
            && !m.food.contains(&p)
            && !m.ants.iter().any(|a| a.pos == p)
            && !m.hills.iter().any(|h| h.pos == p);
        if free {
            m.food.push(p);
        } else {
            still.push(p);
        }
    }
    m.pending_food = still;
}





