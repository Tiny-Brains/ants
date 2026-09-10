//! How a match ends: the four end conditions, and the one cutoff counter behind two of them.

use super::*;
use crate::state::{Ant, Hill};
use crate::turn::{ranks, step};

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
        state::END_REASONS[m.reason as usize],
        "extermination",
        "losing hills is not elimination"
    );
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

#[test]
fn a_colony_that_is_eating_is_never_cut_off_as_idle() {
    // The food cutoff is a **population share**, not "nobody gathered this turn" and not "the hive
    // total did not move": loose food has to be 85% of every ant, hive and food in the game before
    // it means anything. Both weaker readings fire during an ordinary lull between two healthy
    // colonies.
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
