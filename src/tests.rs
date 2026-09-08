//! The rules, checked one at a time.
//!
//! The published rules of Ants are the specification and this is where they are enforced: the game's schemas are
//! documentation the platform never loads (docs/protocol.md §6), so nothing at runtime will catch a
//! rule implemented wrongly. Each test names the rule it is for.

use super::*;
use crate::map::{Geom, Rng};
use crate::state::{worldgen, Ant, Hill, Match};
use crate::turn::{ranks, step};

// A tiny hand-built match, so a rule can be tested in isolation rather than fished out of a real
// game. Two players, no water, no food, no hills unless the test adds them.
fn bare(rows: u8, cols: u8, players: u8) -> Match {
    let g = Geom::new(rows, cols);
    Match {
        seed: 1,
        turn: 0,
        max_turns: 1000,
        players,
        g,
        sym: crate::map::Symmetry::for_preset(&g, players),
        done: false,
        reason: 0,
        water: crate::map::Bits::zeros(g.cells()),
        known: (0..players).map(|_| crate::map::Bits::zeros(g.cells())).collect(),
        ants: Vec::new(),
        food: Vec::new(),
        hills: Vec::new(),
        hive: vec![0; players as usize],
        score: vec![0; players as usize],
        domination_turns: 0,
        idle_food_turns: 0,
    }
}

fn at(m: &Match, r: i32, c: i32) -> u16 {
    m.g.at(r, c)
}

/// Orders for one seat, in `mine` order, as characters for brevity.
fn orders(spec: &[&str]) -> Vec<String> {
    spec.iter().map(|s| s.to_string()).collect()
}

fn play(m: &mut Match, a: &[&str], b: &[&str]) {
    step(m, &[orders(a), orders(b)], m.food.len());
}

// ---------------------------------------------------------------- the map

#[test]
fn the_map_wraps_in_both_directions() {
    // Rule 7: walk off the top and you arrive at the bottom. There are no corners and no edges.
    let m = bare(10, 10, 2);
    assert_eq!(at(&m, -1, 0), at(&m, 9, 0));
    assert_eq!(at(&m, 0, -1), at(&m, 0, 9));
    assert_eq!(at(&m, 10, 10), at(&m, 0, 0));
}

#[test]
fn distance_takes_the_shorter_way_around() {
    // Rule 10, and it is squared so no square root is ever needed.
    let m = bare(10, 10, 2);
    let a = at(&m, 0, 0);
    let b = at(&m, 0, 9);
    assert_eq!(m.g.dist2(a, b), 1, "one step the short way, not nine the long way");
    assert_eq!(m.g.dist2(at(&m, 0, 0), at(&m, 5, 5)), 50);
}

// ---------------------------------------------------------------- moving

#[test]
fn an_order_into_water_is_ignored_and_that_ant_holds() {
    // Rule 23.
    let mut m = bare(8, 8, 2);
    let start = at(&m, 4, 4);
    m.water.set(at(&m, 3, 4) as usize);
    m.ants.push(Ant { pos: start, owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &["N"], &["-"]);
    assert_eq!(m.ants_of(0).next().unwrap().pos, start, "it walked into water");
}

#[test]
fn an_ant_with_no_order_stays_where_it_is() {
    // Rule 22, and anything that is not a direction.
    let mut m = bare(8, 8, 2);
    let start = at(&m, 4, 4);
    m.ants.push(Ant { pos: start, owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &[], &["nonsense"]);
    assert_eq!(m.ants_of(0).next().unwrap().pos, start);
    assert_eq!(m.ants_of(1).next().unwrap().pos, at(&m, 0, 0));
}

// ---------------------------------------------------------------- collisions

#[test]
fn two_ants_on_one_square_both_die_even_when_they_share_an_owner() {
    // Rules 25-27. Your own two ants walking into each other both die.
    let mut m = bare(8, 8, 2);
    m.ants.push(Ant { pos: at(&m, 4, 3), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 4, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &["E", "W"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 0, "both of them walked onto (4,4)");
}

#[test]
fn walking_onto_a_stationary_ant_kills_both() {
    // Rule 27.
    let mut m = bare(8, 8, 2);
    m.ants.push(Ant { pos: at(&m, 4, 4), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 4, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &["-", "W"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 0);
}

#[test]
fn a_collision_kills_even_an_ant_that_would_have_won_its_fight() {
    // Rule 28: collisions are resolved before any fighting.
    let mut m = bare(20, 20, 2);
    // Two of player 0 collide; player 1 is far away and unthreatened.
    m.ants.push(Ant { pos: at(&m, 4, 3), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 4, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 1 });
    play(&mut m, &["E", "W"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 0);
    assert_eq!(m.ants_of(1).count(), 1);
}

// ---------------------------------------------------------------- combat

#[test]
fn one_against_one_is_mutual_destruction() {
    // Rules 30-32: each has focus 1, and each faces an enemy whose focus is <= its own.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 });
    play(&mut m, &["-"], &["-"]);
    assert_eq!(m.ants.len(), 0, "mutual destruction is normal");
}

#[test]
fn two_against_one_kills_the_one_and_costs_nothing() {
    // The shape the rule rewards: keep your ants supported. The lone ant has focus 2; each of the
    // pair has focus 1, and 1 <= 2, so the lone ant dies. Neither of the pair faces an enemy with
    // focus <= 1... except the lone ant, whose focus is 2. 2 <= 1 is false, so they live.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 });
    play(&mut m, &["-", "-"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 2, "the supported pair survives");
    assert_eq!(m.ants_of(1).count(), 0, "the lone ant is outnumbered");
}

#[test]
fn ants_out_of_attack_range_do_not_fight() {
    // Rule 29: attack radius squared is 5, so two straight out and no further.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 8), owner: 1 }); // dist2 = 9 > 5
    play(&mut m, &["-"], &["-"]);
    assert_eq!(m.ants.len(), 2, "three apart is out of range");
}

#[test]
fn a_dying_ant_still_counts_as_an_attacker() {
    // Rule 32: deaths do not cascade. Every ant is judged at the same moment.
    //
    // 0 at (5,5) and (5,4); 1 at (5,6) and (5,7). Focus: 0's ants see 1's ants in range and vice
    // versa. If deaths cascaded, killing one side first would spare the other; judged
    // simultaneously, both sides lose ants.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 });
    m.ants.push(Ant { pos: at(&m, 5, 7), owner: 1 });
    play(&mut m, &["-", "-"], &["-", "-"]);
    assert!(m.ants.len() < 4, "a symmetric engagement must cost both sides");
    assert_eq!(m.ants_of(0).count(), m.ants_of(1).count(), "and cost them equally");
}

// ---------------------------------------------------------------- hills

#[test]
fn a_hill_is_razed_by_an_enemy_that_survives_on_it() {
    // Rules 41 and 43: +2 to the razer, -1 to the owner.
    let mut m = bare(20, 20, 2);
    let hill = at(&m, 5, 5);
    m.hills.push(Hill { pos: hill, owner: 1, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 });
    play(&mut m, &["E"], &[]);
    assert!(m.hills[0].razed);
    assert_eq!(m.score[0], 2);
    assert_eq!(m.score[1], -1);
}

#[test]
fn dying_on_a_hill_razes_nothing() {
    // Rule 68, the consequence people are caught by: battle is resolved before razing, so you
    // have to *survive* on the hill.
    let mut m = bare(20, 20, 2);
    let hill = at(&m, 5, 5);
    m.hills.push(Hill { pos: hill, owner: 1, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 }); // will step onto the hill
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 }); // and meet a defender: 1v1, both die
    play(&mut m, &["E"], &["-"]);
    assert_eq!(m.ants.len(), 0, "one against one is mutual destruction");
    assert!(!m.hills[0].razed, "the attacker did not survive on it");
    assert_eq!(m.score, vec![0, 0]);
}

#[test]
fn your_own_ant_cannot_raze_your_own_hill() {
    // Rule 44.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    play(&mut m, &["-"], &[]);
    assert!(!m.hills[0].razed);
}

#[test]
fn a_razed_hill_is_charged_once_and_never_spawns_again() {
    // Rule 42 and rule 57.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 1, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 });
    m.hive[1] = 3;
    play(&mut m, &["E"], &[]);
    let after = (m.score[0], m.score[1]);
    play(&mut m, &["-"], &[]);
    assert_eq!((m.score[0], m.score[1]), after, "charged once per hill");
    assert_eq!(m.ants_of(1).count(), 0, "a razed hill never spawns another ant");
    assert_eq!(m.hive[1], 3, "and the food waits in the hive");
}

// ---------------------------------------------------------------- food

#[test]
fn food_in_range_of_exactly_one_player_is_collected() {
    // Rules 47-48. Spawn radius squared is 1: the food's own square or the four beside it.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 0 });
    step(&mut m, &[orders(&["-"]), orders(&[])], 0);
    assert_eq!(m.hive[0], 1);
    assert!(m.food.is_empty());
}

#[test]
fn contested_food_is_destroyed() {
    // Rule 49: if ants of two or more players are in range, nobody gets it. Tested against the
    // gathering step directly, because it cannot be reached through a whole turn -- see below.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 1 });
    let n = crate::turn::gather(&mut m);
    assert_eq!(n, 0, "nobody collects contested food");
    assert_eq!(m.hive, vec![0, 0], "contested food is wasted food");
    assert!(m.food.is_empty(), "and it is destroyed rather than left");
}

#[test]
fn rule_49_cannot_actually_be_reached_at_the_standard_radii() {
    // A finding about the rules rather than about this code, and worth a test so it is noticed if
    // the settings ever change.
    //
    // Spawn radius² is 1, so two ants in range of one food are at most dist² = 4 apart; the attack
    // radius² is 5, so they are ALWAYS in each other's attack range. And two enemies in range can
    // never both survive: A lives only if every enemy facing it has a strictly greater focus, and
    // B lives only if A does, and both cannot hold (rule 31). So by the time gathering runs, at
    // most one player has an ant in range.
    let mut m = bare(20, 20, 2);
    let food = at(&m, 5, 5);
    for a in [(5, 6), (5, 4), (4, 5), (6, 5), (5, 5)] {
        for b in [(5, 6), (5, 4), (4, 5), (6, 5), (5, 5)] {
            if a == b {
                continue;
            }
            assert!(
                m.g.dist2(at(&m, a.0, a.1), at(&m, b.0, b.1)) <= crate::map::ATTACK_RADIUS2,
                "two ants in spawn range of one food must be in attack range"
            );
        }
    }
    // And through a real turn, the survivor collects it rather than it being destroyed.
    m.food.push(food);
    // The support has to be in range of the ENEMY, not merely near its own side: what kills is
    // the enemy's focus. (5,3) raises player 1's focus to 2 while player 0's stays at 1, so
    // player 0's ant lives and player 1's dies -- and (5,3) is dist² 4 from the food, out of
    // spawn range, so it does not itself become a claimant.
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 3), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 1 });
    step(&mut m, &[orders(&["-", "-"]), orders(&["-"])], 0);
    assert_eq!(m.ants_of(1).count(), 0, "the outnumbered ant dies");
    assert_eq!(m.ants_of(0).count(), 2, "the supported pair lives");
    assert_eq!(m.hive[0], 1, "the survivor collects it");
    assert_eq!(m.hive[1], 0);
}

#[test]
fn food_out_of_range_stays_where_it_is() {
    // Rule 50.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 9, 9), owner: 0 });
    step(&mut m, &[orders(&["-"]), orders(&[])], 0);
    assert_eq!(m.food.len(), 1);
    assert_eq!(m.hive[0], 0);
}

#[test]
fn food_is_always_one_turn_behind() {
    // Rule 69, and rule 55: spawning happens before gathering, so food collected this turn cannot
    // become an ant until next turn.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_spawn: 0 });
    m.food.push(at(&m, 8, 8));
    m.ants.push(Ant { pos: at(&m, 8, 7), owner: 0 });
    // Player 1 needs a living ant somewhere, or the match ends as a lone survivor (rule 63)
    // before the second turn ever runs.
    m.ants.push(Ant { pos: at(&m, 18, 18), owner: 1 });
    step(&mut m, &[orders(&["-"]), orders(&["-"])], 0);
    assert_eq!(m.hive[0], 1, "collected");
    assert_eq!(m.ants_of(0).count(), 1, "but not yet an ant");
    step(&mut m, &[orders(&["-"]), orders(&["-"])], 0);
    assert_eq!(m.ants_of(0).count(), 2, "the ant arrives the turn after");
    assert_eq!(m.hive[0], 0);
}

#[test]
fn an_ant_on_your_own_hill_blocks_it_from_spawning() {
    // Rule 52: parking an ant on your own hill is how you choose which hill your ants come out of.
    let mut m = bare(20, 20, 2);
    let a = at(&m, 5, 5);
    let b = at(&m, 15, 15);
    m.hills.push(Hill { pos: a, owner: 0, razed: false, last_spawn: 0 });
    m.hills.push(Hill { pos: b, owner: 0, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: a, owner: 0 });
    m.ants.push(Ant { pos: at(&m, 18, 2), owner: 1 }); // so the match does not end (rule 63)
    m.hive[0] = 1;
    step(&mut m, &[orders(&["-"]), orders(&["-"])], 0);
    assert!(m.ants.iter().any(|x| x.pos == b), "it must have spawned on the free hill");
    assert_eq!(m.ants_of(0).count(), 2);
}

#[test]
fn several_free_hills_spawn_least_recently_used_first() {
    // Rule 53, so a colony spreads across its hills rather than piling up on one.
    let mut m = bare(20, 20, 2);
    let a = at(&m, 5, 5);
    let b = at(&m, 15, 15);
    m.hills.push(Hill { pos: a, owner: 0, razed: false, last_spawn: 0 });
    m.hills.push(Hill { pos: b, owner: 0, razed: false, last_spawn: 0 });
    let far = at(&m, 18, 2);
    m.ants.push(Ant { pos: far, owner: 1 }); // so the match does not end (rule 63)
    m.hive[0] = 1;
    step(&mut m, &[orders(&[]), orders(&["-"])], 0);
    let first = m.ants_of(0).next().unwrap().pos;
    // Clear the way and spawn again: it must use the other hill.
    m.ants.retain(|a| a.owner == 1);
    m.hive[0] = 1;
    step(&mut m, &[orders(&[]), orders(&["-"])], 0);
    assert_ne!(m.ants_of(0).next().unwrap().pos, first, "the second spawn used the other hill");
}

// ---------------------------------------------------------------- ending

#[test]
fn the_lone_survivor_takes_every_standing_enemy_hill() {
    // Rules 60 and 63.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 15, 15), owner: 1, razed: false, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    play(&mut m, &["-"], &[]);
    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "lone_survivor");
    assert_eq!(m.score[0], 2, "+2 for the standing enemy hill");
    assert_eq!(m.score[1], -1);
    assert_eq!(ranks(&m), vec![1, 2]);
}

#[test]
fn losing_every_hill_does_not_eliminate_you() {
    // Rules 45 and 67: you are eliminated when you have no living ants, not when you lose your
    // hills. You can be alive with every hill razed.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: true, last_spawn: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 1 });
    play(&mut m, &["-"], &["-"]);
    assert!(!m.done, "both players still have living ants");
    assert!(m.alive(0));
}

#[test]
fn extermination_ends_it() {
    // Rule 64.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 });
    play(&mut m, &["-"], &["-"]); // mutual destruction
    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "extermination");
    assert_eq!(ranks(&m), vec![1, 1], "equal scores share the rank");
}

#[test]
fn the_turn_limit_ends_it_with_the_scores_as_they_stand() {
    // Rule 62.
    let mut m = bare(20, 20, 2);
    m.max_turns = 3;
    m.ants.push(Ant { pos: at(&m, 2, 2), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 17, 17), owner: 1 });
    for _ in 0..3 {
        play(&mut m, &["-"], &["-"]);
    }
    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "turn_limit");
    assert_eq!(m.turn, 3);
}

// ---------------------------------------------------------------- fog

#[test]
fn you_see_nothing_outside_your_vision() {
    // Rules 12-14. An enemy army may be one square outside your vision and you will not know.
    let mut m = bare(40, 40, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 25, 25), owner: 1 });
    m.food.push(at(&m, 25, 26));
    m.reveal(0);
    let v = crate::observe::view(&m, 0);
    assert_eq!(v["foes"].as_array().unwrap().len(), 0, "the enemy is far outside the view radius");
    assert_eq!(v["food"].as_array().unwrap().len(), 0);
    assert_eq!(v["mine"].as_array().unwrap().len(), 1, "your own ants are always yours");
}

#[test]
fn vision_shrinks_when_your_ants_die() {
    // Rule 15: lose ants and you go blind.
    let mut m = bare(40, 40, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 30, 30), owner: 0 });
    let wide = m.visible(0).count();
    m.ants.pop();
    let narrow = m.visible(0).count();
    assert!(narrow < wide, "vision moves with your ants");
}

#[test]
fn water_is_remembered_and_everything_else_is_not() {
    // The decision in observe.rs: `water` is what you have seen; foes, food and hills are what you
    // can see now. Only water is permanent (rule 16).
    let mut m = bare(40, 40, 2);
    let far = at(&m, 5, 20);
    m.water.set(far as usize);
    m.food.push(at(&m, 5, 21));
    m.ants.push(Ant { pos: at(&m, 5, 20 - 1), owner: 0 });
    m.reveal(0);
    let seen_before = crate::observe::view(&m, 0);
    let water_runs = |v: &Value| v["water"]["rle"].as_array().unwrap().clone();
    assert!(seen_before["food"].as_array().unwrap().len() == 1, "the food is in view now");
    let water_then = water_runs(&seen_before);

    // Walk the ant far away. The water stays known; the food does not.
    m.ants[0].pos = at(&m, 35, 35);
    m.reveal(0);
    let after = crate::observe::view(&m, 0);
    assert_eq!(after["food"].as_array().unwrap().len(), 0, "food is only true while you see it");
    assert_eq!(water_runs(&after), water_then, "water you have seen stays true");
}

// ---------------------------------------------------------------- the wave contract

#[test]
fn a_world_is_symmetric_so_neither_player_has_a_better_map() {
    // Rule 46 generalised: a map that were only approximately fair would put a thumb on every
    // rating computed from it.
    for p in crate::map::PRESETS {
        let m = worldgen(0xA11CE, p, 1000);
        let sym = m.sym;
        for i in 0..m.cells() {
            let img = sym.image(&m.g, i as u16, 1) as usize;
            assert_eq!(
                m.water.get(i),
                m.water.get(img),
                "preset {} is not symmetric at cell {i}",
                p.name
            );
        }
        assert_eq!(m.hills.len(), p.players as usize, "one hill each");
        assert_eq!(m.ants.len(), p.players as usize, "one ant each");
        assert_eq!(m.food.len() % p.players as usize, 0, "food comes in whole orbits");
        // And nobody starts inside a lake.
        for h in &m.hills {
            assert!(!m.water.get(h.pos as usize));
        }
    }
}

#[test]
fn the_same_seed_is_the_same_match_every_time() {
    // The property an audit rests on, and the reason the RNG is seeded rather than sampled.
    let a = invoke("tb.ants.worldgen", json!({"seeds": [7, 8], "preset": "standard"})).unwrap();
    let b = invoke("tb.ants.worldgen", json!({"seeds": [7, 8], "preset": "standard"})).unwrap();
    assert_eq!(a["wave_state"], b["wave_state"]);
    let c = invoke("tb.ants.worldgen", json!({"seeds": [7, 9], "preset": "standard"})).unwrap();
    assert_ne!(a["wave_state"], c["wave_state"]);
}

#[test]
fn wave_state_round_trips_exactly_after_a_played_turn() {
    // docs/docs/cartridge.md §2. Tested after a turn, not on a fresh state: a fresh state exercises none of
    // the fields that matter -- scores, the hive, razed hills, the stalemate counters.
    let w = invoke("tb.ants.worldgen", json!({"seeds": [11, 12], "preset": "standard"})).unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(4);
    for _ in 0..25 {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        let acts = random_actions(&views, &mut rng);
        state = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap()
            ["wave_state"]
            .as_str()
            .unwrap()
            .to_string();
    }
    let again = pack(&unpack(&state).unwrap());
    assert_eq!(state, again, "any information not encoded is information the match does not have");
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

#[test]
fn the_water_run_lengths_sum_to_the_cell_count() {
    // docs/protocol.md §6: one of the two invariants the schemas state and cannot enforce.
    let w = invoke("tb.ants.worldgen", json!({"seeds": [3], "preset": "cell"})).unwrap();
    let views = invoke("tb.ants.observe", json!({"wave_state": w["wave_state"]})).unwrap();
    for v in views["views"].as_array().unwrap() {
        let rle = v["view"]["water"]["rle"].as_array().unwrap();
        let total: u64 = rle.chunks(2).map(|p| p[1].as_u64().unwrap()).sum();
        let size = v["view"]["size"].as_array().unwrap();
        assert_eq!(total, size[0].as_u64().unwrap() * size[1].as_u64().unwrap());
    }
}

#[test]
fn an_action_array_is_as_long_as_mine() {
    // docs/protocol.md §6: the other invariant. It is the model's to honour and the engine's to
    // tolerate -- a short array means the rest hold, and never a rejected match.
    let w = invoke("tb.ants.worldgen", json!({"seeds": [5], "preset": "standard"})).unwrap();
    let views = invoke("tb.ants.observe", json!({"wave_state": w["wave_state"]})).unwrap();
    let n = views["views"][0]["view"]["mine"].as_array().unwrap().len();
    assert!(n > 0);
    let short = json!([json!([]), json!([])]);
    let out = invoke(
        "tb.ants.step",
        json!({"wave_state": w["wave_state"], "actions": short}),
    );
    assert!(out.is_ok(), "a short action array is every remaining ant holding, not a fault");
}

#[test]
fn observe_says_nothing_about_a_finished_match() {
    // docs/docs/cartridge.md §1: there is no terminal message. A model receives states while its match runs
    // and nothing afterwards -- it is never told that it lost.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 });
    play(&mut m, &["-"], &["-"]);
    assert!(m.done);
    let state = pack(&Wave { matches: vec![m] });
    let views = invoke("tb.ants.observe", json!({"wave_state": state})).unwrap();
    assert_eq!(views["views"].as_array().unwrap().len(), 0);
}

#[test]
fn observe_echoes_an_opaque_per_seat_handle() {
    let w = invoke("tb.ants.worldgen", json!({"seeds": [1, 2], "preset": "standard"})).unwrap();
    let refs = json!([
        {"m": 0, "seat": 0, "weights_hash": "W00"}, {"m": 0, "seat": 1, "weights_hash": "W01"},
        {"m": 1, "seat": 0, "weights_hash": "W10"}, {"m": 1, "seat": 1, "weights_hash": "W11"}
    ]);
    let views =
        invoke("tb.ants.observe", json!({"wave_state": w["wave_state"], "refs": refs})).unwrap();
    for v in views["views"].as_array().unwrap() {
        let (m, seat) = (v["m"].as_u64().unwrap(), v["seat"].as_u64().unwrap());
        assert_eq!(v["ref"]["weights_hash"], format!("W{m}{seat}"));
    }
}

#[test]
fn actions_may_be_positional_or_explicit_and_they_agree() {
    let w = invoke("tb.ants.worldgen", json!({"seeds": [21, 22], "preset": "standard"})).unwrap();
    let s = w["wave_state"].as_str().unwrap();
    let views = invoke("tb.ants.observe", json!({"wave_state": s})).unwrap();
    let vs = views["views"].as_array().unwrap();

    let positional: Vec<Value> = vs
        .iter()
        .map(|v| json!(vec!["N"; v["view"]["mine"].as_array().unwrap().len()]))
        .collect();
    let explicit: Vec<Value> = vs
        .iter()
        .map(|v| {
            json!({"m": v["m"], "seat": v["seat"],
                   "action": vec!["N"; v["view"]["mine"].as_array().unwrap().len()]})
        })
        .collect();

    let a = invoke("tb.ants.step", json!({"wave_state": s, "actions": positional})).unwrap();
    let b = invoke("tb.ants.step", json!({"wave_state": s, "actions": explicit})).unwrap();
    assert_eq!(a["wave_state"], b["wave_state"]);

    let short = json!([json!(["N"])]);
    assert_eq!(
        invoke("tb.ants.step", json!({"wave_state": s, "actions": short})).unwrap_err().code,
        "BAD_ACTION"
    );
}

#[test]
fn refusals() {
    assert_eq!(invoke("tb.ants.nope", json!({})).unwrap_err().code, "UNKNOWN_FUNCTION");
    assert_eq!(
        invoke("tb.ants.worldgen", json!({"seeds": []})).unwrap_err().code,
        "NO_SEEDS"
    );
    assert_eq!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "preset": "nope"})).unwrap_err().code,
        "NO_SUCH_PRESET"
    );
    // Decision 14: the preset carries the seat count and a caller that disagrees is refused.
    assert_eq!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "preset": "cell", "players": 4}))
            .unwrap_err()
            .code,
        "PLAYER_COUNT"
    );
    assert_eq!(
        invoke("tb.ants.observe", json!({"wave_state": "not base64!!"})).unwrap_err().code,
        "BAD_STATE"
    );
}

#[test]
fn a_wave_plays_to_an_end_and_finishes_with_ranks() {
    let w = invoke("tb.ants.worldgen",
                   json!({"seeds": [31, 32, 33, 34], "preset": "standard", "max_turns": 300}))
        .unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(99);
    let mut turns = 0;
    loop {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        if views["views"].as_array().unwrap().is_empty() {
            break;
        }
        let acts = random_actions(&views, &mut rng);
        state = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap()
            ["wave_state"].as_str().unwrap().to_string();
        turns += 1;
        assert!(turns <= 301, "the wave did not terminate");
    }
    let fin = invoke("tb.ants.finish", json!({"wave_state": &state})).unwrap();
    for r in fin["results"].as_array().unwrap() {
        let ranks = r["ranks"].as_array().unwrap();
        assert_eq!(ranks.len(), 2);
        assert!(ranks.iter().all(|x| x.as_u64().unwrap() >= 1), "ranks are 1-based");
        assert!(state::END_REASONS.contains(&r["reason"].as_str().unwrap()));
        assert_eq!(r["done"], json!(true));
    }
    println!("\na wave of 4 on `standard` ended after {turns} turns");
}

#[test]
fn a_colony_that_is_eating_is_never_cut_off_as_idle() {
    // Rule 66's second form, and the test that would have caught the first version of it.
    //
    // The counter was written as "the hive total did not move", which is wrong because the hive is
    // *drained* by spawning: in any steady state it hovers near zero and its total is unchanged
    // turn after turn while food flows through it perfectly well. Every real match ended at
    // exactly 150 turns. What rule 66 is about is gathering, so gathering is what is counted.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_spawn: 0 });
    // Beside the hill, not on it: an ant standing on its own hill blocks it (rule 52), and a
    // colony that cannot spawn is a different test.
    m.ants.push(Ant { pos: at(&m, 5, 8), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 1 });

    // One food beside that ant, replaced every turn: collected, spawned, collected again -- the
    // hive total is 0 at the end of every single turn, and the colony is plainly not idle.
    for _ in 0..200 {
        if m.food.is_empty() {
            m.food.push(at(&m, 5, 9));
        }
        // Target zero, so the engine adds no food of its own -- this test manages the one food
        // itself, and a randomly placed orbit somewhere else would be exactly the idle case.
        step(&mut m, &[orders(&["-"]), orders(&["-"])], 0);
        assert!(!m.done, "cut off at turn {} as {}", m.turn, state::END_REASONS[m.reason as usize]);
    }
    assert_eq!(m.idle_food_turns, 0);
    assert!(m.ants_of(0).count() > 1, "it should have grown");
}

#[test]
fn food_nobody_touches_does_cut_the_match_off() {
    // The other half: the rule must still fire when it should.
    let mut m = bare(30, 30, 2);
    m.ants.push(Ant { pos: at(&m, 2, 2), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 20, 20), owner: 1 });
    m.food.push(at(&m, 10, 10)); // far from both, and nobody moves
    for _ in 0..state::STALEMATE_TURNS {
        step(&mut m, &[orders(&["-"]), orders(&["-"])], 1);
    }
    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "idle_food");
}

#[test]
fn measure_what_random_play_produces() {
    // Not a rule test: a sanity check that the rules produce a game. If every match ended the same
    // way, or none ever ended at all, something in §11 would be wrong in a way no single-rule test
    // would catch.
    println!("\n=== 24 matches of random play, per preset ===");
    println!("{:<10} {:>6} {:>7} {:>8}  reasons", "preset", "turns", "ants", "scores");
    for p in crate::map::PRESETS {
        let mut reasons: std::collections::BTreeMap<&str, usize> = Default::default();
        let (mut turns, mut ants, mut decisive) = (0usize, 0usize, 0usize);
        for seed in 0..24u64 {
            let mut m = worldgen(seed * 7919 + 13, p, 1000);
            let mut rng = Rng(seed ^ 0xBEEF);
            while !m.done {
                let mv: Vec<Vec<String>> = (0..m.players)
                    .map(|s| {
                        (0..m.mine(s).len())
                            .map(|_| ["N", "E", "S", "W", "-"][rng.below(5) as usize].to_string())
                            .collect()
                    })
                    .collect();
                let target = p.food_per_player as usize * p.players as usize;
                step(&mut m, &mv, target);
            }
            *reasons.entry(state::END_REASONS[m.reason as usize]).or_default() += 1;
            turns += m.turn as usize;
            ants += m.ants.len();
            if m.score[0] != m.score[1] {
                decisive += 1;
            }
        }
        let rs: Vec<String> = reasons.iter().map(|(k, v)| format!("{k} x{v}")).collect();
        println!(
            "{:<10} {:>6} {:>7} {:>7}%  {}",
            p.name,
            turns / 24,
            ants / 24,
            decisive * 100 / 24,
            rs.join(", ")
        );
    }
    println!("\n('scores' is how often the two players finished on different scores)");
}

#[test]
fn a_replay_re_simulates_the_match_it_recorded() {
    // the platform design §8: replays store the action stream, not frames, and the viewer re-simulates.
    // That is only possible because game state is integer-only (§3.3) — which is why `deny.sh`
    // checks the source for floating point rather than trusting that nobody added any.
    //
    // The envelope carries the opening state and every turn's actions. Decoding turn N must give
    // exactly the position the referee was in at turn N.
    let w = invoke("tb.ants.worldgen",
                   json!({"seeds": [4242], "preset": "standard", "max_turns": 60})).unwrap();
    let state0 = w["wave_state"].as_str().unwrap().to_string();

    let mut state = state0.clone();
    let mut rng = Rng(2024);
    let mut deltas = Vec::new();
    let mut live_positions = Vec::new();
    loop {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        if views["views"].as_array().unwrap().is_empty() {
            break;
        }
        let acts = random_actions(&views, &mut rng);
        let out = invoke("tb.ants.step",
                         json!({"wave_state": &state, "actions": acts})).unwrap();
        for d in out["replay_delta"].as_array().unwrap() {
            deltas.push(d.clone());
        }
        state = out["wave_state"].as_str().unwrap().to_string();
        // What the referee actually saw, turn by turn, to compare the decode against.
        let m = &unpack(&state).unwrap().matches[0];
        live_positions.push((m.turn, m.mine(0), m.mine(1), m.score.clone()));
    }
    assert!(live_positions.len() > 5, "the match must actually have been played");

    let payload = json!({ "state0": state0, "deltas": deltas });
    for (turn, mine0, mine1, score) in &live_positions {
        let f = invoke("tb.ants.replay-decode",
                       json!({"payload": payload, "turn": turn})).unwrap();
        let frame = &f["frame"];
        assert_eq!(frame["turn"].as_u64().unwrap() as u16, *turn);
        let ants: Vec<(u64, u64, u64)> = frame["ants"].as_array().unwrap().iter()
            .map(|a| (a[0].as_u64().unwrap(), a[1].as_u64().unwrap(), a[2].as_u64().unwrap()))
            .collect();
        let cols = frame["size"][1].as_u64().unwrap();
        let of = |seat: u64| {
            let mut v: Vec<u64> = ants.iter().filter(|a| a.2 == seat)
                .map(|a| a.0 * cols + a.1).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(of(0), mine0.iter().map(|&p| p as u64).collect::<Vec<_>>(),
                   "player 0 at turn {turn}");
        assert_eq!(of(1), mine1.iter().map(|&p| p as u64).collect::<Vec<_>>(),
                   "player 1 at turn {turn}");
        assert_eq!(frame["score"], json!(score), "score at turn {turn}");
    }
    println!("\nreplayed {} turns exactly, from the action stream alone", live_positions.len());
}

#[test]
fn a_replay_of_a_match_nobody_recorded_is_refused_rather_than_guessed() {
    assert_eq!(
        invoke("tb.ants.replay-decode", json!({"payload": {}, "turn": 0})).unwrap_err().code,
        "BAD_REPLAY"
    );
    assert_eq!(
        invoke("tb.ants.replay-decode", json!({"payload": {"state0": "nonsense!!"}, "turn": 0}))
            .unwrap_err().code,
        "BAD_REPLAY"
    );
}
