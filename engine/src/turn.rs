//! One turn, in the order the specification gives: move, attack, raze hills, spawn ants, gather
//! food, spawn food.
//!
//! The order is not an implementation detail. Food gathered this turn cannot become an ant until
//! the next, because spawning runs before gathering; an ant that dies on a hill razes nothing,
//! because razing runs after the fighting. A cartridge that resolved these steps in a different
//! order would be a different game with the same rules text.

use crate::food;
use crate::grid::{ATTACK_RADIUS2, Bits, SPAWN_RADIUS2, dir_of};
use crate::state::{Ant, CUTOFF_FOOD, CUTOFF_NONE, CUTOFF_PERCENT, Match, STALEMATE_TURNS};

/// One ant's death, where it happened.
///
/// `ant` is the ant as it stood after moving -- the square it died on, its owner and its id -- and
/// `by` the enemies that killed it: in a battle, every enemy in range whose focus was no higher
/// than its own, each as it stood at the same moment, and dead or alive by the end of the turn.
/// A collision has no killer, so `by` is empty. The position after the fight no longer shows any
/// of this, which is why a frame carries it (`replay.rs`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Death {
    pub ant: Ant,
    pub by: Vec<Ant>,
}

/// A hill that fell this turn: where it stood, whose it was, and the seat that razed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Razing {
    pub pos: u16,
    pub owner: u8,
    pub by: u8,
}

/// What a turn did that the position after it no longer shows. Returned by `step` for whoever
/// records a match turn by turn; the rules read none of it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub deaths: Vec<Death>,
    pub razed: Vec<Razing>,
}

/// Advance one match by one turn. `moves[seat]` is that seat's action array, positionally aligned
/// with `mine(seat)`. Returns what the turn did that its position no longer shows.
pub fn step(m: &mut Match, moves: &[Vec<String>]) -> Events {
    if m.done {
        return Events::default();
    }
    let mut deaths = move_ants(m, moves);
    deaths.extend(battle(m));

    // Decided before razing, so it reads the hills as they stood when the ants died: an ant dying
    // on a standing hill the watched bot does not own means a hill is being contested right now, so
    // a game that looks one-sided is not stalled (`ants.py:793`).
    let hill_kill = deaths
        .iter()
        .any(|d| m.hills.iter().any(|h| h.pos == d.ant.pos && !h.razed && h.owner != m.cutoff_bot));

    let mut razed = raze(m);
    spawn(m);
    gather(m);
    food::spawn(m);

    // What everyone can now see, folded into what they know. A model may have no memory of its
    // own, so the engine remembers on its behalf or scouting buys nothing — `observe.rs`.
    // After `spawn`, so a new ant sees from its hill; after `battle`, so an ant that died this turn
    // reveals nothing.
    for pl in 0..m.players {
        m.reveal(pl);
    }

    m.turn += 1;
    update_cutoff(m, hill_kill);
    razed.extend(check_end(m));
    Events { deaths, razed }
}

/// An order into water **or into food** is ignored and that ant stays put; an ant with no order
/// stays; and every ant that finishes on a shared square dies, regardless of owner — your own two
/// ants walking into each other both die.
///
/// Food blocking movement is not a detail. `ants.py:610` refuses a destination that is `FOOD` or
/// `WATER` with the same "move blocked", and the specification says so in as many words. An engine
/// that lets an ant walk onto food puts that ant one square from where every real Ants bot expects
/// it, every turn it happens.
///
/// Each ant takes its id to wherever it ends up, a blocked or held ant included: the ant is rebuilt
/// from `mine_ids`, not from `mine`, whose squares alone would lose which ant stood on each. Two
/// that collide die with their ids, and the ids are never handed out again.
///
/// Returns the ants that died, with no killer: a collision is nobody's kill.
fn move_ants(m: &mut Match, moves: &[Vec<String>]) -> Vec<Death> {
    // Built once per turn rather than scanned per ant: `m.food` is a list, and this is a hot loop.
    let mut blocked = Bits::zeros(m.cells());
    for &f in &m.food {
        blocked.set(f as usize);
    }

    let mut next: Vec<Ant> = Vec::with_capacity(m.ants.len());
    for seat in 0..m.players {
        let orders = moves.get(seat as usize);
        for (i, &(pos, id)) in m.mine_ids(seat).iter().enumerate() {
            // A seat that sent nothing holds every ant, which is also what a crashed or timed-out
            // bot looks like from in here. The reference docks a disqualified player a point per
            // un-razed hill (`ants.py:1404`); that is deliberately absent, because a cartridge has
            // no notion of a seat being disqualified — the platform owns that.
            let order = orders.and_then(|v| v.get(i)).map(String::as_str).unwrap_or("-");
            let dest = match dir_of(order) {
                Some((dr, dc)) => {
                    let (r, c) = m.g.rc(pos);
                    let want = m.g.at(r + dr, c + dc);
                    if m.water.get(want as usize) || blocked.get(want as usize) {
                        pos
                    } else {
                        want
                    }
                }
                None => pos,
            };
            next.push(Ant { pos: dest, owner: seat, id });
        }
    }

    // Counted rather than compared pairwise, so three ants on one square is three deaths and not
    // three pairs.
    let mut occupancy: Vec<u16> = vec![0; m.cells()];
    for a in &next {
        occupancy[a.pos as usize] += 1;
    }
    let dead: Vec<Death> = next
        .iter()
        .filter(|a| occupancy[a.pos as usize] > 1)
        .map(|&a| Death { ant: a, by: Vec::new() })
        .collect();
    m.ants = next.into_iter().filter(|a| occupancy[a.pos as usize] == 1).collect();
    dead
}

/// Focus battle resolution: decided square by square, not "biggest army wins".
///
/// For each ant, its **focus** is how many enemy ants have it in range; its own side does not
/// count. An ant dies if any enemy in its range has a focus less than or equal to its own. Every
/// ant is judged at the same moment from the positions after moving, and deaths do not cascade: an
/// ant that dies this turn still counts as an attacker for everyone it was facing.
///
/// A death names its killers: the enemies in range whose focus was no higher than the victim's,
/// which is exactly the set the rule reads. An enemy in range with a higher focus did not kill it.
fn battle(m: &mut Match) -> Vec<Death> {
    let n = m.ants.len();
    if n < 2 {
        return Vec::new();
    }
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
    // Who killed an ant and whether it died are one question, not two: it dies exactly when that
    // set is not empty. Asked once, so only an ant that dies allocates -- in a big fight this runs
    // for every ant on the board.
    let mut died: Vec<Death> = Vec::new();
    let mut dead = vec![false; n];
    for i in 0..n {
        let by: Vec<Ant> =
            facing[i].iter().filter(|&&j| focus[j] <= focus[i]).map(|&j| m.ants[j]).collect();
        if !by.is_empty() {
            dead[i] = true;
            died.push(Death { ant: m.ants[i], by });
        }
    }
    m.ants = m.ants.iter().zip(&dead).filter(|&(_, &d)| !d).map(|(a, _)| *a).collect();
    died
}

/// A hill is razed when an enemy ant is standing on it after the fighting is over and is still
/// alive there — which is where "dying on a hill razes nothing" comes from.
///
/// Razing scores +2 for the razer and −1 for the owner, charged once per hill because a razed hill
/// is never un-razed. A player's own ant on its own hill razes nothing; it **touches** the hill,
/// which is both what blocks spawning there and what pushes that hill to the back of the queue.
///
/// Returns the hills that fell, for the record of the turn.
fn raze(m: &mut Match) -> Vec<Razing> {
    let mut razings: Vec<(usize, u8, u8)> = Vec::new(); // hill, owner, razer
    let mut touched: Vec<usize> = Vec::new();
    for (hi, h) in m.hills.iter().enumerate() {
        let Some(a) = m.ants.iter().find(|a| a.pos == h.pos) else {
            continue;
        };
        if a.owner == h.owner {
            touched.push(hi);
        } else if !h.razed {
            razings.push((hi, h.owner, a.owner));
        }
    }
    // Stamped for razed hills too, exactly as the reference does: it changes nothing, and diverging
    // here would be a difference with no reason behind it.
    for hi in touched {
        m.hills[hi].last_touched = m.turn + 1;
    }
    let mut fell = Vec::with_capacity(razings.len());
    for (hi, owner, razer) in razings {
        m.hills[hi].razed = true;
        m.score[razer as usize] += 2;
        m.score[owner as usize] -= 1;
        // A hill just fell, so this game is still being fought (`ants.py:752`). The cutoff counter
        // restarts from zero, not from where it was.
        m.cutoff_turns = 0;
        fell.push(Razing { pos: m.hills[hi].pos, owner, by: razer });
    }
    fell
}

/// Each food in the hive becomes one new ant on one of that player's hills, taking that seat's
/// next id.
///
/// A hill can only spawn if no ant is standing on it — parking an ant on your own hill is how you
/// choose which hill your ants come out of. When several are free the least recently *touched* goes
/// first, so a colony spreads rather than piling up. If every hill is blocked or razed the food
/// waits in the hive.
///
/// The reference breaks ties randomly (`ants.py:701`); this breaks them by the hill's place in the
/// board's list, because seeds and actions must determine the match, and a match that replayed
/// differently on two hosts would be a worse bug than an ordering nobody can observe.
///
/// **By place in the list, never by square.** A board lists its hills in orbits, so seat `k`'s `j`th
/// hill is seat 0's `j`th moved by the shift. Ordering by square is not kept by a shift — where the
/// board wraps, seat 0's lower-numbered hill is not the image of seat 1's — so on a board with two
/// hills a seat, breaking ties by square spawned one seat's ants from the hill that corresponds to
/// the other seat's *other* hill, and the seats stopped playing the same board.
fn spawn(m: &mut Match) {
    for seat in 0..m.players {
        while m.hive[seat as usize] > 0 {
            let occupied: Vec<u16> = m.ants.iter().map(|a| a.pos).collect();
            let pick = m
                .hills
                .iter()
                .enumerate()
                .filter(|(_, h)| !h.razed && h.owner == seat && !occupied.contains(&h.pos))
                .min_by_key(|&(i, h)| (h.last_touched, i))
                .map(|(i, _)| i);
            let Some(hi) = pick else { break }; // nothing free: the food waits in the hive
            m.add_ant(m.hills[hi].pos, seat);
            m.hills[hi].last_touched = m.turn + 1;
            m.hive[seat as usize] -= 1;
        }
    }
}

/// Food within spawn radius of ants of **exactly one** player goes into that player's hive; food in
/// range of two or more players is destroyed.
///
/// Because this runs after spawning, food collected this turn becomes an ant next turn at the
/// earliest — the most common surprise in the game.
///
/// The contested case is unreachable at the standard radii, and that is worth knowing. Two ants of
/// different players in spawn range of one food are at most `dist² = 4` apart and the attack radius²
/// is 5, so they are always in each other's attack range, and two enemies in range can never both
/// survive. It is implemented and tested directly because a configuration with a spawn radius larger
/// than the attack radius would reach it.
pub(crate) fn gather(m: &mut Match) {
    let disk = m.g.disk(SPAWN_RADIUS2);
    let mut keep: Vec<u16> = Vec::with_capacity(m.food.len());
    let mut collected: Vec<u16> = vec![0; m.players as usize];

    for &f in &m.food {
        let (fr, fc) = m.g.rc(f);
        let mut claimants: Vec<u8> = Vec::new();
        for (dr, dc) in &disk {
            let p = m.g.at(fr + dr, fc + dc);
            if let Some(a) = m.ants.iter().find(|a| a.pos == p)
                && !claimants.contains(&a.owner)
            {
                claimants.push(a.owner);
            }
        }
        match claimants.len() {
            0 => keep.push(f),                          // nobody in range: it stays
            1 => collected[claimants[0] as usize] += 1, // one player in range: harvested
            _ => {}                                     // contested: destroyed
        }
    }
    m.food = keep;
    for (seat, n) in collected.iter().enumerate() {
        m.hive[seat] += n;
    }
}

// ---------------------------------------------------------------- ending

/// The one cutoff counter, in the shape the reference engine gives it (`ants.py:1499`).
///
/// One counter watching one holder, not two running in parallel. The holder is whichever seat — or
/// loose food, as a pseudo-seat — holds at least `CUTOFF_PERCENT` of the whole population, which is
/// every live ant, plus the hive of every player that still has a hill standing, plus every food on
/// the map. When the holder changes the count restarts at one; when nobody holds the share it drops
/// to zero.
fn update_cutoff(m: &mut Match, hill_kill: bool) {
    let mut pop: Vec<u32> = vec![0; m.players as usize];
    for a in &m.ants {
        pop[a.owner as usize] += 1;
    }
    // Once per standing hill, as the reference does: a player holding two hills counts their hive
    // twice. It matters only on a multi-hill board, and matching is free.
    for h in &m.hills {
        if !h.razed {
            pop[h.owner as usize] += m.hive[h.owner as usize] as u32;
        }
    }
    let food = m.food.len() as u32;
    let total: u32 = pop.iter().sum::<u32>() + food;
    let holds = |share: u32| share as u64 * 100 >= total as u64 * CUTOFF_PERCENT as u64;

    // At 85% at most one entity can hold the share, so there is no order to fix and no tie to
    // break. `total == 0` means no ants and no food, which `check_end` has already called
    // extermination before this can be read.
    let holder = if total == 0 {
        None
    } else {
        (0..m.players)
            .find(|&p| holds(pop[p as usize]))
            .or_else(|| holds(food).then_some(CUTOFF_FOOD))
    };

    match holder {
        Some(who) if who == m.cutoff_bot => {
            if !hill_kill {
                m.cutoff_turns += 1;
            }
        }
        Some(who) => {
            m.cutoff_bot = who;
            m.cutoff_turns = 1;
        }
        None => {
            m.cutoff_bot = CUTOFF_NONE;
            m.cutoff_turns = 0;
        }
    }
}

/// `is_rank_stabilized` (`ants.py:1355`): can anyone still change the finishing order?
///
/// Only a player who is alive and still holds a hill is given the chance — "bots without hills left
/// could still possibly gain in rank, [but] the game is not extended [for] them". That player is
/// assumed to raze every enemy hill still standing; each opponent is assumed to lose every hill it
/// still holds. If that best case lets anyone catch an opponent they are behind, or pass one they
/// are level with, the game goes on.
fn rank_stabilized(m: &Match) -> bool {
    let standing = |owner: u8, mine: bool| {
        m.hills.iter().filter(|h| !h.razed && (h.owner == owner) == mine).count() as i16
    };
    for p in 0..m.players {
        if !m.alive(p) || standing(p, true) == 0 {
            continue;
        }
        let best = m.score[p as usize] + standing(p, false) * 2;
        for o in 0..m.players {
            if o == p {
                continue;
            }
            let worst = m.score[o as usize] - standing(o, true);
            let (sp, so) = (m.score[p as usize], m.score[o as usize]);
            // Behind and able to reach level is enough: drawing level is a rank change.
            if (sp < so && best >= worst) || (sp == so && best > worst) {
                return false;
            }
        }
    }
    true
}

/// The end conditions, in the reference's order (`ants.py:1379`).
///
/// The turn limit comes **last**: there it is the loop bound rather than a condition, and
/// `finish_game` labels a match "turn limit reached" only when nothing else already claimed it.
///
/// Returns the hills the lone-survivor rule took: they are razed and scored as any other, so the
/// turn's record lists them with the rest.
fn check_end(m: &mut Match) -> Vec<Razing> {
    let living = m.living_players();

    if living.is_empty() {
        m.done = true;
        m.reason = 2; // extermination
        return Vec::new();
    }
    if living.len() == 1 && m.players > 1 {
        // The last player with living ants takes every enemy hill still standing as though they
        // had razed it.
        let winner = living[0];
        let mut taken = Vec::new();
        for hi in 0..m.hills.len() {
            if m.hills[hi].razed || m.hills[hi].owner == winner {
                continue;
            }
            m.hills[hi].razed = true;
            m.score[winner as usize] += 2;
            m.score[m.hills[hi].owner as usize] -= 1;
            taken.push(Razing { pos: m.hills[hi].pos, owner: m.hills[hi].owner, by: winner });
        }
        m.done = true;
        m.reason = 1; // lone survivor
        return taken;
    }
    if m.cutoff_turns >= STALEMATE_TURNS {
        m.done = true;
        m.reason = if m.cutoff_bot == CUTOFF_FOOD { 5 } else { 4 };
        return Vec::new();
    }
    if m.players > 1 && rank_stabilized(m) {
        m.done = true;
        m.reason = 3;
        return Vec::new();
    }
    if m.turn >= m.max_turns {
        m.done = true;
        m.reason = 0;
    }
    Vec::new()
}

/// Ranks, 1-based, ties allowed. Highest score wins; equal scores share the rank.
pub fn ranks(m: &Match) -> Vec<u16> {
    (0..m.players)
        .map(|p| 1 + m.score.iter().filter(|&&s| s > m.score[p as usize]).count() as u16)
        .collect()
}
