//! A board played, not just measured.
//!
//! Two ways, for two questions:
//!
//! - **`congruent`: is the game fair on this board, exactly?** Every seat plays one policy that sees
//!   only its own view moved into seat 0's frame. On a board congruent under its shift, with rules
//!   that treat seats alike, the whole match then stays congruent: every seat's view, every turn,
//!   is seat 0's moved by the shift, through fights, razes and food, and every seat ends on the same
//!   score. Any divergence names the turn and the field, and it is a bug in the board or the engine —
//!   there is no luck in it. The replay is decoded and held to the same counts.
//! - **`play` and `wave`: is the board playable, and what does playing it cost?** Seats play
//!   independently — the greedy walker with one shared random stream — and the numbers are colony
//!   sizes, end reasons, and the time and bytes a turn takes.
//!
//! Both are slow, so `check` runs `play` only when asked and `sweep` is its own command.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Value, json};

use crate::grid::{N4, Rng, mix};

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

// ---------------------------------------------------------------- congruent play

/// What a congruent match came to.
#[derive(Debug, Clone)]
pub struct Congruent {
    pub turns: u64,
    pub reason: String,
    /// Every seat's final score, which congruence makes one number.
    pub score: i64,
    /// The largest colony any seat reached.
    pub peak_ants: usize,
    /// Whether any seat ever saw an enemy ant: a match with no contact proves less.
    pub contact: bool,
}

/// One seat's view, moved into seat 0's frame.
#[derive(Debug, PartialEq, Eq)]
struct Frame {
    mine: Vec<(i32, i32)>,
    foes: Vec<(i32, i32, i64)>,
    food: Vec<(i32, i32)>,
    hills: Vec<(i32, i32, i64)>,
    water: Vec<bool>,
    vis: Vec<bool>,
}

struct Board {
    rows: i32,
    cols: i32,
    seats: usize,
    dr: i32,
    dc: i32,
    /// Every other seat's hills, in seat 0's frame: where a seat knows its opponents live, from the
    /// shift alone, before it has seen them.
    enemy_hills: Vec<(i32, i32)>,
}

impl Board {
    fn of(map: &Value) -> Board {
        let n = |v: &Value| v.as_i64().unwrap_or(0) as i32;
        let seats = map["players"].as_u64().unwrap_or(2) as usize;
        let enemy_hills = map["hills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter(|(i, _)| i % seats != 0)
            .map(|(_, h)| (n(&h[0]), n(&h[1])))
            .collect();
        Board {
            rows: n(&map["rows"]),
            cols: n(&map["cols"]),
            seats,
            dr: n(&map["symmetry"]["dr"]),
            dc: n(&map["symmetry"]["dc"]),
            enemy_hills,
        }
    }

    /// A square as seat `s` sees it, moved back `s` shifts into seat 0's frame.
    fn home(&self, s: usize, r: i32, c: i32) -> (i32, i32) {
        let s = s as i32;
        ((r - s * self.dr).rem_euclid(self.rows), (c - s * self.dc).rem_euclid(self.cols))
    }

    fn frame(&self, view: &Value, s: usize) -> Frame {
        let at = |v: &Value, i: usize| v[i].as_i64().unwrap_or(0) as i32;
        let list = |k: &str| view[k].as_array().cloned().unwrap_or_default();
        let mut mine: Vec<_> =
            list("mine").iter().map(|a| self.home(s, at(a, 0), at(a, 1))).collect();
        let mut food: Vec<_> =
            list("food").iter().map(|a| self.home(s, at(a, 0), at(a, 1))).collect();
        let owned = |k: &str| {
            let mut v: Vec<_> = list(k)
                .iter()
                .map(|a| {
                    let (r, c) = self.home(s, at(a, 0), at(a, 1));
                    (r, c, a[2].as_i64().unwrap_or(-1))
                })
                .collect();
            v.sort_unstable();
            v
        };
        mine.sort_unstable();
        food.sort_unstable();
        let bits = |k: &str| {
            let cells = (self.rows * self.cols) as usize;
            let mut out = vec![false; cells];
            let mut x = 0usize;
            for pair in view[k]["rle"].as_array().cloned().unwrap_or_default().chunks(2) {
                let run = pair.get(1).and_then(Value::as_u64).unwrap_or(0) as usize;
                if pair[0].as_u64() == Some(1) {
                    for y in x..(x + run).min(cells) {
                        let (r, c) = self.home(s, y as i32 / self.cols, y as i32 % self.cols);
                        out[(r * self.cols + c) as usize] = true;
                    }
                }
                x += run;
            }
            out
        };
        Frame {
            mine,
            foes: owned("foes"),
            food,
            hills: owned("hills"),
            water: bits("water"),
            vis: bits("vis"),
        }
    }

    /// Orders for every ant of a frame, keyed by the ant's square in that frame: toward the nearest
    /// enemy, else a visible enemy hill, else unclaimed food, else — two times in three — the nearest
    /// enemy hill the shift says is there, over known water, with a hashed wander. Nothing in it reads
    /// an absolute square, so it is the same policy in every seat, and it goes looking for a fight.
    fn orders(&self, f: &Frame, turn: u64) -> BTreeMap<(i32, i32), &'static str> {
        let d2 = |a: (i32, i32), b: (i32, i32)| {
            let dr = (a.0 - b.0).abs().min(self.rows - (a.0 - b.0).abs());
            let dc = (a.1 - b.1).abs().min(self.cols - (a.1 - b.1).abs());
            dr * dr + dc * dc
        };
        let wet = |r: i32, c: i32| {
            f.water[(r.rem_euclid(self.rows) * self.cols + c.rem_euclid(self.cols)) as usize]
        };
        let mut claimed = vec![false; f.food.len()];
        let mut out = BTreeMap::new();
        for &a in &f.mine {
            let h = mix(turn, ((a.0 as u64) << 16) | a.1 as u64);
            let nearest =
                |v: &mut dyn Iterator<Item = (i32, i32)>| v.min_by_key(|&p| (d2(a, p), p));
            let target = nearest(&mut f.foes.iter().map(|&(r, c, _)| (r, c)))
                .or_else(|| {
                    nearest(&mut f.hills.iter().filter(|h| h.2 != 0).map(|&(r, c, _)| (r, c)))
                })
                .or_else(|| {
                    let i = (0..f.food.len())
                        .filter(|&i| !claimed[i])
                        .min_by_key(|&i| (d2(a, f.food[i]), i))?;
                    claimed[i] = true;
                    Some(f.food[i])
                })
                .or_else(|| {
                    if !h.is_multiple_of(3) {
                        nearest(&mut self.enemy_hills.iter().copied())
                    } else {
                        None
                    }
                });
            let dry: Vec<usize> = (0..4).filter(|&d| !wet(a.0 + N4[d].0, a.1 + N4[d].1)).collect();
            let order = match target {
                _ if dry.is_empty() => "-",
                Some(t) if !h.is_multiple_of(9) => {
                    DIRS[*dry
                        .iter()
                        .min_by_key(|&&d| (d2((a.0 + N4[d].0, a.1 + N4[d].1), t), d))
                        .unwrap()]
                }
                _ => DIRS[dry[(h % dry.len() as u64) as usize]],
            };
            out.insert(a, order);
        }
        out
    }
}

/// Play one match with the same frame-relative policy in every seat, holding every seat's view to
/// seat 0's at every turn, and the ending and the replay to the same.
pub fn congruent(map: &Value, seed: u64, turns: u32) -> Result<Congruent, String> {
    congruent_with(map, seed, turns, None)
}

/// `congruent`, with one seat made to play one different order on one turn — which the check must
/// catch, or it is checking nothing.
pub fn congruent_with(
    map: &Value,
    seed: u64,
    turns: u32,
    sabotage: Option<(u64, usize)>,
) -> Result<Congruent, String> {
    let fault = |f: tb_ants::Fault| format!("{} {}", f.code, f.message);
    let b = Board::of(map);
    let w = tb_ants::invoke(
        "tb.ants.worldgen",
        json!({ "seeds": [seed], "map": map, "max_turns": turns }),
    )
    .map_err(fault)?;
    let mut state = w["wave_state"].as_str().unwrap_or_default().to_string();
    let mut deltas = Vec::new();
    let mut counts: Vec<Vec<usize>> = Vec::new();
    let (mut peak, mut contact) = (0usize, false);

    for turn in 0u64.. {
        let views =
            tb_ants::invoke("tb.ants.observe", json!({ "wave_state": state })).map_err(fault)?;
        let views = views["views"].as_array().cloned().unwrap_or_default();
        if views.is_empty() {
            break;
        }
        if views.len() != b.seats {
            return Err(format!("turn {turn}: {} views for {} seats", views.len(), b.seats));
        }
        let frames: Vec<Frame> = (0..b.seats).map(|s| b.frame(&views[s]["view"], s)).collect();
        for (s, f) in frames.iter().enumerate().skip(1) {
            if *f != frames[0] {
                let what = [
                    ("mine", f.mine != frames[0].mine),
                    ("foes", f.foes != frames[0].foes),
                    ("food", f.food != frames[0].food),
                    ("hills", f.hills != frames[0].hills),
                    ("water", f.water != frames[0].water),
                    ("vis", f.vis != frames[0].vis),
                ]
                .iter()
                .filter(|(_, d)| *d)
                .map(|(k, _)| *k)
                .collect::<Vec<_>>()
                .join(", ");
                return Err(format!(
                    "turn {turn}: seat {s}'s view is not seat 0's moved by the shift ({what}); {}",
                    first_asymmetry(&b, map, seed, turns, &deltas)
                ));
            }
        }
        counts.push(frames.iter().map(|f| f.mine.len()).collect());
        peak = peak.max(frames[0].mine.len());
        contact |= !frames[0].foes.is_empty();

        let actions: Vec<Value> = (0..b.seats)
            .map(|s| {
                let orders = b.orders(&frames[s], turn);
                let mine = views[s]["view"]["mine"].as_array().cloned().unwrap_or_default();
                let mut these: Vec<&str> = mine
                    .iter()
                    .map(|a| {
                        let at = |i: usize| a[i].as_i64().unwrap_or(0) as i32;
                        orders.get(&b.home(s, at(0), at(1))).copied().unwrap_or("-")
                    })
                    .collect();
                if sabotage == Some((turn, s)) && !these.is_empty() {
                    these[0] = if these[0] == "-" { "N" } else { "-" };
                }
                json!(these)
            })
            .collect();
        let next =
            tb_ants::invoke("tb.ants.step", json!({ "wave_state": state, "actions": actions }))
                .map_err(fault)?;
        deltas.extend(next["replay_delta"].as_array().cloned().unwrap_or_default());
        state = next["wave_state"].as_str().unwrap_or_default().to_string();
    }

    let fin = tb_ants::invoke("tb.ants.finish", json!({ "wave_state": state })).map_err(fault)?;
    let r = &fin["results"][0];
    let scores: Vec<i64> = r["scores"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    let ranks: Vec<u64> = r["ranks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    if scores.len() != b.seats
        || scores.iter().any(|&x| x != scores[0])
        || ranks.iter().any(|&x| x != ranks[0])
    {
        return Err(format!("the match ended unequal: scores {scores:?}, ranks {ranks:?}"));
    }

    // The replay the match recorded re-simulates to the same colonies, turn by turn, and the same end.
    let payload = json!({ "seed": seed, "max_turns": turns, "map": r["map"], "deltas": deltas });
    let decoded =
        tb_ants::invoke("tb.ants.replay-decode", json!({ "payload": payload, "from": 0 }))
            .map_err(fault)?;
    let frames = decoded["frames"].as_array().cloned().unwrap_or_default();
    for (t, want) in counts.iter().enumerate() {
        let frame = frames.get(t).ok_or(format!("the replay has no frame {t}"))?;
        let mut got = vec![0usize; b.seats];
        for a in frame["ants"].as_array().cloned().unwrap_or_default() {
            got[a[2].as_u64().unwrap_or(0) as usize % b.seats] += 1;
        }
        if &got != want {
            return Err(format!("replay frame {t} has colonies {got:?}, the match had {want:?}"));
        }
    }
    let last = frames.last().ok_or("the replay decoded no frames")?;
    if last["score"] != r["scores"] {
        return Err(format!("the replay ends on {}, the match on {}", last["score"], r["scores"]));
    }

    Ok(Congruent {
        turns: r["turns"].as_u64().unwrap_or(0),
        reason: r["reason"].as_str().unwrap_or("?").to_string(),
        score: scores[0],
        peak_ants: peak,
        contact,
    })
}

// ---------------------------------------------------------------- a wave, measured

/// What a wave of independently played matches came to, and what it cost.
#[derive(Debug, Clone, Default)]
pub struct WaveStats {
    pub reasons: BTreeMap<String, u32>,
    pub turns: Vec<u64>,
    /// First places by seat, summed over the wave.
    pub firsts: Vec<u32>,
    pub peak_ants: usize,
    /// Wall time of one `observe` and one `step`, for the whole wave, averaged over its turns.
    pub micros_per_turn: u64,
    pub max_state_bytes: usize,
    pub max_view_bytes: usize,
}

pub fn wave(map: &Value, seeds: u32, turns: u32) -> Result<WaveStats, String> {
    let fault = |f: tb_ants::Fault| format!("{} {}", f.code, f.message);
    let seats = map["players"].as_u64().unwrap_or(2) as usize;
    let seed_list: Vec<u64> = (1..=seeds as u64).map(|s| s * 7919).collect();
    let w = tb_ants::invoke(
        "tb.ants.worldgen",
        json!({ "seeds": seed_list, "map": map, "max_turns": turns }),
    )
    .map_err(fault)?;
    let mut state = w["wave_state"].as_str().unwrap_or_default().to_string();
    let mut stats = WaveStats { firsts: vec![0; seats], ..Default::default() };
    let mut rng = Rng(mix(seeds as u64, turns as u64));
    let (mut spent, mut n) = (0u128, 0u128);
    loop {
        stats.max_state_bytes = stats.max_state_bytes.max(state.len());
        let t0 = Instant::now();
        let views =
            tb_ants::invoke("tb.ants.observe", json!({ "wave_state": state })).map_err(fault)?;
        let mut elapsed = t0.elapsed().as_micros();
        let views = views["views"].as_array().cloned().unwrap_or_default();
        if views.is_empty() {
            break;
        }
        let mut actions = Vec::with_capacity(views.len());
        for v in &views {
            stats.max_view_bytes = stats.max_view_bytes.max(v["view"].to_string().len());
            stats.peak_ants = stats.peak_ants.max(v["view"]["mine"].as_array().map_or(0, Vec::len));
            actions.push(greedy(&v["view"], &mut rng));
        }
        let t1 = Instant::now();
        let next =
            tb_ants::invoke("tb.ants.step", json!({ "wave_state": state, "actions": actions }))
                .map_err(fault)?;
        elapsed += t1.elapsed().as_micros();
        spent += elapsed;
        n += 1;
        state = next["wave_state"].as_str().unwrap_or_default().to_string();
    }
    let fin = tb_ants::invoke("tb.ants.finish", json!({ "wave_state": state })).map_err(fault)?;
    for r in fin["results"].as_array().cloned().unwrap_or_default() {
        *stats.reasons.entry(r["reason"].as_str().unwrap_or("?").to_string()).or_default() += 1;
        stats.turns.push(r["turns"].as_u64().unwrap_or(0));
        for (k, rank) in r["ranks"].as_array().cloned().unwrap_or_default().iter().enumerate() {
            if rank.as_u64() == Some(1) && k < seats {
                stats.firsts[k] += 1;
            }
        }
    }
    stats.micros_per_turn = (spent / n.max(1)) as u64;
    Ok(stats)
}

/// Where a match that stopped being congruent first stopped, read off the referee's own frames —
/// which carry what no seat can see: food out of sight, razed hills, every ant.
fn first_asymmetry(b: &Board, map: &Value, seed: u64, turns: u32, deltas: &[Value]) -> String {
    let payload = json!({ "seed": seed, "max_turns": turns, "map": map, "deltas": deltas });
    let Ok(decoded) =
        tb_ants::invoke("tb.ants.replay-decode", json!({ "payload": payload, "from": 0 }))
    else {
        return "the replay does not decode".into();
    };
    let at = |v: &Value, i: usize| v[i].as_i64().unwrap_or(0) as i32;
    // Everything in a frame, moved one shift on and relabelled one seat on, must be the frame again.
    let turn = |f: &Value| -> Option<String> {
        let moved = |r: i32, c: i32| ((r + b.dr).rem_euclid(b.rows), (c + b.dc).rem_euclid(b.cols));
        let owned = |k: &str, shift: bool| -> Vec<(i32, i32, i64)> {
            let mut v: Vec<_> = f[k]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|a| {
                    let (r, c) =
                        if shift { moved(at(a, 0), at(a, 1)) } else { (at(a, 0), at(a, 1)) };
                    let o = a[2].as_i64().unwrap_or(0);
                    (r, c, if shift { (o + 1) % b.seats as i64 } else { o })
                })
                .collect();
            v.sort_unstable();
            v
        };
        let plain = |k: &str, shift: bool| -> Vec<(i32, i32)> {
            let mut v: Vec<_> = f[k]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|a| if shift { moved(at(a, 0), at(a, 1)) } else { (at(a, 0), at(a, 1)) })
                .collect();
            v.sort_unstable();
            v
        };
        let diff = |a: &[(i32, i32, i64)], m: &[(i32, i32, i64)]| {
            let only: Vec<_> = a.iter().filter(|x| !m.contains(x)).take(4).collect();
            let moved_only: Vec<_> = m.iter().filter(|x| !a.contains(x)).take(4).collect();
            format!("{only:?} not in the image, image has {moved_only:?}")
        };
        if owned("ants", false) != owned("ants", true) {
            return Some(format!("ants: {}", diff(&owned("ants", false), &owned("ants", true))));
        }
        if owned("hills", false) != owned("hills", true) {
            return Some(format!(
                "standing hills: {}",
                diff(&owned("hills", false), &owned("hills", true))
            ));
        }
        if plain("food", false) != plain("food", true) {
            let (a, m) = (plain("food", false), plain("food", true));
            let only: Vec<_> = a.iter().filter(|x| !m.contains(x)).take(4).collect();
            return Some(format!("food {only:?} has no image"));
        }
        let score: Vec<i64> = f["score"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_i64)
            .collect();
        if score.iter().any(|&x| Some(x) != score.first().copied()) {
            return Some(format!("scores {score:?}"));
        }
        None
    };
    let frames = decoded["frames"].as_array().cloned().unwrap_or_default();
    for (i, f) in frames.iter().enumerate() {
        if let Some(why) = turn(f) {
            if std::env::var("MAPGEN_TRACE").is_ok() && i > 0 {
                trace(b, &frames[i - 1], f, &deltas[i - 1], &why);
            }
            return format!("the board first broke symmetry after turn {}: {why}", f["turn"]);
        }
    }
    "the referee's board stayed symmetric, so the views diverged on something only a seat has"
        .into()
}

/// Debugging aid: the squares around every ant named in `why`, before and after the turn, with the
/// order each ant there was given.
fn trace(b: &Board, before: &Value, after: &Value, delta: &Value, why: &str) {
    let nums: Vec<i32> = why
        .split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect();
    let squares: Vec<(i32, i32)> =
        nums.chunks(3).filter(|c| c.len() == 3).map(|c| (c[0], c[1])).collect();
    let cells = (b.rows * b.cols) as usize;
    let mut water = vec![false; cells];
    let mut x = 0usize;
    for pair in before["water"]["rle"].as_array().cloned().unwrap_or_default().chunks(2) {
        let run = pair[1].as_u64().unwrap_or(0) as usize;
        if pair[0].as_u64() == Some(1) {
            water[x..x + run].fill(true);
        }
        x += run;
    }
    let orders: Vec<Vec<char>> = delta["a"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|a| a.as_str().unwrap_or("").chars().collect())
        .collect();
    let order_at = |r: i32, c: i32, o: usize| -> char {
        let mut mine: Vec<(i32, i32)> = before["ants"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|a| a[2].as_u64() == Some(o as u64))
            .map(|a| (a[0].as_i64().unwrap_or(0) as i32, a[1].as_i64().unwrap_or(0) as i32))
            .collect();
        mine.sort_unstable();
        mine.iter()
            .position(|&p| p == (r, c))
            .and_then(|i| orders.get(o)?.get(i).copied())
            .unwrap_or('?')
    };
    let draw = |f: &Value, r0: i32, c0: i32, with_orders: bool| {
        for dr in -4..=4 {
            let mut line = String::new();
            for dc in -4..=4 {
                let (r, c) = ((r0 + dr).rem_euclid(b.rows), (c0 + dc).rem_euclid(b.cols));
                let ant = f["ants"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|a| a[0] == r && a[1] == c);
                let hill = f["hills"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|a| a[0] == r && a[1] == c);
                let food = f["food"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .any(|a| a[0] == r && a[1] == c);
                let cell = match (ant, hill) {
                    (Some(a), _) => {
                        let o = a[2].as_u64().unwrap_or(0) as usize;
                        if with_orders {
                            format!("{o}{}", order_at(r, c, o))
                        } else {
                            format!("{o} ")
                        }
                    }
                    (None, Some(h)) => format!("H{}", h[2]),
                    _ if food => "* ".into(),
                    _ if water[(r * b.cols + c) as usize] => "##".into(),
                    _ => ". ".into(),
                };
                line.push_str(&cell);
            }
            eprintln!("      {line}");
        }
    };
    for (r, c) in squares {
        eprintln!(
            "  around ({r}, {c}): turn {} with orders, then turn {}",
            before["turn"], after["turn"]
        );
        draw(before, r, c, true);
        eprintln!();
        draw(after, r, c, false);
    }
}
