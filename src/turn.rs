//! One turn, in the fixed order the specification's *Turns and Phases* gives: move, attack, raze
//! hills, spawn ants, gather food, spawn food.
//!
//! The order is not an implementation detail. Real consequences fall out of it, and every one is
//! something a player can plan around: food gathered this turn cannot become an ant until the
//! next, because spawning runs before gathering; and an ant that dies on a hill razes nothing,
//! because razing runs after the fighting. A cartridge that resolved these steps in a different
//! order would be a different game with the same rules text.

use crate::map::{dir_of, Bits, ATTACK_RADIUS2, SPAWN_RADIUS2};
use crate::state::{
    spawn_food, Ant, Match, CUTOFF_FOOD, CUTOFF_NONE, CUTOFF_PERCENT, STALEMATE_TURNS,
};

/// Advance one match by one turn. `moves[seat]` is that seat's action array, positionally aligned
/// with `mine(seat)`.
pub fn step(m: &mut Match, moves: &[Vec<String>]) {
    if m.done {
        return;
    }
    let mut deaths = move_ants(m, moves);
    deaths.extend(battle(m));

    // Whether the cutoff counter is allowed to advance this turn, decided *before* razing so it
    // reads the hills as they stood when the ants died. An ant dying on a standing hill that the
    // watched bot does not own means a hill is being contested right now, so a game that looks
    // one-sided is not stalled -- `ants.py:793`, and the specification's "Update :" paragraph on
    // the ants-not-razing-hills cutoff.
    let hill_kill = deaths.iter().any(|&pos| {
        m.hills.iter().any(|h| h.pos == pos && !h.razed && h.owner != m.cutoff_bot)
    });

    raze(m);
    spawn(m);
    gather(m);
    spawn_food(m);

    // 7. What everyone alive can now see, folded into what they know -- *Bot Input*, and the reason
    // `observe.rs` chose known water over visible water: "known water is the only option in which
    // scouting buys anything at all", because a model is a pure function of one observation and
    // has no channel for state between turns.
    //
    // THIS WAS MISSING. `reveal` ran once, in worldgen, and never again -- so `known` was frozen at
    // turn-zero vision for the whole match and exploring recorded nothing. Nothing failed: every
    // test passed, replays re-simulated exactly, and observations were simply smaller than the
    // design measured (38 run-lengths at turn 700 against the 774 by turn 600 that observe.rs
    // documents). A property that is expensive, deliberate, documented and absent is worse than
    // one that was never chosen.
    //
    // After `spawn`, so a newly spawned ant sees from its hill; after `battle`, so an ant that died
    // this turn reveals nothing.
    for pl in 0..m.players {
        m.reveal(pl);
    }

    m.turn += 1;
    update_cutoff(m, hill_kill);
    check_end(m);
}

// ---------------------------------------------------------------- 1. Move

/// *Turns and Phases*, step one, and *Bot Output*.
///
/// An order into water **or into food** is ignored and that ant stays put (*Blocking*); an ant with
/// no order stays (*Bot Output*); and every ant that finishes on a shared square dies, **regardless
/// of owner** — your own two ants walking into each other both die (*Collisions*).
///
/// Food blocking movement is not a detail. `ants.py:610` refuses a destination that is `FOOD` or
/// `WATER` with the same "move blocked", and the specification says so in as many words: "Food will
/// also block an ants movement. This can happen if food spawns next to an ant. Don't move the ant
/// and it will be gathered the next turn." An engine that lets an ant walk onto food puts that ant
/// one square from where every real Ants bot expects it, every turn it happens.
///
/// Returns the squares ants died on, which is what `step` needs to decide the hill-kill stall.
fn move_ants(m: &mut Match, moves: &[Vec<String>]) -> Vec<u16> {
    // Built once per turn rather than scanned per ant: `m.food` is a list, and this is a hot loop.
    let mut blocked = Bits::zeros(m.cells());
    for &f in &m.food {
        blocked.set(f as usize);
    }

    let mut next: Vec<Ant> = Vec::with_capacity(m.ants.len());
    for seat in 0..m.players {
        let mine = m.mine(seat);
        let orders = moves.get(seat as usize);
        for (i, &pos) in mine.iter().enumerate() {
            // A seat that sent nothing holds every ant, which is also what a crashed or timed-out
            // bot looks like from in here: "their ants remain on the board and can still collide
            // and battle with other ants. Their ants just do not make any future moves."
            //
            // The reference also docks a disqualified player a point per un-razed hill
            // (`ants.py:1404`). That is deliberately absent: a cartridge has no notion of a seat
            // being disqualified — the platform owns that, and so owns the score adjustment.
            let order = orders.and_then(|v| v.get(i)).map(String::as_str).unwrap_or("-");
            let dest = match dir_of(order) {
                Some((dr, dc)) => {
                    let (r, c) = m.g.rc(pos);
                    let want = m.g.at(r + dr, c + dc);
                    if m.water.get(want as usize) || blocked.get(want as usize) {
                        pos // *Blocking*, and food blocks exactly as water does
                    } else {
                        want
                    }
                }
                None => pos, // *Bot Output*, and anything that is not a direction
            };
            next.push(Ant { pos: dest, owner: seat });
        }
    }

    // *Collisions*: two or more on a square and all of them die. Counted rather than compared pairwise,
    // so three ants on one square is three deaths and not three pairs.
    let mut occupancy: Vec<u16> = vec![0; m.cells()];
    for a in &next {
        occupancy[a.pos as usize] += 1;
    }
    let dead: Vec<u16> =
        next.iter().filter(|a| occupancy[a.pos as usize] > 1).map(|a| a.pos).collect();
    m.ants = next.into_iter().filter(|a| occupancy[a.pos as usize] == 1).collect();
    dead
}

// ---------------------------------------------------------------- 2. Battle

/// *Focus Battle Resolution*. Not "biggest army wins": it is decided square by square, and it rewards keeping
/// ants supported.
///
/// For each ant, its **focus** is how many enemy ants have it in range — its own side does not
/// count (*Focus Battle Resolution*). An ant dies if any enemy in its range has a focus **less than
/// or equal to** its own (*Focus Battle Resolution*). Every ant is judged at the same moment from
/// the positions after moving, and deaths do not cascade: an ant that dies this turn still counts
/// as an attacker for everyone it was facing (*Focus Battle Resolution*).
fn battle(m: &mut Match) -> Vec<u16> {
    let n = m.ants.len();
    if n < 2 {
        return Vec::new();
    }
    // Who each ant is facing. Squared radius, so no square roots (*Distance*).
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
    let died: Vec<u16> =
        m.ants.iter().zip(&dead).filter(|(_, &d)| d).map(|(a, _)| a.pos).collect();
    m.ants = m
        .ants
        .iter()
        .zip(&dead)
        .filter(|(_, &d)| !d)
        .map(|(a, _)| *a)
        .collect();
    died
}

// ---------------------------------------------------------------- 3. Raze

/// *Hill Razing*. A hill is razed when an enemy ant is standing on it **after the fighting is over**
/// and is still alive there — which is where "dying on a hill razes nothing" comes from.
///
/// Razing scores +2 for the razer and −1 for the owner (*Scoring*), charged once per hill because a
/// razed hill is never un-razed (*Hill Razing*). A player's own ant on its own hill razes nothing —
/// it **touches** the hill instead, which is both what blocks spawning there (*Ant Spawning*) and
/// what pushes that hill to the back of the spawn queue.
fn raze(m: &mut Match) {
    let mut razings: Vec<(usize, u8, u8)> = Vec::new(); // hill index, owner, razer
    let mut touched: Vec<usize> = Vec::new();
    for (hi, h) in m.hills.iter().enumerate() {
        let Some(a) = m.ants.iter().find(|a| a.pos == h.pos) else { continue };
        if a.owner == h.owner {
            touched.push(hi);
        } else if !h.razed {
            razings.push((hi, h.owner, a.owner));
        }
    }
    // Stamped for razed hills too, exactly as the reference does. It changes nothing — a razed
    // hill never spawns — and diverging here would be a difference with no reason behind it.
    for hi in touched {
        m.hills[hi].last_touched = m.turn + 1;
    }
    for (hi, owner, razer) in razings {
        m.hills[hi].razed = true;
        m.score[razer as usize] += 2;
        m.score[owner as usize] -= 1;
        // A hill just fell, so whatever the cutoff counter was watching, this game is still being
        // fought — `ants.py:752`. The counter restarts from zero, not from where it was.
        m.cutoff_turns = 0;
    }
}

// ---------------------------------------------------------------- 4. Spawn

/// *Ant Spawning*. Each food in the hive becomes one new ant on one of that player's hills.
///
/// A hill can only spawn if no ant is standing on it (*Ant Spawning*) — parking an ant on your own hill
/// is how you choose which hill your ants come out of. When several are free, the least recently
/// *touched* goes first (*Ant Spawning*), so a colony spreads rather than piling up. If every hill is
/// blocked or razed the food waits in the hive (*Ant Spawning*).
///
/// The reference breaks ties randomly (`ants.py:701`). This breaks them by position instead:
/// `cartridge.md` §4 makes determinism law, and a match that replayed differently on two hosts
/// would be a worse bug than an ordering nobody can observe.
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
                .min_by_key(|(_, h)| (h.last_touched, h.pos))
                .map(|(i, _)| i);
            let Some(hi) = pick else { break }; // nothing free: the food waits in the hive
            m.ants.push(Ant { pos: m.hills[hi].pos, owner: seat });
            m.hills[hi].last_touched = m.turn + 1;
            m.hive[seat as usize] -= 1;
        }
    }
}

// ---------------------------------------------------------------- 5. Gather

/// *Food Harvesting*. Food within spawn radius of ants of **exactly one** player goes into that player's
/// hive; food in range of two or more players is destroyed — contested food is wasted food.
///
/// Because this runs after spawning (*Turns and Phases*), food collected this turn becomes an ant
/// next turn at the earliest. That is *Ant Spawning*, and it is the most common surprise in the
/// game.
///
/// **The contested-food case is unreachable at the standard settings, and that is worth knowing.**
/// Two ants of
/// different players in spawn range of one food are at most `dist² = 4` apart, and the attack
/// radius² is 5 — so they are always in each other's attack range. Two enemies in range can never
/// both survive: A lives only if every enemy facing it has a strictly greater focus, and B lives
/// only if A does, and both cannot hold. So by the time gathering runs, at most one player has an
/// ant in range and the food is collected or left. The rule is implemented and tested directly,
/// because a configuration with a spawn radius larger than the attack radius would reach it.
pub(crate) fn gather(m: &mut Match) {
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
            0 => keep.push(f),                          // nobody in range: it stays
            1 => collected[claimants[0] as usize] += 1,  // one player in range: harvested
            _ => {}                                      // contested: destroyed
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
/// **This is one counter watching one holder, not two counters running in parallel.** The holder is
/// whichever seat — or loose food, as a pseudo-seat — holds at least `CUTOFF_PERCENT` of the whole
/// population. The population is every live ant, plus the hive of every player that still has a
/// hill standing, plus every food on the map. When the holder changes the count restarts at one;
/// when nobody holds the share it drops to zero.
///
/// The earlier version of this function got both halves wrong in ways worth recording, because
/// both looked reasonable. The domination test divided by the ant count alone, so a player whose
/// lead sat in the hive rather than on the board never triggered it. And the food test asked
/// "did anyone gather this turn?", which is not a share of anything: it fired during any 150-turn
/// lull between two healthy colonies, which is a live game, not a stalemate.
fn update_cutoff(m: &mut Match, hill_kill: bool) {
    let mut pop: Vec<u32> = vec![0; m.players as usize];
    for a in &m.ants {
        pop[a.owner as usize] += 1;
    }
    // Once per standing hill, which is what the reference does: a player holding two hills counts
    // their hive twice. It matters only on a multi-hill board, and matching is free.
    for h in &m.hills {
        if !h.razed {
            pop[h.owner as usize] += m.hive[h.owner as usize] as u32;
        }
    }
    let food = m.food.len() as u32;
    let total: u32 = pop.iter().sum::<u32>() + food;

    // At 85% at most one entity can hold the share, so there is no order to fix and no tie to
    // break. `total == 0` means no ants and no food, which `check_end` has already called
    // extermination before this can be read.
    let holder = if total == 0 {
        None
    } else {
        (0..m.players)
            .find(|&p| pop[p as usize] as u64 * 100 >= total as u64 * CUTOFF_PERCENT as u64)
            .or(if food as u64 * 100 >= total as u64 * CUTOFF_PERCENT as u64 {
                Some(CUTOFF_FOOD)
            } else {
                None
            })
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

/// *Cutoff Rules*, as `is_rank_stabilized` (`ants.py:1355`): can anyone still change the finishing order?
///
/// Only a player who is **alive and still holds a hill** is given the chance — the specification is
/// explicit that "bots without hills left could still possibly gain in rank, [but] the game is not
/// extended [for] them". That player is assumed to raze every enemy hill still standing; each
/// opponent is assumed to lose every hill it still holds. If that best case lets anyone catch an
/// opponent they are behind, or pass one they are level with, the game goes on.
fn rank_stabilized(m: &Match) -> bool {
    for p in 0..m.players {
        if !m.alive(p) || !m.hills.iter().any(|h| !h.razed && h.owner == p) {
            continue;
        }
        let best = m.score[p as usize]
            + m.hills.iter().filter(|h| !h.razed && h.owner != p).count() as i16 * 2;
        for o in 0..m.players {
            if o == p {
                continue;
            }
            let worst = m.score[o as usize]
                - m.hills.iter().filter(|h| !h.razed && h.owner == o).count() as i16;
            let (sp, so) = (m.score[p as usize], m.score[o as usize]);
            // Behind and able to reach level is enough: drawing level is a rank change.
            if (sp < so && best >= worst) || (sp == so && best > worst) {
                return false;
            }
        }
    }
    true
}

/// *Cutoff Rules*.
///
/// The order is the reference's (`ants.py:1379`), and the turn limit comes **last**: there it is
/// the loop bound rather than a condition, and `finish_game` labels a match "turn limit reached"
/// only when nothing else already claimed it. Checking it first, as this used to, reported
/// `turn_limit` for matches the reference would have called a cutoff.
fn check_end(m: &mut Match) {
    let living = m.living_players();

    if living.is_empty() {
        m.done = true;
        m.reason = 2; // extermination
        return;
    }
    if living.len() == 1 && m.players > 1 {
        // *Scoring*: the last player with living ants takes every enemy hill still standing as
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
    if m.cutoff_turns >= STALEMATE_TURNS {
        m.done = true;
        m.reason = if m.cutoff_bot == CUTOFF_FOOD { 5 } else { 4 };
        return;
    }
    if m.players > 1 && rank_stabilized(m) {
        m.done = true;
        m.reason = 3;
        return;
    }
    if m.turn >= m.max_turns {
        m.done = true;
        m.reason = 0;
    }
}

/// Ranks, 1-based, ties allowed — `cartridge.md` §1. Highest score wins; equal scores share the
/// rank (*Ranking*).
pub fn ranks(m: &Match) -> Vec<u16> {
    (0..m.players)
        .map(|p| 1 + m.score.iter().filter(|&&s| s > m.score[p as usize]).count() as u16)
        .collect()
}
