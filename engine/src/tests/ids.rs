//! Ant ids: which of a seat's ants is which, kept for an ant's whole life.
//!
//! `mine` is re-sorted every turn, so its order says nothing about identity. A model that remembers
//! something about an ant finds it again by the id the view sends beside it, and every one of these
//! rules is one that model would silently mis-key on if it broke.

use super::*;
use crate::observe::view;

/// The id of the ant standing on `(r, c)`.
fn id_at(m: &Match, r: i32, c: i32) -> u32 {
    let pos = at(m, r, c);
    m.ants.iter().find(|a| a.pos == pos).unwrap_or_else(|| panic!("no ant on ({r},{c})")).id
}

/// A view's `mine` and `ids`, zipped.
fn named(v: &Value) -> Vec<((u64, u64), u64)> {
    let mine = v["mine"].as_array().unwrap();
    let ids = v["ids"].as_array().unwrap();
    assert_eq!(mine.len(), ids.len(), "one id per ant in `mine`");
    mine.iter()
        .zip(ids)
        .map(|(a, id)| ((a[0].as_u64().unwrap(), a[1].as_u64().unwrap()), id.as_u64().unwrap()))
        .collect()
}

#[test]
fn an_ant_keeps_its_id_when_it_moves() {
    // Ant 0 walks south past ant 1, so the row-major order the two are sent in flips. The ids
    // follow the ants, not the slots.
    let mut m = two_sided(20, 20);
    m.add_ant(at(&m, 5, 5), 0);
    m.add_ant(at(&m, 6, 8), 0);
    m.add_ant(at(&m, 15, 15), 1);
    assert_eq!(named(&view(&m, 0)), [((5, 5), 0), ((6, 8), 1)]);

    play(&mut m, &["S", "-"], &["-"]);
    play(&mut m, &["S", "-"], &["-"]); // `mine` is now [(6,8), (7,5)]: ant 0 is second
    play(&mut m, &["-", "-"], &["-"]); // and a turn later, in the new order
    assert_eq!(m.turn, 3, "the match is still being played");
    assert_eq!((id_at(&m, 7, 5), id_at(&m, 6, 8)), (0, 1));
    assert_eq!(named(&view(&m, 0)), [((6, 8), 1), ((7, 5), 0)]);
    assert_eq!(named(&view(&m, 1)), [((15, 15), 0)], "ids are per seat, counted from 0");
}

#[test]
fn an_ant_keeps_its_id_when_its_move_is_blocked() {
    // Water, food and no order at all each leave an ant where it was, and each must leave it with
    // its own id: the ant beside it moves, and the order `mine` sends them in changes around them.
    let mut m = two_sided(20, 20);
    m.water.set(at(&m, 4, 5) as usize);
    m.food.push(at(&m, 7, 10));
    m.add_ant(at(&m, 5, 5), 0); // 0: ordered into water
    m.add_ant(at(&m, 4, 7), 0); // 1: walks south, past ant 0
    m.add_ant(at(&m, 7, 9), 0); // 2: ordered onto food
    m.add_ant(at(&m, 3, 12), 0); // 3: holds
    m.add_ant(at(&m, 15, 15), 1);
    // `mine` is row-major: (3,12) ant 3, (4,7) ant 1, (5,5) ant 0, (7,9) ant 2.
    play(&mut m, &["-", "S", "N", "E"], &["-"]);

    assert_eq!(
        [id_at(&m, 5, 5), id_at(&m, 5, 7), id_at(&m, 7, 9), id_at(&m, 3, 12)],
        [0, 1, 2, 3],
        "a blocked ant is the same ant"
    );
    assert_eq!(named(&view(&m, 0)), [((3, 12), 3), ((5, 5), 0), ((5, 7), 1), ((7, 9), 2)]);
}

#[test]
fn an_id_is_never_given_to_a_second_ant() {
    // Two ants collide and die; the ant spawned the same turn takes the next id, not either of
    // theirs. A model keying its notes by id would otherwise hand a dead ant's target to a newborn.
    let mut m = two_sided(20, 20);
    m.add_ant(at(&m, 4, 3), 0);
    m.add_ant(at(&m, 4, 5), 0);
    m.add_ant(at(&m, 15, 15), 1);
    m.hive[0] = 1;
    play(&mut m, &["E", "W"], &["-"]);
    assert_eq!(named(&view(&m, 0)), [((0, 0), 2)], "the newborn on the hill is ant 2");

    // And over real matches on every basic board: a seat's living ants never share an id, an id
    // that died never comes back, and every new id is the next one.
    for mf in boards::all() {
        let w = invoke("tb.ants.worldgen", json!({"seeds": [31], "map": boards::json(&mf.id)}))
            .unwrap();
        let mut state = w["wave_state"].as_str().unwrap().to_string();
        let seats = mf.players as usize;
        let mut alive: Vec<Vec<u64>> = vec![Vec::new(); seats];
        let mut next: Vec<u64> = vec![0; seats];
        let mut rng = Rng(9);
        for turn in 0..300 {
            let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
            if views["views"].as_array().unwrap().is_empty() {
                break;
            }
            for v in views["views"].as_array().unwrap() {
                let seat = v["seat"].as_u64().unwrap() as usize;
                let mut ids: Vec<u64> = named(&v["view"]).into_iter().map(|(_, id)| id).collect();
                ids.sort_unstable();
                assert!(ids.windows(2).all(|p| p[0] < p[1]), "{} t{turn}: an id twice", mf.id);
                for &id in &ids {
                    if !alive[seat].contains(&id) {
                        assert_eq!(
                            id, next[seat],
                            "{} t{turn} seat {seat}: not the next id",
                            mf.id
                        );
                        next[seat] += 1;
                    }
                }
                alive[seat] = ids;
            }
            let acts = random_actions(&views, &mut rng);
            state = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap()
                ["wave_state"]
                .as_str()
                .unwrap()
                .to_string();
        }
        let hills = unpack(&state).unwrap().matches[0].hills.len() as u64;
        assert!(next.iter().sum::<u64>() > hills, "{}: nothing was ever spawned", mf.id);
    }
}

#[test]
fn ids_are_the_same_on_every_seat_under_the_shift() {
    // Seat `k`'s board is seat 0's moved `k` times by the shift, and its `j`th hill is seat 0's
    // `j`th moved. So when every seat plays the same policy in seat 0's frame, seat `k`'s ant on the
    // image of a square carries the id seat 0's ant on that square does: ids per seat, taken in
    // hill-list and spawn order, leave the seats symmetric, which is what `sweep` proves of the rest
    // of the view.
    for mf in boards::all() {
        let board = boards::json(&mf.id);
        let (rows, cols) = (board["rows"].as_u64().unwrap(), board["cols"].as_u64().unwrap());
        let (dr, dc) =
            (board["symmetry"]["dr"].as_u64().unwrap(), board["symmetry"]["dc"].as_u64().unwrap());
        let home = |s: u64, (r, c): (u64, u64)| {
            // `s` shifts back is `players - s` shifts on: the shift has order `players`.
            let k = mf.players as u64 - s;
            ((r + k * dr) % rows, (c + k * dc) % cols)
        };
        let w = invoke("tb.ants.worldgen", json!({"seeds": [77], "map": board})).unwrap();
        let mut state = w["wave_state"].as_str().unwrap().to_string();
        let mut spawned = false;
        for turn in 0u64..400 {
            let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
            let views = views["views"].as_array().unwrap();
            if views.is_empty() {
                break;
            }
            let frames: Vec<Vec<((u64, u64), u64)>> = views
                .iter()
                .map(|v| {
                    let s = v["seat"].as_u64().unwrap();
                    let mut f: Vec<_> =
                        named(&v["view"]).into_iter().map(|(p, id)| (home(s, p), id)).collect();
                    f.sort_unstable();
                    f
                })
                .collect();
            for (s, f) in frames.iter().enumerate().skip(1) {
                assert_eq!(f, &frames[0], "{} turn {turn}: seat {s}'s ids are not seat 0's", mf.id);
            }
            spawned |= frames[0]
                .iter()
                .any(|&(_, id)| id as usize >= mf.hills.len() / mf.players as usize);

            // A walker that reads only the square in seat 0's frame and the turn, so every seat
            // plays the same policy. The shift is a translation, so a direction needs no moving.
            let acts: Vec<Value> = views
                .iter()
                .map(|v| {
                    let s = v["seat"].as_u64().unwrap();
                    let orders: Vec<&str> = named(&v["view"])
                        .into_iter()
                        .map(|(p, _)| {
                            let (r, c) = home(s, p);
                            let h = (r * 131 + c * 31 + turn * 7).wrapping_mul(0x9E37_79B9) >> 7;
                            ["N", "E", "S", "W", "-"][(h % 5) as usize]
                        })
                        .collect();
                    json!(orders)
                })
                .collect();
            state = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap()
                ["wave_state"]
                .as_str()
                .unwrap()
                .to_string();
        }
        assert!(spawned, "{}: no ant was spawned, so only the starting ids were compared", mf.id);
    }
}
