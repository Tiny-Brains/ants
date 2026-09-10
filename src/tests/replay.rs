//! Replays: an action stream re-simulated back into frames.

use super::*;

#[test]
fn a_replay_re_simulates_the_match_it_recorded() {
    // A replay stores the action stream, not frames, and the viewer re-simulates it. Decoding turn
    // N must give exactly the position the referee was in at turn N.
    let w =
        invoke("tb.ants.worldgen", json!({"seeds": [4242], "preset": "standard", "max_turns": 60}))
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
        "preset": "standard",
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
    let w =
        invoke("tb.ants.worldgen", json!({"seeds": [99], "preset": "standard", "max_turns": 40}))
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
fn a_replay_the_platform_actually_wrote_decodes() {
    // A real envelope, taken from object storage after the local stack played it. The other replay
    // tests build their own envelope in-process and would keep passing if Kalam and the engine
    // drifted apart about what a replay is — which has happened, silently, once already.
    let raw = include_str!("../../tests/fixtures/replay-maze-03.json");
    let payload: serde_json::Value = serde_json::from_str(raw).expect("the fixture is JSON");

    // Everything a viewer needs is in the file. No catalogue, no preset table, no second lookup.
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

    // This envelope predates the scoring fix: the engine that wrote it started a match on zero
    // rather than on one point per hill, so every score is one short per hill owned. Ranks still
    // agree exactly and the scores differ by precisely that, which is what makes it a stale fixture
    // rather than a re-simulation bug. Asserted rather than ignored so the offset cannot grow;
    // regenerate the fixture on the current digest and this goes back to a plain equality.
    let stored: Vec<i64> =
        payload["scores"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let resim: Vec<i64> =
        last["frame"]["score"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let shifted: Vec<i64> = stored.iter().map(|s| s + 1).collect();
    assert_eq!(resim, shifted, "one hill each, so one point each, and no other difference");

    // And the range form walks the same match in one pass.
    let ranged =
        invoke("tb.ants.replay-decode", json!({"payload": &payload, "from": 0, "to": turns}))
            .unwrap();
    let frames = ranged["frames"].as_array().unwrap();
    assert_eq!(frames.len(), turns as usize + 1);
    assert_eq!(&frames[0], f0);
    assert_eq!(&frames[turns as usize], &last["frame"]);
}
