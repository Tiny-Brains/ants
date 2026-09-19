//! `explore`: many designs a slot, rendered, validated and scored, for a person to choose from.
//!
//!     mapgen explore --slots slots.toml --out DIR [--n 300] [--only NAME] [--threads 8]
//!
//! Writes `DIR/<slot>.jsonl`, one line a design that became a valid board: the design itself (what a
//! recipe would carry), the map file it renders to, and its numbers. Nothing is chosen here; the
//! score only orders what a person looks at first.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::json;

use crate::design;
use crate::grid::mix;
use crate::sample::{self, Slot};

#[derive(Deserialize)]
struct SlotFile {
    slot: Vec<SlotSpec>,
}

#[derive(Deserialize)]
struct SlotSpec {
    size: String,
    terrain: String,
    seats: u8,
    hills: u8,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let slots_path = flag("--slots").ok_or("--slots FILE")?;
    let out = PathBuf::from(flag("--out").ok_or("--out DIR")?);
    let n: u64 = flag("--n").unwrap_or("300".into()).parse().map_err(|_| "--n: a number")?;
    let threads: usize =
        flag("--threads").unwrap_or("8".into()).parse().map_err(|_| "--threads")?;
    let base: u64 = flag("--seed").unwrap_or("20260919".into()).parse().map_err(|_| "--seed")?;
    let only = flag("--only");
    let text = std::fs::read_to_string(&slots_path).map_err(|e| format!("{slots_path}: {e}"))?;
    let file: SlotFile = toml::from_str(&text).map_err(|e| format!("{slots_path}: {e}"))?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let slots: Vec<(usize, Slot)> = file
        .slot
        .into_iter()
        .map(|s| Slot { size: s.size, terrain: s.terrain, seats: s.seats, hills: s.hills })
        .enumerate()
        .filter(|(_, s)| only.as_deref().is_none_or(|o| s.name() == o))
        .collect();
    let work = std::sync::Mutex::new(slots.into_iter());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let next = work.lock().unwrap().next();
                    let Some((idx, slot)) = next else { break };
                    match explore_slot(&slot, idx, n, base, &out) {
                        Ok(line) => println!("{line}"),
                        Err(e) => println!("{}: {e}", slot.name()),
                    }
                }
            });
        }
    });
    Ok(())
}

fn explore_slot(
    slot: &Slot,
    idx: usize,
    n: u64,
    base: u64,
    out: &std::path::Path,
) -> Result<String, String> {
    let opts = sample::board_options(slot)?;
    let mut lines = Vec::new();
    let mut why: std::collections::BTreeMap<String, u32> = Default::default();
    for i in 0..n {
        let seed = mix(mix(base, idx as u64 + 1), i + 1);
        let d = match sample::sample(slot, &opts, seed) {
            Ok(d) => d,
            Err(e) => {
                *why.entry(format!("sample: {}", e.split(':').next().unwrap_or(&e)))
                    .or_default() += 1;
                continue;
            }
        };
        match design::build(&d) {
            Err(e) => {
                let k = e.split(':').next().unwrap_or(&e).to_string();
                *why.entry(k).or_default() += 1;
            }
            Ok((r, m, v)) => {
                let cells = (d.rows as u32 * d.cols as u32) as f64;
                let own = own_gap(&r.board);
                let verdict = playable(&d, &m, r.repaired as f64 / cells)
                    .and_then(|()| spread(own, m.enemy_walk));
                if let Err(e) = &verdict {
                    *why.entry(e.clone()).or_default() += 1;
                    if std::env::var("EXPLORE_ALL").is_err() {
                        continue;
                    }
                }
                let score = score(&d, &m, r.repaired as f64 / cells, r.filled as f64 / cells);
                lines.push(
                    json!({
                        "i": i,
                        "seed": seed,
                        "family": d.family,
                        "symmetry": d.symmetry,
                        "size": [d.rows, d.cols],
                        "repaired": r.repaired,
                        "own_gap": own,
                        "filled": r.filled,
                        "score": (score * 100.0).round() / 100.0,
                        "rejected": verdict.err(),
                        "metrics": m,
                        "design": d,
                        "map": v,
                    })
                    .to_string(),
                );
            }
        }
    }
    let path = out.join(format!("{}.jsonl", slot.name()));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    Ok(format!(
        "{:<24} {:>4} of {n} valid   {}",
        slot.name(),
        lines.len(),
        why.iter().map(|(k, v)| format!("{k} x{v}")).collect::<Vec<_>>().join(", ")
    ))
}

/// An ordering for a person's first look, not a verdict: rich symmetry, water in the terrain's
/// range, several routes between neighbours, few dead ends, and little the renderer had to repair.
fn score(d: &design::Design, m: &crate::measure::Metrics, repaired: f64, filled: f64) -> f64 {
    let (lo, hi) = match d.terrain.as_str() {
        "open" => (80, 220),
        "cave" => (260, 480),
        "maze" => (220, 450),
        _ => (150, 420),
    };
    let w = m.water_pm as i32;
    let off = if w < lo {
        lo - w
    } else if w > hi {
        w - hi
    } else {
        0
    } as f64
        / 10.0;
    let order = crate::sym::group(&d.symmetry).map_or(1, |g| g.len()) as f64;
    let routes = (m.routes.min(8) as f64 - 2.0).max(-2.0);
    order.ln() * 2.0 + routes * 0.4
        - off * 0.3
        - (m.dead_end_pm as f64 / 10.0)
        - repaired * 300.0
        - filled * 100.0
        - (m.detour_pct.saturating_sub(160) as f64 / 20.0)
}

/// What a board must be to be worth a look at all, whatever it looks like.
fn playable(d: &design::Design, m: &crate::measure::Metrics, repaired: f64) -> Result<(), String> {
    let (lo, hi) = match d.terrain.as_str() {
        "open" => (40, 260),
        "cave" => (200, 560),
        "maze" => (100, 480),
        _ => (80, 460),
    };
    if (m.water_pm as i32) < lo || m.water_pm as i32 > hi {
        return Err("water outside the terrain's range".into());
    }
    if m.routes < 2 {
        return Err("one route between neighbours".into());
    }
    if m.detour_pct > 220 {
        return Err("detour over 220%".into());
    }
    if m.dead_end_pm > 40 {
        return Err("dead ends over 40 per mille".into());
    }
    if repaired > 0.02 {
        return Err("repaired over 2%".into());
    }
    Ok(())
}

/// `mapgen playtest RUN_DIR PICKS [--seeds 4] [--turns 300]`: play the listed candidates, the same
/// greedy walker in every seat, and flag any whose seats grow unevenly -- `check --play`'s alarm.
pub fn playtest(args: &[String]) -> Result<(), String> {
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let dir = PathBuf::from(args.first().ok_or("RUN_DIR")?);
    let picks = std::fs::read_to_string(args.get(1).ok_or("PICKS")?).map_err(|e| e.to_string())?;
    let seeds: u32 = flag("--seeds").unwrap_or("4".into()).parse().map_err(|_| "--seeds")?;
    let turns: u32 = flag("--turns").unwrap_or("300".into()).parse().map_err(|_| "--turns")?;
    let threads: usize =
        flag("--threads").unwrap_or("10".into()).parse().map_err(|_| "--threads")?;
    let mut jobs = Vec::new();
    for line in picks.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let slot = it.next().unwrap_or_default().to_string();
        let ids: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
        let text = std::fs::read_to_string(dir.join(format!("{slot}.jsonl")))
            .map_err(|e| e.to_string())?;
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(l).map_err(|e| e.to_string())?;
            let i = v["i"].as_u64().unwrap_or(u64::MAX);
            if ids.contains(&i) {
                jobs.push((slot.clone(), i, v["map"].clone()));
            }
        }
    }
    let work = std::sync::Mutex::new(jobs.into_iter());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let next = work.lock().unwrap().next();
                    let Some((slot, i, map)) = next else { break };
                    let line = match crate::play::play(&map, seeds, turns) {
                        Err(e) => format!("{slot} {i} ERROR {e}"),
                        Ok(seats) => {
                            let peaks: Vec<u64> = seats.iter().map(|s| s.peak).collect();
                            let (lo, hi) = (
                                *peaks.iter().min().unwrap_or(&0),
                                *peaks.iter().max().unwrap_or(&0),
                            );
                            let alarm =
                                (lo * 2 < hi && hi - lo >= 5 * seeds as u64) || hi <= seeds as u64;
                            format!(
                                "{slot} {i} {} peaks {:?}",
                                if alarm { "UNEVEN" } else { "ok" },
                                peaks.iter().map(|p| p / seeds as u64).collect::<Vec<_>>()
                            )
                        }
                    };
                    println!("{line}");
                }
            });
        }
    });
    Ok(())
}

/// The shortest walk between two of seat 0's own hills, or `None` with one hill a seat.
fn own_gap(b: &crate::make::Board) -> Option<u32> {
    let p = b.s.seats;
    let own: Vec<usize> = b.hills.iter().step_by(p).copied().collect();
    if own.len() < 2 {
        return None;
    }
    let water = |x: usize| b.water[x];
    let mut best = u32::MAX;
    for (i, &h) in own.iter().enumerate() {
        let d = crate::make::walk(&b.t, water, &[h]);
        for &o in &own[i + 1..] {
            best = best.min(d[o]);
        }
    }
    Some(best)
}

/// A seat's hills are spread, not huddled: the shortest walk between two of them is at least ten
/// squares and at least two fifths of the walk to the nearest enemy hill. Two hills a few steps apart in
/// one room are one hill with two doors.
fn spread(own: Option<u32>, enemy_walk: u32) -> Result<(), String> {
    match own {
        Some(g) if g < 10 || g * 5 < enemy_walk * 2 => Err("own hills huddled".into()),
        _ => Ok(()),
    }
}
