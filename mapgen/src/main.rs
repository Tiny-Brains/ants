//! `mapgen` — the board factory for `maps/`.
//!
//!     cargo run -- generate                       every recipe under recipes/, into ../maps/
//!     cargo run -- generate recipes/maze-2.toml   one preset
//!     cargo run -- check                          the committed boards are what the recipes make
//!     cargo run -- check --play 8                 ...and play each one, 8 seeds, every seat alike
//!     cargo run -- show ../maps/maze-2-00.json    draw a board and its numbers
//!
//! `generate` replaces a preset's boards whole: a board left over from a larger `count` would stay
//! in the catalogue and be played. `check` is the gate `build.sh` runs; its failures name the board
//! and the rule.

mod grid;
mod make;
mod measure;
mod play;
mod recipe;
mod set;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use recipe::Recipe;
use serde_json::Value;

fn here(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let maps = flag("--maps").map(PathBuf::from).unwrap_or_else(|| here("../maps"));
    let recipes = flag("--recipes").map(PathBuf::from).unwrap_or_else(|| here("recipes"));
    let named: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(i, a)| !a.starts_with("--") && !args[i - 1].starts_with("--"))
        .map(|(_, a)| PathBuf::from(a))
        .collect();

    let result = match args.first().map(String::as_str) {
        Some("generate") => generate(&named, &recipes, &maps),
        Some("check") => check(&recipes, &maps, flag("--play"), flag("--turns")),
        Some("show") => show(&named),
        _ => Err("usage: mapgen generate [RECIPE...] | check [--play SEEDS] | show MAP...".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mapgen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn load_recipes(dir: &Path) -> Result<Vec<Recipe>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    let recipes = paths.iter().map(|p| Recipe::load(p)).collect::<Result<Vec<_>, _>>()?;
    for (i, r) in recipes.iter().enumerate() {
        if recipes[..i].iter().any(|o| o.preset.name == r.preset.name) {
            return Err(format!("two recipes make preset '{}'", r.preset.name));
        }
    }
    Ok(recipes)
}

fn map_files(dir: &Path) -> Result<Vec<(PathBuf, Value)>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let v = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok((p, v))
        })
        .collect()
}

fn header() {
    println!(
        "  {:<14} {:>7} {:>9} {:>6} {:>6} {:>5} {:>7} {:>8} {:>6} {:>5}",
        "board", "size", "shift", "water", "routes", "walk", "detour", "deadend", "runs", "food"
    );
}

fn row(id: &str, b: &make::Board, m: &measure::Metrics) {
    println!(
        "  {:<14} {:>7} {:>9} {:>5}% {:>6} {:>5} {:>6}% {:>7}‰ {:>6} {:>5}",
        id,
        format!("{}x{}", b.t.rows, b.t.cols),
        format!("({},{})", b.s.dr, b.s.dc),
        format!("{}.{}", m.water_pm / 10, m.water_pm % 10),
        if m.routes >= measure::ROUTES_CAP {
            format!("{}+", m.routes)
        } else {
            m.routes.to_string()
        },
        m.enemy_walk,
        m.detour_pct,
        m.dead_end_pm,
        m.water_runs,
        m.food_per_seat,
    );
}

fn generate(named: &[PathBuf], recipes_dir: &Path, maps: &Path) -> Result<(), String> {
    let recipes = if named.is_empty() {
        load_recipes(recipes_dir)?
    } else {
        named.iter().map(|p| Recipe::load(p)).collect::<Result<Vec<_>, _>>()?
    };
    for r in &recipes {
        let made = set::make_set(r)?;
        for (path, v) in map_files(maps)? {
            if v.get("preset").and_then(Value::as_str) == Some(r.preset.name.as_str()) {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        println!("{} -- {} seats, {} boards", r.preset.name, r.preset.seats, made.len());
        header();
        for m in &made {
            let path = maps.join(format!("{}.json", m.id));
            std::fs::write(&path, &m.text).map_err(|e| format!("{}: {e}", path.display()))?;
            row(&m.id, &m.board, &m.metrics);
        }
    }
    Ok(())
}

fn check(
    recipes_dir: &Path,
    maps: &Path,
    play_seeds: Option<String>,
    turns: Option<String>,
) -> Result<(), String> {
    let recipes = load_recipes(recipes_dir)?;
    let files = map_files(maps)?;
    let mut problems = Vec::new();

    // Every board on disk belongs to a recipe, is accepted by the engine, and obeys the rules.
    for (path, v) in &files {
        let id = v.get("id").and_then(Value::as_str).unwrap_or("?");
        let preset = v.get("preset").and_then(Value::as_str).unwrap_or("?");
        if !recipes.iter().any(|r| r.preset.name == preset) {
            problems.push(format!("{}: preset '{preset}' has no recipe", path.display()));
        }
        if let Err(e) = set::engine_accepts(v) {
            problems.push(format!("{id}: the engine refuses it: {e}"));
        }
        if let Err(e) = set::from_json(v).and_then(|b| measure::measure(&b)) {
            problems.push(format!("{id}: {e}"));
        }
    }

    // Every recipe's set, regenerated, is byte for byte what is committed, and nothing more is.
    for r in &recipes {
        match set::make_set(r) {
            Err(e) => problems.push(e),
            Ok(made) => {
                println!("{} -- {} seats, {} boards", r.preset.name, r.preset.seats, made.len());
                header();
                for m in &made {
                    row(&m.id, &m.board, &m.metrics);
                    let path = maps.join(format!("{}.json", m.id));
                    match std::fs::read_to_string(&path) {
                        Ok(text) if text == m.text => {}
                        Ok(_) => problems.push(format!(
                            "{}: not what recipe '{}' makes -- edited by hand, or the generator \
                             changed; run `mapgen generate`",
                            path.display(),
                            r.preset.name
                        )),
                        Err(_) => problems.push(format!("{}: missing", path.display())),
                    }
                }
                let extra = files.iter().filter(|(_, v)| {
                    v.get("preset").and_then(Value::as_str) == Some(r.preset.name.as_str())
                        && !made
                            .iter()
                            .any(|m| Some(m.id.as_str()) == v.get("id").and_then(Value::as_str))
                });
                for (path, _) in extra {
                    problems.push(format!(
                        "{}: recipe '{}' no longer makes it",
                        path.display(),
                        r.preset.name
                    ));
                }
            }
        }
    }

    if let Some(n) = play_seeds {
        let seeds: u32 = n.parse().map_err(|_| format!("--play {n}: a number of seeds"))?;
        let turns: u32 =
            turns.as_deref().unwrap_or("300").parse().map_err(|_| "--turns: a number")?;
        println!("\nplayed: {seeds} seeds, {turns} turns, the greedy walker in every seat");
        for (_, v) in &files {
            let id = v.get("id").and_then(Value::as_str).unwrap_or("?");
            let seats = play::play(v, seeds, turns)?;
            let peaks: Vec<u64> = seats.iter().map(|s| s.peak).collect();
            let (lo, hi) = (*peaks.iter().min().unwrap_or(&0), *peaks.iter().max().unwrap_or(&0));
            println!(
                "  {:<14} peak colony {:?}  score {:?}  first {:?}",
                id,
                peaks.iter().map(|p| p / seeds as u64).collect::<Vec<_>>(),
                seats.iter().map(|s| s.score).collect::<Vec<_>>(),
                seats.iter().map(|s| s.firsts).collect::<Vec<_>>(),
            );
            // A smoke alarm, not a statistic: one seat's colonies at under half another's, by a
            // margin no run of luck over these seeds explains.
            if lo * 2 < hi && hi - lo >= 5 * seeds as u64 {
                problems
                    .push(format!("{id}: seats grow unevenly under the same policy: {peaks:?}"));
            }
            if hi <= seeds as u64 {
                problems.push(format!("{id}: no colony grew past its first ant in any seed"));
            }
        }
    }

    if problems.is_empty() {
        println!(
            "\ncheck: {} boards from {} recipes, all reproduced and valid",
            files.len(),
            recipes.len()
        );
        Ok(())
    } else {
        Err(format!("{} problem(s):\n  {}", problems.len(), problems.join("\n  ")))
    }
}

fn show(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let b = set::from_json(&v)?;
        let m = measure::measure(&b)?;
        let id = v.get("id").and_then(Value::as_str).unwrap_or("?");
        header();
        row(id, &b, &m);
        println!("{}", draw(&b));
    }
    Ok(())
}

/// The board as text: `#` water, `.` land, a digit for a seat's hill, `*` food.
pub fn draw(b: &make::Board) -> String {
    let mut out = String::new();
    for r in 0..b.t.rows {
        for c in 0..b.t.cols {
            let x = b.t.at(r, c);
            let ch = if let Some(i) = b.hills.iter().position(|&h| h == x) {
                char::from_digit((i % b.s.seats) as u32, 10).unwrap_or('H')
            } else if b.food.contains(&x) {
                '*'
            } else if b.water[x] {
                '#'
            } else {
                '.'
            };
            out.push(ch);
        }
        out.push('\n');
    }
    out
}
