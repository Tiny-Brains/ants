//! One match's state, and how a world is made.

use crate::map::{Bits, Geom, Preset, Rng, Symmetry, SPAWN_RADIUS2, VIEW_RADIUS2};

/// A hill. Razed hills are kept, because a razed hill is a permanent fact of the match (rule 42)
/// and the score already charged for it must not be charged twice (rule 57).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hill {
    pub pos: u16,
    pub owner: u8,
    pub razed: bool,
    /// Rule 53: when several of a player's hills are free, the one used least recently spawns
    /// first, so a colony spreads across its hills rather than piling up on one.
    pub last_spawn: u16,
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
    /// Food collected and not yet spawned. A private store, not a place on the map (rule 48).
    pub hive: Vec<u16>,
    pub score: Vec<i16>,

    /// Rule 66's two stalemate counters, in turns.
    pub domination_turns: u16,
    pub idle_food_turns: u16,

    /// How much food the board is kept stocked with — a property of the map (`mapfile.rs`).
    ///
    /// Carried in the state rather than recovered from the preset table, because two maps may be
    /// the same size and keep different amounts of food, and because a replay must be able to
    /// re-simulate a match whose preset has since been re-tuned.
    pub food_target: u16,

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
    "turn_limit",       // 0 — rule 62
    "lone_survivor",    // 1 — rule 63
    "extermination",    // 2 — rule 64
    "rank_stabilized",  // 3 — rule 65
    "domination",       // 4 — rule 66, first form
    "idle_food",        // 5 — rule 66, second form
];

pub const STALEMATE_TURNS: u16 = 150;

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

    /// Every square within view radius of at least one living ant of `owner` — rules 11 and 12.
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

    /// Fold this turn's vision into what the player knows. Rule 16: water never changes, so
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
/// terrain, the hills and the food are congruent for everyone (rule 46, and see `map::Symmetry`).
/// A map that were only *approximately* fair would put a thumb on every rating computed from it.
pub fn worldgen(seed: u64, p: Preset, max_turns: u16) -> Match {
    let g = Geom::new(p.rows, p.cols);
    let sym = Symmetry::for_preset(&g, p.players);
    let cells = g.cells();
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
        domination_turns: 0,
        idle_food_turns: 0,
        food_target: p.food_per_player as u16 * p.players as u16,
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
        m.hills.push(Hill { pos: img, owner: k as u8, razed: false, last_spawn: 0 });
        // Rule 39: each player begins with a hill, and one ant on it.
        m.ants.push(Ant { pos: img, owner: k as u8 });
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
    let target = m.food_target as usize;
    spawn_food(&mut m, &mut rng, target);

    m.food0 = m.food.clone();
    for pl in 0..p.players {
        m.reveal(pl);
    }
    m
}

/// Place food symmetrically until the map holds `target`. Rule 46: placed symmetrically, so every
/// player is offered the same opportunities in the same shape.
pub fn spawn_food(m: &mut Match, rng: &mut Rng, target: usize) {
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

/// The RNG a turn draws from. Derived from the seed and the turn so the whole match is a pure
/// function of its seed, and so a replay re-simulates without carrying generator state.
pub fn turn_rng(m: &Match) -> Rng {
    Rng(m.seed ^ (m.turn as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}




