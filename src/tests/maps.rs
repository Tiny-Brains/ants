//! Boards as files: the committed catalogue, validation, and how a board is chosen.

use super::*;

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
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
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
            .unwrap_err()
            .code
            .to_string()
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
        invoke(
            "tb.ants.worldgen",
            json!({"seeds": [1], "preset": "cell", "map": frugal.to_json()})
        )
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
            .unwrap_err()
            .code,
        "NO_SUCH_MAP"
    );
}
