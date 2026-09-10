//! Food spawning at the match's hidden rate — `state::spawn_food`.

use super::*;
use crate::state::Ant;

#[test]
fn food_accrues_at_the_hidden_rate() {
    // `ants.py:1464`: `food_extra += Fraction(food_rate * players, food_turn)` each turn, and whole
    // food is spawned. Over 100 turns at 6 per 34 with two seats that is 600/34 = 17 whole food.
    // Nothing here gathers, so what is on the board is exactly what was spawned.
    let mut m = fed(40, 40, 6, 34);
    for _ in 0..100 {
        play(&mut m, &["-"], &["-"]);
        if m.done {
            break;
        }
    }
    let expected = 6 * 2 * 100 / 34;
    // Whole sets only, so the count lands on a multiple of the set size at or just below the rate.
    assert!(
        m.food.len() <= expected as usize && m.food.len() + 2 > expected as usize,
        "expected about {expected} food, got {}",
        m.food.len()
    );
}

#[test]
fn a_zero_rate_spawns_no_food_at_all() {
    // The reference's `do_food_none`, and what every rule test above relies on.
    let mut m = fed(40, 40, 0, 0);
    for _ in 0..50 {
        play(&mut m, &["-"], &["-"]);
    }
    assert!(m.food.is_empty());
}

#[test]
fn food_spawns_in_whole_symmetric_sets() {
    // *Food spawning*, as the specification states it for spawning: "Food spawning is done symmetrically."
    // Every food on the board must have its mirror, or one seat is being fed and the other is not.
    let mut m = fed(40, 40, 11, 19);
    for _ in 0..60 {
        play(&mut m, &["-"], &["-"]);
        if m.done {
            break;
        }
    }
    assert!(!m.food.is_empty(), "the rate should have produced food");
    for &f in &m.food {
        let mirror = m.sym.image(&m.g, f, 1);
        assert!(m.food.contains(&mirror), "food at {f} has no symmetric partner");
    }
}

#[test]
fn every_set_spawns_once_before_any_set_spawns_twice() {
    // "Every set will spawn at least once before a set spawns a second time. This means if you see
    // food spawn, it may be awhile before it spawns again, unless it was the last set of the random
    // order and was then shuffled to be the first set of the next random order."
    //
    // Checked directly on the rotation the engine derives, rather than through play: the order is
    // Fisher-Yates over the board's sets, so a rotation must be a permutation of them.
    let m = fed(20, 20, 6, 34);
    let sets = crate::state::food_sets(&m);
    assert!(sets.len() > 10, "the board should have plenty of sets");

    let mut seen = std::collections::HashSet::new();
    let mut order = Vec::new();
    for cursor in 0..sets.len() {
        let rep = crate::state::shuffled_sets(&sets, m.seed, 0)[cursor];
        assert!(seen.insert(rep), "set {rep} appeared twice inside one rotation");
        order.push(rep);
    }
    let mut sorted = order.clone();
    sorted.sort_unstable();
    let mut canonical = sets.clone();
    canonical.sort_unstable();
    assert_eq!(sorted, canonical, "a rotation is a permutation of every set");
    assert_ne!(order, canonical, "and it is shuffled, not the canonical order");

    // The next rotation is a different order over the same sets.
    let next: Vec<u16> =
        (0..sets.len()).map(|c| crate::state::shuffled_sets(&sets, m.seed, 1)[c]).collect();
    assert_ne!(next, order, "each rotation is shuffled again");
}

#[test]
fn food_sets_skip_hills_water_and_squares_whose_mirrors_touch() {
    // `ants.py:1306` skips hills; the specification excludes sets whose members touch ("It would be
    // unfair to spawn so much food in one place"); and water is skipped so a maze board's rate
    // means what it says.
    let mut m = fed(20, 20, 6, 34);
    let wet = m.g.at(7, 7);
    for p in m.sym.orbit(&m.g, wet) {
        m.water.set(p as usize);
    }
    let sets = crate::state::food_sets(&m);

    for &rep in &sets {
        for p in m.sym.orbit(&m.g, rep) {
            assert!(!m.water.get(p as usize), "a set reached water at {p}");
            assert!(!m.hills.iter().any(|h| h.pos == p), "a set reached a hill at {p}");
        }
    }
    assert!(!sets.contains(&wet), "the flooded set is gone");

    // On a 20x20 board with two seats the symmetry is a shift of (10, 10), so no two members of a
    // set are ever adjacent and the dead-zone rule excludes nothing here. A 1x2 board is the
    // smallest one whose shift is a single square, and there every set touches itself.
    let touching = bare(1, 2, 2);
    assert_eq!(touching.sym.dr, 0);
    assert_eq!(touching.sym.dc, 1, "mirrors land side by side");
    assert!(crate::state::food_sets(&touching).is_empty(), "touching sets are all refused");
}

#[test]
fn food_owed_to_an_occupied_square_is_placed_when_it_frees() {
    // "Food that can't be placed is put into a queue and is placed as soon as the location becomes
    // available" (`ants.py:1105`). The rate is honoured even when the board is crowded.
    let mut m = fed(20, 20, 6, 34);
    // Stand an ant on every square of one set, and its mirror on the mirror.
    let sets = crate::state::food_sets(&m);
    let rep = crate::state::shuffled_sets(&sets, m.seed, 0)[0];
    m.ants.clear();
    for (k, p) in m.sym.orbit(&m.g, rep).into_iter().enumerate() {
        m.ants.push(Ant { pos: p, owner: k as u8 });
    }

    // Run until that first set comes due; the ants are standing on it, so it cannot be placed.
    for _ in 0..12 {
        play(&mut m, &["-"], &["-"]);
    }
    assert!(!m.pending_food.is_empty(), "the food is owed, not lost");
    assert!(!m.food.contains(&rep), "and not on the board while an ant is standing there");

    // Move them off and it lands.
    m.ants.clear();
    m.ants.push(Ant { pos: m.g.at(1, 1), owner: 0 });
    m.ants.push(Ant { pos: m.g.at(11, 11), owner: 1 });
    play(&mut m, &["-"], &["-"]);
    assert!(m.food.contains(&rep), "the queued food was placed as soon as the square freed");
}

#[test]
fn the_hidden_rate_is_drawn_from_the_seed() {
    // "Each game has a hidden food rate." Two matches on the same board are not the same match, and
    // a competitor cannot learn one game's rate and plan around it in the next.
    let rates: std::collections::HashSet<(u16, u16)> =
        (0..64u64).map(crate::state::food_rate_for).collect();
    assert!(rates.len() > 8, "the rate varies with the seed, got {rates:?}");
    for (rate, per) in rates {
        assert!((5..=11).contains(&rate), "rate {rate} outside the reference range");
        assert!((19..=37).contains(&per), "period {per} outside the reference range");
    }
}
