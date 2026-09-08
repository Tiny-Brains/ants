//! One turn, in `RULES.md` §4's fixed order: Move, Battle, Raze, Spawn, Gather, New food.
//!
//! The order is not an implementation detail. §13 collects the consequences that fall out of it,
//! and every one of them is a thing a player can plan around — so a cartridge that resolved these
//! steps in a different order would be a different game with the same rules text.

use crate::map::{dir_of, ATTACK_RADIUS2, SPAWN_RADIUS2};
use crate::state::{spawn_food, turn_rng, Ant, Match, STALEMATE_TURNS};

/// Advance one match by one turn. `moves[seat]` is that seat's action array, positionally aligned
/// with `mine(seat)`.
pub fn step(m: &mut Match, moves: &[Vec<String>], food_target: usize) {
    if m.done {
        return;
    }
    let mut rng = turn_rng(m);

    move_ants(m, moves);
    battle(m);
    raze(m);
    spawn(m);
    let collected = gather(m);
    spawn_food(m, &mut rng, food_target);

    m.turn += 1;
    update_stalemate(m, collected);
    check_end(m);
}

// ---------------------------------------------------------------- 1. Move

/// Rule 18 step 1, and rules 20-27.
///
/// An order into water is ignored and that ant stays put (rule 23); an ant with no order stays
/// (rule 22); and every ant that finishes on a shared square dies, **regardless of owner** — your
/// own two ants walking into each other both die (rules 25-27).
fn move_ants(m: &mut Match, moves: &[Vec<String>]) {
    let mut next: Vec<Ant> = Vec::with_capacity(m.ants.len());
    for seat in 0..m.players {
        let mine = m.mine(seat);
        let orders = moves.get(seat as usize);
        for (i, &pos) in mine.iter().enumerate() {
            let order = orders.and_then(|v| v.get(i)).map(String::as_str).unwrap_or("-");
            let dest = match dir_of(order) {
                Some((dr, dc)) => {
                    let (r, c) = m.g.rc(pos);
                    let want = m.g.at(r + dr, c + dc);
                    if m.water.get(want as usize) {
                        pos // rule 23
                    } else {
                        want
                    }
                }
                None => pos, // rule 22, and anything that is not a direction
            };
            next.push(Ant { pos: dest, owner: seat });
        }
    }

    // Rule 25: two or more on a square and all of them die. Counted rather than compared pairwise,
    // so three ants on one square is three deaths and not three pairs.
    let mut occupancy: Vec<u16> = vec![0; m.cells()];
    for a in &next {
        occupancy[a.pos as usize] += 1;
    }
    m.ants = next.into_iter().filter(|a| occupancy[a.pos as usize] == 1).collect();
}

// ---------------------------------------------------------------- 2. Battle

/// Rules 29-32. Not "biggest army wins": it is decided square by square, and it rewards keeping
/// ants supported.
///
/// For each ant, its **focus** is how many enemy ants have it in range — its own side does not
/// count (rule 30). An ant dies if any enemy in its range has a focus **less than or equal to** its
/// own (rule 31). Every ant is judged at the same moment from the positions after moving, and
/// deaths do not cascade: an ant that dies this turn still counts as an attacker for everyone it
/// was facing (rule 32).
fn battle(m: &mut Match) {
    let n = m.ants.len();
    if n < 2 {
        return;
    }
    // Who each ant is facing. Squared radius, so no square roots (rule 10).
    let mut facing: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            if m.ants[i].owner != m.ants[j].owner
                && m.g.dist2(m.ants[i].pos, m.ants[j].pos) <= ATTACK_RADIUS2
            {
                facing[i].push(j);
                facing[j].push(i);
            }
        }
    }
    let focus: Vec<usize> = facing.iter().map(|f| f.len()).collect();
    // Judged simultaneously, from one snapshot of the focus counts.
    let dead: Vec<bool> = (0..n)
        .map(|i| facing[i].iter().any(|&j| focus[j] <= focus[i]))
        .collect();
    m.ants = m
        .ants
        .iter()
        .zip(&dead)
        .filter(|(_, &d)| !d)
        .map(|(a, _)| *a)
        .collect();
}

// ---------------------------------------------------------------- 3. Raze

/// Rules 41-44. A hill is razed when an enemy ant is standing on it **after the fighting is over**
/// and is still alive there — which is rule 68's consequence: dying on a hill razes nothing.
///
/// Razing scores +2 for the razer and −1 for the owner (rule 43), charged once per hill because a
/// razed hill is never un-razed (rule 42). A player's own ant on its own hill razes nothing (rule
/// 44) — though it blocks spawning there, which is rule 52.
fn raze(m: &mut Match) {
    let mut razings: Vec<(usize, u8, u8)> = Vec::new(); // hill index, owner, razer
    for (hi, h) in m.hills.iter().enumerate() {
        if h.razed {
            continue;
        }
        if let Some(a) = m.ants.iter().find(|a| a.pos == h.pos && a.owner != h.owner) {
            razings.push((hi, h.owner, a.owner));
        }
    }
    for (hi, owner, razer) in razings {
        m.hills[hi].razed = true;
        m.score[razer as usize] += 2;
        m.score[owner as usize] -= 1;
    }
}

// ---------------------------------------------------------------- 4. Spawn

/// Rules 51-55. Each food in the hive becomes one new ant on one of that player's hills.
///
/// A hill can only spawn if no ant is standing on it (rule 52) — parking an ant on your own hill
/// is how you choose which hill your ants come out of. When several are free, the least recently
/// used goes first (rule 53), so a colony spreads rather than piling up. If every hill is blocked
/// or razed the food waits in the hive (rule 54).
fn spawn(m: &mut Match) {
    for seat in 0..m.players {
        loop {
            if m.hive[seat as usize] == 0 {
                break;
            }
            let occupied: Vec<u16> = m.ants.iter().map(|a| a.pos).collect();
            // Least recently used first; ties by position, so two hills last used on the same turn
            // are still ordered the same way on every machine.
            let pick = m
                .hills
                .iter()
                .enumerate()
                .filter(|(_, h)| {
                    !h.razed && h.owner == seat && !occupied.contains(&h.pos)
                })
                .min_by_key(|(_, h)| (h.last_spawn, h.pos))
                .map(|(i, _)| i);
            let Some(hi) = pick else { break }; // rule 54
            m.ants.push(Ant { pos: m.hills[hi].pos, owner: seat });
            m.hills[hi].last_spawn = m.turn + 1;
            m.hive[seat as usize] -= 1;
        }
    }
}

// ---------------------------------------------------------------- 5. Gather

/// Rules 47-50. Food within spawn radius of ants of **exactly one** player goes into that player's
/// hive; food in range of two or more players is destroyed — contested food is wasted food.
///
/// Because this runs after spawning (rule 18), food collected this turn becomes an ant next turn
/// at the earliest. That is rule 69, and it is the most common surprise in the game.
///
/// **Rule 49 is unreachable at the standard settings, and that is worth knowing.** Two ants of
/// different players in spawn range of one food are at most `dist² = 4` apart, and the attack
/// radius² is 5 — so they are always in each other's attack range. Two enemies in range can never
/// both survive: A lives only if every enemy facing it has a strictly greater focus, and B lives
/// only if A does, and both cannot hold. So by the time gathering runs, at most one player has an
/// ant in range and the food is collected or left. The rule is implemented and tested directly,
/// because a configuration with a spawn radius larger than the attack radius would reach it.
pub(crate) fn gather(m: &mut Match) -> u32 {
    let disk = m.g.disk(SPAWN_RADIUS2);
    let mut keep: Vec<u16> = Vec::with_capacity(m.food.len());
    let mut collected: Vec<u16> = vec![0; m.players as usize];

    for &f in &m.food {
        let (fr, fc) = m.g.rc(f);
        let mut claimants: Vec<u8> = Vec::new();
        for (dr, dc) in &disk {
            let p = m.g.at(fr + dr, fc + dc);
            if let Some(a) = m.ants.iter().find(|a| a.pos == p) {
                if !claimants.contains(&a.owner) {
                    claimants.push(a.owner);
                }
            }
        }
        match claimants.len() {
            0 => keep.push(f),                                  // rule 50
            1 => collected[claimants[0] as usize] += 1,         // rule 48
            _ => {}                                             // rule 49: destroyed
        }
    }
    m.food = keep;
    for (seat, n) in collected.iter().enumerate() {
        m.hive[seat] += n;
    }
    collected.iter().map(|&n| n as u32).sum()
}

// ---------------------------------------------------------------- ending

/// Rule 66's two stalemate counters. Razing a hill resets them, so a game that is still being
/// fought is never cut off.
fn update_stalemate(m: &mut Match, collected: u32) {
    let total: usize = m.ants.len();
    let dominating = total > 0
        && (0..m.players).any(|p| m.ants_of(p).count() * 100 >= total * 85)
        && m.hills.iter().any(|h| !h.razed);
    m.domination_turns = if dominating { m.domination_turns + 1 } else { 0 };

    // Food sitting on the map with nobody gathering it.
    //
    // Measured as "nothing was collected this turn while food is on the map", not as "the hive
    // total did not move" -- which was the first version and was wrong in a way that ended every
    // match at exactly 150 turns. The hive is *drained* by spawning, so in any steady state it
    // hovers near zero and its total is unchanged turn after turn while food flows through it
    // perfectly well. What rule 66 is about is gathering, so gathering is what is counted.
    let idle = !m.food.is_empty() && collected == 0;
    m.idle_food_turns = if idle { m.idle_food_turns + 1 } else { 0 };
}

/// Rules 62-67.
fn check_end(m: &mut Match) {
    let living = m.living_players();

    if living.is_empty() {
        m.done = true;
        m.reason = 2; // extermination
        return;
    }
    if living.len() == 1 && m.players > 1 {
        // Rule 60: the last player with living ants takes every enemy hill still standing as
        // though they had razed it.
        let winner = living[0];
        let standing: Vec<usize> = m
            .hills
            .iter()
            .enumerate()
            .filter(|(_, h)| !h.razed && h.owner != winner)
            .map(|(i, _)| i)
            .collect();
        for hi in standing {
            m.hills[hi].razed = true;
            m.score[winner as usize] += 2;
            let owner = m.hills[hi].owner as usize;
            m.score[owner] -= 1;
        }
        m.done = true;
        m.reason = 1; // lone survivor
        return;
    }
    if m.turn >= m.max_turns {
        m.done = true;
        m.reason = 0;
        return;
    }
    if m.domination_turns >= STALEMATE_TURNS {
        m.done = true;
        m.reason = 4;
        return;
    }
    if m.idle_food_turns >= STALEMATE_TURNS {
        m.done = true;
        m.reason = 5;
        return;
    }
    // Rule 65: enough hills have fallen that no remaining player can overtake another. The most
    // anyone can still gain is +2 per standing enemy hill; if that cannot close the gap between
    // the leader and everyone else, the result is already decided.
    let standing_enemy_max: i16 = (0..m.players)
        .map(|p| m.hills.iter().filter(|h| !h.razed && h.owner != p).count() as i16 * 2)
        .max()
        .unwrap_or(0);
    let mut sorted: Vec<i16> = m.score.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    if m.players > 1 && sorted[0] - sorted[1] > standing_enemy_max {
        m.done = true;
        m.reason = 3;
    }
}

/// Ranks, 1-based, ties allowed — `cartridge.md` §1. Highest score wins; equal scores share the
/// rank (rule 61).
pub fn ranks(m: &Match) -> Vec<u16> {
    (0..m.players)
        .map(|p| 1 + m.score.iter().filter(|&&s| s > m.score[p as usize]).count() as u16)
        .collect()
}
