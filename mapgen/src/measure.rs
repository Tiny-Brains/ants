//! What a finished board is, in numbers — and the rules no recipe may bend, checked on the board
//! itself rather than trusted to the stages that built it.
//!
//! Everything here reads only what a map file holds (terrain, shift, hills, food), so `check` can
//! run it over a committed file exactly as `generate` ran it over a fresh board.

use serde::Serialize;

use crate::grid::Torus;
use crate::make::{Board, FAR, walk};
use crate::recipe::{MIN_CLEARING, VIEW_RADIUS2};

/// Routes are counted up to here: past it a board is simply open.
pub const ROUTES_CAP: u32 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Metrics {
    /// Water, per thousand squares.
    pub water_pm: u32,
    /// Square-disjoint routes from the ground around seat 0's first hill to the ground around the
    /// nearest enemy hill, capped at `ROUTES_CAP`. One is a single corridor; many is open country.
    pub routes: u32,
    /// The walk from seat 0's first hill to the nearest enemy hill.
    pub enemy_walk: u32,
    /// The same walk on a board with no water.
    pub open_walk: u32,
    pub detour_pct: u32,
    /// Land squares with at most one land neighbour, per thousand land squares.
    pub dead_end_pm: u32,
    /// Run-length pairs in the water mask.
    pub water_runs: u32,
    /// Land a seat's first ant can see from its hill at turn zero.
    pub home_view_land: u32,
    pub food_per_seat: u32,
}

/// Measure a board, refusing it if it breaks a rule every board obeys.
pub fn measure(b: &Board) -> Result<Metrics, String> {
    let (t, s) = (b.t, b.s);
    let (cells, p) = (t.cells(), s.seats);
    let water = |x: usize| b.water[x];

    if !s.is_exact(&t) {
        return Err(format!("shift ({}, {}) is not of order {p}", s.dr, s.dc));
    }
    if let Some(x) = (0..cells).find(|&x| b.water[x] != b.water[s.image(&t, x, 1)]) {
        return Err(format!("water at {:?} has no image for seat 1", t.rc(x)));
    }
    if b.hills.is_empty() || !b.hills.len().is_multiple_of(p) {
        return Err(format!("{} hills cannot be shared among {p} seats", b.hills.len()));
    }
    for (i, &h) in b.hills.iter().enumerate() {
        if h != s.image(&t, b.hills[i - i % p], i % p) {
            return Err(format!("hill {i} is not the image of its orbit's first"));
        }
        if b.water[h] {
            return Err(format!("hill {i} is under water"));
        }
        let exits = t.n4(h).iter().filter(|&&y| !b.water[y]).count();
        if exits < MIN_CLEARING as usize {
            return Err(format!("hill {i} at {:?} has {exits} way(s) off it", t.rc(h)));
        }
    }
    for (i, &a) in b.hills.iter().enumerate() {
        for (j, &c) in b.hills.iter().enumerate() {
            if i % p != j % p && t.dist2(a, c) <= VIEW_RADIUS2 {
                return Err(format!("hills {i} and {j} are in each other's view at turn zero"));
            }
        }
    }
    if !b.food.len().is_multiple_of(p) || b.food.iter().any(|&f| b.water[f] || b.hills.contains(&f))
    {
        return Err("food is not whole orbits on open land".into());
    }

    let d0 = walk(&t, water, &b.hills[..1]);
    let land = (0..cells).filter(|&x| !b.water[x]).count();
    if let Some(x) = (0..cells).find(|&x| !b.water[x] && d0[x] == FAR) {
        return Err(format!("land at {:?} cannot be walked to", t.rc(x)));
    }

    // Fairness, as identities: every seat's walks to the food, to its enemies' hills, and the land
    // nearer to it than to anyone else, are the same numbers as seat 0's. A shift guarantees this;
    // measuring it is what catches a stage that wrote half an orbit.
    let seat_hills =
        |k: usize| -> Vec<usize> { b.hills.iter().skip(k).step_by(p).copied().collect() };
    let dist: Vec<Vec<u32>> = (0..p).map(|k| walk(&t, water, &seat_hills(k))).collect();
    let sorted = |k: usize, of: &mut dyn Iterator<Item = usize>| -> Vec<u32> {
        let mut v: Vec<u32> = of.map(|x| dist[k][x]).collect();
        v.sort_unstable();
        v
    };
    let mut territory = vec![0usize; p];
    for x in (0..cells).filter(|&x| !b.water[x]) {
        let best = (0..p).map(|k| dist[k][x]).min().unwrap_or(FAR);
        let nearest: Vec<usize> = (0..p).filter(|&k| dist[k][x] == best).collect();
        if nearest.len() == 1 {
            territory[nearest[0]] += 1;
        }
    }
    for k in 1..p {
        let food = |k: usize| sorted(k, &mut b.food.iter().copied());
        let enemies = |k: usize| {
            sorted(k, &mut b.hills.iter().enumerate().filter(|(i, _)| i % p != k).map(|(_, &h)| h))
        };
        if food(k) != food(0) || enemies(k) != enemies(0) || territory[k] != territory[0] {
            return Err(format!("seat {k}'s board is not congruent to seat 0's"));
        }
    }

    let enemy = (0..b.hills.len())
        .filter(|i| i % p != 0)
        .min_by_key(|&i| (d0[b.hills[i]], i))
        .map(|i| b.hills[i])
        .unwrap_or(b.hills[0]);
    let enemy_walk = d0[enemy];
    let open_walk = t.manhattan(b.hills[0], enemy) as u32;
    let dead = (0..cells)
        .filter(|&x| !b.water[x] && t.n4(x).iter().filter(|&&y| !b.water[y]).count() <= 1)
        .count();
    let view =
        (0..cells).filter(|&x| !b.water[x] && t.dist2(x, b.hills[0]) <= VIEW_RADIUS2).count();

    Ok(Metrics {
        water_pm: ((cells - land) * 1000 / cells) as u32,
        routes: routes(&t, &b.water, b.hills[0], enemy),
        enemy_walk,
        open_walk,
        detour_pct: enemy_walk * 100 / open_walk.max(1),
        dead_end_pm: (dead * 1000 / land.max(1)) as u32,
        water_runs: (rle(&b.water).len() / 2) as u32,
        home_view_land: view as u32,
        food_per_seat: (b.food.len() / p) as u32,
    })
}

/// The protocol's run-length encoding: `[value, run, value, run, ...]`, row-major.
pub fn rle(water: &[bool]) -> Vec<u32> {
    let mut out = Vec::new();
    let Some(&first) = water.first() else { return out };
    let (mut cur, mut run) = (first, 0u32);
    for &w in water {
        if w == cur {
            run += 1;
        } else {
            out.extend([cur as u32, run]);
            (cur, run) = (w, 1);
        }
    }
    out.extend([cur as u32, run]);
    out
}

/// Square-disjoint routes between the ground around two hills: a maximum flow with every land
/// square of capacity one, bar the two home grounds themselves. Unit augmenting paths, so it stops
/// at `ROUTES_CAP` without finishing a flow nobody needs.
fn routes(t: &Torus, water: &[bool], a: usize, b: usize) -> u32 {
    const HOME: i64 = MIN_CLEARING as i64;
    const INF: u32 = 1 << 20;
    let n = t.cells();
    let (src, dst) = (2 * n, 2 * n + 1);
    let mut head: Vec<Vec<u32>> = vec![Vec::new(); 2 * n + 2];
    let mut to: Vec<u32> = Vec::new();
    let mut cap: Vec<u32> = Vec::new();
    let mut add = |u: usize, v: usize, c: u32| {
        head[u].push(to.len() as u32);
        to.push(v as u32);
        cap.push(c);
        head[v].push(to.len() as u32);
        to.push(u as u32);
        cap.push(0);
    };
    for x in 0..n {
        if water[x] {
            continue;
        }
        let (at_a, at_b) = (t.chebyshev(a, x) <= HOME, t.chebyshev(b, x) <= HOME);
        add(2 * x, 2 * x + 1, if at_a || at_b { INF } else { 1 });
        for y in t.n4(x) {
            if !water[y] {
                add(2 * x + 1, 2 * y, 1);
            }
        }
        if at_a {
            add(src, 2 * x, INF);
        }
        if at_b {
            add(2 * x + 1, dst, INF);
        }
    }
    let mut flow = 0;
    while flow < ROUTES_CAP {
        let mut via = vec![u32::MAX; 2 * n + 2];
        let mut queue = std::collections::VecDeque::from([src]);
        via[src] = u32::MAX - 1;
        while let Some(u) = queue.pop_front() {
            if u == dst {
                break;
            }
            for &e in &head[u] {
                let v = to[e as usize] as usize;
                if cap[e as usize] > 0 && via[v] == u32::MAX {
                    via[v] = e;
                    queue.push_back(v);
                }
            }
        }
        if via[dst] == u32::MAX {
            break;
        }
        let mut v = dst;
        while v != src {
            let e = via[v] as usize;
            cap[e] -= 1;
            cap[e ^ 1] += 1;
            v = to[e ^ 1] as usize;
        }
        flow += 1;
    }
    flow
}
