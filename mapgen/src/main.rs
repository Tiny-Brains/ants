//! `mapgen` — the board factory: the basic boards in `../maps/`, and a season's wherever it is told.
//!
//!     cargo run -- generate                              every recipe under recipes/, into ../maps/
//!     cargo run -- generate recipes/basic-tiny-2p.toml one board
//!     cargo run -- check                                 the committed boards are what the recipes make
//!     cargo run -- check --play 8                        ...and play each one, 8 seeds, every seat alike
//!     cargo run -- show ../maps/basic-tiny-2p.json     draw a board and its numbers
//!     cargo run --release -- explore --slots S --out D   hundreds of designs a slot, to choose from
//!     cargo run --release -- playtest D PICKS            play the chosen candidates, every seat alike
//!     cargo run -- adopt D PICKS                         write the chosen candidates as recipes
//!     cargo run --release -- sweep                       every area style, 2 to 8 seats, played
//!
//! `--recipes DIR` and `--maps DIR` point `generate`, `check` and `adopt` somewhere else. **A
//! season's boards are made that way, outside every repository**: they reach the platform by
//! an admin's upload and are pushed to a backup only once their season has closed. This repository
//! holds the five basic boards and nothing else -- the envelope admission is built on.
//! **A recipe is a design** (`design.rs`): the board, its shift and point group, and every shape
//! drawn on it, written out as data. `explore` proposes them, a person picks, `adopt` writes the
//! picks down, and `generate` renders them -- so a board somebody chose by eye stays the board it
//! was, whatever later happens to the sampler. One board a recipe, named as its file is.
//!
//! `generate` with no arguments makes `maps/` exactly the recipes' boards: a board no recipe makes
//! would stay in the catalogue and be played. `check` is the gate `build.sh` runs; its failures name
//! the board and the rule.

mod design;
mod explore;
mod grid;
mod make;
mod measure;
mod play;
mod recipe;
mod sample;
mod set;
mod sweep;
mod sym;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use design::Design;
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
        Some("sweep") => sweep::run(&args[1..]),
        Some("explore") => explore::run(&args[1..]),
        Some("playtest") => explore::playtest(&args[1..]),
        Some("adopt") => adopt(&args[1..], &recipes),
        _ => Err(
            "usage: mapgen generate [RECIPE...] | check [--play SEEDS] | show MAP... | explore | playtest | adopt | sweep"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mapgen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn load_designs(dir: &Path) -> Result<Vec<Design>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    paths.iter().map(|p| Design::load(p)).collect()
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
    let all = named.is_empty();
    let designs = if all {
        load_designs(recipes_dir)?
    } else {
        named.iter().map(|p| Design::load(p)).collect::<Result<Vec<_>, _>>()?
    };
    let mut written = Vec::new();
    header();
    for d in &designs {
        let (r, m, v) = design::build(d).map_err(|e| format!("{}: {e}", d.name))?;
        let path = maps.join(format!("{}.json", d.name));
        std::fs::write(&path, design::file_text(&v))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        row(&d.name, &r.board, &m);
        written.push(path);
    }
    if all {
        for (path, _) in map_files(maps)? {
            if !written.contains(&path) {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("  removed {} -- no recipe makes it", path.display());
            }
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
    let designs = load_designs(recipes_dir)?;
    let files = map_files(maps)?;
    let mut problems = Vec::new();

    // Every board on disk belongs to a recipe, is accepted by the engine, and obeys the rules.
    for (path, v) in &files {
        let id = v.get("id").and_then(Value::as_str).unwrap_or("?");
        if !designs.iter().any(|d| d.name == id) {
            problems.push(format!("{}: no recipe makes '{id}'", path.display()));
        }
        if let Err(e) = set::engine_accepts(v) {
            problems.push(format!("{id}: the engine refuses it: {e}"));
        }
        if let Err(e) = set::from_json(v).and_then(|b| measure::measure(&b)) {
            problems.push(format!("{id}: {e}"));
        }
    }

    // Every recipe's board, rendered again, is byte for byte what is committed.
    header();
    for d in &designs {
        match design::build(d) {
            Err(e) => problems.push(format!("{}: {e}", d.name)),
            Ok((r, m, v)) => {
                row(&d.name, &r.board, &m);
                let path = maps.join(format!("{}.json", d.name));
                match std::fs::read_to_string(&path) {
                    Ok(text) if text == design::file_text(&v) => {}
                    Ok(_) => problems.push(format!(
                        "{}: not what recipe '{}' makes -- edited by hand, or the renderer \
                         changed; run `mapgen generate`",
                        path.display(),
                        d.name
                    )),
                    Err(_) => problems.push(format!("{}: missing", path.display())),
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
                "  {:<22} peak colony {:?}  score {:?}  first {:?}",
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
            designs.len()
        );
        Ok(())
    } else {
        Err(format!("{} problem(s):\n  {}", problems.len(), problems.join("\n  ")))
    }
}

/// `mapgen adopt RUN_DIR PICKS`: each `<slot> <candidate> [name]` line of PICKS, found in the explore
/// run's `<slot>.jsonl`, written as `recipes/<name>.toml` -- the design exactly as it was drawn, under
/// the slot's name unless the line gives another.
fn adopt(args: &[String], recipes_dir: &Path) -> Result<(), String> {
    let dir = PathBuf::from(args.first().ok_or("adopt RUN_DIR PICKS")?);
    let picks = args.get(1).ok_or("adopt RUN_DIR PICKS")?;
    let text = std::fs::read_to_string(picks).map_err(|e| format!("{picks}: {e}"))?;
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let mut it = line.split_whitespace();
        let (slot, i) = (it.next().unwrap_or_default(), it.next().unwrap_or_default());
        let i: u64 = i.parse().map_err(|_| format!("{line}: `<slot> <candidate> [name]`"))?;
        let name = it.next().unwrap_or(slot);
        let run = dir.join(format!("{slot}.jsonl"));
        let found = std::fs::read_to_string(&run)
            .map_err(|e| format!("{}: {e}", run.display()))?
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["i"].as_u64() == Some(i))
            .ok_or(format!("{slot}: no candidate {i} in {}", run.display()))?;
        let mut d: Design =
            serde_json::from_value(found["design"].clone()).map_err(|e| e.to_string())?;
        d.name = name.to_string();
        let parts: Vec<&str> = slot.split('-').collect();
        let (size, terrain, seats, hills) = match parts.as_slice() {
            [a, b, c, e] => (*a, *b, c.trim_end_matches('p'), e.trim_end_matches('h')),
            _ => return Err(format!("{slot}: not <size>-<terrain>-<N>p-<H>h")),
        };
        let a_hill = if hills == "1" { "hill" } else { "hills" };
        let head = format!(
            "# {name}: a {size} board, {terrain} terrain, {seats} seats, {hills} {a_hill} a seat.\n\
             # The \"{}\" family under point group {}, {}x{}, seat shift ({}, {}).\n\
             # Drawn by `mapgen explore` (candidate {i}) and picked by eye. A design is\n\
             # data: the renderer draws exactly this, whatever becomes of the sampler that proposed it.\n\n",
            d.family, d.symmetry, d.rows, d.cols, d.shift[0], d.shift[1]
        );
        let path = recipes_dir.join(format!("{name}.toml"));
        std::fs::write(&path, head + &d.to_toml()?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("  {} <- {slot} #{i} ({})", path.display(), d.family);
    }
    Ok(())
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
