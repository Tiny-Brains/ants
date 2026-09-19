//! A recipe's whole set of boards, as the files `maps/` holds — and the reading back of one.
//!
//! Board `i` of a set is drawn from `mix(seed, i + 1)`, attempt `a` from `mix(that, a + 1)`, and the
//! shift cycles through the recipe's list. So a set is a pure function of its recipe: `check`
//! regenerates it and compares bytes, and a board somebody edited by hand, or a generator that
//! drifted, fails there rather than quietly shipping.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::grid::{Rng, Shift, Torus, mix, shifts};
use crate::make::{Board, make};
use crate::measure::{Metrics, measure, rle};
use crate::recipe::Recipe;

pub struct Made {
    pub id: String,
    /// The file, exactly as written.
    pub text: String,
    pub metrics: Metrics,
    pub board: Board,
}

pub fn id(r: &Recipe, index: usize) -> String {
    format!("{}-{index:02}", r.set.name)
}

/// Every board of a recipe's set: what the area generator's own tests and `sweep` draw on.
#[cfg_attr(not(test), allow(dead_code))]
pub fn make_set(r: &Recipe) -> Result<Vec<Made>, String> {
    (0..r.set.count as usize).map(|i| make_one(r, i)).collect()
}

pub fn make_one(r: &Recipe, index: usize) -> Result<Made, String> {
    let t = r.torus();
    let all = shifts(&r.board.shifts, &t, r.seats())?;
    let shift = all[index % all.len()];
    let seed = mix(r.set.seed, index as u64 + 1);
    let id = id(r, index);
    let mut refused: BTreeMap<String, u32> = BTreeMap::new();

    for attempt in 0..r.accept.attempts {
        let mut rng = Rng(mix(seed, attempt as u64 + 1));
        let why = match make(r, shift, &mut rng) {
            Err(e) => e,
            Ok(board) => match measure(&board) {
                // A board that breaks a rule every board obeys is a generator bug, not bad luck.
                Err(e) => return Err(format!("{id}: the generator broke a rule: {e}")),
                Ok(m) => match window(r, &m) {
                    Err(e) => e,
                    Ok(()) => {
                        let v = to_json(&board, &id, r, seed, attempt, &m);
                        engine_accepts(&v)
                            .map_err(|e| format!("{id}: the engine refuses it: {e}"))?;
                        let text = serde_json::to_string(&v).map_err(|e| e.to_string())? + "\n";
                        return Ok(Made { id, text, metrics: m, board });
                    }
                },
            },
        };
        if std::env::var("MAPGEN_DEBUG").is_ok() {
            eprintln!("{id} attempt {attempt}: {why}");
        }
        // Tallied by stage, so the report says which knob is out of reach.
        let stage = why.split(':').next().unwrap_or(&why).to_string();
        *refused.entry(stage).or_default() += 1;
    }
    let mut why: Vec<(u32, String)> = refused.into_iter().map(|(k, v)| (v, k)).collect();
    why.sort_unstable_by(|a, b| b.cmp(a));
    Err(format!(
        "{id}: none of {} attempts passed -- {}",
        r.accept.attempts,
        why.iter().map(|(n, k)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ")
    ))
}

fn window(r: &Recipe, m: &Metrics) -> Result<(), String> {
    let a = &r.accept;
    let inside = |what: &str, v: u32, w: Option<[u32; 2]>, scale: u32| match w {
        Some([lo, hi]) if v < lo * scale || v > hi * scale => Err(format!(
            "{what}: {v} is outside {lo}..={hi}{}",
            if scale > 1 { " (x10)" } else { "" }
        )),
        _ => Ok(()),
    };
    inside("water_pct", m.water_pm, a.water_pct, 10)?;
    inside("routes", m.routes, a.routes, 1)?;
    inside("enemy_walk", m.enemy_walk, a.enemy_walk, 1)?;
    inside("detour_pct", m.detour_pct, a.detour_pct, 1)?;
    inside("dead_end_pm", m.dead_end_pm, a.dead_end_pm, 1)?;
    if let Some(max) = a.water_runs_max
        && m.water_runs > max
    {
        return Err(format!("water_runs: {} is over {max}", m.water_runs));
    }
    Ok(())
}

/// The map file: the engine's own shape (`maps::MapFile::to_json`), plus where it came from.
fn to_json(b: &Board, id: &str, r: &Recipe, seed: u64, attempt: u32, m: &Metrics) -> Value {
    let rc = |x: &usize| {
        let (row, col) = b.t.rc(*x);
        json!([row, col])
    };
    json!({
        "id": id,
        "rows": b.t.rows,
        "cols": b.t.cols,
        "players": b.s.seats,
        "symmetry": { "dr": b.s.dr, "dc": b.s.dc },
        "water": rle(&b.water),
        "hills": b.hills.iter().map(rc).collect::<Vec<_>>(),
        "food": b.food.iter().map(rc).collect::<Vec<_>>(),
        "food_target": b.food.len(),
        "generator": { "recipe": r.set.name, "seed": seed, "attempt": attempt, "metrics": m },
    })
}

/// The engine's own gate, `tb.ants.worldgen`, on the board inline — the validation a match runs.
pub fn engine_accepts(map: &Value) -> Result<(), String> {
    tb_ants::invoke("tb.ants.worldgen", json!({ "seeds": [1], "map": map }))
        .map(|_| ())
        .map_err(|f| format!("{} {}", f.code, f.message))
}

/// A map file read back into squares, whoever wrote it.
pub fn from_json(v: &Value) -> Result<Board, String> {
    let num = |k: &str| v.get(k).and_then(Value::as_i64).ok_or(format!("no numeric '{k}'"));
    let t = Torus { rows: num("rows")? as i32, cols: num("cols")? as i32 };
    let seats = num("players")? as usize;
    let sym = v.get("symmetry").ok_or("no 'symmetry'")?;
    let axis = |k: &str| sym.get(k).and_then(Value::as_i64).ok_or(format!("no symmetry '{k}'"));
    let s = Shift { seats, dr: axis("dr")? as i32, dc: axis("dc")? as i32 };
    let mut water = Vec::with_capacity(t.cells());
    let runs = v.get("water").and_then(Value::as_array).ok_or("no 'water'")?;
    for pair in runs.chunks(2) {
        let (on, n) =
            (pair[0].as_u64() == Some(1), pair.get(1).and_then(Value::as_u64).unwrap_or(0));
        water.extend(std::iter::repeat_n(on, n as usize));
    }
    if water.len() != t.cells() {
        return Err("'water' does not cover the board".into());
    }
    let squares = |k: &str| -> Result<Vec<usize>, String> {
        v.get(k)
            .and_then(Value::as_array)
            .ok_or(format!("no '{k}'"))?
            .iter()
            .map(|p| match (p[0].as_i64(), p[1].as_i64()) {
                (Some(r), Some(c)) => Ok(t.at(r as i32, c as i32)),
                _ => Err(format!("'{k}' holds something that is not [row, col]")),
            })
            .collect()
    };
    Ok(Board { t, s, water, hills: squares("hills")?, food: squares("food")? })
}
