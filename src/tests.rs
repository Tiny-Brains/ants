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
        cutoff_bot: crate::state::CUTOFF_NONE,
        cutoff_turns: 0,
        // No food spawns on a bare board -- `food_rate: 0` is the reference's `do_food_none`.
        // A test that wanted the hidden rate would be testing `spawn_food`, and two below do.
        food_rate: 0,
        food_turn: 0,
        food_extra: 0,
        food_rotation: 0,
        food_cursor: 0,
        pending_food: Vec::new(),
        map_id: String::new(),
        food0: Vec::new(),
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
    step(m, &[orders(a), orders(b)]);
}

// ---------------------------------------------------------------- the map

#[test]
fn the_map_wraps_in_both_directions() {
    // *Map Format*: walk off the top and you arrive at the bottom. There are no corners and no edges.
    let m = bare(10, 10, 2);
    assert_eq!(at(&m, -1, 0), at(&m, 9, 0));
    assert_eq!(at(&m, 0, -1), at(&m, 0, 9));
    assert_eq!(at(&m, 10, 10), at(&m, 0, 0));
}

#[test]
fn distance_takes_the_shorter_way_around() {
    // *Distance*, and it is squared so no square root is ever needed.
    let m = bare(10, 10, 2);
    let a = at(&m, 0, 0);
    let b = at(&m, 0, 9);
    assert_eq!(m.g.dist2(a, b), 1, "one step the short way, not nine the long way");
    assert_eq!(m.g.dist2(at(&m, 0, 0), at(&m, 5, 5)), 50);
}

// ---------------------------------------------------------------- moving

#[test]
fn an_order_into_water_is_ignored_and_that_ant_holds() {
    // *Blocking*.
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
    // *Bot Output*, and anything that is not a direction.
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
    // *Collisions*. Your own two ants walking into each other both die.
    let mut m = bare(8, 8, 2);
    m.ants.push(Ant { pos: at(&m, 4, 3), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 4, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &["E", "W"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 0, "both of them walked onto (4,4)");
}

#[test]
fn walking_onto_a_stationary_ant_kills_both() {
    // *Collisions*.
    let mut m = bare(8, 8, 2);
    m.ants.push(Ant { pos: at(&m, 4, 4), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 4, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 0, 0), owner: 1 });
    play(&mut m, &["-", "W"], &["-"]);
    assert_eq!(m.ants_of(0).count(), 0);
}

#[test]
fn a_collision_kills_even_an_ant_that_would_have_won_its_fight() {
    // *Collisions*: collisions are resolved before any fighting.
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
    // *Focus Battle Resolution*: each has focus 1, and each faces an enemy whose focus is <= its own.
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
    // *Focus Battle Resolution*: attack radius squared is 5, so two straight out and no further.
    let mut m = bare(20, 20, 2);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 8), owner: 1 }); // dist2 = 9 > 5
    play(&mut m, &["-"], &["-"]);
    assert_eq!(m.ants.len(), 2, "three apart is out of range");
}

#[test]
fn a_dying_ant_still_counts_as_an_attacker() {
    // *Focus Battle Resolution*: deaths do not cascade. Every ant is judged at the same moment.
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
    // *Hill Razing*: +2 to the razer, -1 to the owner.
    let mut m = bare(20, 20, 2);
    let hill = at(&m, 5, 5);
    m.hills.push(Hill { pos: hill, owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 });
    play(&mut m, &["E"], &[]);
    assert!(m.hills[0].razed);
    assert_eq!(m.score[0], 2);
    assert_eq!(m.score[1], -1);
}

#[test]
fn dying_on_a_hill_razes_nothing() {
    // *Hill Razing*, the consequence people are caught by: battle is resolved before razing, so you
    // have to *survive* on the hill.
    let mut m = bare(20, 20, 2);
    let hill = at(&m, 5, 5);
    m.hills.push(Hill { pos: hill, owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 0 }); // will step onto the hill
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 1 }); // and meet a defender: 1v1, both die
    play(&mut m, &["E"], &["-"]);
    assert_eq!(m.ants.len(), 0, "one against one is mutual destruction");
    assert!(!m.hills[0].razed, "the attacker did not survive on it");
    assert_eq!(m.score, vec![0, 0]);
}

#[test]
fn your_own_ant_cannot_raze_your_own_hill() {
    // *Hill Razing*.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    play(&mut m, &["-"], &[]);
    assert!(!m.hills[0].razed);
}

#[test]
fn a_razed_hill_is_charged_once_and_never_spawns_again() {
    // *Hill Razing* and *Scoring*.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 1, razed: false, last_touched: 0 });
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
    // *Food Harvesting*. Spawn radius squared is 1: the food's own square or the four beside it.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 0 });
    step(&mut m, &[orders(&["-"]), orders(&[])]);
    assert_eq!(m.hive[0], 1);
    assert!(m.food.is_empty());
}

#[test]
fn contested_food_is_destroyed() {
    // *Food Harvesting*: if ants of two or more players are in range, nobody gets it. Tested against the
    // gathering step directly, because it cannot be reached through a whole turn -- see below.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 5, 6), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 4), owner: 1 });
    crate::turn::gather(&mut m);
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
    // never both survive: A lives only if every enemy facing it has a strictly greater focus, and B
    // lives only if A does, and both cannot hold (*Focus Battle Resolution*). So by the time
    // gathering runs, at most one player has an ant in range.
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
    step(&mut m, &[orders(&["-", "-"]), orders(&["-"])]);
    assert_eq!(m.ants_of(1).count(), 0, "the outnumbered ant dies");
    assert_eq!(m.ants_of(0).count(), 2, "the supported pair lives");
    assert_eq!(m.hive[0], 1, "the survivor collects it");
    assert_eq!(m.hive[1], 0);
}

#[test]
fn food_out_of_range_stays_where_it_is() {
    // *Food Harvesting*.
    let mut m = bare(20, 20, 2);
    m.food.push(at(&m, 5, 5));
    m.ants.push(Ant { pos: at(&m, 9, 9), owner: 0 });
    step(&mut m, &[orders(&["-"]), orders(&[])]);
    assert_eq!(m.food.len(), 1);
    assert_eq!(m.hive[0], 0);
}

#[test]
fn food_is_always_one_turn_behind() {
    // *Ant Spawning*: spawning happens before gathering, so food collected this turn cannot
    // become an ant until the next one.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_touched: 0 });
    // Player 1 needs a living ant somewhere, or the match ends as a lone survivor (*Cutoff Rules*)
    // before the second turn ever runs -- and a standing hill, or *Cutoff Rules* ends it just as
    // fast: a player with no hill left is not given the chance to overtake, so the result is
    // already decided.
    m.hills.push(Hill { pos: at(&m, 15, 15), owner: 1, razed: false, last_touched: 0 });
    m.food.push(at(&m, 8, 8));
    m.ants.push(Ant { pos: at(&m, 8, 7), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 18, 18), owner: 1 });
    step(&mut m, &[orders(&["-"]), orders(&["-"])]);
    assert_eq!(m.hive[0], 1, "collected");
    assert_eq!(m.ants_of(0).count(), 1, "but not yet an ant");
    step(&mut m, &[orders(&["-"]), orders(&["-"])]);
    assert_eq!(m.ants_of(0).count(), 2, "the ant arrives the turn after");
    assert_eq!(m.hive[0], 0);
}

#[test]
fn an_ant_on_your_own_hill_blocks_it_from_spawning() {
    // *Ant Spawning*: parking an ant on your own hill is how you choose which hill your ants come out of.
    let mut m = bare(20, 20, 2);
    let a = at(&m, 5, 5);
    let b = at(&m, 15, 15);
    m.hills.push(Hill { pos: a, owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: b, owner: 0, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: a, owner: 0 });
    m.ants.push(Ant { pos: at(&m, 18, 2), owner: 1 }); // so the match does not end (*Cutoff Rules*)
    m.hive[0] = 1;
    step(&mut m, &[orders(&["-"]), orders(&["-"])]);
    assert!(m.ants.iter().any(|x| x.pos == b), "it must have spawned on the free hill");
    assert_eq!(m.ants_of(0).count(), 2);
}

#[test]
fn several_free_hills_spawn_least_recently_used_first() {
    // *Ant Spawning*, so a colony spreads across its hills rather than piling up on one.
    let mut m = bare(20, 20, 2);
    let a = at(&m, 5, 5);
    let b = at(&m, 15, 15);
    m.hills.push(Hill { pos: a, owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: b, owner: 0, razed: false, last_touched: 0 });
    // An ant and a hill for player 1, so the match ends by neither lone survivor nor rank
    // stabilization (*Cutoff Rules*).
    m.hills.push(Hill { pos: at(&m, 10, 10), owner: 1, razed: false, last_touched: 0 });
    let far = at(&m, 18, 2);
    m.ants.push(Ant { pos: far, owner: 1 });
    m.hive[0] = 1;
    step(&mut m, &[orders(&[]), orders(&["-"])]);
    let first = m.ants_of(0).next().unwrap().pos;
    // Clear the way and spawn again: it must use the other hill.
    m.ants.retain(|a| a.owner == 1);
    m.hive[0] = 1;
    step(&mut m, &[orders(&[]), orders(&["-"])]);
    assert_ne!(m.ants_of(0).next().unwrap().pos, first, "the second spawn used the other hill");
}

// ---------------------------------------------------------------- ending

#[test]
fn the_lone_survivor_takes_every_standing_enemy_hill() {
    // *Scoring*.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 15, 15), owner: 1, razed: false, last_touched: 0 });
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
    // *Hill Razing*: you are eliminated when you have no living ants, not when you lose your
    // hills. You can be alive with every hill razed -- your ants keep moving, fighting and eating.
    //
    // The match still ends, and that is rank stabilization (*Cutoff Rules*) rather than
    // elimination: a player with no hill left is **not** given the chance to overtake ("bots
    // without hills left could still possibly gain in rank, [but] the game is not extended [for]
    // them"), so with player 0 hill-less there is nobody who can still change the finishing order.
    // Being alive and being able to change the result are two different things, and only the first
    // one is what this rule is about.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: true, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 15, 15), owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 14), owner: 1 });
    play(&mut m, &["-"], &["-"]);
    assert!(m.alive(0), "still alive with every hill razed");
    assert_eq!(m.ants_of(0).count(), 1, "and the ant is still on the board");
    assert_ne!(
        state::END_REASONS[m.reason as usize], "extermination",
        "losing hills is not elimination"
    );
}

#[test]
fn extermination_ends_it() {
    // *Endbot Conditions*.
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
    // *Cutoff Rules*.
    let mut m = bare(20, 20, 2);
    m.max_turns = 3;
    // Both keep a hill, so neither rank stabilization nor a cutoff claims the match first. The turn
    // limit is the *last* thing checked, exactly as it is the loop bound rather than a condition in
    // the reference engine.
    m.hills.push(Hill { pos: at(&m, 2, 3), owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 17, 16), owner: 1, razed: false, last_touched: 0 });
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
    // *Fog of War*. An enemy army may be one square outside your vision and you will not know.
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
    // *Fog of War*: lose ants and you go blind.
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
    // can see now. Only water is permanent (*Bot Input*).
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
    // *Food spawning*, generalised: a map that were only approximately fair would put a thumb on every
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
    // *Cutoff Rules*'s food form, and the test that would have caught two wrong versions of it.
    //
    // The first counted "the hive total did not move", which is wrong because the hive is *drained*
    // by spawning: in a steady state it hovers near zero while food flows through it perfectly
    // well. The second counted "nobody gathered this turn", which is not a share of anything and
    // fired during any long lull between two healthy colonies. The rule is a **population share**:
    // loose food has to be 85% of every ant, hive and food in the game before it means anything.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 5, 5), owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 15, 15), owner: 1, razed: false, last_touched: 0 });
    // Beside the hill, not on it: an ant standing on its own hill blocks it (*Ant Spawning*), and a
    // colony that cannot spawn is a different test. Both seats eat, so neither dominates either.
    m.ants.push(Ant { pos: at(&m, 5, 8), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 18), owner: 1 });

    // One food beside each ant, replaced as it is eaten. Nothing here is ever idle.
    for _ in 0..200 {
        if m.food.is_empty() {
            m.food.push(at(&m, 5, 9));
            m.food.push(at(&m, 15, 19));
        }
        // Target zero, so the engine adds no food of its own -- this test manages its own food, and
        // a randomly placed orbit somewhere else would be exactly the idle case.
        step(&mut m, &[orders(&["-"]), orders(&["-"])]);
        assert!(!m.done, "cut off at turn {} as {}", m.turn, state::END_REASONS[m.reason as usize]);
    }
    assert_ne!(m.cutoff_bot, state::CUTOFF_FOOD, "food never held the share");
    assert!(m.ants_of(0).count() > 1, "it should have grown");
}

#[test]
fn food_nobody_touches_does_cut_the_match_off() {
    // The other half: the rule must still fire when it should, which needs food to actually
    // dominate the population -- twenty uneaten food against two ants that never move.
    let mut m = bare(30, 30, 2);
    m.hills.push(Hill { pos: at(&m, 0, 0), owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 25, 25), owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 2, 2), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 20, 20), owner: 1 });
    for c in 5..25 {
        m.food.push(at(&m, 10, c)); // far from both ants, and nobody moves
    }
    for _ in 0..state::STALEMATE_TURNS {
        assert!(!m.done, "ended early at turn {}", m.turn);
        step(&mut m, &[orders(&["-"]), orders(&["-"])]);
    }
    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "idle_food");
}

#[test]
fn two_ants_and_one_food_is_not_a_food_stalemate() {
    // The share is what makes the rule mean something. One uneaten food beside two live colonies
    // is a third of the population, not 85% of it, and the old "nobody gathered" test cut this
    // exact position off at turn 150.
    let mut m = bare(30, 30, 2);
    m.hills.push(Hill { pos: at(&m, 0, 0), owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 25, 25), owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 2, 2), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 20, 20), owner: 1 });
    m.food.push(at(&m, 10, 10));
    for _ in 0..state::STALEMATE_TURNS + 10 {
        step(&mut m, &[orders(&["-"]), orders(&["-"])]);
    }
    assert!(!m.done, "ended as {}", state::END_REASONS[m.reason as usize]);
    assert_eq!(m.cutoff_bot, state::CUTOFF_NONE, "nobody holds 85% of three things");
}

#[test]
fn measure_what_random_play_produces() {
    // Not a rule test: a sanity check that the rules produce a game. If every match ended the same
    // way, or none ever ended at all, something in the rules would be wrong in a way no single-rule
    // test would catch.
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
                step(&mut m, &mv);
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

    // The envelope Kalam writes: the board comes off `finish`, at the moment the drain persists
    // the row, and the seed comes off the match. Built the same way here so this test breaks if
    // the two ever stop agreeing about what a replay is.
    let fin = invoke("tb.ants.finish", json!({"wave_state": &state})).unwrap();
    let payload = json!({
        "seed": 4242,
        "preset": "standard",
        "max_turns": 60,
        "map_id": fin["results"][0]["map_id"],
        "map": fin["results"][0]["map"],
        "deltas": deltas,
    });
    let _ = state0;
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

// ---------------------------------------------------------------- maps as files

#[test]
fn every_committed_map_is_valid_and_symmetric() {
    // What `worldgen`'s construction used to guarantee, now asserted over the whole catalogue.
    // The old symmetry test checked one generated world; this checks every board that ships.
    let cat = crate::mapfile::catalogue();
    assert_eq!(cat.len(), crate::maps_gen::MAPS.len(), "a committed map failed to parse");
    assert!(!cat.is_empty(), "the catalogue is empty");
    for mf in &cat {
        mf.validate().unwrap_or_else(|e| panic!("map {}: {} {}", mf.id, e.code, e.message));
        let m = mf.build(1, 1000).unwrap();
        let g = m.g;
        for i in 0..g.cells() {
            let img = m.sym.image(&g, i as u16, 1) as usize;
            assert_eq!(m.water.get(i), m.water.get(img), "map {} is not symmetric", mf.id);
        }
        assert_eq!(m.hills.len(), mf.players as usize, "map {}: one hill each", mf.id);
        assert_eq!(m.ants.len(), mf.players as usize, "map {}: one ant each", mf.id);
        assert!(m.food.len() % mf.players as usize == 0, "map {}: whole orbits", mf.id);
        for h in &m.hills {
            assert!(!m.water.get(h.pos as usize), "map {}: a hill on water", mf.id);
        }
    }
}

#[test]
fn a_map_survives_the_round_trip_to_a_file_and_back() {
    // The property `mapgen` rests on: what the generator produced, written out and read back, is
    // the same board. Without it a committed map is a lossy photograph of one.
    for p in crate::map::PRESETS {
        let m = worldgen(0xBEEF, p, 1000);
        let a = crate::mapfile::MapFile::from_match(&m, "round-trip", p.name);
        let b = crate::mapfile::MapFile::from_json(&a.to_json()).unwrap();
        assert_eq!(a, b, "preset {} did not survive the round trip", p.name);

        let rebuilt = b.build(0xBEEF, 1000).unwrap();
        assert_eq!(rebuilt.water.bits, m.water.bits, "{}: terrain", p.name);
        assert_eq!(rebuilt.food, m.food0, "{}: turn-zero food", p.name);
        assert_eq!(rebuilt.food_rate, m.food_rate, "{}: hidden food rate", p.name);
        assert_eq!(rebuilt.food_turn, m.food_turn, "{}: hidden food period", p.name);
        let hills = |x: &Match| x.hills.iter().map(|h| (h.pos, h.owner)).collect::<Vec<_>>();
        assert_eq!(hills(&rebuilt), hills(&m), "{}: hills", p.name);
    }
}

#[test]
fn the_seed_chooses_the_board_and_the_caller_may_pin_it() {
    // The platform passes no map: pairing assigns the seed and the seed assigns the board, so a
    // competitor cannot train against a board they picked. A local caller may pin one.
    let pool = crate::mapfile::pool("cell");
    assert!(pool.len() > 1, "a pool of one cannot demonstrate selection");

    let ids = |input: serde_json::Value| -> Vec<String> {
        invoke("tb.ants.worldgen", input).unwrap()["map_ids"]
            .as_array().unwrap().iter()
            .map(|v| v.as_str().unwrap().to_string()).collect()
    };

    // Deterministic, and a function of the seed alone.
    let a = ids(json!({"seeds": [7, 8], "preset": "cell"}));
    let b = ids(json!({"seeds": [7, 8], "preset": "cell"}));
    assert_eq!(a, b, "the same seeds must choose the same boards");
    assert_eq!(a[0], pool[(7 % pool.len() as u64) as usize].id);
    assert_eq!(a[1], pool[(8 % pool.len() as u64) as usize].id);

    // Pinned by id, for every seed in the wave.
    let pinned = ids(json!({"seeds": [7, 8], "preset": "cell", "map": pool[0].id}));
    assert_eq!(pinned, vec![pool[0].id.clone(), pool[0].id.clone()]);

    // Pinned per seed, positionally -- what `match.json` writes.
    let each = ids(json!({"seeds": [7, 8], "preset": "cell",
                          "maps": [pool[1].id, pool[0].id]}));
    assert_eq!(each, vec![pool[1].id.clone(), pool[0].id.clone()]);

    // And an inline board, which is how a competitor plays one the catalogue has never seen.
    let mut mine = pool[0].clone();
    mine.id = "hand-authored".to_string();
    let inline = ids(json!({"seeds": [7], "preset": "cell", "map": mine.to_json()}));
    assert_eq!(inline, vec!["hand-authored".to_string()]);
}

#[test]
fn a_map_that_is_not_symmetric_is_refused_rather_than_played() {
    // The guarantee that moved from construction to assertion. Each of these was impossible to
    // express before a board was a file, and is the first thing someone hand-authoring one will do.
    let base = crate::mapfile::pool("cell")[0].clone();
    let code = |mf: &crate::mapfile::MapFile| -> String {
        invoke("tb.ants.worldgen", json!({"seeds": [1], "preset": "cell", "map": mf.to_json()}))
            .unwrap_err().code.to_string()
    };

    // One cell of water that has no counterpart.
    let mut asym = base.clone();
    asym.water = vec![1, 1, 0, (asym.rows as u32 * asym.cols as u32) - 1];
    assert_eq!(code(&asym), "MAP_NOT_SYMMETRIC");

    // A hill that is not the image of the first.
    let mut moved = base.clone();
    moved.hills[1] = (moved.hills[1].0 + 1, moved.hills[1].1);
    assert_eq!(code(&moved), "MAP_NOT_SYMMETRIC");

    // Food for one seat only.
    let mut greedy = base.clone();
    greedy.food.truncate(1);
    assert_eq!(code(&greedy), "MAP_NOT_SYMMETRIC");

    // A board that starts stocked and never restocks is allowed: `spawn_food` fills up to the
    // target and does nothing when there is already more, so nothing needs protecting from it.
    let mut frugal = base.clone();
    frugal.food_target = 0;
    assert!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "preset": "cell", "map": frugal.to_json()}))
            .is_ok(),
        "a board may hold more food than it keeps stocked"
    );

    // Runs that do not cover the board.
    let mut short = base.clone();
    short.water = vec![0, 4];
    assert_eq!(code(&short), "MAP_BAD_SHAPE");

    // A board that does not divide by its seat count, so the orbit is not a partition.
    let mut odd = base.clone();
    odd.rows = 63;
    odd.water = vec![0, 63 * odd.cols as u32];
    assert_eq!(code(&odd), "MAP_BAD_SHAPE");

    // A hill under water.
    let mut drowned = base.clone();
    let (hr, hc) = drowned.hills[0];
    let g = drowned.geom();
    let mut w = crate::map::Bits::zeros(g.cells());
    for k in 0..drowned.players as i32 {
        w.set(drowned.geom().at(hr + (g.rows / 2) * k, hc + (g.cols / 2) * k) as usize);
    }
    drowned.water = w.rle();
    drowned.food.clear();
    assert_eq!(code(&drowned), "MAP_UNPLAYABLE");

    // And an unknown board is a refusal, not a silently substituted one.
    assert_eq!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "preset": "cell", "map": "no-such-map"}))
            .unwrap_err().code,
        "NO_SUCH_MAP"
    );
}

#[test]
fn a_frame_range_agrees_with_the_frames_asked_for_one_at_a_time() {
    // The scrubber's fix. `decode` re-simulates from turn zero, so a viewer walking a timeline
    // frame by frame is quadratic; `decode_range` walks the match once. It is only worth having
    // if it produces exactly the same frames.
    let w = invoke("tb.ants.worldgen",
                   json!({"seeds": [99], "preset": "standard", "max_turns": 40})).unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(7);
    let mut deltas = Vec::new();
    loop {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        if views["views"].as_array().unwrap().is_empty() {
            break;
        }
        let acts = random_actions(&views, &mut rng);
        let out = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap();
        deltas.extend(out["replay_delta"].as_array().unwrap().iter().cloned());
        state = out["wave_state"].as_str().unwrap().to_string();
    }
    let fin = invoke("tb.ants.finish", json!({"wave_state": &state})).unwrap();
    let payload = json!({
        "seed": 99, "max_turns": 40,
        "map": fin["results"][0]["map"], "deltas": deltas,
    });

    let ranged = invoke("tb.ants.replay-decode",
                        json!({"payload": &payload, "from": 0, "to": 12})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 13, "from 0 to 12 inclusive is thirteen frames");
    for (t, got) in frames.iter().enumerate() {
        let one = invoke("tb.ants.replay-decode",
                         json!({"payload": &payload, "turn": t})).unwrap();
        assert_eq!(got, &one["frame"], "frame {t} differs between the range and the single call");
    }
}

#[test]
fn a_replay_the_platform_actually_wrote_decodes() {
    // A REAL envelope, taken from MinIO after the local stack played it: Kalam's `put` task wrote
    // this, through Orion, from the component committed beside it. The other replay tests build
    // their own envelope in-process and would keep passing if Kalam and the engine drifted apart
    // about what a replay is -- which is exactly what had happened, silently, for the whole life
    // of this file: `decode` required a `state0` that `put` never wrote, so no stored replay
    // could be viewed at all.
    let raw = include_str!("../tests/fixtures/replay-maze-03.json");
    let payload: serde_json::Value = serde_json::from_str(raw).expect("the fixture is JSON");

    // Everything a viewer needs is in the file. No catalogue, no preset table, no second lookup.
    assert_eq!(payload["map_id"], "maze-03");
    assert!(payload["map"].is_object(), "the envelope carries its board");
    assert!(payload["seed"].is_u64(), "and the seed that drove food respawn");
    assert!(payload["max_turns"].is_u64(), "and the turn limit it was played under");

    let turns = payload["turns"].as_u64().unwrap() as u16;
    let first = invoke("tb.ants.replay-decode",
                       json!({"payload": &payload, "turn": 0})).unwrap();
    let f0 = &first["frame"];
    assert_eq!(f0["turn"], 0);
    assert_eq!(f0["size"], json!([96, 96]));
    assert_eq!(f0["ants"].as_array().unwrap().len(), 2, "one ant each at turn zero");
    assert_eq!(f0["hills"].as_array().unwrap().len(), 2, "and one hill each");

    // The whole match, and the end it recorded.
    let last = invoke("tb.ants.replay-decode",
                      json!({"payload": &payload, "turn": turns})).unwrap();
    assert_eq!(last["frame"]["turn"].as_u64().unwrap() as u16, turns);
    assert_eq!(last["frame"]["ranks"], payload["engine_ranks"], "the verdict re-simulates");

    // **This envelope predates the scoring fix, and says so.** It carries an `engine_digest`, and
    // that engine started a match on zero rather than on one point per hill -- so every score it
    // recorded is one short per hill owned, and on maze-03 that is one hill each. Ranks are the
    // verdict and they still agree exactly; the scores differ by precisely the starting points and
    // by nothing else, which is what makes this a stale fixture rather than a re-simulation bug.
    //
    // Regenerate the fixture from a platform run on the new digest and this goes back to a plain
    // equality. Until then the offset is asserted rather than ignored, so it cannot quietly grow.
    let stored: Vec<i64> =
        payload["scores"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let resim: Vec<i64> = last["frame"]["score"]
        .as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let shifted: Vec<i64> = stored.iter().map(|s| s + 1).collect();
    assert_eq!(resim, shifted, "one hill each, so one point each, and no other difference");

    // And the range form walks the same match in one pass.
    let ranged = invoke("tb.ants.replay-decode",
                        json!({"payload": &payload, "from": 0, "to": turns})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), turns as usize + 1);
    assert_eq!(&frames[0], f0);
    assert_eq!(&frames[turns as usize], &last["frame"]);
}


// ---------------------------------------------------------------- the reference engine's rules
//
// Everything below was found by auditing this engine against `aichallenge/ants/ants.py` and the
// published specification. Each test names the rule and, where the two disagree, the engine wins:
// the prose was written once and the code is what every bot was actually scored against.

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

#[test]
fn food_blocks_movement_exactly_as_water_does() {
    // `ants.py:610` refuses a destination that is FOOD or WATER with the same "move blocked", and
    // the specification says it in words: "Food will also block an ants movement. This can happen
    // if food spawns next to an ant. Don't move the ant and it will be gathered the next turn."
    let mut m = two_sided(20, 20);
    let start = at(&m, 5, 5);
    m.food.push(at(&m, 5, 6));
    m.ants.push(Ant { pos: start, owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 1 });

    // Target zero, so the board is not re-stocked and the one food can be watched to the end.
    step(&mut m, &[orders(&["E"]), orders(&["-"])]);

    assert_eq!(m.ants_of(0).next().unwrap().pos, start, "the order east was ignored");
    assert_eq!(m.hive[0], 1, "and the food it could not walk onto was gathered anyway");
    assert!(m.food.is_empty());
}

#[test]
fn food_only_blocks_the_square_it_is_on() {
    // The other half, so the fix cannot be "ants stop moving near food".
    let mut m = two_sided(20, 20);
    m.food.push(at(&m, 5, 6));
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 1 });

    play(&mut m, &["N"], &["-"]);

    assert_eq!(m.ants_of(0).next().unwrap().pos, at(&m, 4, 5), "north was never blocked");
}

#[test]
fn each_player_starts_with_one_point_per_hill() {
    // `ants.py:152`: "points start at # of hills to prevent negative scores".
    let m = worldgen(7, crate::map::preset("standard").unwrap(), 1000);
    assert_eq!(m.score, vec![1, 1], "one hill each, so one point each");
}

#[test]
fn losing_your_only_hill_leaves_you_on_zero() {
    // The whole reason the starting points exist, in the specification's own words: "if you don't
    // attack and lose all your hills, you will end up with 0 points."
    let mut m = two_sided(20, 20);
    // `bare` builds its hills after construction, so mirror what worldgen does for itself.
    m.score = vec![1, 1];
    m.ants.push(Ant { pos: at(&m, 15, 15), owner: 0 }); // far away, and never attacks
    m.ants.push(Ant { pos: at(&m, 0, 1), owner: 1 });

    play(&mut m, &["-"], &["W"]);

    assert!(m.hills[0].razed);
    assert_eq!(m.score[0], 0, "lost its only hill and razed nothing: zero, not minus one");
    assert_eq!(m.score[1], 3, "one to start with, two for the razing");
}

// ---------------------------------------------------------------- the cutoff counter

#[test]
fn hive_food_counts_toward_the_dominant_share() {
    // `ants.py:1503`: the population is every ant, **plus the hive of every player that still has a
    // hill standing**, plus every food on the map. Counting ants alone — which this engine used to
    // do — misses a player whose lead is sitting in the hive waiting to be spawned.
    let mut m = two_sided(20, 20);
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    for c in 0..3 {
        m.ants.push(Ant { pos: at(&m, 15, 10 + c), owner: 1 });
    }
    // One ant against three is not domination; one ant and twenty in the hive is.
    m.hive[0] = 20;

    play(&mut m, &["-"], &["-", "-", "-"]);

    assert_eq!(m.cutoff_bot, 0, "player 0 holds the share through its hive");
    assert_eq!(m.cutoff_turns, 1);
}

#[test]
fn a_death_on_a_contested_hill_stalls_the_cutoff() {
    // The specification's "Update :" paragraph, and `ants.py:793`. A bot sitting on a big lead is
    // given time to finish the job: while ants are dying on a hill it does not own, the counter
    // does not advance. Without this, a dominant bot is cut off in the middle of its assault.
    let mut m = two_sided(30, 30);
    let target = m.hills[1].pos; // player 1's hill, at (15, 15)
    for c in 0..20 {
        m.ants.push(Ant { pos: at(&m, 2, c), owner: 0 });
    }
    m.ants.push(Ant { pos: at(&m, 25, 25), owner: 1 });

    // A quiet turn first, so the counter has a holder to advance.
    play(&mut m, &[], &[]);
    assert_eq!(m.cutoff_bot, 0);
    assert_eq!(m.cutoff_turns, 1);

    // Now two of player 0's ants walk onto player 1's hill together and kill each other there.
    m.ants.push(Ant { pos: at(&m, 15, 14), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 16), owner: 0 });
    let mine = m.mine(0);
    let ord: Vec<String> = mine
        .iter()
        .map(|&p| {
            if p == at(&m, 15, 14) {
                "E".into()
            } else if p == at(&m, 15, 16) {
                "W".into()
            } else {
                "-".into()
            }
        })
        .collect();
    step(&mut m, &[ord, orders(&["-"])]);

    assert!(!m.ants.iter().any(|a| a.pos == target), "both died on the hill");
    assert!(!m.hills[1].razed, "and dying on a hill razes nothing");
    assert_eq!(m.cutoff_turns, 1, "the counter was stalled, not advanced");

    // And it resumes once the fighting stops.
    play(&mut m, &[], &[]);
    assert_eq!(m.cutoff_turns, 2);
}

#[test]
fn razing_a_hill_resets_the_cutoff_counter() {
    // `ants.py:752`. A hill just fell, so whatever the counter was watching, this game is still
    // being fought — it restarts from zero rather than carrying on from where it was.
    let mut m = two_sided(30, 30);
    let target = m.hills[1].pos; // player 1's hill, at (15, 15)
    for c in 0..20 {
        m.ants.push(Ant { pos: at(&m, 2, c), owner: 0 });
    }
    m.ants.push(Ant { pos: at(&m, 25, 25), owner: 1 });

    for _ in 0..3 {
        play(&mut m, &[], &[]);
    }
    assert_eq!(m.cutoff_bot, 0);
    assert_eq!(m.cutoff_turns, 3);

    // One ant, alone, steps onto the hill and survives there.
    m.ants.push(Ant { pos: at(&m, 15, 14), owner: 0 });
    let mine = m.mine(0);
    let ord: Vec<String> =
        mine.iter().map(|&p| if p == at(&m, 15, 14) { "E".into() } else { "-".into() }).collect();
    step(&mut m, &[ord, orders(&["-"])]);

    assert!(m.hills[1].razed, "the hill fell");
    assert!(m.ants.iter().any(|a| a.pos == target && a.owner == 0));
    assert_eq!(m.cutoff_turns, 1, "reset to zero by the razing, then this turn counted");
}

// ---------------------------------------------------------------- *Cutoff Rules*, as the engine has it

#[test]
fn a_player_without_hills_is_not_given_the_chance_to_overtake() {
    // `ants.py:1362` considers only players that are alive **and** still hold a hill. The
    // specification is explicit: "Even though bots without hills left could still possibly gain in
    // rank, the game is not extended [for] them, only those with hills."
    //
    // Player 0 here has an ant one square from player 1's hill and would go ahead by razing it. It
    // never gets the turn, and that is the rule rather than a bug.
    let mut m = bare(20, 20, 2);
    m.hills.push(Hill { pos: at(&m, 10, 10), owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 10, 9), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 2, 2), owner: 1 });

    play(&mut m, &["-"], &["-"]);

    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "rank_stabilized");
    assert!(m.alive(0), "still alive, just out of chances");
}

#[test]
fn an_opponent_is_assumed_to_lose_every_hill_it_still_holds() {
    // The `min_score` half of `is_rank_stabilized`: an opponent's floor is its score *after* losing
    // every hill it holds, not its score today. Player 0 is three behind with one hill each: it can
    // reach 0 + 2 = 2, and player 1 can fall to 3 − 1 = 2. Level is a rank change, so this match is
    // still live. Compare the opponent at face value instead and the game ends here, wrongly.
    let mut m = two_sided(20, 20);
    m.score = vec![0, 3];
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 16), owner: 1 });

    play(&mut m, &["-"], &["-"]);

    assert!(!m.done, "the trailing player can still draw level, so the game goes on");
}

#[test]
fn a_lead_no_hill_can_close_ends_the_match() {
    // The same test in the other direction, so it is measuring something. Four ahead, one hill
    // each: player 0 can reach 2 and player 1 can only fall, so nothing anyone does changes the
    // order.
    let mut m = two_sided(20, 20);
    m.score = vec![0, 4];
    m.ants.push(Ant { pos: at(&m, 5, 5), owner: 0 });
    m.ants.push(Ant { pos: at(&m, 15, 16), owner: 1 });

    play(&mut m, &["-"], &["-"]);

    assert!(m.done);
    assert_eq!(state::END_REASONS[m.reason as usize], "rank_stabilized");
}

// ---------------------------------------------------------------- *Ant Spawning*, touched not spawned

#[test]
fn standing_on_your_own_hill_touches_it_for_spawn_priority() {
    // `ants.py:812`: the raze phase stamps `last_touched` whenever the owner's own ant is standing
    // on its own hill. Stamping only on spawn — which this engine used to do — breaks the one
    // technique the specification spells out: "You can control which hill to spawn at by keeping an
    // ant nearby to block the hill when you don't want it to spawn ants."
    let mut m = bare(20, 20, 2);
    let a = at(&m, 5, 5);
    let b = at(&m, 15, 15);
    m.hills.push(Hill { pos: a, owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: b, owner: 0, razed: false, last_touched: 0 });
    m.hills.push(Hill { pos: at(&m, 10, 10), owner: 1, razed: false, last_touched: 0 });
    m.ants.push(Ant { pos: at(&m, 18, 2), owner: 1 });

    // Turn one: an ant of player 0 sits on hill `a`. Nothing spawns; `a` is touched.
    m.ants.push(Ant { pos: a, owner: 0 });
    play(&mut m, &["-"], &["-"]);
    assert!(m.hills[0].last_touched > m.hills[1].last_touched, "a was touched, b was not");

    // Turn two: clear the way and offer one food. The untouched hill must go first.
    m.ants.retain(|ant| ant.owner == 1);
    m.hive[0] = 1;
    play(&mut m, &[], &["-"]);

    assert_eq!(m.ants_of(0).next().unwrap().pos, b, "the least recently touched hill spawned");
}

// ---------------------------------------------------------------- the specification's own fights
//
// Worked examples copied from the Focus Battle Resolution page. They are the best conformance
// corpus the game has, and encoding them by name makes the correspondence checkable by eye.

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

#[test]
fn spec_scenario_one_on_one_on_one() {
    // ...B.     ...2.     ...x.
    // .A...  -> .2... ->  .x...
    // ...C.     ...2.     ...x.
    // Three colours, all mutually in range, every focus 2: all three die.
    let m = fight(20, 20, 3, &[(1, 1, 0), (0, 3, 1), (2, 3, 2)]);
    assert!(survivors(&m).is_empty(), "all three die");
}

#[test]
fn spec_scenario_ant_sandwich() {
    // A . B . C -> 1.2.1 -> A . x . C
    // The ant in the middle is the only one with its attention split.
    let m = fight(20, 20, 3, &[(0, 0, 0), (0, 2, 1), (0, 4, 2)]);
    assert_eq!(survivors(&m), vec![(0, 0, 0), (0, 4, 2)], "only the centre ant dies");
}

#[test]
fn spec_scenario_one_on_two_on_one() {
    // ...B.     ...3.     ...x.
    // .A.A.  -> .2.2. ->  .A.A.
    // ...C.     ...3.     ...x.
    // Two ants supporting each other kill two enemies and take nothing.
    let m = fight(20, 20, 3, &[(1, 1, 0), (1, 3, 0), (0, 3, 1), (2, 3, 2)]);
    assert_eq!(survivors(&m), vec![(1, 1, 0), (1, 3, 0)], "B and C die, both A live");
}

#[test]
fn spec_scenario_wall_punch() {
    // AAAAAAAAA    013565310    AAxxxxxAA
    // ...BBB...  -> ...555... -> ...xxx...
    // ...BBB...    ...333...    ...xBx...
    //
    // The most discriminating example on the page: the ants on the ends of the wall live because
    // the only enemy that reaches them is far more occupied, and the middle ant of the back rank
    // lives because everything attacking it is more occupied than it is.
    let mut ants: Vec<(i32, i32, u8)> = (0..9).map(|c| (0, c, 0u8)).collect();
    for c in 3..6 {
        ants.push((1, c, 1));
        ants.push((2, c, 1));
    }
    let m = fight(20, 20, 2, &ants);
    assert_eq!(
        survivors(&m),
        vec![(0, 0, 0), (0, 1, 0), (0, 7, 0), (0, 8, 0), (2, 4, 1)],
        "four wall ants and the one ant behind the line"
    );
}

#[test]
fn spec_scenario_bob_and_bill_versus_roy_and_ralph() {
    // ..B....     ..1....     ..B....
    // ..B.R..  -> ..2.2.. ->  ..x.x..
    // ....R..     ....1..     ....R..
    // The two ants whose attention is split die; the two that could concentrate survive.
    let m = fight(20, 20, 2, &[(0, 2, 0), (1, 2, 0), (1, 4, 1), (2, 4, 1)]);
    assert_eq!(survivors(&m), vec![(0, 2, 0), (2, 4, 1)], "Bill and Roy die");
}

// ---------------------------------------------------------------- food, at the hidden rate

/// A board with a hidden rate set by hand, so the accrual can be watched exactly.
fn fed(rows: u8, cols: u8, rate: u16, per: u16) -> Match {
    let mut m = two_sided(rows, cols);
    m.food_rate = rate;
    m.food_turn = per;
    m.ants.push(Ant { pos: m.g.at(3, 3), owner: 0 });
    m.ants.push(Ant { pos: m.g.at(rows as i32 - 3, cols as i32 - 3), owner: 1 });
    m
}

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
        let rep = crate::state::nth_set(&sets, m.seed, 0, cursor);
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
        (0..sets.len()).map(|c| crate::state::nth_set(&sets, m.seed, 1, c)).collect();
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
    let rep = crate::state::nth_set(&sets, m.seed, 0, 0);
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
