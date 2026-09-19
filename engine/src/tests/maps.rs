//! Boards as files: the basic boards, validation, and how a caller hands one over.

use super::*;
use crate::maps::MapFile;

/// A hand-drawn board: `water` and `hills` are seat 0's, and each is written to its whole orbit
/// under the shift, so a test states only the one thing it means to break.
fn hand(
    size: (u8, u8),
    players: u8,
    shift: (i32, i32),
    water: &[(i32, i32)],
    hills: &[(i32, i32)],
) -> MapFile {
    let g = Geom::new(size.0, size.1);
    let sym = Symmetry::new(players, shift.0, shift.1);
    let mut bits = Bits::zeros(g.cells());
    for &(r, c) in water {
        for k in 0..players as i32 {
            bits.set(sym.image(&g, g.at(r, c), k) as usize);
        }
    }
    let mut orbit_hills = Vec::new();
    for &(r, c) in hills {
        for k in 0..players as i32 {
            orbit_hills.push(g.rc(sym.image(&g, g.at(r, c), k)));
        }
    }
    MapFile {
        id: "hand".to_string(),
        rows: size.0,
        cols: size.1,
        players,
        symmetry: shift,
        water: bits.rle(),
        hills: orbit_hills,
        food: Vec::new(),
        food_target: 0,
    }
}

fn refusal(mf: &MapFile) -> String {
    invoke("tb.ants.worldgen", json!({"seeds": [1], "map": mf.to_json()}))
        .unwrap_err()
        .code
        .to_string()
}

#[test]
fn every_basic_board_is_valid_and_symmetric() {
    // What the generator's construction guarantees, asserted over every board this repository ships.
    let all = boards::all();
    for mf in &all {
        mf.validate().unwrap_or_else(|e| panic!("map {}: {} {}", mf.id, e.code, e.message));
        let m = mf.build(1, 1000).unwrap();
        let g = m.g;
        for i in 0..g.cells() {
            let img = m.sym.image(&g, i as u16, 1) as usize;
            assert_eq!(m.water.get(i), m.water.get(img), "map {} is not symmetric", mf.id);
        }
        assert_eq!(m.hills.len() % mf.players as usize, 0, "map {}: whole orbits of hills", mf.id);
        assert_eq!(m.ants.len(), m.hills.len(), "map {}: one ant a hill", mf.id);
        assert!(m.food.len() % mf.players as usize == 0, "map {}: whole orbits", mf.id);
        for h in &m.hills {
            assert!(!m.water.get(h.pos as usize), "map {}: a hill on water", mf.id);
        }
    }
}

#[test]
fn the_basic_boards_reach_from_two_seats_to_eight() {
    // They are the envelope: `tools/package.py` derives `limits.boards` from them, the
    // reference set is drawn on them, and every season upload must fit inside what they span. A set
    // that stopped at six seats would quietly refuse every eight-seat board a season designed.
    let seats: Vec<u8> = boards::all().iter().map(|m| m.players).collect();
    assert_eq!(seats.iter().min(), Some(&2), "{seats:?}");
    assert_eq!(seats.iter().max(), Some(&8), "{seats:?}");
}

#[test]
fn a_map_survives_the_round_trip_to_a_file_and_back() {
    // What a replay rests on: the board `finish` writes into the envelope, read back, is the same
    // board -- its shift included.
    for mf in boards::all() {
        let back = MapFile::from_json(&mf.to_json()).unwrap();
        assert_eq!(back, mf, "{} did not survive the round trip", mf.id);

        let m = mf.build(0xBEEF, 1000).unwrap();
        let from = MapFile::from_match(&m, &mf.id);
        assert_eq!(from.symmetry, mf.symmetry, "{}: shift", mf.id);
        assert_eq!(from.water, mf.water, "{}: terrain", mf.id);
        assert_eq!(from.hills, mf.hills, "{}: hills", mf.id);
        assert_eq!(from.food, mf.food, "{}: turn-zero food", mf.id);

        let sym = m.sym;
        let w = crate::codec::Wave { matches: vec![m] };
        let unpacked = crate::codec::unpack(&crate::codec::pack(&w)).unwrap();
        assert_eq!(unpacked.matches[0].sym, sym, "{}: the shift in wave_state", mf.id);
    }
}

#[test]
fn the_caller_hands_over_the_board_and_the_engine_carries_none() {
    // The component has no catalogue, so a board is never named -- it is sent, whole. On the
    // ladder pair chooses it and the claim carries it; a competitor still cannot pick their own.
    let duel = boards::json(boards::DUEL);
    let other = boards::board("basic-small-3p");
    let ids = |input: serde_json::Value| -> Vec<String> {
        invoke("tb.ants.worldgen", input).unwrap()["map_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };

    // One board for the wave, and the same seeds are the same matches.
    let a = invoke("tb.ants.worldgen", json!({"seeds": [7, 8], "map": duel})).unwrap();
    let b = invoke("tb.ants.worldgen", json!({"seeds": [7, 8], "map": duel})).unwrap();
    assert_eq!(a["wave_state"], b["wave_state"], "the same board and seeds are the same wave");
    assert_eq!(ids(json!({"seeds": [7, 8], "map": duel})), vec![boards::DUEL, boards::DUEL]);

    // One per seed, positionally, and a null there falls back to `map`.
    let each = ids(json!({"seeds": [7, 8], "map": duel, "maps": [other.to_json(), null]}));
    assert_eq!(each, vec![other.id.clone(), boards::DUEL.to_string()]);

    // A board nobody has ever shipped plays exactly as a basic one does.
    let mut mine = boards::board(boards::DUEL);
    mine.id = "hand-authored".to_string();
    assert_eq!(ids(json!({"seeds": [7], "map": mine.to_json()})), vec!["hand-authored"]);

    // No board, and a board by name, are refusals -- never a board chosen for the caller.
    let no = invoke("tb.ants.worldgen", json!({"seeds": [1]})).unwrap_err();
    assert_eq!(no.code, "NO_MAP");
    let named = invoke("tb.ants.worldgen", json!({"seeds": [1], "map": "basic-tiny-2p"}));
    assert_eq!(named.unwrap_err().code, "MAP_BAD_SHAPE");
    let short = invoke("tb.ants.worldgen", json!({"seeds": [1, 2], "maps": [duel]}));
    assert_eq!(short.unwrap_err().code, "NO_MAP", "a seed with no board of its own and no `map`");
}

#[test]
fn a_map_that_is_not_symmetric_is_refused_rather_than_played() {
    // Each of these is the first thing someone hand-authoring a board will do.
    let base = boards::board(boards::DUEL);

    // One cell of water that has no counterpart.
    let mut asym = base.clone();
    asym.water = vec![1, 1, 0, (asym.rows as u32 * asym.cols as u32) - 1];
    assert_eq!(refusal(&asym), "MAP_NOT_SYMMETRIC");

    // A hill that is not the image of the first.
    let mut moved = base.clone();
    moved.hills[1] = (moved.hills[1].0 + 1, moved.hills[1].1);
    assert_eq!(refusal(&moved), "MAP_NOT_SYMMETRIC");

    // Food for one seat only.
    let mut greedy = base.clone();
    greedy.food.truncate(1);
    assert_eq!(refusal(&greedy), "MAP_NOT_SYMMETRIC");

    // A board that starts stocked and never restocks is allowed.
    let mut frugal = base.clone();
    frugal.food_target = 0;
    assert!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "map": frugal.to_json()})).is_ok(),
        "a board may hold more food than it keeps stocked"
    );

    // Runs that do not cover the board.
    let mut short = base.clone();
    short.water = vec![0, 4];
    assert_eq!(refusal(&short), "MAP_BAD_SHAPE");

    // A board its shift does not go into, so the orbits are not a partition.
    let mut odd = base.clone();
    odd.rows = 63;
    odd.water = vec![0, 63 * odd.cols as u32];
    assert_eq!(refusal(&odd), "MAP_BAD_SHAPE");

    // A hill under water.
    let mut drowned = base.clone();
    let (g, sym) = (drowned.geom(), drowned.sym());
    let mut w = Bits::zeros(g.cells());
    let (hr, hc) = drowned.hills[0];
    for k in 0..drowned.players as i32 {
        w.set(sym.image(&g, g.at(hr, hc), k) as usize);
    }
    drowned.water = w.rle();
    drowned.food.clear();
    assert_eq!(refusal(&drowned), "MAP_UNPLAYABLE");

    // And a board by name is a refusal, not a silently substituted one: there is nothing to look
    // a name up in.
    assert_eq!(
        invoke("tb.ants.worldgen", json!({"seeds": [1], "map": "no-such-map"})).unwrap_err().code,
        "MAP_BAD_SHAPE"
    );
}

#[test]
fn the_shift_is_the_boards_and_is_played_as_written() {
    // Seats half the rows apart and not the columns: water symmetric under that shift is a fair
    // board, and the match seats seat 1 exactly there.
    let rows_apart = hand((16, 16), 2, (8, 0), &[(3, 5), (3, 6)], &[(1, 1)]);
    let w = invoke("tb.ants.worldgen", json!({"seeds": [1], "map": rows_apart.to_json()})).unwrap();
    let m = &crate::codec::unpack(w["wave_state"].as_str().unwrap()).unwrap().matches[0];
    assert_eq!((m.sym.dr, m.sym.dc), (8, 0));
    assert_eq!(m.g.rc(m.hills[1].pos), (9, 1), "seat 1's hill is seat 0's moved by the shift");

    // The same board declared with the diagonal shift is not symmetric under it.
    let mut diagonal = rows_apart.clone();
    diagonal.symmetry = (8, 8);
    assert_eq!(refusal(&diagonal), "MAP_NOT_SYMMETRIC");

    // A shift that needs four steps to come home cannot seat two players, and one that never comes
    // home cannot seat anyone.
    let mut quarter = rows_apart.clone();
    quarter.symmetry = (4, 0);
    assert_eq!(refusal(&quarter), "MAP_BAD_SHAPE");
    let mut stray = rows_apart.clone();
    stray.symmetry = (5, 0);
    assert_eq!(refusal(&stray), "MAP_BAD_SHAPE");

    // A board that names no shift gets the diagonal one.
    let mut unnamed = hand((16, 16), 2, (8, 8), &[(3, 5)], &[(1, 1)]).to_json();
    unnamed.as_object_mut().unwrap().remove("symmetry");
    assert_eq!(MapFile::from_json(&unnamed).unwrap().symmetry, (8, 8));
}

#[test]
fn land_that_cannot_be_walked_to_is_refused() {
    // A square of land sealed in water: food spawned on it could never be gathered, and would still
    // count as loose food for the idle-food ending.
    let pocket = [(4, 11), (6, 11), (5, 10), (5, 12)];
    let sealed = hand((16, 16), 2, (8, 8), &pocket, &[(1, 1)]);
    assert_eq!(refusal(&sealed), "MAP_UNPLAYABLE");

    // Touching at a corner is not a path: ants move in four directions.
    let corner = [(4, 11), (5, 10), (6, 11), (5, 12), (4, 10), (4, 12), (6, 10)];
    let only_diagonal = hand((16, 16), 2, (8, 8), &corner, &[(1, 1)]);
    assert_eq!(refusal(&only_diagonal), "MAP_UNPLAYABLE");

    // Open the pocket on one side and it is a board.
    let opened = hand((16, 16), 2, (8, 8), &pocket[..3], &[(1, 1)]);
    assert!(invoke("tb.ants.worldgen", json!({"seeds": [1], "map": opened.to_json()})).is_ok());
}

#[test]
fn a_hill_with_no_way_off_is_refused() {
    // The check used to include the hill's own square, which was land by then, so it could not fail.
    let ring = [(0, 1), (2, 1), (1, 0), (1, 2)];
    let walled = hand((16, 16), 2, (8, 8), &ring, &[(1, 1)]);
    let e = invoke("tb.ants.worldgen", json!({"seeds": [1], "map": walled.to_json()})).unwrap_err();
    assert_eq!(e.code, "MAP_UNPLAYABLE");
    assert!(e.message.contains("walled in"), "{}", e.message);
}

#[test]
fn several_hills_a_seat_come_in_whole_orbits() {
    // Two hills a seat, listed orbit by orbit: hill i is seat i % players's.
    let two = hand((16, 16), 2, (8, 8), &[], &[(1, 1), (3, 12)]);
    let w = invoke("tb.ants.worldgen", json!({"seeds": [1], "map": two.to_json()})).unwrap();
    let m = &crate::codec::unpack(w["wave_state"].as_str().unwrap()).unwrap().matches[0];
    let owners: Vec<u8> = m.hills.iter().map(|h| h.owner).collect();
    assert_eq!(owners, vec![0, 1, 0, 1]);
    assert_eq!(m.score, vec![2, 2]);

    // The same hills listed seat by seat are not orbits.
    let mut by_seat = two.clone();
    by_seat.hills.swap(1, 2);
    assert_eq!(refusal(&by_seat), "MAP_NOT_SYMMETRIC");

    // And a seat cannot have more hills than another.
    let mut odd = two.clone();
    odd.hills.pop();
    assert_eq!(refusal(&odd), "MAP_UNPLAYABLE");

    // Nor may food stand on one.
    let mut fed = two.clone();
    fed.food = fed.hills[..2].to_vec();
    assert_eq!(refusal(&fed), "MAP_UNPLAYABLE");
}
