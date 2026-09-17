//! A board played, not just measured: the same simple policy in every seat, over several seeds, to
//! see whether any seat does systematically worse.
//!
//! Measurement proves the board is congruent for every seat; this is the smoke alarm for what
//! measurement cannot see — a rule in the engine that treats seats differently, or a board that is
//! fair on paper and unplayable in practice (a colony that never grows past its first ant). It is
//! slow, so `check` runs it only when asked.

use serde_json::{Value, json};

use crate::grid::{N4, Rng};

const DIRS: [&str; 4] = ["N", "E", "S", "W"];

pub struct Seat {
    pub score: i64,
    pub firsts: u32,
    /// The largest colony the seat reached, summed over the matches.
    pub peak: u64,
}

pub fn play(map: &Value, seeds: u32, turns: u32) -> Result<Vec<Seat>, String> {
    let fault = |f: tb_ants::Fault| format!("{} {}", f.code, f.message);
    let seats = map["players"].as_u64().unwrap_or(2) as usize;
    let seeds: Vec<u64> = (1..=seeds as u64).collect();
    let w = tb_ants::invoke(
        "tb.ants.worldgen",
        json!({ "seeds": seeds, "map": map, "max_turns": turns }),
    )
    .map_err(fault)?;
    let mut state = w["wave_state"].as_str().unwrap_or_default().to_string();
    let mut out: Vec<Seat> = (0..seats).map(|_| Seat { score: 0, firsts: 0, peak: 0 }).collect();
    let mut peaks = vec![vec![0u64; seats]; seeds.len()];
    let mut rng = Rng(0x9A7E);
    loop {
        let views =
            tb_ants::invoke("tb.ants.observe", json!({ "wave_state": state })).map_err(fault)?;
        let views = views["views"].as_array().cloned().unwrap_or_default();
        if views.is_empty() {
            break;
        }
        let mut actions = Vec::with_capacity(views.len());
        for v in &views {
            let (m, seat) =
                (v["m"].as_u64().unwrap_or(0) as usize, v["seat"].as_u64().unwrap_or(0) as usize);
            let mine = v["view"]["mine"].as_array().map_or(0, Vec::len) as u64;
            peaks[m][seat] = peaks[m][seat].max(mine);
            actions.push(greedy(&v["view"], &mut rng));
        }
        let next =
            tb_ants::invoke("tb.ants.step", json!({ "wave_state": state, "actions": actions }))
                .map_err(fault)?;
        state = next["wave_state"].as_str().unwrap_or_default().to_string();
    }
    let fin = tb_ants::invoke("tb.ants.finish", json!({ "wave_state": state })).map_err(fault)?;
    for (m, r) in fin["results"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        for k in 0..seats {
            out[k].score += r["scores"][k].as_i64().unwrap_or(0);
            out[k].firsts += (r["ranks"][k].as_u64() == Some(1)) as u32;
            out[k].peak += peaks[m][k];
        }
    }
    Ok(out)
}

/// Each ant steps toward the nearest visible food no other ant has claimed, over known water, and
/// wanders when there is none — the reference observations' walker, from outside the engine.
fn greedy(view: &Value, rng: &mut Rng) -> Value {
    let (rows, cols) = (
        view["size"][0].as_i64().unwrap_or(1) as i32,
        view["size"][1].as_i64().unwrap_or(1) as i32,
    );
    let at = |r: i32, c: i32| (r.rem_euclid(rows) * cols + c.rem_euclid(cols)) as usize;
    let mut water = Vec::with_capacity((rows * cols) as usize);
    for pair in view["water"]["rle"].as_array().cloned().unwrap_or_default().chunks(2) {
        let n = pair.get(1).and_then(Value::as_u64).unwrap_or(0) as usize;
        water.extend(std::iter::repeat_n(pair[0].as_u64() == Some(1), n));
    }
    let pos = |v: &Value| (v[0].as_i64().unwrap_or(0) as i32, v[1].as_i64().unwrap_or(0) as i32);
    let d2 = |a: (i32, i32), b: (i32, i32)| {
        let dr = (a.0 - b.0).abs().min(rows - (a.0 - b.0).abs());
        let dc = (a.1 - b.1).abs().min(cols - (a.1 - b.1).abs());
        dr * dr + dc * dc
    };
    let food: Vec<(i32, i32)> =
        view["food"].as_array().cloned().unwrap_or_default().iter().map(pos).collect();
    let mut claimed = vec![false; food.len()];
    let mut orders = Vec::new();
    for a in view["mine"].as_array().cloned().unwrap_or_default().iter().map(pos) {
        let target = (0..food.len()).filter(|&i| !claimed[i]).min_by_key(|&i| d2(a, food[i]));
        let dry: Vec<usize> = (0..4)
            .filter(|&d| !water.get(at(a.0 + N4[d].0, a.1 + N4[d].1)).copied().unwrap_or(false))
            .collect();
        let order = match target {
            Some(i) if !dry.is_empty() => {
                claimed[i] = true;
                let best = dry
                    .iter()
                    .copied()
                    .min_by_key(|&d| d2((a.0 + N4[d].0, a.1 + N4[d].1), food[i]));
                DIRS[best.unwrap_or(0)]
            }
            _ if !dry.is_empty() => DIRS[dry[rng.below(dry.len() as u64) as usize]],
            _ => "-",
        };
        orders.push(order);
    }
    json!(orders)
}
