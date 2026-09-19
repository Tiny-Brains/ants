//! The symmetry a designed board is drawn under.
//!
//! **Two symmetries, for two reasons.** The seat shift is fairness: seat `k`'s board is seat 0's
//! moved `k` times, which the engine checks and the game relies on. The point group is decoration: a
//! mirror or a rotation about every seat's centre, which nothing checks and nothing needs. It is
//! what makes a board look drawn rather than rolled — a base laid out like a snowflake, a pinwheel,
//! a pair of mirrored wings — and it costs nothing in fairness, because each of its maps carries
//! the seats' centres onto the seats' centres, so the two together are still one group.
//!
//! A point group is a subgroup of the square's eight symmetries, and not every one fits every
//! board: a rotation needs a square board, and every map must carry the shift into the shifts
//! (`fits`). Two players on a square board take all eight; five take the pinwheel; three, six and
//! seven, whose centres cannot sit on a square lattice, take a half-turn or a mirror.
//!
//! Everything here is integer or IEEE basic arithmetic (`+ - * /` and `sqrt`, which are correctly
//! rounded everywhere), so a board is the same on every machine that renders its design.

use crate::grid::{Shift, Torus};

/// `(r, c) -> (m0 r + m1 c, m2 r + m3 c)`: one of the square's eight symmetries.
pub type Mat = [i32; 4];

pub const ID: Mat = [1, 0, 0, 1];
const R90: Mat = [0, 1, -1, 0];
const R180: Mat = [-1, 0, 0, -1];
const R270: Mat = [0, -1, 1, 0];
/// Rows flipped: a mirror in the horizontal line through the centre.
const MR: Mat = [-1, 0, 0, 1];
/// Columns flipped: a mirror in the vertical line.
const MC: Mat = [1, 0, 0, -1];
/// The main diagonal.
const TR: Mat = [0, 1, 1, 0];
/// The other diagonal.
const AT: Mat = [0, -1, -1, 0];

/// Every point group a design may name, largest first.
pub const GROUPS: [&str; 10] = ["d4", "c4", "d2", "d2x", "c2", "d1r", "d1c", "d1t", "d1a", "c1"];

pub fn group(name: &str) -> Option<Vec<Mat>> {
    Some(match name {
        "c1" => vec![ID],
        "c2" => vec![ID, R180],
        "d1r" => vec![ID, MR],
        "d1c" => vec![ID, MC],
        "d1t" => vec![ID, TR],
        "d1a" => vec![ID, AT],
        "d2" => vec![ID, R180, MR, MC],
        "d2x" => vec![ID, R180, TR, AT],
        "c4" => vec![ID, R90, R180, R270],
        "d4" => vec![ID, R90, R180, R270, MR, MC, TR, AT],
        _ => return None,
    })
}

#[inline]
pub fn apply(m: &Mat, (r, c): (i32, i32)) -> (i32, i32) {
    (m[0] * r + m[1] * c, m[2] * r + m[3] * c)
}

#[inline]
pub fn applyf(m: &Mat, (r, c): (f64, f64)) -> (f64, f64) {
    (m[0] as f64 * r + m[1] as f64 * c, m[2] as f64 * r + m[3] as f64 * c)
}

/// The square's symmetries are orthogonal, so the inverse is the transpose.
#[inline]
pub fn inverse(m: &Mat) -> Mat {
    [m[0], m[2], m[1], m[3]]
}

/// Whether `m` is a map of this torus at all, and carries the seat shift into the shifts.
pub fn fits(t: &Torus, s: &Shift, m: &Mat) -> bool {
    if (m[1] != 0 || m[2] != 0) && t.rows != t.cols {
        return false;
    }
    let (r, c) = apply(m, (s.dr, s.dc));
    (0..s.seats as i32)
        .any(|k| (r - k * s.dr).rem_euclid(t.rows) == 0 && (c - k * s.dc).rem_euclid(t.cols) == 0)
}

pub fn group_fits(t: &Torus, s: &Shift, name: &str) -> bool {
    group(name).is_some_and(|g| g.iter().all(|m| fits(t, s, m)))
}

/// The largest point groups this board and shift admit, largest first.
pub fn admitted(t: &Torus, s: &Shift) -> Vec<&'static str> {
    GROUPS.iter().copied().filter(|g| group_fits(t, s, g)).collect()
}

/// The lattice the seats' centres lift to in the plane: every `k·shift + (a·rows, b·cols)`.
pub struct Lattice {
    /// The shortest distance between two seats' centres.
    pub spacing: f64,
    /// Halfway to each neighbouring centre, from seat 0's: the middle of each border.
    pub mids: Vec<(f64, f64)>,
    /// Where three or four seats' territories meet: the corners of seat 0's territory.
    pub corners: Vec<(f64, f64)>,
}

pub fn lattice(t: &Torus, s: &Shift) -> Lattice {
    let p = s.seats as i32;
    let mut pts: Vec<(i64, i64)> = Vec::new();
    for k in 0..p {
        for a in -3..=3 {
            for b in -3..=3 {
                let v = ((k * s.dr + a * t.rows) as i64, (k * s.dc + b * t.cols) as i64);
                if v != (0, 0) {
                    pts.push(v);
                }
            }
        }
    }
    pts.sort_unstable_by_key(|&(r, c)| (r * r + c * c, r, c));
    pts.dedup();
    let n2 = |(r, c): (i64, i64)| r * r + c * c;
    let spacing2 = n2(pts[0]);

    // A vector is relevant when its midpoint is nearer 0 and it than any other lattice point.
    let relevant: Vec<(i64, i64)> = pts
        .iter()
        .copied()
        .filter(|&w| {
            let (mr, mc) = (w.0 as f64 / 2.0, w.1 as f64 / 2.0);
            let half = (mr * mr + mc * mc) * (1.0 - 1e-9);
            n2(w) < 16 * spacing2
                && pts.iter().all(|&o| {
                    o == w || {
                        let (dr, dc) = (mr - o.0 as f64, mc - o.1 as f64);
                        dr * dr + dc * dc > half
                    }
                })
        })
        .collect();
    let mut by_angle = relevant.clone();
    by_angle.sort_by(|&a, &b| angle_order(a, b));
    let mids = by_angle.iter().map(|&(r, c)| (r as f64 / 2.0, c as f64 / 2.0)).collect();
    let mut corners = Vec::new();
    for i in 0..by_angle.len() {
        let (a, b) = (by_angle[i], by_angle[(i + 1) % by_angle.len()]);
        let det = (a.0 * b.1 - a.1 * b.0) as f64;
        if det.abs() < 1e-9 {
            continue;
        }
        let (ea, eb) = (n2(a) as f64 / 2.0, n2(b) as f64 / 2.0);
        corners.push((
            (ea * b.1 as f64 - a.1 as f64 * eb) / det,
            (a.0 as f64 * eb - ea * b.0 as f64) / det,
        ));
    }
    Lattice { spacing: (spacing2 as f64).sqrt(), mids, corners }
}

/// Counter-clockwise from the positive column axis, with no trigonometry: the half-plane first,
/// then the cross product.
fn angle_order(a: (i64, i64), b: (i64, i64)) -> std::cmp::Ordering {
    let half = |(r, c): (i64, i64)| if r < 0 || (r == 0 && c > 0) { 0 } else { 1 };
    half(a).cmp(&half(b)).then_with(|| (b.0 * a.1 - b.1 * a.0).cmp(&0).reverse())
}

/// The full group's orbits: the seat shift and the point group about `origin`, together.
pub struct Orbits {
    /// Each square's orbit, as the smallest square in it.
    pub rep: Vec<u32>,
    /// Each orbit's squares, indexed by its representative (empty for every other square).
    pub members: Vec<Vec<u32>>,
}

pub fn orbits(t: &Torus, s: &Shift, ops: &[Mat], origin: (i32, i32)) -> Orbits {
    let n = t.cells();
    let mut uf: Vec<u32> = (0..n as u32).collect();
    fn find(uf: &mut [u32], mut x: u32) -> u32 {
        while uf[x as usize] != x {
            uf[x as usize] = uf[uf[x as usize] as usize];
            x = uf[x as usize];
        }
        x
    }
    for x in 0..n {
        let (r, c) = t.rc(x);
        let mut images = vec![t.at(r + s.dr, c + s.dc)];
        for m in ops {
            let (a, b) = apply(m, (r - origin.0, c - origin.1));
            images.push(t.at(origin.0 + a, origin.1 + b));
        }
        for y in images {
            let (ra, rb) = (find(&mut uf, x as u32), find(&mut uf, y as u32));
            if ra != rb {
                uf[ra.max(rb) as usize] = ra.min(rb);
            }
        }
    }
    let mut rep = vec![0u32; n];
    let mut members = vec![Vec::new(); n];
    for (x, slot) in rep.iter_mut().enumerate() {
        let r = find(&mut uf, x as u32);
        *slot = r;
        members[r as usize].push(x as u32);
    }
    Orbits { rep, members }
}
