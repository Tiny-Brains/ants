//! The torus, the shift that seats every player on it, and the seeded randomness boards are drawn
//! from.
//!
//! Integer arithmetic throughout, for the same reason the engine uses it: a committed board must be
//! reproducible from its recipe on any machine, and `check` proves it byte for byte. Nothing here
//! iterates a hash map, whose order changes from one process to the next.

/// Moves in the protocol's order: N, E, S, W. Ants move only along these, so connectivity is always
/// 4-connectivity: land touching only at a corner is not a path.
pub const N4: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Torus {
    pub rows: i32,
    pub cols: i32,
}

impl Torus {
    pub fn cells(&self) -> usize {
        (self.rows * self.cols) as usize
    }

    #[inline]
    pub fn at(&self, r: i32, c: i32) -> usize {
        (r.rem_euclid(self.rows) * self.cols + c.rem_euclid(self.cols)) as usize
    }

    #[inline]
    pub fn rc(&self, i: usize) -> (i32, i32) {
        (i as i32 / self.cols, i as i32 % self.cols)
    }

    #[inline]
    pub fn step(&self, i: usize, dr: i32, dc: i32) -> usize {
        let (r, c) = self.rc(i);
        self.at(r + dr, c + dc)
    }

    /// The shorter way round on each axis, as unsigned distances.
    #[inline]
    pub fn gap(&self, a: usize, b: usize) -> (i64, i64) {
        let (ar, ac) = self.rc(a);
        let (br, bc) = self.rc(b);
        let dr = (ar - br).abs().min(self.rows - (ar - br).abs());
        let dc = (ac - bc).abs().min(self.cols - (ac - bc).abs());
        (dr as i64, dc as i64)
    }

    #[inline]
    pub fn dist2(&self, a: usize, b: usize) -> i64 {
        let (dr, dc) = self.gap(a, b);
        dr * dr + dc * dc
    }

    /// The walk between two squares on a board with no water.
    pub fn manhattan(&self, a: usize, b: usize) -> i64 {
        let (dr, dc) = self.gap(a, b);
        dr + dc
    }

    pub fn chebyshev(&self, a: usize, b: usize) -> i64 {
        let (dr, dc) = self.gap(a, b);
        dr.max(dc)
    }

    #[inline]
    pub fn n4(&self, i: usize) -> [usize; 4] {
        N4.map(|(dr, dc)| self.step(i, dr, dc))
    }
}

/// Seat `k`'s board is seat 0's moved by `k · (dr, dc)` — the engine's `grid::Symmetry`, which reads
/// the same two numbers back out of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shift {
    pub seats: usize,
    pub dr: i32,
    pub dc: i32,
}

impl Shift {
    #[inline]
    pub fn image(&self, t: &Torus, i: usize, k: usize) -> usize {
        let (r, c) = t.rc(i);
        t.at(r + self.dr * k as i32, c + self.dc * k as i32)
    }

    /// Whether `seats` steps come back to the start and no fewer do.
    pub fn is_exact(&self, t: &Torus) -> bool {
        let home =
            |k: i32| (self.dr * k).rem_euclid(t.rows) == 0 && (self.dc * k).rem_euclid(t.cols) == 0;
        self.seats >= 2 && home(self.seats as i32) && (1..self.seats as i32).all(|k| !home(k))
    }

    /// The squares `k · (dr, dc)` lands on, for every `k`: the subgroup this shift generates. Two
    /// shifts that generate the same one are the same seating with the seats numbered the other way.
    fn subgroup(&self, t: &Torus) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.seats).map(|k| self.image(t, 0, k)).collect();
        v.sort_unstable();
        v
    }
}

/// Where each square sits in its orbit: `cell == image(rep[cell], k[cell])`, with `rep` the orbit's
/// smallest square. Every decision a stage makes is made once, on the representative, and written to
/// the whole orbit — which is what makes a board symmetric by construction rather than by luck.
pub struct Orbits {
    pub rep: Vec<u32>,
    pub k: Vec<u8>,
}

pub fn orbits(t: &Torus, s: &Shift) -> Orbits {
    let n = t.cells();
    let mut rep = vec![u32::MAX; n];
    let mut k = vec![0u8; n];
    for x in 0..n {
        if rep[x] != u32::MAX {
            continue;
        }
        for j in 0..s.seats {
            let y = s.image(t, x, j);
            rep[y] = x as u32;
            k[y] = j as u8;
        }
    }
    Orbits { rep, k }
}

/// The shifts a recipe names, resolved on a board.
///
/// `diagonal` is one `seats`-th of the way down both axes, `rows` down the rows only, `cols` along
/// the columns only, and `any` every shift of exactly the right order, one per seating. The first
/// three need the axis they move along to divide by the seat count.
pub fn shifts(names: &[String], t: &Torus, seats: usize) -> Result<Vec<Shift>, String> {
    let p = seats as i32;
    let mut out: Vec<Shift> = Vec::new();
    let push = |s: Shift, out: &mut Vec<Shift>| {
        if s.is_exact(t) && !out.iter().any(|o| o.subgroup(t) == s.subgroup(t)) {
            out.push(s);
        }
    };
    for name in names {
        let need = |axis: i32, what: &str| {
            if axis % p == 0 {
                Ok(axis / p)
            } else {
                Err(format!("shift '{name}': {what} {axis} does not divide by {seats} seats"))
            }
        };
        match name.as_str() {
            "diagonal" => push(
                Shift { seats, dr: need(t.rows, "rows")?, dc: need(t.cols, "cols")? },
                &mut out,
            ),
            "rows" => push(Shift { seats, dr: need(t.rows, "rows")?, dc: 0 }, &mut out),
            "cols" => push(Shift { seats, dr: 0, dc: need(t.cols, "cols")? }, &mut out),
            "any" => {
                for dr in (0..t.rows).filter(|dr| (dr * p) % t.rows == 0) {
                    for dc in (0..t.cols).filter(|dc| (dc * p) % t.cols == 0) {
                        push(Shift { seats, dr, dc }, &mut out);
                    }
                }
            }
            other => return Err(format!("no shift '{other}': diagonal, rows, cols or any")),
        }
    }
    if out.is_empty() {
        return Err(format!(
            "no shift of order {seats} fits a {}x{} board from {names:?}",
            t.rows, t.cols
        ));
    }
    Ok(out)
}

/// splitmix64, as the engine draws its own randomness.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, n)`, rejecting the biased tail.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let limit = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next();
            if v < limit {
                return v % n;
            }
        }
    }

    /// Uniform in `[lo, hi]`.
    pub fn within(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo { lo } else { lo + self.below((hi - lo + 1) as u64) as u32 }
    }

    pub fn percent(&mut self, pct: u32) -> bool {
        (self.below(100) as u32) < pct
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i as u64 + 1) as usize);
        }
    }
}

/// One seed from two, so a set's boards and a board's attempts each draw their own sequence.
pub fn mix(a: u64, b: u64) -> u64 {
    Rng(a ^ b.wrapping_mul(0xD6E8_FEB8_6659_FD93)).next()
}

pub const fn isqrt(n: i64) -> i64 {
    let mut r = 0;
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}
