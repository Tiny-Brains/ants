//! The wave contract: what each exported function accepts, returns and refuses.

use super::*;
use crate::turn::step;

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
    // docs/cartridge.md §2. Tested after a turn, not on a fresh state: a fresh state exercises none of
    // the fields that matter -- scores, the hive, razed hills, the stalemate counters.
    let w = invoke("tb.ants.worldgen", json!({"seeds": [11, 12], "preset": "standard"})).unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(4);
    for _ in 0..25 {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        let acts = random_actions(&views, &mut rng);
        state = invoke(
            "tb.ants.step",
            json!({"wave_state": &state, "actions": acts}),
        )
        .unwrap()["wave_state"]
            .as_str()
            .unwrap()
            .to_string();
    }
    let again = pack(&unpack(&state).unwrap());
    assert_eq!(state, again, "any information not encoded is information the match does not have");
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
    let out = invoke("tb.ants.step", json!({"wave_state": w["wave_state"], "actions": short}));
    assert!(out.is_ok(), "a short action array is every remaining ant holding, not a fault");
}

#[test]
fn observe_says_nothing_about_a_finished_match() {
    // docs/cartridge.md §1: there is no terminal message. A model receives states while its match runs
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

    let positional: Vec<Value> =
        vs.iter().map(|v| json!(vec!["N"; v["view"]["mine"].as_array().unwrap().len()])).collect();
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
    assert_eq!(invoke("tb.ants.worldgen", json!({"seeds": []})).unwrap_err().code, "NO_SEEDS");
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
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [31, 32, 33, 34], "preset": "standard", "max_turns": 300}),
    )
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
        state = invoke(
            "tb.ants.step",
            json!({"wave_state": &state, "actions": acts}),
        )
        .unwrap()["wave_state"]
            .as_str()
            .unwrap()
            .to_string();
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
