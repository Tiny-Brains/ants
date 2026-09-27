//! Replays: an action stream re-simulated back into frames.

use super::*;

#[test]
fn a_replay_re_simulates_the_match_it_recorded() {
    // A replay stores the action stream, not frames, and the viewer re-simulates it. Decoding turn
    // N must give exactly the position the referee was in at turn N.
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [4242], "map": boards::json(boards::DUEL), "max_turns": 60}),
    )
    .unwrap();
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
        let out = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap();
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
        "max_turns": 60,
        "map_id": fin["results"][0]["map_id"],
        "map": fin["results"][0]["map"],
        "deltas": deltas,
    });
    let _ = state0;
    for (turn, mine0, mine1, score) in &live_positions {
        let f = invoke("tb.ants.replay-decode", json!({"payload": payload, "turn": turn})).unwrap();
        let frame = &f["frame"];
        assert_eq!(frame["turn"].as_u64().unwrap() as u16, *turn);
        let ants: Vec<(u64, u64, u64)> = frame["ants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a[0].as_u64().unwrap(), a[1].as_u64().unwrap(), a[2].as_u64().unwrap()))
            .collect();
        let cols = frame["size"][1].as_u64().unwrap();
        let of = |seat: u64| {
            let mut v: Vec<u64> =
                ants.iter().filter(|a| a.2 == seat).map(|a| a.0 * cols + a.1).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(
            of(0),
            mine0.iter().map(|&p| p as u64).collect::<Vec<_>>(),
            "player 0 at turn {turn}"
        );
        assert_eq!(
            of(1),
            mine1.iter().map(|&p| p as u64).collect::<Vec<_>>(),
            "player 1 at turn {turn}"
        );
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
            .unwrap_err()
            .code,
        "BAD_REPLAY"
    );
}

#[test]
fn a_frame_range_agrees_with_the_frames_asked_for_one_at_a_time() {
    // The scrubber's fix. `decode` re-simulates from turn zero, so a viewer walking a timeline
    // frame by frame is quadratic; `decode_range` walks the match once. It is only worth having
    // if it produces exactly the same frames.
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [99], "map": boards::json(boards::DUEL), "max_turns": 40}),
    )
    .unwrap();
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

    let ranged =
        invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0, "to": 12})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 13, "from 0 to 12 inclusive is thirteen frames");
    for (t, got) in frames.iter().enumerate() {
        let one = invoke("tb.ants.replay-decode", json!({"payload": &payload, "turn": t})).unwrap();
        assert_eq!(got, &one["frame"], "frame {t} differs between the range and the single call");
    }
}

#[test]
fn a_frames_discoveries_add_up_to_exactly_what_its_seat_knows() {
    // The viewer draws each seat's explored territory by folding every frame's `discovered` from
    // turn zero. That is the engine's own memory -- the `known` mask observations are built from --
    // only if the fold equals it at every turn the referee played, and nothing is announced twice.
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [31], "map": boards::json(boards::DUEL), "max_turns": 80}),
    )
    .unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut known = vec![unpack(&state).unwrap().matches[0].known.clone()];
    let mut rng = Rng(11);
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
        known.push(unpack(&state).unwrap().matches[0].known.clone());
    }
    let fin = invoke("tb.ants.finish", json!({"wave_state": &state})).unwrap();
    let payload = json!({
        "seed": 31, "max_turns": 80,
        "map": fin["results"][0]["map"], "deltas": deltas,
    });

    let ranged = invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), known.len(), "a frame for every turn the referee played");
    let cols = frames[0]["size"][1].as_u64().unwrap() as usize;
    let mut fold: Vec<Bits> = known[0].iter().map(|k| Bits::zeros(k.len)).collect();
    let mut explored = false;
    for (t, frame) in frames.iter().enumerate() {
        for (seat, cells) in frame["discovered"].as_array().unwrap().iter().enumerate() {
            for rc in cells.as_array().unwrap() {
                let i = rc[0].as_u64().unwrap() as usize * cols + rc[1].as_u64().unwrap() as usize;
                assert!(!fold[seat].get(i), "turn {t} announced a square seat {seat} already knew");
                fold[seat].set(i);
                explored |= t > 0;
            }
        }
        assert_eq!(fold, known[t], "the fold is not what the seats knew at turn {t}");
    }
    assert!(explored, "the ants must have seen past where they started");
}

#[test]
fn a_replay_the_platform_actually_wrote_decodes() {
    // A real envelope, taken from object storage after the local stack played it. The other replay
    // tests build their own envelope in-process and would keep passing if Kalam and the engine
    // drifted apart about what a replay is — which has happened, silently, once already.
    //
    // Captured from a `kalam-match` run after the Orion 1.8.1 rebuild: `maze-03`, 256 turns,
    // `rank_stabilized` 3–0. So it also pins the post-rebuild envelope shape — `orion_version`
    // where `evaluator_digest` and `dialect_version` used to be.
    let raw = include_str!("fixtures/replay-maze-03.json");
    let payload: serde_json::Value = serde_json::from_str(raw).expect("the fixture is JSON");

    // Everything a viewer needs is in the file. No catalogue, no season, no second lookup.
    assert_eq!(payload["map_id"], "maze-03");
    assert!(payload["map"].is_object(), "the envelope carries its board");
    assert!(payload["seed"].is_u64(), "and the seed that drove food respawn");
    assert!(payload["max_turns"].is_u64(), "and the turn limit it was played under");

    let turns = payload["turns"].as_u64().unwrap() as u16;
    let first = invoke("tb.ants.replay-decode", json!({"payload": &payload, "turn": 0})).unwrap();
    let f0 = &first["frame"];
    assert_eq!(f0["turn"], 0);
    assert_eq!(f0["size"], json!([96, 96]));
    assert_eq!(f0["ants"].as_array().unwrap().len(), 2, "one ant each at turn zero");
    assert_eq!(f0["hills"].as_array().unwrap().len(), 2, "and one hill each");

    // The whole match, and the end it recorded.
    let last =
        invoke("tb.ants.replay-decode", json!({"payload": &payload, "turn": turns})).unwrap();
    assert_eq!(last["frame"]["turn"].as_u64().unwrap() as u16, turns);
    assert_eq!(last["frame"]["ranks"], payload["engine_ranks"], "the verdict re-simulates");

    // A plain equality again. The fixture this replaced was written by an engine that opened a
    // match on zero rather than on one point per hill, so it had to assert that offset instead;
    // this one was played on the digest the cartridge currently ships, so the scores re-simulate
    // exactly. If this ever needs an offset again, the fixture is stale -- re-capture it rather
    // than widening the assertion.
    assert_eq!(last["frame"]["score"], payload["scores"], "and so do the scores");

    // And the range form walks the same match in one pass.
    let ranged =
        invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0, "to": turns}))
            .unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), turns as usize + 1);
    assert_eq!(&frames[0], f0);
    assert_eq!(&frames[turns as usize], &last["frame"]);
}

#[test]
fn a_range_over_a_recording_that_stops_early_ends_where_the_recording_does() {
    // A match cut off after five turns, asked for everything from turn zero: six frames, not one for
    // every turn a u16 can name.
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [7], "map": boards::json(boards::DUEL), "max_turns": 1000}),
    )
    .unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(5);
    let mut deltas = Vec::new();
    for _ in 0..5 {
        let views = invoke("tb.ants.observe", json!({"wave_state": &state})).unwrap();
        let acts = random_actions(&views, &mut rng);
        let out = invoke("tb.ants.step", json!({"wave_state": &state, "actions": acts})).unwrap();
        deltas.extend(out["replay_delta"].as_array().unwrap().iter().cloned());
        state = out["wave_state"].as_str().unwrap().to_string();
    }
    let map = crate::maps::MapFile::from_match(&unpack(&state).unwrap().matches[0], "cut");
    let payload = json!({"seed": 7, "max_turns": 1000, "map": map.to_json(), "deltas": deltas});
    let ranged = invoke("tb.ants.replay-decode", json!({"payload": payload, "from": 0})).unwrap();
    assert_eq!(ranged["frames"].as_array().unwrap().len(), 6, "turns 0 to 5");
    assert_eq!(ranged["to"], 5);
}

/// The book's fight lesson, as an envelope: an eight-by-twelve board, hills at (1,1) and (5,7),
/// and two ants that walk into each other's range on turn 4.
fn fight_envelope() -> Value {
    json!({
        "seed": 1, "max_turns": 14, "map_id": "lesson-fight",
        "map": {
            "id": "lesson-fight", "rows": 8, "cols": 12, "players": 2, "water": [0, 96],
            "hills": [[1, 1], [5, 7]], "food": [], "food_target": 0,
            "symmetry": { "dr": 4, "dc": 6 }
        },
        "deltas": [
            { "t": 0, "a": ["S", "N"] }, { "t": 1, "a": ["S", "N"] },
            { "t": 2, "a": ["E", "W"] }, { "t": 3, "a": ["E", "W"] }
        ]
    })
}

#[test]
fn a_frame_carries_each_ants_id_and_the_deaths_of_its_turn() {
    // Frame N is the board after N turns, and it reports what turn N did: the ants that died, on
    // the square they died on, and who killed them. Both are gone from `ants`; the ids say which
    // ants they were in the frame before.
    let ranged =
        invoke("tb.ants.replay-decode", json!({"payload": fight_envelope(), "from": 0, "to": 4}))
            .unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 5);

    assert_eq!(frames[0]["ants"], json!([[1, 1, 0, 0], [5, 7, 1, 0]]), "[r, c, owner, id]");
    assert_eq!(frames[0]["deaths"], json!([]), "nothing has happened at turn zero");
    assert_eq!(frames[0]["razed"], json!([]));
    for f in &frames[1..4] {
        assert_eq!(f["deaths"], json!([]), "turn {} killed nobody", f["turn"]);
        assert_eq!(f["ants"].as_array().unwrap().len(), 2);
    }
    assert_eq!(frames[3]["ants"], json!([[3, 2, 0, 0], [3, 6, 1, 0]]), "four columns apart");

    let last = &frames[4];
    assert_eq!(last["ants"], json!([]), "both died on turn 4");
    assert_eq!(
        last["deaths"],
        json!([
            { "ant": [3, 3, 0, 0], "by": [[3, 5, 1, 0]] },
            { "ant": [3, 5, 1, 0], "by": [[3, 3, 0, 0]] }
        ]),
        "each on the square it died on, killed by the other"
    );
    assert_eq!(last["done"], true);
    assert_eq!(last["ranks"], json!([1, 1]));

    // A single frame says the same as the range.
    let one =
        invoke("tb.ants.replay-decode", json!({"payload": fight_envelope(), "turn": 4})).unwrap();
    assert_eq!(&one["frame"], last);
}

#[test]
fn a_frame_reports_the_hill_that_fell_on_its_turn_and_who_took_it() {
    // The book's raze lesson: blue walks four north and holds; red walks four south and six east
    // and stands on the empty hill on turn 10.
    let payload = json!({
        "seed": 1, "max_turns": 14, "map_id": "lesson-raze",
        "map": {
            "id": "lesson-raze", "rows": 8, "cols": 12, "players": 2, "water": [0, 96],
            "hills": [[1, 1], [5, 7]], "food": [], "food_target": 0,
            "symmetry": { "dr": 4, "dc": 6 }
        },
        "deltas": (0..10).map(|t| json!({
            "t": t,
            "a": [if t < 4 { "S" } else { "E" }, if t < 4 { "N" } else { "-" }]
        })).collect::<Vec<_>>()
    });
    let ranged =
        invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0, "to": 10})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 11);
    for f in &frames[..10] {
        assert_eq!(f["razed"], json!([]), "no hill fell before turn 10");
        assert_eq!(f["hills"].as_array().unwrap().len(), 2);
    }
    let last = &frames[10];
    assert_eq!(last["razed"], json!([[5, 7, 1, 0]]), "[r, c, owner, by]");
    assert_eq!(last["hills"], json!([[1, 1, 0]]), "and the fallen hill is gone from the list");
    assert_eq!(last["score"], json!([3, 0]));
    assert_eq!(last["deaths"], json!([]));
}

#[test]
fn every_ant_that_leaves_the_frames_is_in_the_deaths_of_the_turn_it_left_on() {
    // Under random play on a real board: an ant in one frame and gone from the next is a death that
    // frame reports, its killers were all standing in the frame before, and a hill gone from the
    // list is a razing. The record and the position cannot disagree.
    let w = invoke(
        "tb.ants.worldgen",
        json!({"seeds": [17], "map": boards::json(boards::DUEL), "max_turns": 200}),
    )
    .unwrap();
    let mut state = w["wave_state"].as_str().unwrap().to_string();
    let mut rng = Rng(3);
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
        "seed": 17, "max_turns": 200,
        "map": fin["results"][0]["map"], "deltas": deltas,
    });
    let ranged = invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0})).unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert!(frames.len() > 2);

    let key = |a: &Value| (a[2].as_u64().unwrap(), a[3].as_u64().unwrap());
    let mut died = 0;
    for t in 1..frames.len() {
        let (was, now) = (&frames[t - 1], &frames[t]);
        let before: Vec<(u64, u64)> = was["ants"].as_array().unwrap().iter().map(key).collect();
        let after: Vec<(u64, u64)> = now["ants"].as_array().unwrap().iter().map(key).collect();
        let deaths: Vec<(u64, u64)> =
            now["deaths"].as_array().unwrap().iter().map(|d| key(&d["ant"])).collect();
        for a in &before {
            if !after.contains(a) {
                assert!(deaths.contains(a), "turn {t}: ant {a:?} left with no death reported");
                died += 1;
            }
        }
        for d in now["deaths"].as_array().unwrap() {
            let a = key(&d["ant"]);
            assert!(
                before.contains(&a),
                "turn {t}: {a:?} died without having stood in the frame before"
            );
            assert!(!after.contains(&a), "turn {t}: {a:?} died and is still drawn");
            for k in d["by"].as_array().unwrap() {
                let k = key(k);
                assert!(
                    before.contains(&k),
                    "turn {t}: killer {k:?} was not standing the turn before"
                );
                assert_ne!(k.0, a.0, "turn {t}: a killer is never a friend");
            }
        }
        let hills_was = was["hills"].as_array().unwrap().len();
        let hills_now = now["hills"].as_array().unwrap().len();
        assert_eq!(
            hills_was - hills_now,
            now["razed"].as_array().unwrap().len(),
            "turn {t}: a hill left the list without a razing, or the other way round"
        );
    }
    assert!(died > 0, "random play on the duel board must kill something in two hundred turns");
}
