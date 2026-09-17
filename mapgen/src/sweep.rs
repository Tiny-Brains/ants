//! `mapgen sweep` — the generator and the engine, tested across the space of boards rather than the
//! four presets that ship.
//!
//!     cargo run --release -- sweep                               every style, 2 to 8 seats
//!     cargo run --release -- sweep --seats 8 --styles cave,rooms --boards 6 --turns 400
//!     cargo run --release -- sweep --export /tmp/boards --json /tmp/sweep.json
//!
//! Each style is a recipe shape — open ground, mazes at the connectivity floor and far above it,
//! rooms, caves, arenas, islands, walls four thick, four hills a seat — drawn on a board sized for the
//! seat count with every shift of that order, several boards each. Every board is then:
//!
//! 1. generated, measured and validated by the engine (`set::make_one`);
//! 2. played to the end with one frame-relative policy in every seat (`play::congruent`), which must
//!    keep every seat's view equal to seat 0's moved by the shift, every turn, end on equal scores,
//!    and replay to the same colonies;
//! 3. played as a wave of independent matches (`play::wave`), for end reasons, colony sizes, and the
//!    time and bytes a turn costs.
//!
//! A recipe no draw could satisfy is reported and is not a failure: it is a region of the knob space
//! with no boards in it. A rule a generated board breaks, a board the engine refuses, a match that
//! stops being congruent, or a replay that disagrees, is a failure, and the command exits non-zero.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::{Value, json};

use crate::measure::Metrics;
use crate::play::{Congruent, WaveStats, congruent, wave};
use crate::recipe::Recipe;
use crate::set::make_one;

/// A recipe shape: the `[areas]`, `[connectivity]`, `[fill]` and `[hills]` a style is, whatever the
/// board and seat count it is drawn at.
pub const STYLES: [(&str, &str); 11] = [
    (
        "open",
        "[areas]\nsize = [300, 600]\n[fill]\nstyle = \"scatter\"\npct = 12\nblob = [2, 5]\n[hills]\nclearing = 3",
    ),
    (
        "boulders",
        "[areas]\nsize = [300, 600]\n[fill]\nstyle = \"scatter\"\npct = 30\nblob = [3, 8]",
    ),
    (
        "voronoi-maze",
        "[areas]\nsize = [9, 16]\nclosure_pct = 100\nwall = 1\n[connectivity]\nloops_pct = 5\n[hills]\nroom = true\nhome_doors_min = 2",
    ),
    (
        "tree-maze",
        "[areas]\nsize = [9, 9]\ngrid = true\nclosure_pct = 100\nwall = 1\n[connectivity]\nloops_pct = 0\n[hills]\nroom = true",
    ),
    (
        "loopy-maze",
        "[areas]\nsize = [16, 16]\ngrid = true\nclosure_pct = 90\nwall = 1\n[connectivity]\nloops_pct = 50\n[hills]\nroom = true",
    ),
    (
        "rooms",
        "[areas]\nsize = [144, 144]\ngrid = true\ncoverage_pct = 85\nclosure_pct = 80\nwall = 2\n[connectivity]\nloops_pct = 35\ndoor_min = 2\n[fill]\nstyle = \"scatter\"\npct = 4\nblob = [1, 2]\n[hills]\nhome_doors_min = 2",
    ),
    (
        "cave",
        "[areas]\nsize = [260, 520]\nwarp = 6\narenas_pct = 15\ncoverage_pct = 70\nclosure_pct = 45\nwall = 2\n[connectivity]\nloops_pct = 30\ndoor_min = 3\n[fill]\nstyle = \"cellular\"\npct = 38\n[hills]\nper_seat = 2",
    ),
    (
        "arenas",
        "[areas]\nsize = [120, 240]\nwarp = 3\narenas_pct = 40\ncoverage_pct = 90\nclosure_pct = 60\nwall = 3\n[connectivity]\nloops_pct = 20\ndoor_min = 2",
    ),
    (
        "islands",
        "[areas]\nsize = [150, 300]\nwarp = 4\ncoverage_pct = 45\nclosure_pct = 30\nwall = 2\n[connectivity]\nloops_pct = 25\ndoor_min = 2",
    ),
    (
        "fortress",
        "[areas]\nsize = [200, 400]\nclosure_pct = 70\nwall = 4\n[connectivity]\nloops_pct = 30\ndoor_min = 2\n[hills]\nhome_closure_pct = 90\nhome_doors_min = 2",
    ),
    (
        "colonies",
        "[areas]\nsize = [300, 600]\n[fill]\nstyle = \"scatter\"\npct = 10\nblob = [2, 4]\n[hills]\nper_seat = 4",
    ),
];

/// A square board for `seats`: its side divides by the seat count, and a seat's step along an axis
/// divides by every lattice a style uses (3, 4 and 12), so every style draws at every count. About
/// 3,500 to 11,500 squares a seat.
pub fn side(seats: u8) -> u8 {
    match seats {
        2 => 96,
        3 => 144,
        4 => 192,
        5 => 240,
        6 => 144,
        7 => 168,
        _ => 192,
    }
}

pub fn recipe(style: &str, seats: u8, boards: u32) -> Result<Recipe, String> {
    let body = STYLES.iter().find(|(n, _)| *n == style).ok_or(format!("no style '{style}'"))?.1;
    let per_seat = if body.contains("per_seat = 4") {
        4
    } else if body.contains("per_seat = 2") {
        2
    } else {
        1
    };
    let n = side(seats);
    Recipe::parse(&format!(
        "[preset]\nname = \"sweep-{style}-{seats}\"\nseats = {seats}\ncount = {boards}\nseed = {seed}\n\
         [board]\nrows = {n}\ncols = {n}\nshifts = [\"any\"]\n{body}\n\
         [food]\nper_seat = {food}\nbootstrap = 2\n",
        seed = 0x5EE9_0000 + seats as u64,
        food = 6 + 2 * per_seat,
    ))
    .map_err(|e| format!("sweep-{style}-{seats}: {e}"))
}

struct Job {
    style: &'static str,
    seats: u8,
    index: usize,
}

#[derive(Default)]
struct Row {
    style: &'static str,
    seats: u8,
    index: usize,
    id: String,
    shift: (i32, i32),
    gen_ms: u128,
    attempt: u64,
    metrics: Option<Metrics>,
    /// Why no board came out: a recipe no draw satisfied (`soft`), or a rule broken (`bug`).
    gen_error: Option<(bool, String)>,
    congruent: Option<Result<Congruent, String>>,
    wave: Option<Result<WaveStats, String>>,
    text: String,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let seats: Vec<u8> = match flag("--seats") {
        None => (2..=8).collect(),
        Some(s) if s.contains('-') => {
            let (a, b) = s.split_once('-').unwrap_or(("2", "8"));
            (a.parse().map_err(|_| "--seats: 2-8 or 2,4,8")?
                ..=b.parse().map_err(|_| "--seats: 2-8 or 2,4,8")?)
                .collect()
        }
        Some(s) => s
            .split(',')
            .map(|x| x.parse().map_err(|_| "--seats: 2-8 or 2,4,8".to_string()))
            .collect::<Result<_, _>>()?,
    };
    let styles: Vec<&'static str> = match flag("--styles") {
        None => STYLES.iter().map(|(n, _)| *n).collect(),
        Some(s) => s
            .split(',')
            .map(|x| {
                STYLES.iter().map(|(n, _)| *n).find(|n| *n == x).ok_or(format!("no style '{x}'"))
            })
            .collect::<Result<_, _>>()?,
    };
    let num = |k: &str, d: u32| {
        flag(k).map_or(Ok(d), |v| v.parse().map_err(|_| format!("{k}: a number")))
    };
    let (boards, turns, seeds) = (num("--boards", 3)?, num("--turns", 250)?, num("--seeds", 4)?);
    let threads =
        num("--threads", std::thread::available_parallelism().map_or(4, |n| n.get() as u32))?
            .max(1);

    let mut recipes = BTreeMap::new();
    let mut jobs = Vec::new();
    for &style in &styles {
        for &p in &seats {
            recipes.insert((style, p), recipe(style, p, boards)?);
            for index in 0..boards as usize {
                jobs.push(Job { style, seats: p, index });
            }
        }
    }
    println!(
        "sweep: {} styles x seats {:?} x {boards} boards = {} boards; congruent play to {turns} turns, a wave of {seeds}; {threads} threads",
        styles.len(),
        seats,
        jobs.len()
    );

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let rows: Mutex<Vec<Row>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(job) = jobs.get(i) else { break };
                    let row = one(&recipes[&(job.style, job.seats)], job, turns, seeds);
                    let bad = row.gen_error.as_ref().is_some_and(|(soft, _)| !soft)
                        || matches!(row.congruent, Some(Err(_)))
                        || matches!(row.wave, Some(Err(_)));
                    eprintln!(
                        "  [{:>3}/{}] {:<24} {}",
                        i + 1,
                        jobs.len(),
                        format!("{}-{}-{}", job.style, job.seats, job.index),
                        if bad {
                            "FAILED"
                        } else if row.gen_error.is_some() {
                            "no board"
                        } else {
                            "ok"
                        }
                    );
                    rows.lock().unwrap().push(row);
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|r| (styles.iter().position(|s| *s == r.style), r.seats, r.index));

    if let Some(dir) = flag("--export") {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
        for r in rows.iter().filter(|r| !r.text.is_empty()) {
            std::fs::write(format!("{dir}/{}.json", r.id), &r.text)
                .map_err(|e| format!("{dir}: {e}"))?;
        }
    }
    let report = report(&rows, &styles, &seats, started.elapsed().as_secs());
    if let Some(path) = flag("--json") {
        std::fs::write(&path, serde_json::to_string_pretty(&json_rows(&rows)).unwrap_or_default())
            .map_err(|e| format!("{path}: {e}"))?;
    }
    report
}

fn one(r: &Recipe, job: &Job, turns: u32, seeds: u32) -> Row {
    let mut row =
        Row { style: job.style, seats: job.seats, index: job.index, ..Default::default() };
    let t0 = Instant::now();
    let made = make_one(r, job.index);
    row.gen_ms = t0.elapsed().as_millis();
    let made = match made {
        Ok(m) => m,
        Err(e) => {
            row.gen_error = Some((e.contains("attempts passed"), e));
            return row;
        }
    };
    let v: Value = serde_json::from_str(&made.text).unwrap_or(Value::Null);
    row.id = made.id.clone();
    row.shift = (made.board.s.dr, made.board.s.dc);
    row.attempt = v["generator"]["attempt"].as_u64().unwrap_or(0);
    row.metrics = Some(made.metrics.clone());
    row.congruent = Some(congruent(&v, 1000 + job.index as u64, turns));
    row.wave = Some(wave(&v, seeds, turns));
    row.text = made.text;
    row
}

fn range<T: Ord + Copy + std::fmt::Display>(v: impl Iterator<Item = T>) -> String {
    let v: Vec<T> = v.collect();
    match (v.iter().min(), v.iter().max()) {
        (Some(a), Some(b)) if a == b => format!("{a}"),
        (Some(a), Some(b)) => format!("{a}-{b}"),
        _ => "-".into(),
    }
}

fn report(rows: &[Row], styles: &[&str], seats: &[u8], secs: u64) -> Result<(), String> {
    println!(
        "\n  {:<13} {:>5} {:>7} {:>7} {:>6} {:>10} {:>7} {:>9} {:>11} {:>12} {:>8} {:>7}",
        "style",
        "seats",
        "boards",
        "gen ms",
        "tries",
        "water %",
        "routes",
        "detour %",
        "congruent",
        "contact/end",
        "ms/turn",
        "view KB"
    );
    let mut bugs = Vec::new();
    let mut soft = Vec::new();
    for &style in styles {
        for &p in seats {
            let group: Vec<&Row> =
                rows.iter().filter(|r| r.style == style && r.seats == p).collect();
            let made: Vec<&Row> = group.iter().copied().filter(|r| r.metrics.is_some()).collect();
            let m =
                |f: fn(&Metrics) -> u32| range(made.iter().map(|r| f(r.metrics.as_ref().unwrap())));
            let cong: Vec<&Congruent> =
                made.iter().filter_map(|r| r.congruent.as_ref()?.as_ref().ok()).collect();
            let waves: Vec<&WaveStats> =
                made.iter().filter_map(|r| r.wave.as_ref()?.as_ref().ok()).collect();
            let ms = waves.iter().map(|w| w.micros_per_turn).max().unwrap_or(0);
            let kb = waves.iter().map(|w| w.max_view_bytes).max().unwrap_or(0);
            println!(
                "  {:<13} {:>5} {:>7} {:>7} {:>6} {:>10} {:>7} {:>9} {:>11} {:>12} {:>8} {:>7}",
                style,
                p,
                format!("{}/{}", made.len(), group.len()),
                made.iter().map(|r| r.gen_ms).max().unwrap_or(0),
                range(made.iter().map(|r| r.attempt)),
                range(made.iter().map(|r| r.metrics.as_ref().unwrap().water_pm / 10)),
                m(|x| x.routes),
                m(|x| x.detour_pct),
                format!("{}/{}", cong.len(), made.len()),
                format!("{}/{}", cong.iter().filter(|c| c.contact).count(), cong.len()),
                format!("{}.{:01}", ms / 1000, (ms % 1000) / 100),
                kb / 1000,
            );
            for r in &group {
                match &r.gen_error {
                    Some((true, e)) => soft.push(e.clone()),
                    Some((false, e)) => bugs.push(e.clone()),
                    None => {}
                }
                if let Some(Err(e)) = &r.congruent {
                    bugs.push(format!("{} (shift {:?}): congruent play: {e}", r.id, r.shift));
                }
                if let Some(Err(e)) = &r.wave {
                    bugs.push(format!("{} (shift {:?}): wave: {e}", r.id, r.shift));
                }
            }
        }
    }

    // How matches end, by seat count, over both kinds of play.
    println!(
        "\n  endings by seat count (congruent play | independent waves), and first places by seat in the waves"
    );
    for &p in seats {
        let group: Vec<&Row> = rows.iter().filter(|r| r.seats == p).collect();
        let mut cong: BTreeMap<String, u32> = BTreeMap::new();
        let mut wav: BTreeMap<String, u32> = BTreeMap::new();
        let mut firsts = vec![0u32; p as usize];
        let (mut peak, mut turns, mut matches) = (0usize, 0u64, 0u64);
        for r in &group {
            if let Some(Ok(c)) = &r.congruent {
                *cong.entry(c.reason.clone()).or_default() += 1;
                peak = peak.max(c.peak_ants);
            }
            if let Some(Ok(w)) = &r.wave {
                for (k, n) in &w.reasons {
                    *wav.entry(k.clone()).or_default() += n;
                }
                for (k, n) in w.firsts.iter().enumerate() {
                    firsts[k] += n;
                }
                turns += w.turns.iter().sum::<u64>();
                matches += w.turns.len() as u64;
                peak = peak.max(w.peak_ants);
            }
        }
        let show = |m: &BTreeMap<String, u32>| {
            m.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
        };
        println!(
            "  {p} seats: {} | {}; {} turns a wave match; peak colony {peak}; firsts {:?}",
            show(&cong),
            show(&wav),
            turns / matches.max(1),
            firsts
        );
    }

    let made = rows.iter().filter(|r| r.metrics.is_some()).count();
    println!("\n  {} boards drawn of {}, in {secs} s", made, rows.len());
    if !soft.is_empty() {
        println!("  {} with no board, the recipe out of reach at that size:", soft.len());
        for e in &soft {
            println!("    {e}");
        }
    }
    if bugs.is_empty() {
        println!(
            "  no rule broken, no board refused, every congruent match stayed congruent and replayed"
        );
        Ok(())
    } else {
        Err(format!("{} failure(s):\n  {}", bugs.len(), bugs.join("\n  ")))
    }
}

fn json_rows(rows: &[Row]) -> Value {
    json!(rows
        .iter()
        .map(|r| json!({
            "style": r.style, "seats": r.seats, "index": r.index, "id": r.id,
            "shift": [r.shift.0, r.shift.1], "gen_ms": r.gen_ms as u64, "attempt": r.attempt,
            "metrics": r.metrics,
            "gen_error": r.gen_error.as_ref().map(|(soft, e)| json!({"soft": soft, "error": e})),
            "congruent": r.congruent.as_ref().map(|c| match c {
                Ok(c) => json!({"turns": c.turns, "reason": c.reason, "score": c.score, "peak_ants": c.peak_ants, "contact": c.contact}),
                Err(e) => json!({"error": e}),
            }),
            "wave": r.wave.as_ref().map(|w| match w {
                Ok(w) => json!({"reasons": w.reasons, "turns": w.turns, "firsts": w.firsts, "peak_ants": w.peak_ants,
                                "micros_per_turn": w.micros_per_turn, "max_state_bytes": w.max_state_bytes,
                                "max_view_bytes": w.max_view_bytes}),
                Err(e) => json!({"error": e}),
            }),
        }))
        .collect::<Vec<_>>())
}
