//! The board factory and the artifact generators — everything the three host binaries need.
//!
//! Nothing here runs during a match. `worldgen` grows a world so `mapgen` can write it to a file;
//! `mapfile::build` is what a match actually opens from. None of it is reachable from a plugin
//! call, so the linker leaves it out of the component.

use serde_json::{json, Value};

use crate::codec::{pack, Wave};
use crate::map::{self, Bits, Geom, Preset, Rng, DIRS, DIR_NAMES, SPAWN_RADIUS2};
use crate::state::Match;
use crate::{mapfile, turn, MAX_TURNS};

/// The presets, for the registration manifest. `src/bin/manifest.rs` generates `cartridge.json`
/// from this, so the manifest and the engine cannot disagree about how many seats a map is played
/// at.
pub fn presets() -> &'static [map::Preset] {
    &map::PRESETS
}

/// Run the procedural generator once and hand back the board it produced, as a map file. This is
/// the whole of `src/bin/mapgen.rs`.
pub fn generate_map(preset_name: &str, seed: u64, id: &str) -> Option<Value> {
    let p = map::preset(preset_name)?;
    let m = worldgen(seed, p, MAX_TURNS);
    Some(mapfile::MapFile::from_match(&m, id, p.name).to_json())
}

/// Play one match to a busy turn and hand back what every seat could see.
///
/// **The gate is only as good as its worst case.** Admission validates an adapter against these
/// observations and nothing else, so a set drawn from turn zero — two ants, no contact, almost
/// nothing known — would admit adapters that are struck every turn of a real match. This plays to a
/// turn where colonies have grown, explored and met, and takes the views from there.
///
/// `observe` returns nothing for a finished match and a greedy field ends one, so the last live
/// state is kept: asking for turn 250 of a match that ended on turn 190 gives the observations from
/// turn 189 rather than an empty file.
pub fn reference_observations(preset_name: &str, seed: u64, until_turn: u16) -> Option<Value> {
    map::preset(preset_name)?;
    let mf = mapfile::for_seed(preset_name, seed)?;
    let mut m = mf.build(seed, MAX_TURNS).ok()?;

    let mut last_live = m.clone();
    let mut rng = Rng(seed ^ 0x5EED_0B5E_2A17_C0DE);
    while m.turn < until_turn && !m.done {
        let moves: Vec<Vec<String>> =
            (0..m.players).map(|seat| greedy_orders(&m, seat, &mut rng)).collect();
        last_live = m.clone();
        turn::step(&mut m, &moves);
    }

    let m = if m.done { last_live } else { m };
    let reached = m.turn;
    let w = Wave { matches: vec![m] };
    let views = crate::f_observe(&json!({ "wave_state": pack(&w) })).ok()?;
    Some(json!({
        "generated_from": {
            "preset": preset_name, "seed": seed,
            "asked_for_turn": until_turn, "turn": reached,
            "map": mf.id,
        },
        "observations": views.get("views")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v["view"].clone()).collect::<Vec<_>>())
            .unwrap_or_default(),
    }))
}

/// A greedy walker: each ant steps toward the nearest food no other ant has claimed, and wanders
/// when there is none in sight.
///
/// Not play, and not meant to be — what the reference set has to contain is a board in a demanding
/// state, and a random walk will not produce one: it collects food by accident, so the colony never
/// grows and after two hundred turns a seat still has four ants. The claim matters as much as the
/// greed: without it the whole colony converges on one square and every ant dies there.
fn greedy_orders(m: &Match, seat: u8, rng: &mut Rng) -> Vec<String> {
    let mine = m.mine(seat);
    let mut claimed: Vec<u16> = Vec::new();
    let mut orders = Vec::with_capacity(mine.len());
    for &pos in &mine {
        let target = m
            .food
            .iter()
            .copied()
            .filter(|f| !claimed.contains(f))
            .min_by_key(|&f| m.g.dist2(pos, f));
        if let Some(f) = target {
            claimed.push(f);
        }
        orders.push(match target {
            Some(f) if m.g.dist2(pos, f) > 0 => {
                // Whichever of the four steps ends up closest; ties fall to the first, which is
                // stable and therefore reproducible.
                let (r, c) = m.g.rc(pos);
                let best = DIRS
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (dr, dc))| m.g.dist2(m.g.at(r + dr, c + dc), f))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                DIR_NAMES[best]
            }
            // Nothing visible: wander, so the colony covers ground rather than sitting on a hill
            // and knowing nothing about the board.
            _ => DIR_NAMES[rng.below(4) as usize],
        }
        .to_string());
    }
    orders
}

// ---------------------------------------------------------------- worldgen

/// Build one match from one seed.
///
/// The whole world is generated on a fundamental domain and translated to every player, so the
/// terrain, the hills and the food are congruent for everyone. A map that were only approximately
/// fair would put a thumb on every rating computed from it.
pub fn worldgen(seed: u64, p: Preset, max_turns: u16) -> Match {
    let g = Geom::new(p.rows, p.cols);
    let mut rng = Rng(seed);
    let mut m = Match::new(seed, g, p.players, max_turns, Bits::zeros(g.cells()));
    let sym = m.sym;

    // Water, in blobs on the fundamental domain, then translated. Blobs rather than per-cell noise
    // because a map of speckles is not a map anyone can play, and because `water` travels
    // run-length encoded, so speckles would make every observation several times larger.
    let domain = g.cells() / p.players as usize;
    let target = domain * p.water_pct as usize / 100;
    let mut placed = 0usize;
    let mut guard = 0;
    while placed < target && guard < 100_000 {
        guard += 1;
        let r0 = rng.below(g.rows as u32) as i32;
        let c0 = rng.below(g.cols as u32) as i32;
        let h = 1 + rng.below(p.blob) as i32;
        let w = 1 + rng.below(p.blob) as i32;
        for dr in 0..h {
            for dc in 0..w {
                let pos = g.at(r0 + dr, c0 + dc);
                for img in sym.orbit(&g, pos) {
                    if !m.water.get(img as usize) {
                        m.water.set(img as usize);
                        placed += 1;
                    }
                }
            }
        }
    }

    // One hill per player, far enough from its own image that the colonies do not start on top of
    // each other, with its neighbourhood cleared so nobody is walled in by a blob that landed on
    // them.
    let hill0 = loop {
        let pos = g.at(rng.below(g.rows as u32) as i32, rng.below(g.cols as u32) as i32);
        if g.dist2(pos, sym.image(&g, pos, 1)) >= 400 {
            break pos;
        }
    };
    let (hr, hc) = g.rc(hill0);
    for (dr, dc) in &g.disk(SPAWN_RADIUS2 * 8) {
        for img in sym.orbit(&g, g.at(hr + dr, hc + dc)) {
            m.water.clear(img as usize);
        }
    }
    m.open_on(&sym.orbit(&g, hill0));

    // Food beside the hills so a colony can bootstrap (`Preset::hill_food`), then the rest of the
    // board's food.
    let near = g.disk(36); // within six squares of the hill
    let mut placed_near = 0u32;
    let mut tries = 0;
    while placed_near < p.hill_food && tries < 500 {
        tries += 1;
        let (dr, dc) = near[rng.below(near.len() as u32) as usize];
        if place_orbit(&mut m, g.at(hr + dr, hc + dc)) {
            placed_near += 1;
        }
    }
    fill_food(&mut m, &mut rng, p.food_per_player as usize * p.players as usize);

    m.food0 = m.food.clone();
    m
}

/// Place a whole orbit of food, or none of it: a partially placeable orbit would be an asymmetric
/// map.
fn place_orbit(m: &mut Match, pos: u16) -> bool {
    let orbit = m.sym.orbit(&m.g, pos);
    if !orbit.iter().all(|&i| m.free_for_food(i)) {
        return false;
    }
    m.food.extend(orbit);
    true
}

/// Stock a board with turn-zero food, symmetrically, until it holds `target`.
///
/// This builds a board; it does not run a match. Food during a match accrues at the hidden rate
/// instead, in `spawn_food`.
fn fill_food(m: &mut Match, rng: &mut Rng, target: usize) {
    let mut guard = 0;
    while m.food.len() < target && guard < 10_000 {
        guard += 1;
        let pos = m.g.at(rng.below(m.g.rows as u32) as i32, rng.below(m.g.cols as u32) as i32);
        place_orbit(m, pos);
    }
}
