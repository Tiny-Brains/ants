//! A designed board: shapes drawn under a symmetry, rather than areas grown from noise.
//!
//! ```text
//!   base      all land, or all water to carve from
//!   steps     shapes (disk, ring, seg, box) in water or land, and patterns (noise, ridge, maze,
//!             rooms, dots, border, smooth), each drawn once in seat 0's frame and copied to every
//!             image under the full group
//!   hills     a clearing around each, carved
//!   repair    land the steps cut off is joined back by the shortest tunnel, or filled if tiny
//!   food      a bootstrap ring near each hill, then the rest by bias, whole orbits of the group
//! ```
//!
//! **A design is data, and the renderer is small.** Everything a sampler chose — the board, the
//! shift, the point group, every shape's place and size, every pattern's seed — is written into the
//! design, so a board somebody picked by eye is a file that renders the same board again, whatever
//! later happens to the sampler that proposed it. Only this renderer is the contract.
//!
//! **Symmetric by construction, twice.** Every step paints a scratch mask, the mask is widened to
//! whole orbits of the full group, and only then written: so no rasterising accident can leave the
//! board lopsided, under the shift (fairness) or the point group (looks). `measure` checks the first
//! anyway, as it does for every board.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::grid::{Rng, Shift, Torus};
use crate::make::{Board, FAR, walk};
use crate::measure::{Metrics, measure, rle};
use crate::sym::{self, Mat, Orbits};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub name: String,
    pub terrain: String,
    pub family: String,
    pub rows: u8,
    pub cols: u8,
    pub seats: u8,
    /// Seat `k`'s board is seat 0's moved `k` times by this.
    pub shift: [i32; 2],
    /// Seat 0's centre: where its point group is centred, and what its hills are placed from.
    pub origin: [i32; 2],
    /// The point group about every centre, by name (`sym::GROUPS`).
    pub symmetry: String,
    /// The board before the first step: water to carve from, or land.
    #[serde(default)]
    pub water: bool,
    pub steps: Vec<Step>,
    /// Seat 0's hills, from its centre.
    pub hills: Vec<[i32; 2]>,
    pub food: FoodPlan,
}

/// One drawing step. Which fields mean anything depends on `op`; the rest stay at zero and are not
/// written out.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// `water` or `land` (a shape); `noise`, `ridge`, `maze`, `rooms`, `dots`, `border`, `smooth`.
    pub op: String,
    /// For `water` and `land`: `disk`, `ring`, `seg` or `box`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shape: String,
    /// Where, from seat 0's centre, in squares.
    #[serde(default, skip_serializing_if = "zero2")]
    pub at: [f64; 2],
    /// A segment's other end.
    #[serde(default, skip_serializing_if = "zero2")]
    pub to: [f64; 2],
    /// Disk: `[radius, _]`; ring: `[inner, outer]`; box: half the rows and columns it spans; dots:
    /// each dot's radius.
    #[serde(default, skip_serializing_if = "zero2")]
    pub r: [f64; 2],
    /// A segment's thickness; a border's.
    #[serde(default, skip_serializing_if = "zero")]
    pub width: f64,
    /// `l2` (default), `l1`, `linf` or `oct`: what "radius" means.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub metric: String,
    /// Maze, rooms and dots: the lattice spacing, which must divide the board and the shift.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub pitch: u32,
    /// Maze: a corridor's width. Rooms: a wall's thickness.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub corridor: u32,
    /// Rooms: a door's width.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub door: u32,
    /// Noise and ridge: the share made water. Rooms: the share of walls taken down whole.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub pct: u32,
    /// Maze and rooms: passages beyond the fewest that join everything, as a share.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub loops: u32,
    /// Maze: the share of dead ends opened into a loop.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub braid: u32,
    /// Noise and ridge: blur radius, so feature size. Border: how far it wanders.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub scale: u32,
    /// Noise, ridge, smooth: cellular smoothing passes.
    #[serde(default, skip_serializing_if = "zero_u")]
    pub smooth: u32,
    /// Only where the distance to the nearest centre, over the spacing between centres, is in
    /// `[lo, hi]`. Absent is everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<[f64; 2]>,
    /// What `band` measures from: the seats' centres (default), or `hills`, the nearest hill of any
    /// seat -- so a pattern can keep clear of every hill however they are spread.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(default, skip_serializing_if = "zero_u64", with = "seed")]
    pub seed: u64,
    /// `water` or `land`, for patterns that can draw either (noise, ridge). Default water.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub paint: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FoodPlan {
    /// Food each seat has at turn zero, bootstrap included. Whole orbits, so it can land a little
    /// under when no orbit is small enough to finish on the number.
    pub per_seat: u32,
    /// Food within a short walk of each hill.
    pub bootstrap: u32,
    /// `home`, `contested` or `uniform`: where the rest goes.
    pub bias: String,
    /// Of the rest, how many go on the borders' middles and corners first.
    #[serde(default)]
    pub sites: u32,
    #[serde(with = "seed")]
    pub seed: u64,
}

/// A seed spans all 64 bits and a TOML integer only 63, so a recipe writes it as hex. It reads a
/// plain number too, which is how `explore` writes it in JSON.
mod seed {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:#018x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(u64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(n) => Ok(n),
            Raw::S(s) => match s.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16),
                None => s.parse(),
            }
            .map_err(serde::de::Error::custom),
        }
    }
}

fn zero(v: &f64) -> bool {
    *v == 0.0
}
fn zero2(v: &[f64; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}
fn zero_u(v: &u32) -> bool {
    *v == 0
}
fn zero_u64(v: &u64) -> bool {
    *v == 0
}

pub const CLEARING: i32 = 2;

/// What rendering came to: the board, and what the renderer had to do to make it one.
pub struct Rendered {
    pub board: Board,
    /// Squares of water tunnelled through to join land a step cut off.
    pub repaired: usize,
    /// Squares of land filled because they were too small to be worth a tunnel.
    pub filled: usize,
}

pub struct Canvas {
    pub t: Torus,
    pub s: Shift,
    pub ops: Vec<Mat>,
    pub origin: (i32, i32),
    pub orb: Orbits,
    pub water: Vec<bool>,
    pub centres: Vec<(i32, i32)>,
    pub spacing: f64,
    /// Distance to the nearest centre, and to the next nearest, for every square.
    pub near: Vec<f64>,
    pub near2: Vec<f64>,
    /// Distance to the nearest hill of any seat.
    pub near_hill: Vec<f64>,
    /// The borders' middles and corners, from seat 0's centre.
    pub mids: Vec<(f64, f64)>,
    pub corners: Vec<(f64, f64)>,
}

impl Canvas {
    pub fn new(d: &Design) -> Result<Canvas, String> {
        let t = Torus { rows: d.rows as i32, cols: d.cols as i32 };
        let s = Shift { seats: d.seats as usize, dr: d.shift[0], dc: d.shift[1] };
        if t.rows < 8 || t.cols < 8 {
            return Err(format!("a {}x{} board is too small", t.rows, t.cols));
        }
        if !s.is_exact(&t) {
            return Err(format!(
                "shift {:?} is not of order {} on {}x{}",
                d.shift, d.seats, t.rows, t.cols
            ));
        }
        let ops = sym::group(&d.symmetry).ok_or(format!("no point group '{}'", d.symmetry))?;
        if !ops.iter().all(|m| sym::fits(&t, &s, m)) {
            return Err(format!(
                "point group {} does not fit shift {:?} on {}x{}",
                d.symmetry, d.shift, t.rows, t.cols
            ));
        }
        let origin = (d.origin[0], d.origin[1]);
        let orb = sym::orbits(&t, &s, &ops, origin);
        let lat = sym::lattice(&t, &s);
        let centres: Vec<(i32, i32)> = (0..s.seats as i32)
            .map(|k| {
                let (r, c) = t.rc(t.at(origin.0 + k * s.dr, origin.1 + k * s.dc));
                (r, c)
            })
            .collect();
        let n = t.cells();
        let (mut near, mut near2) = (vec![0.0; n], vec![0.0; n]);
        for x in 0..n {
            let (r, c) = t.rc(x);
            let (mut a, mut b) = (f64::MAX, f64::MAX);
            for &(cr, cc) in &centres {
                for wr in -1..=1 {
                    for wc in -1..=1 {
                        let (dr, dc) =
                            ((r - cr - wr * t.rows) as f64, (c - cc - wc * t.cols) as f64);
                        let d = (dr * dr + dc * dc).sqrt();
                        if d < a {
                            b = a;
                            a = d;
                        } else if d < b {
                            b = d;
                        }
                    }
                }
            }
            near[x] = a;
            near2[x] = b;
        }
        let mut hill_pts = Vec::new();
        for h in &d.hills {
            for k in 0..s.seats as i32 {
                hill_pts.push((origin.0 + h[0] + k * s.dr, origin.1 + h[1] + k * s.dc));
            }
        }
        let near_hill: Vec<f64> = (0..n)
            .map(|x| {
                let (r, c) = t.rc(x);
                hill_pts
                    .iter()
                    .map(|&(hr, hc)| {
                        let (dr, dc) = ((r - hr).rem_euclid(t.rows), (c - hc).rem_euclid(t.cols));
                        let (dr, dc) = (dr.min(t.rows - dr) as f64, dc.min(t.cols - dc) as f64);
                        (dr * dr + dc * dc).sqrt()
                    })
                    .fold(f64::MAX, f64::min)
            })
            .collect();
        Ok(Canvas {
            t,
            s,
            ops,
            origin,
            orb,
            water: vec![d.water; n],
            centres,
            spacing: lat.spacing,
            near,
            near2,
            near_hill,
            mids: lat.mids,
            corners: lat.corners,
        })
    }

    fn in_band(&self, x: usize, band: Option<[f64; 2]>, from: &str) -> bool {
        match band {
            None => true,
            Some([lo, hi]) => {
                let d = if from == "hills" { self.near_hill[x] } else { self.near[x] };
                let v = d / self.spacing;
                v >= lo && v <= hi
            }
        }
    }

    /// Widen a painted mask to whole orbits, then write it.
    fn commit(&mut self, paint: &[bool], value: bool) {
        for members in &self.orb.members {
            if members.iter().any(|&y| paint[y as usize]) {
                for &y in members {
                    self.water[y as usize] = value;
                }
            }
        }
    }

    /// Paint one shape at every image of it under the full group.
    fn stamp(&self, paint: &mut [bool], sh: &Shape) {
        let t = self.t;
        let (anchor, reach) = sh.anchor();
        for m in &self.ops {
            let inv = sym::inverse(m);
            for &(cr, cc) in &self.centres {
                let a = sym::applyf(m, anchor);
                let (ar, ac) = (cr as f64 + a.0, cc as f64 + a.1);
                let (r0, r1) = ((ar - reach).floor() as i32, (ar + reach).ceil() as i32);
                let (c0, c1) = ((ac - reach).floor() as i32, (ac + reach).ceil() as i32);
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        let q = sym::applyf(&inv, ((r - cr) as f64, (c - cc) as f64));
                        if sh.contains(q) {
                            paint[t.at(r, c)] = true;
                        }
                    }
                }
            }
        }
    }

    pub fn step(&mut self, st: &Step) -> Result<(), String> {
        let n = self.t.cells();
        let mut paint = vec![false; n];
        match st.op.as_str() {
            "water" | "land" => {
                let sh = Shape::of(st)?;
                self.stamp(&mut paint, &sh);
                let v = st.op == "water";
                self.commit(&paint, v);
            }
            "noise" | "ridge" => {
                let field = self.noise(st.seed, st.scale.max(1));
                let inside: Vec<usize> =
                    (0..n).filter(|&x| self.in_band(x, st.band, &st.from)).collect();
                if inside.is_empty() {
                    return Ok(());
                }
                let mut vals: Vec<i64> = inside.iter().map(|&x| field[x]).collect();
                vals.sort_unstable();
                let pct = st.pct.min(100) as usize;
                if st.op == "noise" {
                    // The top `pct` of the field.
                    let cut = vals[(vals.len() * (100 - pct) / 100).min(vals.len() - 1)];
                    for &x in &inside {
                        paint[x] = field[x] > cut || (pct == 100);
                    }
                } else {
                    // A window about the median: the field's contour lines, which wind.
                    let lo = vals[(vals.len() * (100 - pct) / 200).min(vals.len() - 1)];
                    let hi = vals[(vals.len() * (100 + pct) / 200).min(vals.len() - 1)];
                    for &x in &inside {
                        paint[x] = field[x] >= lo && field[x] <= hi;
                    }
                }
                let v = st.paint != "land";
                self.commit(&paint, v);
                if st.smooth > 0 {
                    self.smooth(st.smooth, st.band, &st.from);
                }
            }
            "smooth" => self.smooth(st.smooth.max(1), st.band, &st.from),
            "dots" => {
                let q = st.pitch as i32;
                self.lattice_ok(q)?;
                let (rows, cols) = (self.t.rows / q, self.t.cols / q);
                for i in 0..rows {
                    for j in 0..cols {
                        let (r, c) = (
                            self.origin.0 + st.at[0] as i32 + i * q,
                            self.origin.1 + st.at[1] as i32 + j * q,
                        );
                        let x = self.t.at(r, c);
                        if !self.in_band(x, st.band, &st.from) {
                            continue;
                        }
                        let rad = st.r[0];
                        let reach = rad.ceil() as i32 + 1;
                        for dr in -reach..=reach {
                            for dc in -reach..=reach {
                                if metric(&st.metric, (dr as f64, dc as f64)) <= rad {
                                    paint[self.t.at(r + dr, c + dc)] = true;
                                }
                            }
                        }
                    }
                }
                self.commit(&paint, true);
            }
            "border" => {
                let wob = if st.scale > 0 { Some(self.noise(st.seed, st.scale)) } else { None };
                let (lo, hi) = match &wob {
                    Some(f) => (*f.iter().min().unwrap_or(&0), *f.iter().max().unwrap_or(&1)),
                    None => (0, 1),
                };
                for x in 0..n {
                    let mut edge = self.near2[x] - self.near[x];
                    if let Some(f) = &wob {
                        let u = (f[x] - lo) as f64 / ((hi - lo).max(1)) as f64;
                        edge += (u - 0.5) * st.r[0] * 2.0;
                    }
                    paint[x] = edge.abs() <= st.width / 2.0 && self.in_band(x, st.band, &st.from);
                }
                self.commit(&paint, true);
            }
            "maze" => self.maze(st)?,
            "rooms" => self.rooms(st)?,
            other => return Err(format!("no step '{other}'")),
        }
        Ok(())
    }

    fn lattice_ok(&self, q: i32) -> Result<(), String> {
        if q < 2
            || self.t.rows % q != 0
            || self.t.cols % q != 0
            || self.s.dr % q != 0
            || self.s.dc % q != 0
        {
            return Err(format!(
                "pitch {q} must divide the {}x{} board and the shift ({}, {})",
                self.t.rows, self.t.cols, self.s.dr, self.s.dc
            ));
        }
        Ok(())
    }

    /// A smooth field, the same on every square of an orbit: a draw per orbit, blurred. A box blur
    /// commutes with every map in the group, so the field stays symmetric.
    fn noise(&self, seed: u64, scale: u32) -> Vec<i64> {
        let mut rng = Rng(seed);
        let n = self.t.cells();
        let mut raw = vec![0i64; n];
        for (x, v) in raw.iter_mut().enumerate() {
            if self.orb.rep[x] as usize == x {
                *v = rng.below(2001) as i64 - 1000;
            }
        }
        let mut v: Vec<i64> = (0..n).map(|x| raw[self.orb.rep[x] as usize]).collect();
        for _ in 0..3 {
            box_blur(&self.t, &mut v, scale as i32);
        }
        v
    }

    /// The cave rule, on land and water alike: water where five of the eight neighbours are, and
    /// water with four stays.
    fn smooth(&mut self, passes: u32, band: Option<[f64; 2]>, from: &str) {
        let t = self.t;
        for _ in 0..passes {
            let mut next = self.water.clone();
            for (x, v) in next.iter_mut().enumerate() {
                if !self.in_band(x, band, from) {
                    continue;
                }
                let (r, c) = t.rc(x);
                let mut wet = 0;
                for dr in -1..=1 {
                    for dc in -1..=1 {
                        if (dr, dc) != (0, 0) && self.water[t.at(r + dr, c + dc)] {
                            wet += 1;
                        }
                    }
                }
                *v = wet >= 5 || (self.water[x] && wet >= 4);
            }
            self.water = next;
        }
    }

    /// The nodes of a lattice through every centre, `q` apart: `(row count, col count, square)`.
    fn nodes(&self, q: i32) -> (i32, i32, Vec<usize>) {
        let (nr, nc) = (self.t.rows / q, self.t.cols / q);
        let v = (0..nr)
            .flat_map(|i| (0..nc).map(move |j| (i, j)))
            .map(|(i, j)| self.t.at(self.origin.0 + i * q, self.origin.1 + j * q))
            .collect();
        (nr, nc, v)
    }

    /// Edges of the node lattice grouped into orbits of the full group. Edge `2·node + 0` joins a
    /// node to the next one along its row, `2·node + 1` to the next one down its column.
    fn edge_orbits(&self, q: i32) -> Vec<Vec<usize>> {
        let (nr, nc, _) = self.nodes(q);
        let (t, s) = (self.t, self.s);
        let node_of = |r: i32, c: i32| -> usize {
            let i = (r - self.origin.0).rem_euclid(t.rows) / q;
            let j = (c - self.origin.1).rem_euclid(t.cols) / q;
            (i * nc + j) as usize
        };
        let pos = |node: usize| -> (i32, i32) {
            let (i, j) = (node as i32 / nc, node as i32 % nc);
            (self.origin.0 + i * q, self.origin.1 + j * q)
        };
        let edge_of = |a: (i32, i32), b: (i32, i32)| -> usize {
            // Whichever end the edge starts from, as the lattice numbers it.
            let (ar, ac) = (a.0.rem_euclid(t.rows), a.1.rem_euclid(t.cols));
            let (br, bc) = (b.0.rem_euclid(t.rows), b.1.rem_euclid(t.cols));
            let right =
                |x: (i32, i32), y: (i32, i32)| x.0 == y.0 && (x.1 + q).rem_euclid(t.cols) == y.1;
            let down =
                |x: (i32, i32), y: (i32, i32)| x.1 == y.1 && (x.0 + q).rem_euclid(t.rows) == y.0;
            if right((ar, ac), (br, bc)) {
                2 * node_of(ar, ac)
            } else if right((br, bc), (ar, ac)) {
                2 * node_of(br, bc)
            } else if down((ar, ac), (br, bc)) {
                2 * node_of(ar, ac) + 1
            } else {
                2 * node_of(br, bc) + 1
            }
        };
        let ne = (nr * nc * 2) as usize;
        let mut uf: Vec<usize> = (0..ne).collect();
        fn find(uf: &mut [usize], mut x: usize) -> usize {
            while uf[x] != x {
                uf[x] = uf[uf[x]];
                x = uf[x];
            }
            x
        }
        for e in 0..ne {
            let a = pos(e / 2);
            let b = if e % 2 == 0 { (a.0, a.1 + q) } else { (a.0 + q, a.1) };
            let mut images = vec![edge_of((a.0 + s.dr, a.1 + s.dc), (b.0 + s.dr, b.1 + s.dc))];
            for m in &self.ops {
                let ma = sym::apply(m, (a.0 - self.origin.0, a.1 - self.origin.1));
                let mb = sym::apply(m, (b.0 - self.origin.0, b.1 - self.origin.1));
                images.push(edge_of(
                    (self.origin.0 + ma.0, self.origin.1 + ma.1),
                    (self.origin.0 + mb.0, self.origin.1 + mb.1),
                ));
            }
            for f in images {
                let (ra, rb) = (find(&mut uf, e), find(&mut uf, f));
                if ra != rb {
                    uf[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let mut groups: Vec<Vec<usize>> = vec![Vec::new(); ne];
        for e in 0..ne {
            let r = find(&mut uf, e);
            groups[r].push(e);
        }
        groups.into_iter().filter(|g| !g.is_empty()).collect()
    }

    /// A spanning set of edge orbits over the node lattice, then `loops` more, then dead ends
    /// braided shut: which edges end up open.
    fn passages(&self, q: i32, seed: u64, loops: u32, braid: u32, pre: &[bool]) -> Vec<bool> {
        let (nr, nc, _) = self.nodes(q);
        let nn = (nr * nc) as usize;
        let mut orbits = self.edge_orbits(q);
        let mut rng = Rng(seed);
        rng.shuffle(&mut orbits);
        let mut uf: Vec<usize> = (0..nn).collect();
        fn find(uf: &mut [usize], mut x: usize) -> usize {
            while uf[x] != x {
                uf[x] = uf[uf[x]];
                x = uf[x];
            }
            x
        }
        let ends = |e: usize| -> (usize, usize) {
            let a = e / 2;
            let (i, j) = (a as i32 / nc, a as i32 % nc);
            let b =
                if e.is_multiple_of(2) { i * nc + (j + 1) % nc } else { ((i + 1) % nr) * nc + j };
            (a, b as usize)
        };
        let mut open = pre.to_vec();
        let pre_open: Vec<usize> = (0..open.len()).filter(|&e| open[e]).collect();
        for e in pre_open {
            {
                let (a, b) = ends(e);
                let (ra, rb) = (find(&mut uf, a), find(&mut uf, b));
                uf[ra.max(rb)] = ra.min(rb);
            }
        }
        for g in &orbits {
            if g.iter().any(|&e| open[e]) {
                continue;
            }
            let useful = g.iter().any(|&e| {
                let (a, b) = ends(e);
                find(&mut uf, a) != find(&mut uf, b)
            });
            if useful {
                for &e in g {
                    open[e] = true;
                    let (a, b) = ends(e);
                    let (ra, rb) = (find(&mut uf, a), find(&mut uf, b));
                    if ra != rb {
                        uf[ra.max(rb)] = ra.min(rb);
                    }
                }
            }
        }
        for g in &orbits {
            let draw = rng.percent(loops);
            if draw && !g.iter().any(|&e| open[e]) {
                for &e in g {
                    open[e] = true;
                }
            }
        }
        if braid > 0 {
            let mut degree = vec![0u32; nn];
            for (e, &o) in open.iter().enumerate() {
                if o {
                    let (a, b) = ends(e);
                    degree[a] += 1;
                    degree[b] += 1;
                }
            }
            for g in &orbits {
                if g.iter().any(|&e| open[e]) {
                    continue;
                }
                let dead = g.iter().any(|&e| {
                    let (a, b) = ends(e);
                    degree[a] <= 1 || degree[b] <= 1
                });
                if dead && rng.percent(braid) {
                    for &e in g {
                        open[e] = true;
                        let (a, b) = ends(e);
                        degree[a] += 1;
                        degree[b] += 1;
                    }
                }
            }
        }
        open
    }

    /// Corridors `corridor` wide between the nodes of a lattice `pitch` apart, walls in between.
    fn maze(&mut self, st: &Step) -> Result<(), String> {
        let q = st.pitch as i32;
        self.lattice_ok(q)?;
        let w = st.corridor.max(1) as i32;
        if w % 2 == 0 || w >= q {
            return Err(format!("maze corridor {w} must be odd and under the pitch {q}"));
        }
        let (_, _, nodes) = self.nodes(q);
        let open = self.passages(q, st.seed, st.loops, st.braid, &vec![false; nodes.len() * 2]);
        let t = self.t;
        let h = (w - 1) / 2;
        let mut land = vec![false; t.cells()];
        for (i, &x) in nodes.iter().enumerate() {
            let (r, c) = t.rc(x);
            for dr in -h..=h {
                for dc in -h..=h {
                    land[t.at(r + dr, c + dc)] = true;
                }
            }
            for dir in 0..2 {
                if !open[2 * i + dir] {
                    continue;
                }
                let (lr, lc) = if dir == 0 { (h, q + h) } else { (q + h, h) };
                for dr in -h..=lr {
                    for dc in -h..=lc {
                        land[t.at(r + dr, c + dc)] = true;
                    }
                }
            }
        }
        for (x, l) in land.iter_mut().enumerate() {
            if self.in_band(x, st.band, &st.from) {
                self.water[x] = true;
            } else {
                *l = false;
            }
        }
        self.commit(&land, false);
        Ok(())
    }

    /// Rooms `pitch` across, centred on the lattice nodes, walled `corridor` thick, some walls taken
    /// down whole and a door `door` wide in enough of the rest to join every room. Where four walls
    /// meet a post stays, whether or not the walls do: columns in a hall.
    ///
    /// Measured in doubled coordinates, so the line half-way between two nodes is an integer: a
    /// square is in a wall when its doubled distance from that line is under the thickness.
    fn rooms(&mut self, st: &Step) -> Result<(), String> {
        let q = st.pitch as i32;
        self.lattice_ok(q)?;
        let wall = st.corridor.max(1) as i32;
        let door = st.door.max(1) as i32;
        // A wall centred between two nodes is symmetric only when its thickness and the pitch
        // differ in parity; a door centred on a wall only when it is odd.
        if (wall + q) % 2 == 0 || door % 2 == 0 || door + 2 > q - wall {
            return Err(format!(
                "rooms: wall {wall} and pitch {q} must differ in parity, and door {door} be odd and \
                 leave wall either side"
            ));
        }
        let (_, _, nodes) = self.nodes(q);
        let orbits = self.edge_orbits(q);
        let mut rng = Rng(st.seed ^ 0x5EED);
        let mut merged = vec![false; nodes.len() * 2];
        for g in &orbits {
            if rng.percent(st.pct) {
                for &e in g {
                    merged[e] = true;
                }
            }
        }
        let open = self.passages(q, st.seed, st.loops, 0, &merged);
        let t = self.t;
        let mut water = vec![false; t.cells()];
        let mut land = vec![false; t.cells()];
        let span = q + wall - 1;
        let reach = (q + wall) / 2 + 1;
        for (i, &x) in nodes.iter().enumerate() {
            let (r, c) = t.rc(x);
            for dir in 0..2 {
                let e = 2 * i + dir;
                // The wall's middle, doubled: half a pitch along the edge from this node.
                let (mr, mc) = if dir == 0 { (2 * r, 2 * c + q) } else { (2 * r + q, 2 * c) };
                for dr in -reach..=reach {
                    for dc in -reach..=reach {
                        let (y, z) = (mr.div_euclid(2) + dr, mc.div_euclid(2) + dc);
                        let (ur, uc) = (2 * y - mr, 2 * z - mc);
                        let (across, along) = if dir == 0 { (uc, ur) } else { (ur, uc) };
                        if across.abs() > wall - 1 || along.abs() > span {
                            continue;
                        }
                        let cell = t.at(y, z);
                        let post = along.abs() >= q - (wall - 1);
                        if post || !merged[e] {
                            water[cell] = true;
                        }
                        if open[e] && !merged[e] && along.abs() < door {
                            land[cell] = true;
                        }
                    }
                }
            }
        }
        for x in 0..t.cells() {
            if !self.in_band(x, st.band, &st.from) {
                water[x] = false;
                land[x] = false;
            }
        }
        self.commit(&water, true);
        self.commit(&land, false);
        Ok(())
    }

    /// Land in 4-connected pieces: a piece per square (`u32::MAX` for water), and each piece's size.
    fn parts(&self) -> (Vec<u32>, Vec<usize>) {
        let t = self.t;
        let mut part = vec![u32::MAX; t.cells()];
        let mut size = Vec::new();
        for x in 0..t.cells() {
            if part[x] != u32::MAX || self.water[x] {
                continue;
            }
            let id = size.len() as u32;
            let mut stack = vec![x];
            part[x] = id;
            let mut n = 0;
            while let Some(y) = stack.pop() {
                n += 1;
                for z in t.n4(y) {
                    if part[z] == u32::MAX && !self.water[z] {
                        part[z] = id;
                        stack.push(z);
                    }
                }
            }
            size.push(n);
        }
        (part, size)
    }

    /// Join every piece of land to the one seat 0's first hill stands on, by the shortest tunnel
    /// through the water between, copied to every image; pieces too small to be worth one are filled.
    fn repair(&mut self, hill: usize, keep: &[bool]) -> Result<(usize, usize), String> {
        let t = self.t;
        let (mut dug, mut filled) = (0, 0);
        for _ in 0..400 {
            let (part, size) = self.parts();
            let main = part[hill];
            if main == u32::MAX {
                return Err("repair: seat 0's hill is under water".into());
            }
            let mut crumbs = vec![false; t.cells()];
            let mut any = false;
            for x in 0..t.cells() {
                if part[x] != u32::MAX && part[x] != main && size[part[x] as usize] < 10 && !keep[x]
                {
                    crumbs[x] = true;
                    any = true;
                }
            }
            if any {
                filled += crumbs.iter().filter(|&&c| c).count();
                // Pieces come in whole orbits, so filling them all keeps the board symmetric.
                for (w, &c) in self.water.iter_mut().zip(&crumbs) {
                    if c {
                        *w = true;
                    }
                }
                continue;
            }
            let Some(start) = (0..t.cells()).find(|&x| part[x] != u32::MAX && part[x] != main)
            else {
                return Ok((dug, filled));
            };
            let c = part[start];
            let mut prev = vec![usize::MAX; t.cells()];
            let mut q = VecDeque::new();
            for x in 0..t.cells() {
                if part[x] == c {
                    prev[x] = x;
                    q.push_back(x);
                }
            }
            let mut hit = None;
            'bfs: while let Some(x) = q.pop_front() {
                for y in t.n4(x) {
                    if prev[y] != usize::MAX {
                        continue;
                    }
                    prev[y] = x;
                    if !self.water[y] && part[y] == main {
                        hit = Some(y);
                        break 'bfs;
                    }
                    if self.water[y] {
                        q.push_back(y);
                    }
                }
            }
            let Some(end) = hit else {
                return Err("repair: a piece of land cannot reach the rest".into());
            };
            let mut path = vec![false; t.cells()];
            let mut x = prev[end];
            while part[x] != c {
                path[x] = true;
                x = prev[x];
            }
            dug += path.iter().filter(|&&p| p).count();
            self.commit(&path, false);
        }
        Err("repair: the land would not join".into())
    }
}

/// A shape in seat 0's frame.
pub enum Shape {
    Disk { at: (f64, f64), r: f64, metric: String },
    Ring { at: (f64, f64), r0: f64, r1: f64, metric: String },
    Seg { a: (f64, f64), b: (f64, f64), w: f64 },
    Box { at: (f64, f64), hr: f64, hc: f64 },
}

impl Shape {
    fn of(st: &Step) -> Result<Shape, String> {
        let at = (st.at[0], st.at[1]);
        Ok(match st.shape.as_str() {
            "disk" => Shape::Disk { at, r: st.r[0], metric: st.metric.clone() },
            "ring" => Shape::Ring { at, r0: st.r[0], r1: st.r[1], metric: st.metric.clone() },
            "seg" => Shape::Seg { a: at, b: (st.to[0], st.to[1]), w: st.width },
            "box" => Shape::Box { at, hr: st.r[0], hc: st.r[1] },
            other => return Err(format!("no shape '{other}'")),
        })
    }

    /// Where the shape is, and how far from there it can reach.
    fn anchor(&self) -> ((f64, f64), f64) {
        match self {
            Shape::Disk { at, r, .. } => (*at, r * 1.5 + 1.0),
            Shape::Ring { at, r1, .. } => (*at, r1 * 1.5 + 1.0),
            Shape::Seg { a, b, w } => {
                let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                let half = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() / 2.0;
                (m, half + w + 1.0)
            }
            Shape::Box { at, hr, hc } => (*at, hr.max(*hc) * 1.5 + 1.0),
        }
    }

    fn contains(&self, q: (f64, f64)) -> bool {
        match self {
            Shape::Disk { at, r, metric: m } => metric(m, (q.0 - at.0, q.1 - at.1)) <= *r,
            Shape::Ring { at, r0, r1, metric: m } => {
                let d = metric(m, (q.0 - at.0, q.1 - at.1));
                d >= *r0 && d <= *r1
            }
            Shape::Seg { a, b, w } => seg_dist(q, *a, *b) <= w / 2.0,
            Shape::Box { at, hr, hc } => (q.0 - at.0).abs() <= *hr && (q.1 - at.1).abs() <= *hc,
        }
    }
}

pub fn metric(name: &str, (r, c): (f64, f64)) -> f64 {
    let (r, c) = (r.abs(), c.abs());
    match name {
        "l1" => r + c,
        "linf" => r.max(c),
        "oct" => r.max(c).max((r + c) / std::f64::consts::SQRT_2),
        _ => (r * r + c * c).sqrt(),
    }
}

fn seg_dist(q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vr, vc) = (b.0 - a.0, b.1 - a.1);
    let (wr, wc) = (q.0 - a.0, q.1 - a.1);
    let len2 = vr * vr + vc * vc;
    let u = if len2 == 0.0 { 0.0 } else { ((wr * vr + wc * vc) / len2).clamp(0.0, 1.0) };
    let (dr, dc) = (wr - u * vr, wc - u * vc);
    (dr * dr + dc * dc).sqrt()
}

/// A box blur, rows then columns, wrapping.
fn box_blur(t: &Torus, v: &mut [i64], b: i32) {
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

/// Render a design into a board.
pub fn render(d: &Design) -> Result<Rendered, String> {
    let mut cv = Canvas::new(d)?;
    for st in &d.steps {
        cv.step(st)?;
    }
    let (t, s, p) = (cv.t, cv.s, cv.s.seats);
    if d.hills.is_empty() {
        return Err("a design needs a hill".into());
    }
    // Seat 0's hills, then every seat's, orbit by orbit as the map file lists them.
    let hill0: Vec<usize> =
        d.hills.iter().map(|h| t.at(cv.origin.0 + h[0], cv.origin.1 + h[1])).collect();
    let mut keep = vec![false; t.cells()];
    let mut clear = vec![false; t.cells()];
    for &h in &hill0 {
        let (r, c) = t.rc(h);
        for dr in -CLEARING..=CLEARING {
            for dc in -CLEARING..=CLEARING {
                clear[t.at(r + dr, c + dc)] = true;
            }
        }
    }
    cv.commit(&clear, false);
    for (x, _) in clear.iter().enumerate().filter(|(_, c)| **c) {
        {
            for &y in &cv.orb.members[cv.orb.rep[x] as usize] {
                keep[y as usize] = true;
            }
        }
    }
    let (repaired, filled) = cv.repair(hill0[0], &keep)?;
    let mut hills = Vec::new();
    for &h in &hill0 {
        for k in 0..p {
            hills.push(s.image(&t, h, k));
        }
    }
    let mut sorted = hills.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != hills.len() {
        return Err("two hills share a square".into());
    }
    let food = place_food(&cv, &hills, &hill0, &d.food)?;
    Ok(Rendered { board: Board { t, s, water: cv.water.clone(), hills, food }, repaired, filled })
}

fn place_food(
    cv: &Canvas,
    hills: &[usize],
    hill0: &[usize],
    plan: &FoodPlan,
) -> Result<Vec<usize>, String> {
    let (t, s, p) = (cv.t, cv.s, cv.s.seats);
    let water = |x: usize| cv.water[x];
    let mut rng = Rng(plan.seed);
    let mut taken = vec![false; t.cells()];
    for &h in hills {
        taken[h] = true;
    }
    let mut food: Vec<usize> = Vec::new();
    let mut is_food = vec![false; t.cells()];
    let mut per_seat = 0u32;

    let orbit_of = |x: usize| -> &Vec<u32> { &cv.orb.members[cv.orb.rep[x] as usize] };
    let fits = |x: usize, is_food: &Vec<bool>, taken: &Vec<bool>| -> bool {
        let o = orbit_of(x);
        o.iter().all(|&y| {
            let y = y as usize;
            !cv.water[y]
                && !taken[y]
                && t.n4(y).iter().all(|&z| !is_food[z] && !(o.contains(&(z as u32)) && z != y))
        })
    };
    let add = |x: usize, food: &mut Vec<usize>, is_food: &mut Vec<bool>| -> u32 {
        let o = orbit_of(x).clone();
        for &y in &o {
            food.push(y as usize);
            is_food[y as usize] = true;
        }
        (o.len() / p) as u32
    };

    // Bootstrap, hill by hill: food a short walk away.
    for &h in hill0 {
        let d = walk(&t, water, &[h]);
        let mut near: Vec<usize> = (0..t.cells()).filter(|&x| d[x] >= 2 && d[x] <= 6).collect();
        rng.shuffle(&mut near);
        let mut got = near.iter().filter(|&&x| is_food[x]).count() as u32;
        for x in near {
            if got >= plan.bootstrap {
                break;
            }
            if is_food[x] || !fits(x, &is_food, &taken) {
                continue;
            }
            let o = orbit_of(x).clone();
            let here =
                o.iter().filter(|&&y| d[y as usize] >= 2 && d[y as usize] <= 6).count() as u32;
            per_seat += add(x, &mut food, &mut is_food);
            got += here.max(1);
        }
    }

    let own = walk(&t, water, hill0);
    let other = |x: usize| (1..p).map(|k| own[s.image(&t, x, p - k)]).min().unwrap_or(FAR);
    let target = plan.per_seat;

    // The borders' middles and corners: contested ground by construction.
    if plan.sites > 0 {
        let mut spots: Vec<(i64, usize)> = Vec::new();
        for &(sr, sc) in cv.mids.iter().chain(cv.corners.iter()) {
            let (r, c) = (
                (cv.origin.0 as f64 + sr).round() as i32,
                (cv.origin.1 as f64 + sc).round() as i32,
            );
            for dr in -3..=3 {
                for dc in -3..=3 {
                    let x = t.at(r + dr, c + dc);
                    spots.push(((dr * dr + dc * dc) as i64 * 1000 + rng.below(1000) as i64, x));
                }
            }
        }
        spots.sort_unstable();
        let mut placed = 0;
        for (_, x) in spots {
            if placed >= plan.sites || per_seat >= target {
                break;
            }
            if is_food[x] || own[x] == FAR || !fits(x, &is_food, &taken) {
                continue;
            }
            let n = (orbit_of(x).len() / p) as u32;
            if per_seat + n > target {
                continue;
            }
            per_seat += add(x, &mut food, &mut is_food);
            placed += n;
        }
    }

    let band = 6u32;
    let mut pool: Vec<usize> = (0..t.cells())
        .filter(|&x| !cv.water[x] && own[x] != FAR)
        .filter(|&x| match plan.bias.as_str() {
            "home" => own[x].saturating_add(band) < other(x),
            "contested" => own[x].abs_diff(other(x)) <= band,
            _ => true,
        })
        .collect();
    let mut anywhere: Vec<usize> = (0..t.cells()).filter(|&x| !cv.water[x]).collect();
    rng.shuffle(&mut pool);
    rng.shuffle(&mut anywhere);
    for x in pool.into_iter().chain(anywhere) {
        if per_seat >= target {
            break;
        }
        if is_food[x] || !fits(x, &is_food, &taken) {
            continue;
        }
        let n = (orbit_of(x).len() / p) as u32;
        if per_seat + n > target {
            continue;
        }
        per_seat += add(x, &mut food, &mut is_food);
    }
    if per_seat == 0 {
        return Err("food: no room for any".into());
    }
    Ok(food)
}

/// The map file: the engine's own shape, plus the design's provenance.
pub fn to_json(d: &Design, b: &Board, m: &Metrics) -> Value {
    let rc = |x: &usize| {
        let (row, col) = b.t.rc(*x);
        json!([row, col])
    };
    json!({
        "id": d.name,
        "rows": b.t.rows,
        "cols": b.t.cols,
        "players": b.s.seats,
        "symmetry": { "dr": b.s.dr, "dc": b.s.dc },
        "water": rle(&b.water),
        "hills": b.hills.iter().map(rc).collect::<Vec<_>>(),
        "food": b.food.iter().map(rc).collect::<Vec<_>>(),
        "food_target": b.food.len(),
        "generator": { "recipe": d.name, "family": d.family, "symmetry": d.symmetry, "metrics": m },
    })
}

impl Design {
    pub fn load(path: &std::path::Path) -> Result<Design, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let d: Design = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if d.name != stem {
            return Err(format!(
                "{}: names the board '{}'; a recipe is named after its board",
                path.display(),
                d.name
            ));
        }
        Ok(d)
    }

    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|e| e.to_string())
    }
}

/// A map file exactly as written, so `check` can compare bytes.
pub fn file_text(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default() + "\n"
}

/// Render, measure, and have the engine accept it: a design that fails any is refused whole.
pub fn build(d: &Design) -> Result<(Rendered, Metrics, Value), String> {
    let r = render(d)?;
    let m = measure(&r.board)?;
    let v = to_json(d, &r.board, &m);
    crate::set::engine_accepts(&v)?;
    Ok((r, m, v))
}
