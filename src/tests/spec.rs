//! The specification's own worked fights.
//!
//! Copied from the Focus Battle Resolution page. They are the best conformance corpus the game has,
//! and encoding them by name makes the correspondence checkable by eye.

use super::*;

#[test]
fn spec_scenario_one_on_one_on_one() {
    // ...B.     ...2.     ...x.
    // .A...  -> .2... ->  .x...
    // ...C.     ...2.     ...x.
    // Three colours, all mutually in range, every focus 2: all three die.
    let m = fight(20, 20, 3, &[(1, 1, 0), (0, 3, 1), (2, 3, 2)]);
    assert!(survivors(&m).is_empty(), "all three die");
}

#[test]
fn spec_scenario_ant_sandwich() {
    // A . B . C -> 1.2.1 -> A . x . C
    // The ant in the middle is the only one with its attention split.
    let m = fight(20, 20, 3, &[(0, 0, 0), (0, 2, 1), (0, 4, 2)]);
    assert_eq!(survivors(&m), vec![(0, 0, 0), (0, 4, 2)], "only the centre ant dies");
}

#[test]
fn spec_scenario_one_on_two_on_one() {
    // ...B.     ...3.     ...x.
    // .A.A.  -> .2.2. ->  .A.A.
    // ...C.     ...3.     ...x.
    // Two ants supporting each other kill two enemies and take nothing.
    let m = fight(20, 20, 3, &[(1, 1, 0), (1, 3, 0), (0, 3, 1), (2, 3, 2)]);
    assert_eq!(survivors(&m), vec![(1, 1, 0), (1, 3, 0)], "B and C die, both A live");
}

#[test]
fn spec_scenario_wall_punch() {
    // AAAAAAAAA    013565310    AAxxxxxAA
    // ...BBB...  -> ...555... -> ...xxx...
    // ...BBB...    ...333...    ...xBx...
    //
    // The most discriminating example on the page: the ants on the ends of the wall live because
    // the only enemy that reaches them is far more occupied, and the middle ant of the back rank
    // lives because everything attacking it is more occupied than it is.
    let mut ants: Vec<(i32, i32, u8)> = (0..9).map(|c| (0, c, 0u8)).collect();
    for c in 3..6 {
        ants.push((1, c, 1));
        ants.push((2, c, 1));
    }
    let m = fight(20, 20, 2, &ants);
    assert_eq!(
        survivors(&m),
        vec![(0, 0, 0), (0, 1, 0), (0, 7, 0), (0, 8, 0), (2, 4, 1)],
        "four wall ants and the one ant behind the line"
    );
}

#[test]
fn spec_scenario_bob_and_bill_versus_roy_and_ralph() {
    // ..B....     ..1....     ..B....
    // ..B.R..  -> ..2.2.. ->  ..x.x..
    // ....R..     ....1..     ....R..
    // The two ants whose attention is split die; the two that could concentrate survive.
    let m = fight(20, 20, 2, &[(0, 2, 0), (1, 2, 0), (1, 4, 1), (2, 4, 1)]);
    assert_eq!(survivors(&m), vec![(0, 2, 0), (2, 4, 1)], "Bill and Roy die");
}
