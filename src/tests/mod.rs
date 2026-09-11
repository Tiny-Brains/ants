//! The rules, checked one at a time.
//!
//! The published rules of Ants are the specification and this is where they are enforced: the
//! game's schemas are documentation the platform never loads, so nothing at runtime will catch a
//! rule implemented wrongly. Each test names the rule it is for.

mod ending;
mod equivalence;
mod food;
mod maps;
mod replay;
mod rules;
mod spec;
mod wave;

use crate::authoring::worldgen;
use crate::map::{Bits, Geom, Rng};
use crate::state::{Ant, Hill, Match};
use crate::turn::step;
use crate::*;
use serde_json::{Value, json};

/// A hand-built match, so a rule can be tested in isolation rather than fished out of a real game:
/// no water, no food, no hills unless the test adds them, and no food spawning — `food_rate: 0` is
/// the reference's `do_food_none`.
fn bare(rows: u8, cols: u8, players: u8) -> Match {
    let g = Geom::new(rows, cols);
    let mut m = Match::new(1, g, players, 1000, Bits::zeros(g.cells()));
    m.food_rate = 0;
    m.food_turn = 0;
    m
}

fn at(m: &Match, r: i32, c: i32) -> u16 {
    m.g.at(r, c)
}

/// Orders for one seat, in `mine` order, as characters for brevity.
fn orders(spec: &[&str]) -> Vec<String> {
    spec.iter().map(|s| s.to_string()).collect()
}

fn play(m: &mut Match, a: &[&str], b: &[&str]) {
    step(m, &[orders(a), orders(b)]);
}

/// Two hills, two ants, equal footing — the shape most rule tests need so that neither the lone
/// survivor nor the rank-stabilized cutoff (*Cutoff Rules*) ends the match before the rule under
/// test runs.
fn two_sided(rows: u8, cols: u8) -> Match {
    let mut m = bare(rows, cols, 2);
    let h0 = m.g.at(0, 0);
    let h1 = m.g.at(rows as i32 / 2, cols as i32 / 2);
    m.hills.push(Hill { pos: h0, owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: h1, owner: 1, razed: false, last_touched: 0 });
    m
}

/// Place `(row, col, owner)` ants and resolve one quiet turn.
fn fight(rows: u8, cols: u8, players: u8, ants: &[(i32, i32, u8)]) -> Match {
    let mut m = bare(rows, cols, players);
    for &(r, c, owner) in ants {
        let pos = m.g.at(r, c);
        m.ants.push(Ant { pos, owner });
    }
    let no: Vec<Vec<String>> = (0..players).map(|_| Vec::new()).collect();
    step(&mut m, &no);
    m
}

fn survivors(m: &Match) -> Vec<(i32, i32, u8)> {
    let mut v: Vec<(i32, i32, u8)> = m
        .ants
        .iter()
        .map(|a| {
            let (r, c) = m.g.rc(a.pos);
            (r, c, a.owner)
        })
        .collect();
    v.sort_unstable();
    v
}

/// A board with a hidden rate set by hand, so the accrual can be watched exactly.
fn fed(rows: u8, cols: u8, rate: u16, per: u16) -> Match {
    let mut m = two_sided(rows, cols);
    m.food_rate = rate;
    m.food_turn = per;
    m.ants.push(Ant { pos: m.g.at(3, 3), owner: 0 });
    m.ants.push(Ant { pos: m.g.at(rows as i32 - 3, cols as i32 - 3), owner: 1 });
    m
}

fn random_actions(views: &Value, rng: &mut Rng) -> Vec<Value> {
    const D: [&str; 5] = ["N", "E", "S", "W", "-"];
    views["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let n = v["view"]["mine"].as_array().unwrap().len();
            json!((0..n).map(|_| D[rng.below(5) as usize]).collect::<Vec<_>>())
        })
        .collect()
}
