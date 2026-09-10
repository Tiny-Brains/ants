//! The rules of one turn: moving, collisions, combat, hills, spawning, gathering and fog.

use super::*;
use crate::state::{Ant, Hill};
use crate::turn::step;

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

#[test]
fn each_player_starts_with_one_point_per_hill() {
    // `ants.py:152`: "points start at # of hills to prevent negative scores".
    let m = worldgen(7, crate::map::preset("standard").unwrap(), 1000);
    assert_eq!(m.score, vec![1, 1], "one hill each, so one point each");
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
fn contested_food_is_unreachable_at_the_standard_radii() {
    // A property of the settings rather than of this code, tested so a change to them is noticed.
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

#[test]
fn a_view_is_observer_relative() {
    // *docs/protocol.md* §3.1: "relabel every seat in the world, ask the same player under its new
    // label, and the bytes must be identical". The engine used to emit raw seat numbers, so seat 1
    // saw its own hill labelled `1` while seat 0 saw its own labelled `0` -- two seats of one match
    // are meant to be two samples of one distribution, and they were not.
    //
    // The property, stated as a symmetry: build a position, then build its mirror with the two
    // seats swapped, and seat 0's view of the first must be byte-identical to seat 1's view of the
    // second. `schema/validate.py` checks this shape against a stand-in; this checks the engine.
    let build = |a: u8, b: u8| {
        let mut m = bare(40, 40, 2);
        m.ants.push(Ant { pos: at(&m, 5, 5), owner: a });
        m.ants.push(Ant { pos: at(&m, 5, 7), owner: b });
        m.hills.push(Hill { pos: at(&m, 5, 4), owner: a, razed: false, last_touched: 0 });
        m.hills.push(Hill { pos: at(&m, 5, 8), owner: b, razed: false, last_touched: 0 });
        m.reveal(0);
        m.reveal(1);
        m
    };
    let straight = build(0, 1);
    let mirrored = build(1, 0);
    assert_eq!(
        crate::observe::view(&straight, 0),
        crate::observe::view(&mirrored, 1),
        "the same player under a different label must see the same bytes"
    );

    // And the labels are the ones state.schema.json promises: yourself 0, an opponent 1 upward.
    let v = crate::observe::view(&straight, 1);
    let own: Vec<i64> = v["hills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h[0] == json!(5) && h[1] == json!(8))
        .map(|h| h[2].as_i64().unwrap())
        .collect();
    assert_eq!(own, vec![0], "seat 1's own hill is owner 0 in seat 1's view");
    for f in v["foes"].as_array().unwrap() {
        assert_eq!(f[2], json!(1), "an opponent is 1 upward, never your own seat number");
    }
}
