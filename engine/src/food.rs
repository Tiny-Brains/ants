//! Food, at the match's hidden rate — the last step of a turn.
//!
//! The reference model, from `ants.py`'s `do_food_symmetric`. Three parts:
//!
//!   1. A hidden rate. `food_rate * players / food_turn` food accrues per turn, kept exactly as a
//!      rational (`ants.py:1464`). Whole food is spawned; the remainder carries.
//!   2. Symmetric sets, shuffled, each used once per rotation. That is what makes food fair *and*
//!      unpredictable: you cannot camp a square, but you also cannot be starved while your opponent
//!      is fed.
//!   3. A queue. Food owed to an occupied square is not lost; it is placed when the square frees.

use crate::grid::Rng;
use crate::state::Match;

/// The hidden food rate's range, drawn per match exactly as the reference draws it
/// (`ants.py:54-59`). The specification only says "each game has a hidden food rate", so the engine
/// is the only statement of what the rate actually is.
const FOOD_RATE: (u32, u32) = (5, 11);
const FOOD_TURN: (u32, u32) = (19, 37);

/// Draw a match's hidden food rate from its seed.
pub fn rate_for(seed: u64) -> (u16, u16) {
    let mut r = Rng(seed ^ 0xF00D_5EED_A11E_2C1D);
    let rate = FOOD_RATE.0 + r.below(FOOD_RATE.1 - FOOD_RATE.0 + 1);
    let turn = FOOD_TURN.0 + r.below(FOOD_TURN.1 - FOOD_TURN.0 + 1);
    (rate as u16, turn as u16)
}

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
///
/// This is a pass over every square of the board, run each time food falls due, and it was the
/// engine's largest single cost. It gives the same answer without building any orbit, because each
/// exclusion is a test on one image at a time: the square is its orbit's smallest member when no
/// image is smaller, and a set is clear of water, hills and touching when every image is. The square
/// is itself image 0, so water and hills — one bitmap, since a hill never moves and a razed one
/// keeps its square — settle most of the board before any image is looked at. And along a row only
/// the column moves, so each image's row is wrapped once a row and its column once a board, instead
/// of two divisions for every image of every square.
pub fn sets(m: &Match) -> Vec<u16> {
    let (rows, cols) = (m.g.rows, m.g.cols);
    let mut blocked = m.water.clone();
    for h in &m.hills {
        blocked.set(h.pos as usize);
    }
    // Image k of (r, c) is `at(r + dr·k, c + dc·k)`, as `Symmetry::image` has it.
    let ks: Vec<i32> = (1..m.players as i32).collect();
    let col_shift: Vec<i32> = ks.iter().map(|&k| (m.sym.dc * k).rem_euclid(cols)).collect();
    let mut image_row = vec![0i32; ks.len()];

    let mut out = Vec::new();
    for r in 0..rows {
        for (i, &k) in ks.iter().enumerate() {
            image_row[i] = (r + m.sym.dr * k).rem_euclid(rows);
        }
        'square: for c in 0..cols {
            let pos = r * cols + c;
            if blocked.get(pos as usize) {
                continue;
            }
            for (&ir, &shift) in image_row.iter().zip(&col_shift) {
                let ic = if c + shift >= cols { c + shift - cols } else { c + shift };
                let p = ir * cols + ic;
                if p < pos || blocked.get(p as usize) || m.g.dist2_rc(r, c, ir, ic) == 1 {
                    continue 'square;
                }
            }
            out.push(pos as u16);
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
pub fn shuffled(sets: &[u16], seed: u64, rotation: u16) -> Vec<u16> {
    let mut v = sets.to_vec();
    let mut r = Rng(seed ^ 0x5E75_C0DE_0000_0000 ^ rotation as u64);
    for i in (1..v.len()).rev() {
        v.swap(i, r.below(i as u32 + 1) as usize);
    }
    v
}

/// One turn's food, at the hidden rate.
pub fn spawn(m: &mut Match) {
    if m.food_rate > 0 && m.food_turn > 0 {
        m.food_extra += m.food_rate as u32 * m.players as u32;
        let due = m.food_extra / m.food_turn as u32;
        if due > 0 {
            let mut amount = due;
            let sets = sets(m);
            if !sets.is_empty() {
                let mut order = shuffled(&sets, m.seed, m.food_rotation);
                let mut buf = [0u16; 16];
                // Whole sets only: a set is spawned when the accrued food covers all of it, and
                // what is left over stays accrued. Spawning half a set would be an asymmetric map.
                loop {
                    if m.food_cursor as usize >= order.len() {
                        m.food_rotation = m.food_rotation.wrapping_add(1);
                        m.food_cursor = 0;
                        order = shuffled(&sets, m.seed, m.food_rotation);
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
