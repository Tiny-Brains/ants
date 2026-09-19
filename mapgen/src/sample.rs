//! Proposing designs: one slot (size, terrain, seats, hills) and a seed in, a `Design`
//! out.
//!
//! This is the part that is allowed to change. A sampler proposes; a person picks; what they pick
//! is written down as a design, which `design::render` draws the same way however this file later
//! changes. So everything here is taste, and nothing here is a contract.
//!
//! **Every family is a composition of motifs placed where the lattice says they belong**: a base
//! about each centre, something on each border's middle, something on each corner where three
//! territories meet. That is what keeps a board from looking rolled: the eye finds the same idea at
//! the same kind of place everywhere, and the point group repeats it.

use crate::design::{CLEARING, Design, FoodPlan, Step};
use crate::grid::{Rng, Shift, Torus, mix};
use crate::recipe::VIEW_RADIUS2;
use crate::sym;

#[derive(Clone, Debug)]
pub struct Slot {
    pub size: String,
    pub terrain: String,
    pub seats: u8,
    pub hills: u8,
}

impl Slot {
    pub fn name(&self) -> String {
        format!("{}-{}-{}p-{}h", self.size, self.terrain, self.seats, self.hills)
    }
}

/// `[smallest side, largest side]` a size class allows, and the largest side it must exceed.
pub fn class(size: &str) -> Result<(i32, i32, i32), String> {
    Ok(match size {
        "tiny" => (24, 32, 0),
        "small" => (32, 48, 32),
        "medium" => (48, 64, 48),
        "large" => (64, 96, 64),
        "xlarge" => (96, 127, 96),
        other => return Err(format!("no size '{other}'")),
    })
}

#[derive(Clone, Debug)]
pub struct BoardOpt {
    pub rows: i32,
    pub cols: i32,
    pub shift: (i32, i32),
    pub spacing: f64,
    pub fatness: f64,
    pub groups: Vec<&'static str>,
    /// Lattice spacings that divide the board and the shift, for mazes, rooms and dots.
    pub pitches: Vec<i32>,
}

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// Every board and seating a slot could be played on, fat enough to play well.
pub fn board_options(slot: &Slot) -> Result<Vec<BoardOpt>, String> {
    let (lo, hi, over) = class(&slot.size)?;
    let p = slot.seats as i32;
    let mut out = Vec::new();
    for cols in lo.max(over + 1)..=hi {
        for rows in lo..=cols {
            // Landscape, and no more than a third longer than it is tall.
            if cols * 3 > rows * 4 {
                continue;
            }
            let t = Torus { rows, cols };
            let mut seen: Vec<Vec<(i32, i32)>> = Vec::new();
            for dr in (0..rows).filter(|dr| (dr * p) % rows == 0) {
                for dc in (0..cols).filter(|dc| (dc * p) % cols == 0) {
                    let s = Shift { seats: p as usize, dr, dc };
                    if !s.is_exact(&t) {
                        continue;
                    }
                    let mut sub: Vec<(i32, i32)> =
                        (0..p).map(|k| ((k * dr) % rows, (k * dc) % cols)).collect();
                    sub.sort_unstable();
                    if seen.contains(&sub) {
                        continue;
                    }
                    seen.push(sub);
                    let d2 = |r: i32, c: i32| {
                        let (r, c) = (r.rem_euclid(rows), c.rem_euclid(cols));
                        let (r, c) = (r.min(rows - r), c.min(cols - c));
                        (r * r + c * c) as i64
                    };
                    let seat2 = (1..p).map(|k| d2(k * dr, k * dc)).min().unwrap_or(0);
                    let own2 = (rows as i64).pow(2).min((cols as i64).pow(2));
                    let l2 = seat2.min(own2);
                    let fat = l2 as f64 / (rows as f64 * cols as f64 / p as f64);
                    if seat2 <= VIEW_RADIUS2 + 20 || fat < 0.55 {
                        continue;
                    }
                    let g = gcd(gcd(rows, cols), gcd(dr, dc));
                    let pitches = (3..=16).filter(|q| g % q == 0).collect();
                    out.push(BoardOpt {
                        rows,
                        cols,
                        shift: (dr, dc),
                        spacing: (seat2 as f64).sqrt(),
                        fatness: fat,
                        groups: sym::admitted(&t, &s),
                        pitches,
                    });
                }
            }
        }
    }
    if out.is_empty() {
        return Err(format!("{}: no board fits", slot.name()));
    }
    Ok(out)
}

fn order(g: &str) -> usize {
    sym::group(g).map_or(1, |v| v.len())
}

fn pick<'a, T>(rng: &mut Rng, items: &'a [T], weight: impl Fn(&T) -> f64) -> Option<&'a T> {
    let total: f64 = items.iter().map(&weight).sum();
    if total <= 0.0 {
        return None;
    }
    let mut x = rng.below(1_000_000) as f64 / 1_000_000.0 * total;
    for it in items {
        x -= weight(it);
        if x <= 0.0 {
            return Some(it);
        }
    }
    items.last()
}

fn uni(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * (rng.below(10_001) as f64 / 10_000.0)
}

fn chance(rng: &mut Rng, pct: u32) -> bool {
    rng.percent(pct)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn pt(r: f64, c: f64) -> [f64; 2] {
    [round2(r), round2(c)]
}

/// A point `rho` from the centre at `deg` degrees, counter-clockwise from the column axis.
fn polar(rho: f64, deg: f64) -> [f64; 2] {
    let a = deg.to_radians();
    pt(rho * a.sin(), rho * a.cos())
}

fn shape(op: &str, sh: &str) -> Step {
    Step { op: op.into(), shape: sh.into(), ..Default::default() }
}

fn disk(op: &str, at: [f64; 2], r: f64, metric: &str) -> Step {
    Step { at, r: [round2(r), 0.0], metric: metric.into(), ..shape(op, "disk") }
}

fn ring(op: &str, at: [f64; 2], r0: f64, r1: f64, metric: &str) -> Step {
    Step { at, r: [round2(r0), round2(r1)], metric: metric.into(), ..shape(op, "ring") }
}

fn seg(op: &str, a: [f64; 2], b: [f64; 2], w: f64) -> Step {
    Step { at: a, to: b, width: round2(w), ..shape(op, "seg") }
}

fn boxed(op: &str, at: [f64; 2], hr: f64, hc: f64) -> Step {
    Step { at, r: [round2(hr), round2(hc)], ..shape(op, "box") }
}

fn norm(v: (f64, f64)) -> f64 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

/// The ground kept open round a hill before any motif starts: its clearing and a step more.
const PAD: f64 = CLEARING as f64 + 1.5;

/// One hill's base: where it stands, from seat 0's centre, and how far its own motif may reach
/// before it would meet another hill's.
#[derive(Clone, Copy, Debug)]
struct Base {
    at: (f64, f64),
    room: f64,
}

/// What a family needs to know about the board it is drawing on.
struct Ctx {
    rng: Rng,
    /// Distance between neighbouring centres.
    s: f64,
    /// One base per orbit of seat 0's hills. Drawing round one draws round every image of it, so
    /// each is drawn once — twice would lay two different motifs over every hill in the orbit.
    bases: Vec<Base>,
    /// Every hill of every seat, from seat 0's centre, with its images across the wrap.
    hill_pts: Vec<(f64, f64)>,
    /// When no hill stands on the centre: how much open ground there is round it.
    hub: Option<f64>,
    mids: Vec<(f64, f64)>,
    corners: Vec<(f64, f64)>,
    pitches: Vec<i32>,
    /// Whether the point group has a quarter turn, so a gate on one axis is a gate on all four.
    quarter: bool,
    /// The room lattice the hills were placed on, for the families that draw rooms.
    grid: Option<i32>,
    steps: Vec<Step>,
}

fn add(a: (f64, f64), b: [f64; 2]) -> [f64; 2] {
    pt(a.0 + b[0], a.1 + b[1])
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// The direction from `a` towards `b`, in `polar`'s degrees.
fn towards(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).atan2(b.1 - a.1).to_degrees()
}

impl Ctx {
    fn u(&mut self, lo: f64, hi: f64) -> f64 {
        uni(&mut self.rng, lo, hi.max(lo))
    }
    fn pct(&mut self, p: u32) -> bool {
        chance(&mut self.rng, p)
    }
    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        self.rng.within(lo, hi)
    }
    fn seed(&mut self) -> u64 {
        self.rng.next() | 1
    }
    fn metric(&mut self, options: &[&str]) -> String {
        options[self.rng.below(options.len() as u64) as usize].to_string()
    }
    fn angle(&mut self) -> f64 {
        match self.rng.below(5) {
            0 | 1 => 0.0,
            2 => 45.0,
            3 => 22.5,
            _ => self.u(0.0, 90.0).round(),
        }
    }
    /// A tunnel's width, in proportion to the board.
    fn tunnel(&mut self) -> f64 {
        (self.s * self.u(0.08, 0.13)).clamp(2.2, 4.5)
    }
    fn push(&mut self, st: Step) {
        self.steps.push(st);
    }
    fn at(v: (f64, f64)) -> [f64; 2] {
        pt(v.0, v.1)
    }
    /// How large water at `q` may be and still leave every hill its clearing.
    fn clearance(&self, q: (f64, f64)) -> f64 {
        self.hill_pts.iter().map(|&h| dist(h, q)).fold(f64::MAX, f64::min) - PAD - 1.0
    }
    /// A gate through everything between radii `a` and `b` of a base, at `deg`.
    fn gate(&mut self, base: (f64, f64), a: f64, b: f64, deg: f64, w: f64) {
        let st = seg("land", add(base, polar(a, deg)), add(base, polar(b, deg)), w);
        self.push(st);
    }
    fn pitch_in(&mut self, lo: i32, hi: i32) -> Option<i32> {
        let v: Vec<i32> = self.pitches.iter().copied().filter(|&q| q >= lo && q <= hi).collect();
        if v.is_empty() { None } else { Some(v[self.rng.below(v.len() as u64) as usize]) }
    }
    /// The direction a base's gate faces: towards the centre, or anywhere for the base on it.
    fn inward(&mut self, b: &Base) -> f64 {
        if norm(b.at) < 0.5 { self.angle() } else { towards(b.at, (0.0, 0.0)) }
    }
}

pub const FAMILIES: [(&str, &[&str]); 4] = [
    ("open", &["colonnade", "lakes", "circles", "stars", "groves", "rivers", "pinwheel"]),
    ("cave", &["kaleido", "grotto", "lagoon", "veins", "swirl"]),
    ("maze", &["braid", "rings", "garden", "spiral"]),
    ("rooms", &["grid", "fortress", "dungeon", "halls"]),
];

/// Which families can draw on a board with these lattice spacings.
fn families(terrain: &str, pitches: &[i32]) -> Vec<&'static str> {
    let has = |lo: i32, hi: i32| pitches.iter().any(|&q| q >= lo && q <= hi);
    FAMILIES
        .iter()
        .find(|(t, _)| *t == terrain)
        .map(|(_, f)| f.to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|f| match *f {
            "braid" => has(6, 12),
            "garden" => has(5, 10),
            "grid" => has(7, 16),
            "halls" => pitches.iter().any(|&q| q >= 8 && q % 4 == 0 && q <= 16),
            _ => true,
        })
        .collect()
}

/// How much a family wants a rich point group: patterns drawn from noise, and mazes, look rolled
/// under a half-turn alone and drawn under four or eight maps; shapes placed by hand look drawn
/// under anything.
fn family_weight(family: &str, group_order: usize) -> f64 {
    let g = group_order as f64;
    match family {
        "groves" | "kaleido" | "lagoon" | "veins" | "braid" | "swirl" => {
            (g / 4.0).powi(2).max(0.05)
        }
        "fortress" | "pinwheel" | "spiral" => (g / 4.0).max(0.25),
        _ => 1.0,
    }
}

/// The named subgroups of `group`, largest first.
fn subgroups(group: &str) -> Vec<&'static str> {
    let g = sym::group(group).unwrap_or_default();
    sym::GROUPS
        .iter()
        .copied()
        .filter(|name| sym::group(name).is_some_and(|ops| ops.iter().all(|m| g.contains(m))))
        .collect()
}

pub fn sample(slot: &Slot, opts: &[BoardOpt], seed: u64) -> Result<Design, String> {
    let mut rng = Rng(mix(seed, 0xB0A2D));
    let p = slot.seats as usize;
    let h = slot.hills as usize;

    // The board: fat seatings and rich symmetry first.
    let fits: Vec<&BoardOpt> =
        opts.iter().filter(|o| !families(&slot.terrain, &o.pitches).is_empty()).collect();
    let o = *pick(&mut rng, &fits, |o| {
        let g = order(o.groups[0]) as f64;
        let square = if o.rows == o.cols { 1.6 } else { 1.0 };
        o.fatness.powi(4) * (1.0 + g) * square
    })
    .ok_or("no board a family can draw on")?;
    let t = Torus { rows: o.rows, cols: o.cols };
    let s = Shift { seats: p, dr: o.shift.0, dc: o.shift.1 };

    let wanted = *pick(&mut rng, &o.groups, |g| (order(g) as f64).powi(2)).ok_or("no group")?;
    let fams = families(&slot.terrain, &o.pitches);
    let family = *pick(&mut rng, &fams, |f| family_weight(f, order(wanted))).ok_or("no family")?;

    // The hills, spread out, and the terrain's point group with them: the terrain takes exactly
    // the group the hills are symmetric under, so every image of a base has a hill in it. A richer
    // group whose orbits cannot add up to the hill count gives way to a smaller one.
    // Rooms put every hill at a room's centre, so the lattice is chosen before the hills are.
    let grid_opts: Vec<i32> = match family {
        "grid" => o.pitches.iter().copied().filter(|&q| (7..=16).contains(&q)).collect(),
        "halls" => {
            o.pitches.iter().copied().filter(|&q| (8..=16).contains(&q) && q % 4 == 0).collect()
        }
        _ => Vec::new(),
    };
    let grid = if grid_opts.is_empty() {
        None
    } else {
        Some(grid_opts[rng.below(grid_opts.len() as u64) as usize])
    };
    let mut chosen = None;
    'g: for sub in subgroups(wanted) {
        for _ in 0..30 {
            if let Some(hs) = hills(&mut rng, &t, &s, sub, h, o.spacing, grid) {
                chosen = Some((sub, hs));
                break 'g;
            }
        }
    }
    let (group, orbits) = chosen.ok_or("no hill layout fits")?;
    let hill_list: Vec<(i32, i32)> = orbits.iter().flatten().copied().collect();
    let mut hill_pts = Vec::new();
    for &(r, c) in &hill_list {
        for k in 0..p as i32 {
            for a in -1..=1 {
                for b in -1..=1 {
                    hill_pts.push((
                        (r + k * s.dr + a * t.rows) as f64,
                        (c + k * s.dc + b * t.cols) as f64,
                    ));
                }
            }
        }
    }
    let bases: Vec<Base> = orbits
        .iter()
        .map(|orb| {
            let at = (orb[0].0 as f64, orb[0].1 as f64);
            let near =
                hill_pts.iter().map(|&q| dist(q, at)).filter(|&d| d > 0.5).fold(f64::MAX, f64::min);
            Base { at, room: near / 2.0 - 0.5 }
        })
        .collect();
    let hub = if hill_list.contains(&(0, 0)) {
        None
    } else {
        Some(
            hill_list.iter().map(|&(r, c)| norm((r as f64, c as f64))).fold(f64::MAX, f64::min)
                - PAD
                - 1.0,
        )
    };
    let lat = sym::lattice(&t, &s);
    let mut cx = Ctx {
        rng: Rng(rng.next()),
        s: o.spacing,
        bases,
        hill_pts,
        hub,
        mids: lat.mids.clone(),
        corners: lat.corners.clone(),
        pitches: o.pitches.clone(),
        quarter: matches!(group, "c4" | "d4"),
        grid,
        steps: Vec::new(),
    };
    let water = match (slot.terrain.as_str(), family) {
        ("open", "colonnade") => open_colonnade(&mut cx),
        ("open", "lakes") => open_lakes(&mut cx),
        ("open", "circles") => open_circles(&mut cx),
        ("open", "rivers") => open_rivers(&mut cx),
        ("open", "stars") => open_stars(&mut cx),
        ("open", "groves") => open_groves(&mut cx),
        ("open", "pinwheel") => open_pinwheel(&mut cx),
        ("cave", "kaleido") => cave_kaleido(&mut cx),
        ("cave", "grotto") => cave_grotto(&mut cx),
        ("cave", "lagoon") => cave_lagoon(&mut cx),
        ("cave", "veins") => cave_veins(&mut cx),
        ("cave", "swirl") => cave_swirl(&mut cx),
        ("maze", "braid") => maze_braid(&mut cx),
        ("maze", "rings") => maze_rings(&mut cx),
        ("maze", "garden") => maze_garden(&mut cx),
        ("maze", "spiral") => maze_spiral(&mut cx),
        ("rooms", "grid") => rooms_grid(&mut cx),
        ("rooms", "fortress") => rooms_fortress(&mut cx),
        ("rooms", "dungeon") => rooms_dungeon(&mut cx),
        ("rooms", "halls") => rooms_halls(&mut cx),
        _ => return Err(format!("no family {family} for {}", slot.terrain)),
    }?;

    let origin = frame(&t, &s, &mut rng);
    let cells = (o.rows * o.cols) as f64 / p as f64;
    let per_seat = ((cells / 90.0).sqrt() * 4.0 + 3.0 * h as f64).round().clamp(8.0, 48.0) as u32;
    let bias = ["home", "contested", "uniform"][rng.below(3) as usize].to_string();
    let food = FoodPlan {
        per_seat,
        bootstrap: if h >= 3 { 2 } else { 3 },
        bias,
        sites: if rng.percent(50) { (per_seat / 4).max(1) } else { 0 },
        seed: rng.next() | 1,
    };
    Ok(Design {
        name: slot.name(),
        terrain: slot.terrain.clone(),
        family: family.to_string(),
        rows: o.rows as u8,
        cols: o.cols as u8,
        seats: slot.seats,
        shift: [o.shift.0, o.shift.1],
        origin: [origin.0, origin.1],
        symmetry: group.to_string(),
        water,
        steps: cx.steps,
        hills: hill_list.iter().map(|&(r, c)| [r, c]).collect(),
        food,
    })
}

/// Where seat 0's centre goes on the page: wherever leaves every centre furthest from the edges,
/// so no base is drawn cut in two. The game is on a torus and does not care.
fn frame(t: &Torus, s: &Shift, rng: &mut Rng) -> (i32, i32) {
    let mut best = (-1, Vec::new());
    for r0 in 0..t.rows {
        for c0 in 0..t.cols {
            let m = (0..s.seats as i32)
                .map(|k| {
                    let (r, c) =
                        ((r0 + k * s.dr).rem_euclid(t.rows), (c0 + k * s.dc).rem_euclid(t.cols));
                    r.min(t.rows - 1 - r).min(c).min(t.cols - 1 - c)
                })
                .min()
                .unwrap_or(0);
            if m > best.0 {
                best = (m, vec![(r0, c0)]);
            } else if m == best.0 {
                best.1.push((r0, c0));
            }
        }
    }
    best.1[rng.below(best.1.len() as u64) as usize]
}

/// Seat 0's hills, from its centre, as orbits of the point group: one on the centre when the count
/// is odd, the rest spread across the seat's territory.
///
/// **A seat's hills are spread, never clustered.** A second hill in the same room as the first is
/// one hill with two doors: it takes the same raid to reach, falls to the same attack, and grows
/// the colony from the same corner. So each is at least two fifths of the way to a neighbour from
/// every other, and each stays inside its own seat's territory with a margin, so a hill is never
/// nearer an enemy's centre than its own. And no enemy hill is in view at turn zero.
fn hills(
    rng: &mut Rng,
    t: &Torus,
    s: &Shift,
    group: &str,
    h: usize,
    spacing: f64,
    grid: Option<i32>,
) -> Option<Vec<Vec<(i32, i32)>>> {
    let ops = sym::group(group)?;
    let mut orbits: Vec<Vec<(i32, i32)>> = Vec::new();
    let mut all: Vec<(i32, i32)> = Vec::new();
    // An odd count stands one hill on the centre, or -- under a mirror, whose axis holds points
    // of their own -- leaves the centre empty and makes a triangle.
    let centre = h % 2 == 1 && (rng.percent(55) || ops.len() > 2);
    if centre {
        orbits.push(vec![(0, 0)]);
        all.push((0, 0));
    }
    let sep = (0.4 * spacing).max((2 * CLEARING + 5) as f64);
    let lo = if centre { 0.32 } else { 0.22 } * spacing;
    let hi = 0.42 * spacing;
    let p = s.seats as i32;
    let mut centres: Vec<(f64, f64)> = Vec::new();
    for k in 0..p {
        for a in -1..=1 {
            for b in -1..=1 {
                if (k, a, b) != (0, 0, 0) {
                    centres.push(((k * s.dr + a * t.rows) as f64, (k * s.dc + b * t.cols) as f64));
                }
            }
        }
    }
    let f = |v: (i32, i32)| (v.0 as f64, v.1 as f64);
    let mut tries = 0;
    while all.len() < h && tries < 80 {
        tries += 1;
        let u = match grid {
            Some(q) => {
                // A room's centre, far enough out.
                let d = [(0, 1), (1, 1), (1, 0), (1, -1)][rng.below(4) as usize];
                let ks: Vec<i32> = (1..=8)
                    .filter(|k| {
                        let r = norm(((k * q * d.0) as f64, (k * q * d.1) as f64));
                        r >= lo && r <= hi
                    })
                    .collect();
                if ks.is_empty() {
                    continue;
                }
                let k = ks[rng.below(ks.len() as u64) as usize];
                (k * q * d.0, k * q * d.1)
            }
            None => {
                let rho = uni(rng, lo, hi);
                let a = match rng.below(5) {
                    0 => 0.0,
                    1 => 45.0,
                    2 => 90.0,
                    _ => uni(rng, 0.0, 360.0),
                }
                .to_radians();
                ((rho * a.sin()).round() as i32, (rho * a.cos()).round() as i32)
            }
        };
        if u == (0, 0) {
            continue;
        }
        let mut orbit: Vec<(i32, i32)> = ops.iter().map(|m| sym::apply(m, u)).collect();
        orbit.sort_unstable();
        orbit.dedup();
        orbit.retain(|&v| v != u);
        orbit.insert(0, u);
        if all.len() + orbit.len() > h {
            continue;
        }
        let apart = orbit
            .iter()
            .enumerate()
            .all(|(i, &a)| all.iter().chain(orbit[..i].iter()).all(|&b| dist(f(a), f(b)) >= sep));
        let inside = orbit.iter().all(|&a| {
            let own = norm(f(a));
            centres.iter().all(|&c| own + 0.16 * spacing <= dist(f(a), c))
        });
        if apart && inside {
            all.extend(orbit.iter().copied());
            orbits.push(orbit);
        }
    }
    if all.len() != h {
        return None;
    }
    // No enemy hill in view: seat 0's hills against every other seat's.
    for &a in &all {
        for &b in &all {
            for k in 1..p {
                let (r, c) = (b.0 + k * s.dr - a.0, b.1 + k * s.dc - a.1);
                let (r, c) = (r.rem_euclid(t.rows), c.rem_euclid(t.cols));
                let (r, c) = (r.min(t.rows - r) as i64, c.min(t.cols - c) as i64);
                if r * r + c * c <= VIEW_RADIUS2 + 8 {
                    return None;
                }
            }
        }
    }
    Some(orbits)
}

// ------------------------------------------------------------------------------------------ shared

/// A gate through a feature at a lattice site, cut along the line from the centre to the site, so
/// every seat walks in from its own side.
fn site_gate(cx: &mut Ctx, site: (f64, f64), from: f64, to: f64, w: f64) {
    let u = unit(site);
    let a = pt(site.0 - u.0 * from, site.1 - u.1 * from);
    let b = pt(site.0 - u.0 * to, site.1 - u.1 * to);
    {
        let st = seg("land", a, b, w);
        cx.push(st);
    }
}

/// The direction from seat 0's centre to a point, as a unit vector.
fn unit(v: (f64, f64)) -> (f64, f64) {
    let d = norm(v).max(1e-9);
    (v.0 / d, v.1 / d)
}

/// Water at every site in `sites`, as large as `r` wherever that leaves the hills their ground.
fn site_water(cx: &mut Ctx, sites: &[(f64, f64)], r: f64, metric: &str) -> Option<f64> {
    let fit = sites.iter().map(|&q| cx.clearance(q)).fold(r, f64::min);
    if fit < 1.0 {
        return None;
    }
    for q in sites {
        {
            let st = disk("water", Ctx::at(*q), fit, metric);
            cx.push(st);
        }
    }
    Some(fit)
}

/// Something at every corner where territories meet, or at every border's middle: the contested
/// ground, marked the same way everywhere.
fn site_features(cx: &mut Ctx) {
    let s = cx.s;
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    match cx.rng.below(5) {
        0 => {
            let r = (cx.u(0.07, 0.11) * s).max(1.5);
            let m = cx.metric(&["l2", "oct", "l1"]);
            site_water(cx, &corners, r, &m);
        }
        1 => {
            let r = (cx.u(0.05, 0.08) * s).max(1.2);
            let m = cx.metric(&["l2", "linf", "l1"]);
            site_water(cx, &mids, r, &m);
        }
        2 => {
            // A pond with an island, reached by one causeway from each side.
            let r = (cx.u(0.12, 0.18) * s).max(4.0);
            let m = cx.metric(&["l2", "oct"]);
            if let Some(r) = site_water(cx, &corners, r, &m) {
                for c in &corners {
                    {
                        let st = disk("land", Ctx::at(*c), (r * 0.45).max(1.5), &m);
                        cx.push(st);
                    }
                }
                for c in &corners {
                    site_gate(cx, *c, 0.0, r + 1.5, 2.0);
                }
            }
        }
        3 => {
            // Standing stones: a ring of pillars round each corner.
            let r = (cx.u(0.08, 0.13) * s).max(3.0);
            let r = corners.iter().map(|&q| cx.clearance(q)).fold(r, f64::min);
            if r >= 2.5 {
                let n = if r > 5.0 { 8 } else { 6 };
                for c in &corners {
                    for i in 0..n {
                        let a = (i as f64 * 360.0 / n as f64).to_radians();
                        {
                            let st =
                                disk("water", pt(c.0 + r * a.sin(), c.1 + r * a.cos()), 0.7, "l2");
                            cx.push(st);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// A point on the board clear of every hill's ground, drawn `rho` from the centre, or nothing.
fn clear_point(cx: &mut Ctx, lo: f64, hi: f64, room: f64) -> Option<[f64; 2]> {
    for _ in 0..20 {
        let rho = cx.u(lo, hi);
        let a = cx.u(0.0, 360.0);
        let q = polar(rho, a);
        if cx.clearance((q[0], q[1])) >= room {
            return Some(q);
        }
    }
    None
}

// ------------------------------------------------------------------------------------------ open

/// Pillars round every hill, in one ring or two, and a hall of them across the borders.
fn open_colonnade(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let size = [0.5, 1.0, 1.0, 1.5][cx.rng.below(4) as usize];
    let m = cx.metric(&["l2", "linf", "l1"]);
    let mut outer: f64 = PAD;
    for b in cx.bases.clone() {
        let mut r = PAD + cx.u(1.0, 2.0);
        let mut a = cx.angle();
        let rings = cx.int(1, 3);
        for _ in 0..rings {
            if r + size + 1.0 > b.room {
                break;
            }
            let n = cx.int(1, 3);
            let spread = cx.u(10.0, 22.0);
            for i in 0..n {
                {
                    let st = disk("water", add(b.at, polar(r, a + i as f64 * spread)), size, &m);
                    cx.push(st);
                }
            }
            outer = outer.max(r + size);
            r += cx.u(3.5, 5.5) + size;
            a += cx.u(8.0, 30.0);
        }
    }
    if let Some(hr) = cx.hub
        && hr > 3.0
        && cx.pct(60)
    {
        {
            let st = disk("water", [0.0, 0.0], (hr * 0.35).clamp(1.0, 2.5), &m);
            cx.push(st);
        }
    }
    if let Some(q) = cx.pitch_in(4, 8) {
        let st = Step {
            op: "dots".into(),
            pitch: q as u32,
            r: [[0.5, 0.5, 1.0][cx.rng.below(3) as usize], 0.0],
            metric: cx.metric(&["l2", "linf"]),
            band: Some([(outer + 1.5) / s, 3.0]),
            from: "hills".into(),
            ..Default::default()
        };
        cx.push(st);
    } else {
        site_features(cx);
    }
    Ok(false)
}

/// Lakes where territories meet, sometimes with an island and its causeways, and one on the
/// centre between a seat's hills.
fn open_lakes(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    let r = cx.u(0.14, 0.24) * s;
    let m = cx.metric(&["l2", "oct", "l2", "l1"]);
    if let Some(r) = site_water(cx, &corners, r, &m)
        && cx.pct(55)
        && r > 5.0
    {
        let ri = (r * cx.u(0.35, 0.5)).max(2.0);
        let bw = cx.u(2.0, 3.0);
        for c in &corners {
            {
                let st = disk("land", Ctx::at(*c), ri, &m);
                cx.push(st);
            }
        }
        for c in &corners {
            site_gate(cx, *c, 0.0, r + 1.5, bw);
        }
    }
    if cx.pct(55) {
        let rm = (cx.u(0.05, 0.1) * s).max(1.2);
        let mm = cx.metric(&["l2", "l1"]);
        site_water(cx, &mids, rm, &mm);
    }
    if let Some(hr) = cx.hub
        && hr > 3.0
        && cx.pct(70)
    {
        {
            let st = disk("water", [0.0, 0.0], hr * cx.u(0.5, 0.85), &m);
            cx.push(st);
        }
    }
    let rocks = cx.int(1, 3);
    for _ in 0..rocks {
        let sz = cx.u(0.8, 2.0);
        if let Some(q) = clear_point(cx, 0.1 * s, 0.45 * s, sz) {
            let mt = cx.metric(&["l2", "linf", "l1"]);
            {
                let st = disk("water", q, sz, &mt);
                cx.push(st);
            }
        }
    }
    Ok(false)
}

/// Crop circles: thin rings with gaps round every hill, and round where territories meet.
fn open_circles(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let m = cx.metric(&["l2", "l2", "oct"]);
    for b in cx.bases.clone() {
        let mut r = PAD + cx.u(0.3, 1.5);
        let mut a = cx.inward(&b);
        let rings = cx.int(1, 2);
        for _ in 0..rings {
            if r + 1.5 > b.room {
                break;
            }
            {
                let st = ring("water", Ctx::at(b.at), r, r + 0.8, &m);
                cx.push(st);
            }
            let gates = cx.int(1, 2);
            for g in 0..gates {
                cx.gate(b.at, r - 1.5, r + 2.5, a + g as f64 * 180.0, 3.0);
            }
            a += cx.u(30.0, 90.0);
            r += cx.u(3.0, 4.5);
        }
    }
    if let Some(hr) = cx.hub
        && hr > 4.0
    {
        {
            let st = ring("water", [0.0, 0.0], hr * 0.7, hr * 0.7 + 0.8, &m);
            cx.push(st);
        }
        let a = cx.angle();
        cx.gate((0.0, 0.0), hr * 0.7 - 1.5, hr * 0.7 + 2.5, a, 3.0);
    }
    let corners = cx.corners.clone();
    let mut rc = cx.u(1.5, 2.5);
    let outer = {
        let r0 = (cx.u(0.14, 0.22) * s).max(4.0);
        corners.iter().map(|&q| cx.clearance(q)).fold(r0, f64::min)
    };
    if cx.pct(50) && outer > 1.5 {
        for c in &corners {
            {
                let st = disk("water", Ctx::at(*c), 1.0, "l2");
                cx.push(st);
            }
        }
    }
    while rc + 0.8 <= outer {
        for c in &corners {
            {
                let st = ring("water", Ctx::at(*c), rc, rc + 0.8, &m);
                cx.push(st);
            }
        }
        for c in &corners {
            site_gate(cx, *c, rc - 1.0, rc + 1.8, 3.0);
        }
        rc += cx.u(2.6, 3.6);
    }
    Ok(false)
}

/// Stars where territories meet, and dashed rays out of every hill.
fn open_stars(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let corners = cx.corners.clone();
    let arms = [4, 6, 8][cx.rng.below(3) as usize];
    let w = cx.u(1.4, 2.4);
    let len = {
        let r0 = (cx.u(0.12, 0.2) * s).max(4.0);
        corners.iter().map(|&q| cx.clearance(q)).fold(r0, f64::min)
    };
    let turn = cx.u(0.0, 45.0);
    if len >= 2.5 {
        for c in &corners {
            {
                let st = disk("water", Ctx::at(*c), (w * 0.9).max(1.2), "l2");
                cx.push(st);
            }
            for i in 0..arms {
                let a = (turn + i as f64 * 360.0 / arms as f64).to_radians();
                {
                    let st =
                        seg("water", Ctx::at(*c), pt(c.0 + len * a.sin(), c.1 + len * a.cos()), w);
                    cx.push(st);
                }
            }
        }
    }
    let rw = [1.0, 1.0, 1.6][cx.rng.below(3) as usize];
    for b in cx.bases.clone() {
        let rays = cx.int(1, 3);
        let a0 = cx.inward(&b) + 180.0;
        let spread = cx.u(35.0, 70.0);
        for i in 0..rays {
            let a = a0 + (i as f64 - (rays - 1) as f64 / 2.0) * spread;
            let mut rho = PAD + cx.u(1.0, 2.0);
            let dash = cx.u(2.0, 3.5);
            let gap = cx.u(1.5, 2.5);
            while rho + dash < b.room + 2.0 {
                {
                    let st =
                        seg("water", add(b.at, polar(rho, a)), add(b.at, polar(rho + dash, a)), rw);
                    cx.push(st);
                }
                rho += dash + gap;
            }
        }
    }
    Ok(false)
}

/// Groves: clumps drawn from a symmetric field, kept off every hill's ground.
fn open_groves(cx: &mut Ctx) -> Result<bool, String> {
    let seed = cx.seed();
    let st = Step {
        op: "noise".into(),
        pct: cx.int(14, 24),
        scale: cx.int(2, 4),
        smooth: cx.int(1, 3),
        band: Some([(PAD + 2.0) / cx.s, 3.0]),
        from: "hills".into(),
        seed,
        ..Default::default()
    };
    cx.push(st);
    Ok(false)
}

/// Rivers along the borders, winding, with a bridge at every border's middle.
fn open_rivers(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let w = [2.0, 3.0, 3.0, 4.0][cx.rng.below(4) as usize];
    let seed = cx.seed();
    let st = Step {
        op: "border".into(),
        width: w,
        scale: cx.int(3, 6),
        r: [round2(cx.u(1.0, (0.06 * s).max(1.5))), 0.0],
        seed,
        ..Default::default()
    };
    cx.push(st);
    let mids = cx.mids.clone();
    let bw = cx.u(2.5, 4.0);
    let reach = w + 3.0 + (0.06 * s).max(1.5) * 2.0;
    for m in &mids {
        site_gate(cx, *m, -reach, reach, bw);
    }
    if cx.pct(50) {
        let corners = cx.corners.clone();
        let r = (cx.u(0.08, 0.13) * s).max(2.5);
        site_water(cx, &corners, r, "l2");
    }
    if let Some(hr) = cx.hub
        && hr > 3.0
        && cx.pct(50)
    {
        {
            let st = disk("water", [0.0, 0.0], hr * cx.u(0.4, 0.7), "l2");
            cx.push(st);
        }
    }
    Ok(false)
}

/// One arm of a spiral from radius `r0` to `r1` about `c`, turning `twist` degrees on the way, as
/// the points of a polyline.
fn arm(c: (f64, f64), r0: f64, r1: f64, a0: f64, twist: f64, n: usize) -> Vec<[f64; 2]> {
    (0..=n)
        .map(|i| {
            let f = i as f64 / n as f64;
            add(c, polar(r0 + (r1 - r0) * f, a0 + twist * f))
        })
        .collect()
}

/// How many arms a spiral round `b` draws itself: a base on the centre gets its others from the
/// point group; one off it has only its own.
fn arms_for(cx: &mut Ctx, b: &Base) -> (u32, f64) {
    if norm(b.at) < 0.5 {
        let n = if cx.quarter { cx.int(1, 2) } else { cx.int(2, 3) };
        (n, if cx.quarter { 90.0 } else { 180.0 } / n as f64)
    } else {
        let n = cx.int(2, 3);
        (n, 360.0 / n as f64)
    }
}

/// A pinwheel of dashed arms out of every hill: stepping stones that turn.
fn open_pinwheel(cx: &mut Ctx) -> Result<bool, String> {
    let twist = cx.u(50.0, 110.0) * if cx.pct(50) { 1.0 } else { -1.0 };
    let w = [1.0, 1.4, 2.0][cx.rng.below(3) as usize];
    let dashed = cx.pct(70);
    for b in cx.bases.clone() {
        let (n, spread) = arms_for(cx, &b);
        let a0 = cx.angle();
        for k in 0..n {
            let pts = arm(b.at, PAD + 1.0, b.room + 1.5, a0 + k as f64 * spread, twist, 12);
            for (i, pair) in pts.windows(2).enumerate() {
                if !dashed || i % 2 == 0 {
                    {
                        let st = seg("water", pair[0], pair[1], w);
                        cx.push(st);
                    }
                }
            }
        }
    }
    if cx.pct(50) {
        let corners = cx.corners.clone();
        let r = (cx.u(0.05, 0.09) * cx.s).max(1.5);
        site_water(cx, &corners, r, "l2");
    }
    Ok(false)
}

// ------------------------------------------------------------------------------------------ cave

/// A hall carved round every hill, as large as its room allows.
fn cave_halls(cx: &mut Ctx, lo: f64, hi: f64) {
    for b in cx.bases.clone() {
        let r = (PAD + cx.u(lo, hi)).min(b.room * 0.85).max(PAD - 0.5);
        {
            let st = disk("land", Ctx::at(b.at), r, "l2");
            cx.push(st);
        }
    }
}

/// A symmetric field, cut at a level and smoothed: caverns that look like a kaleidoscope, with a
/// hall round every hill.
fn cave_kaleido(cx: &mut Ctx) -> Result<bool, String> {
    let seed = cx.seed();
    let st = Step {
        op: "noise".into(),
        pct: cx.int(42, 54),
        scale: if cx.s < 24.0 { 1 } else { cx.int(1, 3) },
        smooth: cx.int(3, 5),
        seed,
        ..Default::default()
    };
    cx.push(st);
    cave_halls(cx, 0.5, 2.0);
    let corners = cx.corners.clone();
    if cx.pct(60) {
        let r = (cx.u(0.08, 0.14) * cx.s).max(2.5);
        for c in &corners {
            {
                let st = disk("land", Ctx::at(*c), r, "l2");
                cx.push(st);
            }
        }
    }
    {
        let st = Step { op: "smooth".into(), smooth: 1, ..Default::default() };
        cx.push(st);
    }
    Ok(false)
}

/// Chambers carved from rock — every hill, every corner, the centre between a seat's hills — and
/// the tunnels between them, roughened.
fn cave_grotto(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    cave_halls(cx, 0.5, 2.5);
    let tw = cx.tunnel();
    if let Some(hr) = cx.hub {
        {
            let st = disk("land", [0.0, 0.0], (hr * 0.5).clamp(2.0, 0.1 * s + 2.0), "l2");
            cx.push(st);
        }
        for b in cx.bases.clone() {
            {
                let st = seg("land", [0.0, 0.0], Ctx::at(b.at), tw * 0.85);
                cx.push(st);
            }
        }
    }
    let rc = (cx.u(0.08, 0.15) * s).max(2.5);
    for c in &corners {
        {
            let st = disk("land", Ctx::at(*c), rc, "l2");
            cx.push(st);
        }
    }
    let nearest = |q: (f64, f64), set: &[(f64, f64)]| {
        *set.iter().min_by(|a, b| dist(q, **a).total_cmp(&dist(q, **b))).unwrap_or(&(0.0, 0.0))
    };
    if cx.pct(60) {
        let rooms = cx.pct(60);
        let rm = (cx.u(0.05, 0.1) * s).max(2.0);
        for m in &mids {
            {
                let st = seg("land", [0.0, 0.0], Ctx::at(*m), tw);
                cx.push(st);
            }
            if rooms {
                {
                    let st = disk("land", Ctx::at(*m), rm, "l2");
                    cx.push(st);
                }
            }
        }
        for c in &corners {
            let m = nearest(*c, &mids);
            {
                let st = seg("land", Ctx::at(m), Ctx::at(*c), tw * 0.85);
                cx.push(st);
            }
        }
    } else {
        for c in &corners {
            {
                let st = seg("land", [0.0, 0.0], Ctx::at(*c), tw);
                cx.push(st);
            }
        }
    }
    if cx.pct(50) {
        // Each hill's own way out, to its nearest border.
        for b in cx.bases.clone() {
            if norm(b.at) > 0.5 {
                let m = nearest(b.at, &mids);
                {
                    let st = seg("land", Ctx::at(b.at), Ctx::at(m), tw * 0.8);
                    cx.push(st);
                }
            }
        }
    }
    {
        let st = Step { op: "smooth".into(), smooth: cx.int(1, 2), ..Default::default() };
        cx.push(st);
    }
    let seed = cx.seed();
    let st = Step {
        op: "noise".into(),
        pct: cx.int(6, 12),
        scale: 1,
        smooth: 1,
        band: Some([(PAD + 1.0) / s, 3.0]),
        from: "hills".into(),
        seed,
        ..Default::default()
    };
    cx.push(st);
    Ok(true)
}

/// Organic lakes off every hill's ground, crossed by causeways.
fn cave_lagoon(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let seed = cx.seed();
    let st = Step {
        op: "noise".into(),
        pct: cx.int(40, 55),
        scale: if s < 24.0 { cx.int(1, 2) } else { cx.int(2, 5) },
        smooth: cx.int(3, 5),
        band: Some([(PAD + 2.0) / s, 3.0]),
        from: "hills".into(),
        seed,
        ..Default::default()
    };
    cx.push(st);
    let mids = cx.mids.clone();
    let corners = cx.corners.clone();
    let w = cx.u(2.0, 3.0);
    let to = if cx.pct(60) { mids } else { corners.clone() };
    for m in &to {
        {
            let st = seg("land", [0.0, 0.0], Ctx::at(*m), w);
            cx.push(st);
        }
    }
    for b in cx.bases.clone() {
        if norm(b.at) > 0.5 {
            {
                let st = seg("land", [0.0, 0.0], Ctx::at(b.at), w);
                cx.push(st);
            }
        }
    }
    if cx.pct(50) {
        let r = (cx.u(0.05, 0.09) * s).max(2.0);
        for c in &corners {
            {
                let st = disk("land", Ctx::at(*c), r, "l2");
                cx.push(st);
            }
        }
    }
    Ok(false)
}

/// The contour lines of a symmetric field: walls that wind like marble.
fn cave_veins(cx: &mut Ctx) -> Result<bool, String> {
    let seed = cx.seed();
    let st = Step {
        op: "ridge".into(),
        pct: cx.int(16, 26),
        scale: cx.int(3, 6),
        smooth: cx.int(0, 1),
        seed,
        ..Default::default()
    };
    cx.push(st);
    if cx.pct(50) {
        let seed2 = cx.seed();
        let st = Step {
            op: "noise".into(),
            pct: cx.int(8, 16),
            scale: cx.int(2, 4),
            smooth: 2,
            band: Some([(PAD + 2.0) / cx.s, 3.0]),
            from: "hills".into(),
            seed: seed2,
            ..Default::default()
        };
        cx.push(st);
    }
    cave_halls(cx, 0.0, 1.5);
    Ok(false)
}

/// Thick spiral arms of rock out of the centre, roughened into a cave, a hall at every hill.
fn cave_swirl(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let arms = if cx.quarter { cx.int(1, 2) } else { cx.int(2, 3) };
    let twist = cx.u(60.0, 140.0) * if cx.pct(50) { 1.0 } else { -1.0 };
    let w = cx.u(3.0, (0.12 * s).max(3.5));
    let a0 = cx.angle();
    let spread = if cx.quarter { 90.0 / arms as f64 } else { 180.0 / arms as f64 };
    let r0 = if cx.hub.is_some() { w / 2.0 + 1.0 } else { PAD + w / 2.0 + 1.0 };
    for k in 0..arms {
        let pts = arm((0.0, 0.0), r0, 0.55 * s, a0 + k as f64 * spread, twist, 14);
        for pair in pts.windows(2) {
            {
                let st = seg("water", pair[0], pair[1], w);
                cx.push(st);
            }
        }
    }
    let seed = cx.seed();
    let st = Step {
        op: "noise".into(),
        pct: cx.int(10, 20),
        scale: cx.int(1, 2),
        smooth: 2,
        band: Some([(PAD + 2.0) / s, 3.0]),
        from: "hills".into(),
        seed,
        ..Default::default()
    };
    cx.push(st);
    {
        let st = Step { op: "smooth".into(), smooth: 2, ..Default::default() };
        cx.push(st);
    }
    cave_halls(cx, 0.0, 1.0);
    Ok(false)
}

// ------------------------------------------------------------------------------------------ maze

fn maze_plazas(cx: &mut Ctx) {
    let m = cx.metric(&["l2", "oct", "linf"]);
    for b in cx.bases.clone() {
        let r = (PAD + cx.u(0.5, 1.5)).min(b.room * 0.85);
        {
            let st = disk("land", Ctx::at(b.at), r, &m);
            cx.push(st);
        }
    }
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    if cx.pct(60) {
        let r = cx.u(2.5, 4.5);
        let fountain = cx.pct(50);
        for c in &corners {
            {
                let st = disk("land", Ctx::at(*c), r, &m);
                cx.push(st);
            }
        }
        if fountain && corners.iter().all(|&c| cx.clearance(c) >= 1.0) {
            for c in &corners {
                {
                    let st = disk("water", Ctx::at(*c), 1.0, "l2");
                    cx.push(st);
                }
            }
        }
    }
    if cx.pct(30) {
        let r = cx.u(2.0, 3.5);
        for c in &mids {
            {
                let st = disk("land", Ctx::at(*c), r, &m);
                cx.push(st);
            }
        }
    }
    if let Some(hr) = cx.hub
        && hr > 3.0
        && cx.pct(40)
    {
        {
            let st = disk("land", [0.0, 0.0], (hr * 0.6).max(2.0), &m);
            cx.push(st);
        }
    }
}

/// Corridors wider than a maze's, walls long enough to read as walls: few loops, dead ends
/// braided shut.
fn maze_braid(cx: &mut Ctx) -> Result<bool, String> {
    let q = cx.pitch_in(6, 12).ok_or("braid: no pitch")?;
    let walls: Vec<i32> =
        [1, 2, 3].into_iter().filter(|w| (q - w) % 2 == 1 && q - w >= 3).collect();
    let wall = walls[cx.rng.below(walls.len() as u64) as usize];
    let seed = cx.seed();
    let st = Step {
        op: "maze".into(),
        pitch: q as u32,
        corridor: (q - wall) as u32,
        loops: cx.int(8, 25),
        braid: cx.int(60, 100),
        seed,
        ..Default::default()
    };
    cx.push(st);
    maze_plazas(cx);
    Ok(false)
}

/// A formal garden: a hedge maze across the borders, a hedged court round every hill, fountains
/// where territories meet.
fn maze_garden(cx: &mut Ctx) -> Result<bool, String> {
    let q = cx.pitch_in(5, 10).ok_or("garden: no pitch")?;
    let wall = if (q - 1) % 2 == 1 { 1 } else { 2 };
    let s = cx.s;
    let inner = PAD + cx.u(2.5, 4.0);
    let seed = cx.seed();
    let st = Step {
        op: "maze".into(),
        pitch: q as u32,
        corridor: (q - wall) as u32,
        loops: cx.int(15, 35),
        braid: cx.int(70, 100),
        band: Some([inner / s, 3.0]),
        from: "hills".into(),
        seed,
        ..Default::default()
    };
    cx.push(st);
    let m = cx.metric(&["l2", "oct", "linf"]);
    for b in cx.bases.clone() {
        let r = (inner - cx.u(1.0, 1.5)).min(b.room - 1.0);
        if r > PAD {
            {
                let st = ring("water", Ctx::at(b.at), r, r + 1.0, &m);
                cx.push(st);
            }
            let a = cx.inward(&b);
            cx.gate(b.at, r - 1.0, r + 2.5, a, 3.0);
            if cx.pct(50) {
                cx.gate(b.at, r - 1.0, r + 2.5, a + 180.0, 3.0);
            }
        }
    }
    let corners = cx.corners.clone();
    let rc = cx.u(3.0, 4.5);
    for c in &corners {
        {
            let st = disk("land", Ctx::at(*c), rc, &m);
            cx.push(st);
        }
    }
    let f = cx.u(1.0, 1.8);
    site_water(cx, &corners, f, "l2");
    Ok(false)
}

/// A labyrinth of rings round every hill, gates staggered so the way in winds.
fn maze_rings(cx: &mut Ctx) -> Result<bool, String> {
    let m = cx.metric(&["l2", "oct", "linf", "l2", "l1"]);
    let gap = cx.u(3.0, 4.5);
    let w = [1.0, 1.0, 1.5][cx.rng.below(3) as usize];
    let limit = if m == "linf" { 0.8 } else { 1.0 };
    for b in cx.bases.clone() {
        let mut r = PAD + cx.u(0.3, 1.2);
        let mut a = if m == "l1" { 45.0 } else { cx.inward(&b) };
        let mut k = 0;
        while r + w < (b.room + 1.5) * limit && k < 6 {
            {
                let st = ring("water", Ctx::at(b.at), r, r + w, &m);
                cx.push(st);
            }
            let deg = a + if k % 2 == 0 { 0.0 } else { 180.0 };
            cx.gate(b.at, r - 1.0, r + w + 1.0, deg, 3.0);
            if cx.pct(35) {
                cx.gate(b.at, r - 1.0, r + w + 1.0, deg + 90.0, 3.0);
            }
            if k > 0 && cx.pct(60) {
                let sd = deg + 90.0;
                {
                    let st =
                        seg("water", add(b.at, polar(r - gap + w, sd)), add(b.at, polar(r, sd)), w);
                    cx.push(st);
                }
            }
            r += gap + w;
            a += cx.u(-8.0, 8.0);
            k += 1;
        }
    }
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    match cx.rng.below(3) {
        0 => {
            {
                let st = Step {
                    op: "border".into(),
                    width: cx.u(1.0, 2.0).round(),
                    ..Default::default()
                };
                cx.push(st);
            }
            for mm in &mids {
                site_gate(cx, *mm, -3.0, 3.0, 3.0);
            }
        }
        1 => {
            let rr = {
                let r0 = cx.u(2.5, 4.0);
                corners.iter().map(|&q| cx.clearance(q)).fold(r0, f64::min)
            };
            if rr >= 2.0 {
                for c in &corners {
                    {
                        let st = ring("water", Ctx::at(*c), rr, rr + 1.0, &m);
                        cx.push(st);
                    }
                }
                for c in &corners {
                    site_gate(cx, *c, rr - 1.0, rr + 2.0, 3.0);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Spiral walls out of every hill, broken often enough that no corridor between two arms is a
/// dead end for long.
fn maze_spiral(cx: &mut Ctx) -> Result<bool, String> {
    let twist = cx.u(90.0, 200.0) * if cx.pct(50) { 1.0 } else { -1.0 };
    let w = [1.0, 1.0, 1.5][cx.rng.below(3) as usize];
    for b in cx.bases.clone() {
        let (n, spread) = arms_for(cx, &b);
        let a0 = cx.angle();
        for k in 0..n {
            let pts = arm(b.at, PAD + 0.5, b.room * 1.05 + 1.0, a0 + k as f64 * spread, twist, 16);
            for (i, pair) in pts.windows(2).enumerate() {
                if i % 5 != 4 {
                    {
                        let st = seg("water", pair[0], pair[1], w);
                        cx.push(st);
                    }
                }
            }
        }
    }
    let corners = cx.corners.clone();
    if cx.pct(60) {
        let r = cx.u(2.5, 4.0);
        for c in &corners {
            {
                let st = disk("land", Ctx::at(*c), r, "l2");
                cx.push(st);
            }
        }
    }
    Ok(false)
}

// ------------------------------------------------------------------------------------------ rooms

fn wall_for(q: i32, rng: &mut Rng) -> i32 {
    // Parity opposite to the pitch's.
    let opts: Vec<i32> = [1, 2, 3].into_iter().filter(|w| (w + q) % 2 == 1).collect();
    opts[rng.below(opts.len() as u64) as usize]
}

/// Rooms on a lattice, a few walls down, a door in enough of the rest; each hill in a room of its
/// own.
fn rooms_grid(cx: &mut Ctx) -> Result<bool, String> {
    let q = cx.grid.ok_or("grid: no pitch")?;
    let wall = wall_for(q, &mut cx.rng);
    let door = if q - wall >= 9 && cx.pct(40) { 5 } else { 3 };
    let seed = cx.seed();
    let st = Step {
        op: "rooms".into(),
        pitch: q as u32,
        corridor: wall as u32,
        door,
        pct: cx.int(5, 22),
        loops: cx.int(15, 45),
        seed,
        ..Default::default()
    };
    cx.push(st);
    Ok(false)
}

/// Large rooms with a pillar in each quarter: halls.
fn rooms_halls(cx: &mut Ctx) -> Result<bool, String> {
    let q = cx.grid.ok_or("halls: no pitch")?;
    let wall = [1, 3][cx.rng.below(2) as usize];
    let seed = cx.seed();
    let st = Step {
        op: "rooms".into(),
        pitch: q as u32,
        corridor: wall as u32,
        door: if q - wall >= 9 { 5 } else { 3 },
        pct: cx.int(5, 25),
        loops: cx.int(20, 50),
        seed,
        ..Default::default()
    };
    cx.push(st);
    let st = Step {
        op: "dots".into(),
        pitch: (q / 2) as u32,
        at: [(q / 4) as f64, (q / 4) as f64],
        r: [if q >= 12 { 1.0 } else { 0.5 }, 0.0],
        metric: "l2".into(),
        band: Some([(PAD + 0.5) / cx.s, 3.0]),
        from: "hills".into(),
        ..Default::default()
    };
    cx.push(st);
    Ok(false)
}

/// A keep round every hill, a curtain wall round the keep where there is room, realm walls
/// between the seats, outposts where territories meet.
fn rooms_fortress(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let m = cx.metric(&["linf", "oct", "linf", "l1"]);
    let wk = if s > 30.0 {
        [1.5, 2.0, 3.0][cx.rng.below(3) as usize]
    } else {
        [1.0, 1.5, 2.0][cx.rng.below(3) as usize]
    };
    let reach = match m.as_str() {
        "linf" => 0.72,
        "oct" => 0.9,
        _ => 1.0,
    };
    for b in cx.bases.clone() {
        let rk = PAD + cx.u(0.3, 1.2);
        if rk + wk > (b.room + 1.0) * reach {
            continue;
        }
        {
            let st = ring("water", Ctx::at(b.at), rk, rk + wk, &m);
            cx.push(st);
        }
        let a = cx.inward(&b);
        cx.gate(b.at, rk - 1.0, rk + wk + 1.0, a, 3.0);
        let ro = rk + wk + cx.u(3.5, 6.0);
        if ro + wk <= (b.room + 1.0) * reach {
            {
                let st = ring("water", Ctx::at(b.at), ro, ro + wk, &m);
                cx.push(st);
            }
            cx.gate(b.at, ro - 1.0, ro + wk + 1.0, a + 180.0, 3.0);
            if cx.pct(50) {
                cx.gate(b.at, ro - 1.0, ro + wk + 1.0, a + 90.0, 3.0);
            }
            if cx.pct(60) {
                let tr = cx.u(1.2, 2.2);
                let corner = if m == "linf" { ro * std::f64::consts::SQRT_2 } else { ro };
                let deg = if m == "l1" { 0.0 } else { 45.0 };
                {
                    let st =
                        disk("water", add(b.at, polar(corner + wk / 2.0, deg + a)), tr, "linf");
                    cx.push(st);
                }
            }
        }
    }
    if cx.pct(55) {
        let w = if s > 30.0 { 2.0 } else { 1.0 };
        {
            let st = Step { op: "border".into(), width: w, ..Default::default() };
            cx.push(st);
        }
        let mids = cx.mids.clone();
        for mm in &mids {
            site_gate(cx, *mm, -3.0, 3.0, 3.0);
        }
    }
    let corners = cx.corners.clone();
    if cx.pct(70) {
        let rr = {
            let r0 = cx.u(2.5, 4.0);
            corners.iter().map(|&q| cx.clearance(q)).fold(r0, f64::min)
        };
        if rr >= 2.0 {
            let mm = cx.metric(&["linf", "oct"]);
            for c in &corners {
                {
                    let st = ring("water", Ctx::at(*c), rr, rr + 1.0, &mm);
                    cx.push(st);
                }
            }
            for c in &corners {
                site_gate(cx, *c, rr - 1.0, rr + 2.0, 3.0);
            }
        }
    }
    Ok(false)
}

/// Rooms cut from rock and the corridors between them: a hall at every hill, a hub on the centre
/// joining them, rooms at the borders' middles and corners, each corner reached from its nearest
/// middle.
fn rooms_dungeon(cx: &mut Ctx) -> Result<bool, String> {
    let s = cx.s;
    let cw = (s * 0.1).clamp(2.0, 5.0).round();
    for b in cx.bases.clone() {
        let r = (PAD + cx.u(0.5, 2.5)).min(b.room * 0.85);
        if cx.pct(50) {
            {
                let st = boxed("land", Ctx::at(b.at), r, r);
                cx.push(st);
            }
        } else {
            {
                let st = disk("land", Ctx::at(b.at), r * 1.08, "oct");
                cx.push(st);
            }
        }
        if norm(b.at) > 0.5 {
            {
                let st = seg("land", [0.0, 0.0], Ctx::at(b.at), cw);
                cx.push(st);
            }
        }
    }
    if let Some(hr) = cx.hub {
        let r = (hr * cx.u(0.5, 0.8)).clamp(2.0, 0.16 * s + 2.0);
        {
            let st = boxed("land", [0.0, 0.0], r, r);
            cx.push(st);
        }
    }
    let corners = cx.corners.clone();
    let mids = cx.mids.clone();
    let mid_room = (cx.u(0.08, 0.13) * s).max(1.5);
    let rooms_at_mids = cx.pct(60);
    for mm in &mids {
        {
            let st = seg("land", [0.0, 0.0], Ctx::at(*mm), cw);
            cx.push(st);
        }
        if rooms_at_mids {
            {
                let st = boxed("land", Ctx::at(*mm), mid_room, mid_room);
                cx.push(st);
            }
        }
    }
    if cx.pct(75) {
        let rc = (cx.u(0.1, 0.16) * s).max(2.0);
        for c in &corners {
            {
                let st = boxed("land", Ctx::at(*c), rc, rc);
                cx.push(st);
            }
            if let Some(m) = mids.iter().min_by(|a, b| dist(*c, **a).total_cmp(&dist(*c, **b))) {
                {
                    let st = seg("land", Ctx::at(*m), Ctx::at(*c), cw);
                    cx.push(st);
                }
            }
        }
    }
    Ok(true)
}
