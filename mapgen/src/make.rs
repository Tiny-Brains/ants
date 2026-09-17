//! One board, from a recipe, a shift and a seed.
//!
//! ```text
//!   areas     seeds, warped Voronoi labels, fragments folded into a neighbour
//!   homes     the areas the hills stand in (with a room stamped around each, if asked)
//!   cover     areas made solid water until `coverage_pct` is carved, never cutting the rest apart
//!   walls     every boundary between two carved areas, `wall` cells thick
//!   doors     a spanning tree of doors, plus `loops_pct` more, each `closure_pct` of its boundary shut
//!   hills     one orbit per home, with a clearing kept open
//!   fill      scattered rocks or cellular caverns, never disconnecting the land
//!   prune     any land still cut off becomes water
//!   food      a bootstrap ring by walk distance, then the rest by bias
//! ```
//!
//! **Symmetric by construction.** Every write is to a whole orbit, and every decision is made on
//! one square of the orbit and copied to the rest by the shift, so there is never a moment where
//! the board is lopsided and a later step must repair it. `measure` checks the result anyway.
//!
//! **Why doors are chosen per orbit.** A door between area `(i, 0)` and `(i', d)` is copied to
//! `(i, k)` and `(i', d + k)` for every seat `k`. So a door set that joins every area is a tree in each
//! seat's share plus at least one orbit linking the shares — which is a ring through every seat, and
//! why the fewest routes between two seats' territories is two, not one. A single route would need
//! a passage that is its own image, and a shift has none.
//!
//! A stage that cannot honour the recipe on this draw returns why, and the set draws again.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::grid::{N4, Orbits, Rng, Shift, Torus, isqrt, orbits};
use crate::recipe::{Bias, FillStyle, Recipe, VIEW_RADIUS2};

/// A finished board: what a map file holds, as squares.
pub struct Board {
    pub t: Torus,
    pub s: Shift,
    pub water: Vec<bool>,
    /// In orbits: hill `i` is seat `i % seats`'s.
    pub hills: Vec<usize>,
    /// In orbits.
    pub food: Vec<usize>,
}

pub fn make(r: &Recipe, s: Shift, rng: &mut Rng) -> Result<Board, String> {
    let mut plan = Plan::new(r, s);
    plan.areas(rng)?;
    plan.homes(rng)?;
    plan.cover(rng)?;
    plan.walls();
    plan.doors(rng)?;
    plan.hills()?;
    plan.fill(rng)?;
    plan.prune()?;
    plan.food(rng)?;
    Ok(plan.board())
}

/// Unreached, in a walk-distance field.
pub const FAR: u32 = u32::MAX;

/// Walk distance from any of `from` over land, 4-connected, wrapping.
pub fn walk(t: &Torus, water: impl Fn(usize) -> bool, from: &[usize]) -> Vec<u32> {
    let mut d = vec![FAR; t.cells()];
    let mut q = VecDeque::new();
    for &x in from {
        if d[x] == FAR {
            d[x] = 0;
            q.push_back(x);
        }
    }
    while let Some(x) = q.pop_front() {
        for y in t.n4(x) {
            if d[y] == FAR && !water(y) {
                d[y] = d[x] + 1;
                q.push_back(y);
            }
        }
    }
    d
}

struct Plan<'a> {
    r: &'a Recipe,
    t: Torus,
    s: Shift,
    p: usize,
    orb: Orbits,
    seeds: Vec<usize>,
    /// Area of each square, lifted: `i * seats + k` is quotient area `i` as seat `k` has it.
    label: Vec<u32>,
    /// Per quotient area.
    solid: Vec<bool>,
    wall: Vec<bool>,
    fill: Vec<bool>,
    /// Squares no later stage may fill: doorways and hill clearings.
    keep: Vec<bool>,
    /// `(quotient area, seat 0's hill square)`.
    homes: Vec<(usize, usize)>,
    hills: Vec<usize>,
    food: Vec<usize>,
}

impl<'a> Plan<'a> {
    fn new(r: &'a Recipe, s: Shift) -> Plan<'a> {
        let t = r.torus();
        let n = t.cells();
        Plan {
            r,
            t,
            s,
            p: s.seats,
            orb: orbits(&t, &s),
            seeds: Vec::new(),
            label: vec![0; n],
            solid: Vec::new(),
            wall: vec![false; n],
            fill: vec![false; n],
            keep: vec![false; n],
            homes: Vec::new(),
            hills: Vec::new(),
            food: Vec::new(),
        }
    }

    // ------------------------------------------------------------ squares and orbits

    #[inline]
    fn image(&self, x: usize, k: usize) -> usize {
        self.s.image(&self.t, x, k)
    }

    #[inline]
    fn carved(&self, x: usize) -> bool {
        !self.solid[self.label[x] as usize / self.p]
    }

    #[inline]
    fn land(&self, x: usize) -> bool {
        self.carved(x) && !self.wall[x] && !self.fill[x]
    }

    /// Land a later stage may still fill.
    #[inline]
    fn open(&self, x: usize) -> bool {
        self.land(x) && !self.keep[x]
    }

    /// Lifted area `id` as seat `j` further round has it.
    fn shifted(&self, id: u32, j: usize) -> u32 {
        let (i, k) = (id as usize / self.p, id as usize % self.p);
        (i * self.p + (k + j) % self.p) as u32
    }

    fn relabel(&mut self, x: usize, id: u32) {
        for j in 0..self.p {
            let y = self.image(x, j);
            self.label[y] = self.shifted(id, j);
        }
    }

    fn set_orbit(v: &mut [bool], t: &Torus, s: &Shift, x: usize, on: bool) {
        for j in 0..s.seats {
            v[s.image(t, x, j)] = on;
        }
    }

    fn square(&self, x: usize, radius: i32) -> impl Iterator<Item = usize> + '_ {
        (-radius..=radius)
            .flat_map(move |dr| (-radius..=radius).map(move |dc| self.t.step(x, dr, dc)))
    }

    fn water_fn(&self) -> impl Fn(usize) -> bool + '_ {
        |x| !self.land(x)
    }

    // ------------------------------------------------------------ areas

    fn areas(&mut self, rng: &mut Rng) -> Result<(), String> {
        let a = &self.r.areas;
        let (t, p) = (self.t, self.p);
        let cells = t.cells();
        let avg = ((a.size[0] + a.size[1]) / 2) as i64;
        let want = ((cells / p) as i64 / avg).max(2) as usize;

        let mut lattice = None;
        if a.grid {
            // One seed every `side` squares, offset once per board: square areas, so a closed set of
            // them is a corridor maze. `Recipe::check` has made `side` divide the board and the shift,
            // so the lattice is its own image and one seed per orbit is every seed.
            let side = isqrt(avg) as i32;
            let (or, oc) = (rng.below(side as u64) as i32, rng.below(side as u64) as i32);
            lattice = Some((side, or, oc));
            for r in (0..t.rows).step_by(side as usize) {
                for c in (0..t.cols).step_by(side as usize) {
                    let x = t.at(r + or, c + oc);
                    if self.orb.k[x] == 0 {
                        self.seeds.push(x);
                    }
                }
            }
        } else {
            // Seeds far enough apart, from each other and from every image, that the areas come out
            // about the same size: a dart throw rather than a grid, so no two boards share a skeleton.
            let d2 = (isqrt(avg) * 3 / 4).max(1).pow(2);
            let mut tries = 0;
            while self.seeds.len() < want && tries < want * 60 {
                tries += 1;
                let x = rng.below(cells as u64) as usize;
                let clear = (1..p).all(|k| t.dist2(x, self.image(x, k)) >= d2)
                    && self
                        .seeds
                        .iter()
                        .all(|&q| (0..p).all(|k| t.dist2(x, self.image(q, k)) >= d2));
                if clear {
                    self.seeds.push(x);
                }
            }
        }
        if self.seeds.len() < 2 {
            return Err("areas: fewer than two areas fit".into());
        }
        let n = self.seeds.len();
        // An arena's distance counts for a quarter, so its area reaches twice as far.
        let weight: Vec<i64> =
            (0..n).map(|_| if rng.percent(a.arenas_pct) { 1 } else { 4 }).collect();
        let (dr, dc) = (self.field(rng), self.field(rng));

        let points: Vec<(usize, u32)> = (0..n)
            .flat_map(|i| (0..p).map(move |k| (i, k)))
            .map(|(i, k)| (self.image(self.seeds[i], k), (i * p + k) as u32))
            .collect();
        let by_seed: BTreeMap<usize, u32> = points.iter().map(|&(pos, id)| (pos, id)).collect();
        for x in 0..cells {
            if self.orb.k[x] != 0 {
                continue;
            }
            let (r, c) = t.rc(x);
            let (wr, wc) = (r + dr[x], c + dc[x]);
            let w = t.at(wr, wc);
            // On a lattice the area is the lattice square a (warped) square falls in. Nearest-seed
            // would tie halfway between two seeds and break the ties unevenly where the board wraps.
            if let Some((side, or, oc)) = lattice {
                let seed = t.at(
                    (wr - or).rem_euclid(t.rows) / side * side + or,
                    (wc - oc).rem_euclid(t.cols) / side * side + oc,
                );
                self.label[x] = by_seed[&seed];
                continue;
            }
            let mut best = (i64::MAX, u32::MAX);
            for &(pos, id) in &points {
                let d = t.dist2(w, pos) * weight[id as usize / p];
                if (d, id) < best {
                    best = (d, id);
                }
            }
            self.label[x] = best.1;
        }
        for x in 0..cells {
            if self.orb.k[x] != 0 {
                let rep = self.orb.rep[x] as usize;
                self.label[x] = self.shifted(self.label[rep], self.orb.k[x] as usize);
            }
        }
        self.solid = vec![false; n];
        self.defragment();
        Ok(())
    }

    /// A smooth symmetric displacement field in `[-warp, warp]`: noise on each orbit, box-blurred
    /// three times. A blur commutes with a shift, so a symmetric field stays symmetric.
    fn field(&self, rng: &mut Rng) -> Vec<i32> {
        let t = self.t;
        let amp = self.r.areas.warp as i64;
        if amp == 0 {
            return vec![0; t.cells()];
        }
        let mut raw = vec![0i64; t.cells()];
        for (x, v) in raw.iter_mut().enumerate() {
            if self.orb.k[x] == 0 {
                *v = rng.below(2001) as i64 - 1000;
            }
        }
        let mut v: Vec<i64> = (0..t.cells()).map(|x| raw[self.orb.rep[x] as usize]).collect();
        let size = (self.r.areas.size[0] + self.r.areas.size[1]) as i64 / 2;
        let b = (isqrt(size) / 2).max(2) as i32;
        for _ in 0..3 {
            blur(&t, &mut v, b);
        }
        let m = v.iter().map(|x| x.abs()).max().unwrap_or(1).max(1);
        v.iter().map(|&x| (x * amp / m) as i32).collect()
    }

    /// A warped Voronoi cell can come out in pieces. Keep each area's largest piece and give every
    /// other piece to the neighbour it touches most, so an area is always one walkable region.
    fn defragment(&mut self) {
        let (t, p) = (self.t, self.p);
        let cells = t.cells();
        let mut stamp = vec![usize::MAX; cells];
        for i in 0..self.seeds.len() {
            let id = (i * p) as u32;
            let mut pieces: Vec<Vec<usize>> = Vec::new();
            for x in 0..cells {
                if self.label[x] != id || stamp[x] == i {
                    continue;
                }
                let mut piece = vec![x];
                stamp[x] = i;
                let mut at = 0;
                while at < piece.len() {
                    for y in t.n4(piece[at]) {
                        if self.label[y] == id && stamp[y] != i {
                            stamp[y] = i;
                            piece.push(y);
                        }
                    }
                    at += 1;
                }
                pieces.push(piece);
            }
            if pieces.len() < 2 {
                continue;
            }
            // Pieces are found in square order, so on a tie the first found is kept.
            let keep = (0..pieces.len())
                .max_by_key(|&j| (pieces[j].len(), std::cmp::Reverse(j)))
                .unwrap_or(0);
            for (j, piece) in pieces.iter().enumerate() {
                if j == keep {
                    continue;
                }
                let mut touch: BTreeMap<u32, u32> = BTreeMap::new();
                for &x in piece {
                    for y in t.n4(x) {
                        if self.label[y] != id {
                            *touch.entry(self.label[y]).or_default() += 1;
                        }
                    }
                }
                if let Some((&to, _)) =
                    touch.iter().max_by_key(|&(&l, &n)| (n, std::cmp::Reverse(l)))
                {
                    for &x in piece {
                        self.relabel(x, to);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ homes

    fn homes(&mut self, rng: &mut Rng) -> Result<(), String> {
        let h = &self.r.hills;
        let (t, p) = (self.t, self.p);
        // A hill's clearing, and the wall that may stand inside its area's edge, must both fit.
        let reach = (h.clearing + self.r.areas.wall.div_ceil(2)) as i32;
        let mut order: Vec<usize> = (0..self.seeds.len()).collect();
        rng.shuffle(&mut order);
        for i in order {
            if self.homes.len() == h.per_seat as usize {
                break;
            }
            let id = (i * p) as u32;
            let cell = if h.room {
                self.seeds[i]
            } else {
                let fits: Vec<usize> = (0..t.cells())
                    .filter(|&x| {
                        self.label[x] == id && self.square(x, reach).all(|y| self.label[y] == id)
                    })
                    .collect();
                if fits.is_empty() {
                    continue;
                }
                fits[rng.below(fits.len() as u64) as usize]
            };
            // Rooms and clearings apart from every other hill's, this seat's own images included.
            let apart = |a: usize, b: usize| t.chebyshev(a, b) >= 2 * reach as i64 + 2;
            let clear = (1..p).all(|k| apart(cell, self.image(cell, k)))
                && self.homes.iter().all(|&(_, o)| (0..p).all(|k| apart(cell, self.image(o, k))));
            if clear {
                self.homes.push((i, cell));
            }
        }
        if self.homes.len() < h.per_seat as usize {
            return Err(format!("homes: no room for {} hills a seat", h.per_seat));
        }
        if h.room {
            for (i, cell) in self.homes.clone() {
                let squares: Vec<usize> = self.square(cell, reach).collect();
                for y in squares {
                    self.relabel(y, (i * p) as u32);
                }
            }
            self.defragment();
        }
        Ok(())
    }

    // ------------------------------------------------------------ coverage

    /// How long each boundary between two lifted areas is, in touching pairs of squares.
    fn adjacency(&self) -> BTreeMap<(u32, u32), u32> {
        let t = self.t;
        let mut adj = BTreeMap::new();
        for x in 0..t.cells() {
            for y in [t.step(x, 0, 1), t.step(x, 1, 0)] {
                let (a, b) = (self.label[x], self.label[y]);
                if a != b {
                    *adj.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
        }
        adj
    }

    fn live(&self) -> Vec<bool> {
        let mut live = vec![false; self.seeds.len() * self.p];
        for &l in &self.label {
            live[l as usize] = !self.solid[l as usize / self.p];
        }
        live
    }

    /// Whether every carved area can be reached from every other across boundaries at least
    /// `need` long.
    fn joined(&self, adj: &BTreeMap<(u32, u32), u32>, need: u32) -> bool {
        let live = self.live();
        let mut uf = Uf::new(live.len());
        for (&(a, b), &n) in adj {
            if n >= need && live[a as usize] && live[b as usize] {
                uf.union(a as usize, b as usize);
            }
        }
        let mut roots = (0..live.len()).filter(|&i| live[i]).map(|i| uf.find(i));
        let first = roots.next();
        roots.all(|r| Some(r) == first)
    }

    fn door_need(&self) -> u32 {
        if self.r.areas.wall == 0 { 1 } else { self.r.connectivity.door_min }
    }

    fn cover(&mut self, rng: &mut Rng) -> Result<(), String> {
        let adj = self.adjacency();
        let need = self.door_need();
        if !self.joined(&adj, need) {
            return Err("cover: the areas do not touch widely enough to be joined".into());
        }
        let cov = self.r.areas.coverage_pct as usize;
        if cov == 100 {
            return Ok(());
        }
        let n = self.seeds.len();
        let mut size = vec![0usize; n];
        for &l in &self.label {
            size[l as usize / self.p] += 1;
        }
        let target = self.t.cells() * (100 - cov) / 100;
        let mut order: Vec<usize> = (0..n).collect();
        rng.shuffle(&mut order);
        let mut solid = 0;
        for i in order {
            if solid >= target {
                break;
            }
            if size[i] == 0 || self.homes.iter().any(|&(h, _)| h == i) {
                continue;
            }
            self.solid[i] = true;
            if self.joined(&adj, need) {
                solid += size[i];
            } else {
                self.solid[i] = false;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ walls and doors

    /// Wall every square of a carved area that has a different carved area within `wall` steps south
    /// and east of it.
    ///
    /// So every boundary is walled on its north-west side, exactly `wall` thick: a square and the one
    /// south of it, or east of it, in different areas always have a wall between them, which is what
    /// makes a boundary with no door shut. A direction is the same for every seat under a shift, so
    /// the walls are symmetric; an order on the areas would be too, but it walls a small area on every
    /// side at once and leaves nothing inside it.
    fn walls(&mut self) {
        let w = self.r.areas.wall as i32;
        if w == 0 {
            return;
        }
        let t = self.t;
        for x in 0..t.cells() {
            if !self.carved(x) {
                continue;
            }
            let a = self.label[x];
            'scan: for dr in 0..=w {
                for dc in 0..=w - dr {
                    let y = t.step(x, dr, dc);
                    if y != x && self.carved(y) && self.label[y] != a {
                        self.wall[x] = true;
                        break 'scan;
                    }
                }
            }
        }
    }

    /// Whether opening `x` would make it touch open ground of a third area, which is a door nobody
    /// chose.
    fn breaches(&self, x: usize, a: u32, b: u32) -> bool {
        self.t
            .n4(x)
            .into_iter()
            .any(|y| self.carved(y) && !self.wall[y] && self.label[y] != a && self.label[y] != b)
    }

    fn doors(&mut self, rng: &mut Rng) -> Result<(), String> {
        if self.r.areas.wall == 0 {
            return Ok(());
        }
        let (t, p) = (self.t, self.p);
        let conn = &self.r.connectivity;
        let n = self.seeds.len();

        // One key per orbit of boundaries: quotient areas `i1 <= i2` and how many seats round the
        // second is from the first.
        let mut keys = BTreeSet::new();
        for &(a, b) in self.adjacency().keys() {
            if !self.carved_id(a) || !self.carved_id(b) {
                continue;
            }
            let (ia, ka, ib, kb) = (a as usize / p, a as usize % p, b as usize / p, b as usize % p);
            let ((i1, k1), (i2, k2)) =
                if ia <= ib { ((ia, ka), (ib, kb)) } else { ((ib, kb), (ia, ka)) };
            let mut d = (k2 + p - k1) % p;
            if i1 == i2 {
                d = d.min(p - d);
            }
            keys.insert((i1, i2, d));
        }
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
        for x in 0..t.cells() {
            if (self.label[x] as usize).is_multiple_of(p) {
                members[self.label[x] as usize / p].push(x);
            }
        }
        let mut doors: Vec<Door> = Vec::new();
        for (i1, i2, d) in keys {
            let (a, b) = ((i1 * p) as u32, (i2 * p + d) as u32);
            let mut door = Door { key: (i1, i2, d), pairs: Vec::new(), boundary: 0 };
            for &x in &members[i1] {
                for (dir, y) in t.n4(x).into_iter().enumerate() {
                    if self.label[y] == b {
                        door.boundary += 1;
                        if !self.breaches(x, a, b) && !self.breaches(y, b, a) {
                            door.pairs.push((x, y, dir));
                        }
                    }
                }
            }
            if door.pairs.len() >= conn.door_min as usize {
                doors.push(door);
            }
        }
        rng.shuffle(&mut doors);

        // The fewest door orbits that join every carved area: Kruskal over orbits, carving each as
        // it is chosen and counting it only if the cut went through. A door that could not open —
        // every run of its boundary turned out to touch a third area once its neighbours' doors were
        // cut — joins nothing, and the next boundary is tried instead.
        let mut uf = Uf::new(n * p);
        let mut open = vec![false; doors.len()];
        for (di, door) in doors.iter().enumerate() {
            let (i1, i2, d) = door.key;
            let useful = (0..p).any(|j| uf.find(i1 * p + j) != uf.find(i2 * p + (d + j) % p));
            if useful && self.carve(door, rng) {
                open[di] = true;
                for j in 0..p {
                    uf.union(i1 * p + j, i2 * p + (d + j) % p);
                }
            }
        }
        // An area no door could reach — too small, or boxed in by junctions on every side — is filled
        // in rather than left as a sealed room. Seat 0's home decides which group is the board; every
        // seat's home must be in it, and what is filled in must be a sliver of the board, or the draw
        // failed.
        let main = uf.find(self.homes[0].0 * p);
        if (0..p).any(|k| uf.find(self.homes[0].0 * p + k) != main)
            || self.homes.iter().any(|&(h, _)| uf.find(h * p) != main)
        {
            return Err("doors: the seats' homes could not be joined".into());
        }
        let mut dropped = 0;
        for i in 0..n {
            if !self.solid[i] && uf.find(i * p) != main {
                self.solid[i] = true;
                dropped += 1;
            }
        }
        if dropped * 20 > n {
            return Err(format!("doors: {dropped} of {n} areas could not be joined"));
        }
        // Then the loops, drawn for every boundary whatever the share, so a recipe that changes only
        // `loops_pct` keeps the same tree.
        for (di, door) in doors.iter().enumerate() {
            let draw = rng.percent(conn.loops_pct);
            if !open[di] && draw && self.carve(door, rng) {
                open[di] = true;
            }
        }
        for (home, _) in self.homes.clone() {
            let touches = |door: &Door| door.key.0 == home || door.key.1 == home;
            let mut count = doors.iter().zip(&open).filter(|(d, o)| **o && touches(d)).count();
            for (di, door) in doors.iter().enumerate() {
                if count >= self.r.hills.home_doors_min as usize {
                    break;
                }
                if !open[di] && touches(door) && self.carve(door, rng) {
                    open[di] = true;
                    count += 1;
                }
            }
        }
        Ok(())
    }

    fn carved_id(&self, id: u32) -> bool {
        !self.solid[id as usize / self.p]
    }

    /// Open one door orbit: a run of the boundary as long as the closure leaves, cut straight
    /// through the wall on both sides. Whether any of it reached open ground on both sides.
    fn carve(&mut self, door: &Door, rng: &mut Rng) -> bool {
        let p = self.p;
        let (i1, i2, d) = door.key;
        let (a, b) = ((i1 * p) as u32, (i2 * p + d) as u32);
        let home = self.homes.iter().any(|&(h, _)| h == i1 || h == i2);
        let areas = &self.r.areas;
        let closure = if home {
            self.r.hills.home_closure_pct.unwrap_or(areas.closure_pct)
        } else {
            areas.closure_pct
        };
        let width = ((100 - closure) as usize * door.boundary / 100)
            .max(self.r.connectivity.door_min as usize)
            .min(door.pairs.len());

        // Grow the doorway along the boundary from a random pair, so it is one opening and not
        // several scattered ones.
        let mut by_square: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (pi, &(x, _, _)) in door.pairs.iter().enumerate() {
            by_square.entry(x).or_default().push(pi);
        }
        // A few starts: the doors already cut can leave one stretch of a boundary touching a third
        // area's open ground, and another stretch clear.
        for _ in 0..door.pairs.len().min(4) {
            let start = rng.below(door.pairs.len() as u64) as usize;
            let mut taken = vec![start];
            let mut seen = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(pi) = queue.pop_front() {
                let x = door.pairs[pi].0;
                for y in self.square(x, 1).collect::<Vec<_>>() {
                    for &pj in by_square.get(&y).map(Vec::as_slice).unwrap_or(&[]) {
                        if taken.len() < width && seen.insert(pj) {
                            taken.push(pj);
                            queue.push_back(pj);
                        }
                    }
                }
            }
            let mut through = false;
            for pi in taken {
                let (x, y, dir) = door.pairs[pi];
                let (dr, dc) = N4[dir];
                let near = self.cut(x, -dr, -dc, (a, b));
                let far = self.cut(y, dr, dc, (b, a));
                through |= near && far;
            }
            if through {
                return true;
            }
        }
        false
    }

    /// From a boundary square, clear wall moving away from the boundary until open ground, then keep
    /// two squares of that ground clear so nothing fills the doorway. A square whose opening would
    /// touch a third area stays wall, and the doorway stops there.
    fn cut(&mut self, from: usize, dr: i32, dc: i32, (id, other): (u32, u32)) -> bool {
        let (t, s) = (self.t, self.s);
        let mut x = from;
        for _ in 0..=self.r.areas.wall + 1 {
            if self.label[x] != id || !self.wall[x] || self.breaches(x, id, other) {
                break;
            }
            Self::set_orbit(&mut self.wall, &t, &s, x, false);
            Self::set_orbit(&mut self.keep, &t, &s, x, true);
            x = t.step(x, dr, dc);
        }
        let through = self.label[x] == id && !self.wall[x];
        for _ in 0..2 {
            if self.label[x] != id || self.wall[x] || !self.carved(x) {
                break;
            }
            Self::set_orbit(&mut self.keep, &t, &s, x, true);
            x = t.step(x, dr, dc);
        }
        through
    }

    // ------------------------------------------------------------ hills

    fn hills(&mut self) -> Result<(), String> {
        let (t, s, p) = (self.t, self.s, self.p);
        let clearing = self.r.hills.clearing as i32;
        for (_, cell) in self.homes.clone() {
            for k in 0..p {
                self.hills.push(self.image(cell, k));
            }
            for y in self.square(cell, clearing).collect::<Vec<_>>() {
                if !self.carved(y) || self.wall[y] {
                    return Err("hills: a wall stands in a hill's clearing".into());
                }
                Self::set_orbit(&mut self.keep, &t, &s, y, true);
            }
        }
        for (i, &a) in self.hills.iter().enumerate() {
            for (j, &b) in self.hills.iter().enumerate() {
                if i % p != j % p && t.dist2(a, b) <= VIEW_RADIUS2 {
                    return Err("hills: an enemy hill is in view at turn zero".into());
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ fill

    fn fill(&mut self, rng: &mut Rng) -> Result<(), String> {
        match self.r.fill.style {
            FillStyle::None => Ok(()),
            FillStyle::Scatter => {
                self.scatter(rng);
                Ok(())
            }
            FillStyle::Cellular => self.cellular(rng),
        }
    }

    fn reached(&self) -> usize {
        walk(&self.t, self.water_fn(), &self.hills[..1]).iter().filter(|&&d| d != FAR).count()
    }

    fn scatter(&mut self, rng: &mut Rng) {
        let (t, p) = (self.t, self.p);
        let f = &self.r.fill;
        let cells = t.cells();
        let target = (0..cells).filter(|&x| self.open(x)).count() * f.pct as usize / 100;
        let mut land = (0..cells).filter(|&x| self.land(x)).count();
        let (mut placed, mut tries) = (0, 0);
        while placed < target && tries < 20_000 {
            tries += 1;
            let x = rng.below(cells as u64) as usize;
            let (h, w) =
                (rng.within(f.blob[0], f.blob[1]) as i32, rng.within(f.blob[0], f.blob[1]) as i32);
            let rock: Vec<usize> =
                (0..h).flat_map(|dr| (0..w).map(move |dc| t.step(x, dr, dc))).collect();
            if !rock.iter().all(|&y| self.open(y)) {
                continue;
            }
            let mut changed = Vec::new();
            for &y in &rock {
                for j in 0..p {
                    let z = self.image(y, j);
                    if !self.fill[z] {
                        self.fill[z] = true;
                        changed.push(z);
                    }
                }
            }
            // Kept only if every square of land can still be walked to.
            if self.reached() != land - changed.len() {
                for z in changed {
                    self.fill[z] = false;
                }
                continue;
            }
            land -= changed.len();
            placed += changed.len();
        }
    }

    fn cellular(&mut self, rng: &mut Rng) -> Result<(), String> {
        let (t, s) = (self.t, self.s);
        let f = &self.r.fill;
        for x in 0..t.cells() {
            if self.orb.k[x] == 0 && self.open(x) && rng.percent(f.pct) {
                Self::set_orbit(&mut self.fill, &t, &s, x, true);
            }
        }
        // The cave rule: water where five of the eight neighbours are, and water that has four
        // stays. It commutes with the shift, so a symmetric start stays symmetric.
        for _ in 0..f.smooth {
            let mut next = self.fill.clone();
            for (x, v) in next.iter_mut().enumerate() {
                if !self.carved(x) || self.wall[x] || self.keep[x] {
                    continue;
                }
                let wet = self.square(x, 1).filter(|&y| y != x && !self.land(y)).count();
                *v = wet >= 5 || (self.fill[x] && wet >= 4);
            }
            self.fill = next;
        }
        self.reconnect()
    }

    /// Join every cavern the automaton cut off back to the one seat 0's hill stands in, through the
    /// fill between them, or fill it in if it is too small to be worth a tunnel.
    fn reconnect(&mut self) -> Result<(), String> {
        let (t, s) = (self.t, self.s);
        let cells = t.cells();
        loop {
            let (part, size) = self.parts();
            let main = part[self.hills[0]];
            let mut crumbs = false;
            for x in 0..cells {
                if self.land(x) && part[x] != main && size[part[x] as usize] < 6 {
                    self.fill[x] = true;
                    crumbs = true;
                }
            }
            if crumbs {
                continue;
            }
            let Some(start) = (0..cells).find(|&x| self.land(x) && part[x] != main) else {
                return Ok(());
            };
            let c = part[start];
            let mut prev = vec![usize::MAX; cells];
            let mut q = VecDeque::new();
            for x in 0..cells {
                if part[x] == c {
                    prev[x] = x;
                    q.push_back(x);
                }
            }
            let mut hit = None;
            'search: while let Some(x) = q.pop_front() {
                for y in t.n4(x) {
                    if prev[y] != usize::MAX {
                        continue;
                    }
                    if self.land(y) && part[y] == main {
                        prev[y] = x;
                        hit = Some(y);
                        break 'search;
                    }
                    if self.carved(y) && !self.wall[y] && self.fill[y] {
                        prev[y] = x;
                        q.push_back(y);
                    }
                }
            }
            match hit {
                Some(end) => {
                    let mut x = prev[end];
                    while part[x] != c {
                        Self::set_orbit(&mut self.fill, &t, &s, x, false);
                        x = prev[x];
                    }
                }
                None => {
                    for x in (0..cells).filter(|&x| part[x] == c) {
                        Self::set_orbit(&mut self.fill, &t, &s, x, true);
                    }
                }
            }
        }
    }

    /// 4-connected pieces of land: a piece per square (`u32::MAX` for water), and each piece's size.
    fn parts(&self) -> (Vec<u32>, Vec<usize>) {
        let t = self.t;
        let mut part = vec![u32::MAX; t.cells()];
        let mut size = Vec::new();
        for x in 0..t.cells() {
            if part[x] != u32::MAX || !self.land(x) {
                continue;
            }
            let id = size.len() as u32;
            let mut stack = vec![x];
            part[x] = id;
            let mut n = 0;
            while let Some(y) = stack.pop() {
                n += 1;
                for z in t.n4(y) {
                    if part[z] == u32::MAX && self.land(z) {
                        part[z] = id;
                        stack.push(z);
                    }
                }
            }
            size.push(n);
        }
        (part, size)
    }

    // ------------------------------------------------------------ prune and food

    fn prune(&mut self) -> Result<(), String> {
        let d = walk(&self.t, self.water_fn(), &self.hills[..1]);
        if self.hills.iter().any(|&h| d[h] == FAR) {
            return Err("prune: the seats' hills are not joined by land".into());
        }
        let cut: Vec<usize> =
            (0..self.t.cells()).filter(|&x| self.land(x) && d[x] == FAR).collect();
        let land = (0..self.t.cells()).filter(|&x| self.land(x)).count();
        // Walling a narrow neck of an area can sever its tip from its door; slivers like that are
        // filled in. More than a sixteenth of the land means the doors themselves failed.
        if cut.len() * 16 > land {
            return Err(format!(
                "prune: {} squares of land were cut off, the doors failed",
                cut.len()
            ));
        }
        for x in cut {
            self.fill[x] = true;
        }
        Ok(())
    }

    fn food(&mut self, rng: &mut Rng) -> Result<(), String> {
        let (t, p) = (self.t, self.p);
        let f = &self.r.food;
        let cells = t.cells();
        let mut taken = vec![false; cells];
        for &h in &self.hills {
            taken[h] = true;
        }
        let per = self.r.hills.per_seat as usize;

        let place = |plan: &mut Plan, x: usize, taken: &mut Vec<bool>| -> bool {
            let orbit: Vec<usize> = (0..p).map(|k| plan.image(x, k)).collect();
            let apart =
                orbit.iter().all(|&y| orbit.iter().all(|&z| y == z || t.manhattan(y, z) > 1));
            if !plan.land(x) || !apart || orbit.iter().any(|&y| taken[y]) {
                return false;
            }
            for y in orbit {
                taken[y] = true;
                plan.food.push(y);
            }
            true
        };

        for j in 0..per {
            let d = walk(&t, self.water_fn(), &[self.hills[j * p]]);
            let [lo, hi] = f.bootstrap_walk;
            let mut near: Vec<usize> = (0..cells).filter(|&x| d[x] >= lo && d[x] <= hi).collect();
            rng.shuffle(&mut near);
            let mut got = 0;
            for x in near {
                if got == f.bootstrap {
                    break;
                }
                if place(self, x, &mut taken) {
                    got += 1;
                }
            }
            if got < f.bootstrap {
                return Err("food: no room for the bootstrap ring".into());
            }
        }

        let need = f.per_seat - f.bootstrap * per as u32;
        let own_hills: Vec<usize> = (0..per).map(|j| self.hills[j * p]).collect();
        let own = walk(&t, self.water_fn(), &own_hills);
        // Seat k's walk to x is seat 0's walk to x moved k seats back.
        let other = |x: usize| (1..p).map(|k| own[self.image(x, p - k)]).min().unwrap_or(FAR);
        let band = f.band;
        let mut biased: Vec<usize> = (0..cells)
            .filter(|&x| self.land(x) && own[x] != FAR)
            .filter(|&x| match f.bias {
                Bias::Uniform => true,
                Bias::Contested => own[x].abs_diff(other(x)) <= band,
                Bias::Home => own[x].saturating_add(band) < other(x),
            })
            .collect();
        let mut anywhere: Vec<usize> = (0..cells).filter(|&x| self.land(x)).collect();
        rng.shuffle(&mut biased);
        rng.shuffle(&mut anywhere);
        let mut got = 0;
        for x in biased.into_iter().chain(anywhere) {
            if got == need {
                break;
            }
            if place(self, x, &mut taken) {
                got += 1;
            }
        }
        if got < need {
            return Err("food: no room for the board's food".into());
        }
        Ok(())
    }

    fn board(self) -> Board {
        let water = (0..self.t.cells()).map(|x| !self.land(x)).collect();
        Board { t: self.t, s: self.s, water, hills: self.hills, food: self.food }
    }
}

struct Door {
    key: (usize, usize, usize),
    /// `(square in the first area, square in the second, the move from one to the other)`, for the
    /// stretch of boundary a door may be cut in.
    pairs: Vec<(usize, usize, usize)>,
    boundary: usize,
}

struct Uf(Vec<usize>);

impl Uf {
    fn new(n: usize) -> Uf {
        Uf((0..n).collect())
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// A box blur, rows then columns, wrapping, divided back down so repeated passes do not overflow.
fn blur(t: &Torus, v: &mut [i64], b: i32) {
    let span = (2 * b + 1) as i64;
    let mut out = vec![0i64; v.len()];
    for r in 0..t.rows {
        let mut acc: i64 = (-b..=b).map(|dc| v[t.at(r, dc)]).sum();
        for c in 0..t.cols {
            out[t.at(r, c)] = acc / span;
            acc += v[t.at(r, c + b + 1)] - v[t.at(r, c - b)];
        }
    }
    for c in 0..t.cols {
        let mut acc: i64 = (-b..=b).map(|dr| out[t.at(dr, c)]).sum();
        for r in 0..t.rows {
            v[t.at(r, c)] = acc / span;
            acc += out[t.at(r + b + 1, c)] - out[t.at(r - b, c)];
        }
    }
}
