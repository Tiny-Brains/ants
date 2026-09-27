//! What a turn reports beyond its position: the ants that died and who killed them, and the hills
//! that fell. The rules read none of it; a frame writes it out so a viewer can draw a fight.

use super::*;
use crate::turn::{Death, Razing};

/// `(row, col, owner, id)` of an ant, for a comparison that reads at a glance.
fn who(m: &Match, a: &crate::state::Ant) -> (i32, i32, u8, u32) {
    let (r, c) = m.g.rc(a.pos);
    (r, c, a.owner, a.id)
}

fn sorted(m: &Match, ants: &[crate::state::Ant]) -> Vec<(i32, i32, u8, u32)> {
    let mut v: Vec<_> = ants.iter().map(|a| who(m, a)).collect();
    v.sort_unstable();
    v
}

#[test]
fn a_death_names_every_enemy_whose_focus_killed_it() {
    // ...B.     B faces two A's (focus 2); each A faces B alone (focus 1). B dies, killed by both,
    // .A.A.     and both A live. The killers are the enemies the rule reads: those in range whose
    //           focus was no higher than the victim's.
    let mut m = bare(20, 20, 2);
    m.add_ant(m.g.at(1, 1), 0);
    m.add_ant(m.g.at(1, 3), 0);
    m.add_ant(m.g.at(0, 3), 1);
    let ev = step(&mut m, &[Vec::new(), Vec::new()]);

    assert_eq!(ev.deaths.len(), 1, "one ant died");
    let d = &ev.deaths[0];
    assert_eq!(who(&m, &d.ant), (0, 3, 1, 0), "the lone ant, on the square it died on");
    assert_eq!(sorted(&m, &d.by), vec![(1, 1, 0, 0), (1, 3, 0, 1)], "both attackers killed it");
    assert!(ev.razed.is_empty());
    assert_eq!(m.ants.len(), 2, "and both attackers live");
}

#[test]
fn a_one_on_one_kills_both_and_each_names_the_other() {
    let mut m = bare(20, 20, 2);
    m.add_ant(m.g.at(5, 5), 0);
    m.add_ant(m.g.at(5, 7), 1);
    let ev = step(&mut m, &[Vec::new(), Vec::new()]);

    let mut deaths: Vec<_> =
        ev.deaths.iter().map(|d| (who(&m, &d.ant), sorted(&m, &d.by))).collect();
    deaths.sort();
    assert_eq!(
        deaths,
        vec![((5, 5, 0, 0), vec![(5, 7, 1, 0)]), ((5, 7, 1, 0), vec![(5, 5, 0, 0)]),],
        "each died, and each was killed by the other"
    );
    assert!(m.ants.is_empty());
}

#[test]
fn an_enemy_in_range_with_a_higher_focus_is_not_a_killer() {
    // A A B B in a row: the inner ants (focus 2) die; each was killed by BOTH enemies in its range,
    // because the outer enemy's focus (1) and the inner enemy's (2) are both no higher than 2. The
    // outer ants (focus 1) face one enemy of focus 2, which is higher, so they live and kill.
    let mut m = bare(20, 20, 2);
    m.add_ant(m.g.at(0, 0), 0);
    m.add_ant(m.g.at(0, 1), 0);
    m.add_ant(m.g.at(0, 2), 1);
    m.add_ant(m.g.at(0, 3), 1);
    let ev = step(&mut m, &[Vec::new(), Vec::new()]);

    let mut deaths: Vec<_> =
        ev.deaths.iter().map(|d| (who(&m, &d.ant), sorted(&m, &d.by))).collect();
    deaths.sort();
    assert_eq!(
        deaths,
        vec![
            ((0, 1, 0, 1), vec![(0, 2, 1, 0), (0, 3, 1, 1)]),
            ((0, 2, 1, 0), vec![(0, 0, 0, 0), (0, 1, 0, 1)]),
        ],
        "the inner ants die, each killed by both enemies in its range"
    );
    assert_eq!(sorted(&m, &m.ants), vec![(0, 0, 0, 0), (0, 3, 1, 1)], "the outer ants live");
}

#[test]
fn a_collision_is_nobodys_kill() {
    // Two of one colony ordered onto one square: both die, and neither death names a killer.
    let mut m = bare(20, 20, 2);
    m.add_ant(m.g.at(4, 4), 0);
    m.add_ant(m.g.at(4, 6), 0);
    m.add_ant(m.g.at(15, 15), 1);
    let ev = step(&mut m, &[orders(&["E", "W"]), orders(&["-"])]);

    let mut deaths: Vec<_> = ev.deaths.iter().map(|d| (who(&m, &d.ant), d.by.clone())).collect();
    deaths.sort_by_key(|d| d.0);
    assert_eq!(
        deaths,
        vec![((4, 5, 0, 0), Vec::new()), ((4, 5, 0, 1), Vec::new())],
        "both on the square they met on, with no killer"
    );
    assert_eq!(m.ants.len(), 1, "only the bystander is left");
}

#[test]
fn a_razing_names_the_hill_its_owner_and_who_took_it() {
    let mut m = two_sided(20, 20);
    let h1 = m.hills[1].pos;
    m.add_ant(m.g.at(0, 5), 0);
    m.add_ant(h1, 0); // an enemy standing on seat 1's undefended hill
    m.add_ant(m.g.at(3, 3), 1);
    let ev = step(&mut m, &[orders(&["-", "-"]), orders(&["-"])]);

    assert_eq!(ev.razed, vec![Razing { pos: h1, owner: 1, by: 0 }]);
    assert!(m.hills[1].razed);
    assert!(ev.deaths.is_empty());
}

#[test]
fn a_finished_match_reports_nothing() {
    let mut m = two_sided(20, 20);
    m.done = true;
    let ev = step(&mut m, &[Vec::new(), Vec::new()]);
    assert_eq!(ev, crate::turn::Events::default());
    let _: Option<&Death> = ev.deaths.first();
}
