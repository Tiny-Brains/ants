//! The generator, checked: every seat count it promises, the determinism `check` rests on, the
//! connectivity knob doing what it says, and the committed boards being what their recipes make.

use std::path::Path;

use crate::measure::measure;
use crate::recipe::Recipe;
use crate::set::{from_json, make_one, make_set};

/// A recipe for `seats` on a board that divides by them, in one of two styles: scattered warped
/// areas with caverns inside, or a lattice maze.
fn recipe(seats: u8, maze: bool, loops: u32) -> Recipe {
    let side = 24 * seats as u32;
    let areas = if maze {
        "size = [9, 9]\ngrid = true\nclosure_pct = 100\nwall = 1".to_string()
    } else {
        "size = [80, 160]\nwarp = 3\ncoverage_pct = 80\nclosure_pct = 50\nwall = 2".to_string()
    };
    let fill = if maze { "" } else { "[fill]\nstyle = \"cellular\"\npct = 35\n" };
    let hills = if maze { "room = true\nhome_doors_min = 2" } else { "per_seat = 2" };
    Recipe::parse(&format!(
        r#"
[preset]
name = "test-{seats}"
seats = {seats}
count = 1
seed = 7

[board]
rows = {side}
cols = {side}
shifts = ["diagonal"]

[areas]
{areas}

[connectivity]
loops_pct = {loops}
door_min = {door}

{fill}
[hills]
{hills}

[food]
per_seat = 6
bootstrap = 2
"#,
        door = if maze { 1 } else { 2 },
    ))
    .unwrap_or_else(|e| panic!("the test recipe for {seats} seats: {e}"))
}

#[test]
fn every_seat_count_from_two_to_eight_makes_a_fair_board_the_engine_accepts() {
    // `make_one` measures every board (whole orbits, one body of land, congruent seats, no enemy
    // hill in view) and passes it through `tb.ants.worldgen` before it returns one.
    for seats in 2..=8u8 {
        for maze in [false, true] {
            let r = recipe(seats, maze, 20);
            let made =
                make_one(&r, 0).unwrap_or_else(|e| panic!("{seats} seats, maze {maze}: {e}"));
            let per_seat = r.hills.per_seat as usize;
            assert_eq!(made.board.s.seats, seats as usize);
            assert_eq!(made.board.hills.len(), seats as usize * per_seat, "{seats} seats");
            assert_eq!(made.metrics.food_per_seat, 6);
            assert!(made.metrics.routes >= 1, "{seats} seats, maze {maze}: no route between seats");
        }
    }
}

#[test]
fn a_recipe_and_a_seed_make_the_same_bytes_every_time() {
    // What `check` rests on: regenerating a committed set must reproduce it exactly.
    let r = recipe(2, false, 30);
    assert_eq!(make_one(&r, 0).unwrap().text, make_one(&r, 0).unwrap().text);
    assert_ne!(
        make_one(&r, 0).unwrap().text,
        make_one(&r, 1).unwrap().text,
        "boards of a set differ"
    );
}

#[test]
fn more_loops_make_more_routes_between_seats() {
    // The connectivity knob: a tree of doors leaves few square-disjoint routes between one seat's
    // home and the next; opening every door leaves many. Summed over several boards, because one
    // board's routes depend on where its homes landed.
    let routes = |loops: u32| -> u32 {
        let mut r = recipe(2, true, loops);
        r.preset.count = 4;
        make_set(&r).unwrap().iter().map(|m| m.metrics.routes).sum()
    };
    let (tree, open) = (routes(0), routes(100));
    assert!(tree >= 4, "every board keeps at least one route: {tree}");
    assert!(open > tree * 2, "loops_pct = 100 gave {open} routes against {tree} for a tree");
}

#[test]
fn a_recipe_that_cannot_be_honoured_is_refused_before_a_board_is_drawn() {
    let base = |edit: &str| {
        format!(
            r#"
[preset]
name = "bad"
seats = 2
count = 1
seed = 1
[board]
rows = 48
cols = 48
shifts = ["diagonal"]
[areas]
size = [60, 120]
wall = 1
[food]
per_seat = 4
{edit}
"#
        )
    };
    let refused = |edit: &str, why: &str| {
        let text = base(edit);
        let e = Recipe::parse(&text).err().unwrap_or_else(|| panic!("accepted: {edit}"));
        assert!(e.contains(why), "{edit}: expected '{why}' in: {e}");
    };
    // The design's rules, not tuning: a clearing of at least two, seats from two to eight, walls
    // that can close a boundary, and no enemy hill in view at turn zero.
    refused("[hills]\nclearing = 1", "clearing");
    let wallless = base("[connectivity]\nloops_pct = 50").replace("wall = 1", "wall = 0");
    assert!(Recipe::parse(&wallless).unwrap_err().contains("must be 100"));
    let nine = base("").replace("seats = 2", "seats = 9");
    assert!(Recipe::parse(&nine).unwrap_err().contains("2 to 8"));
    let near = base("").replace("rows = 48", "rows = 16").replace("\"diagonal\"", "\"rows\"");
    assert!(Recipe::parse(&near).unwrap_err().contains("view radius"));
    // And knobs that contradict each other.
    let grid = base("").replace("size = [60, 120]", "size = [49, 49]\ngrid = true");
    assert!(Recipe::parse(&grid).unwrap_err().contains("lattice"));
    let overfed = base("").replace("per_seat = 4", "per_seat = 4\nbootstrap = 5");
    assert!(Recipe::parse(&overfed).unwrap_err().contains("bootstrap"));
    refused("[fill]\npct = 70", "fill.pct");
    let unknown = base("[areas.extra]\nx = 1");
    assert!(Recipe::parse(&unknown).is_err(), "an unknown knob is a typo, not a no-op");
}

#[test]
fn the_committed_boards_are_what_their_recipes_make() {
    // `mapgen check` as a test, so `cargo test` alone catches a hand-edited board or a generator
    // change nobody regenerated for.
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut recipes: Vec<_> = std::fs::read_dir(here.join("recipes"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    recipes.sort();
    assert!(!recipes.is_empty());
    for path in recipes {
        let r = Recipe::load(&path).unwrap();
        for made in make_set(&r).unwrap() {
            let file = here.join("../maps").join(format!("{}.json", made.id));
            let text = std::fs::read_to_string(&file).unwrap_or_else(|_| {
                panic!("{} is missing -- run `mapgen generate`", file.display())
            });
            assert!(text == made.text, "{} is not what its recipe makes", made.id);
            let back = from_json(&serde_json::from_str(&text).unwrap()).unwrap();
            assert_eq!(
                measure(&back).unwrap(),
                made.metrics,
                "{}: the metrics it records",
                made.id
            );
        }
    }
}

#[test]
fn a_congruent_match_stays_congruent_on_every_seat_count_and_the_check_catches_one_that_does_not() {
    // The engine and the board, together: one frame-relative policy in every seat keeps every view
    // equal to seat 0's moved by the shift, to the end, and the replay agrees. Then the same match
    // with one seat's first ant told to do otherwise on turn 3 must be caught.
    for seats in [2u8, 3, 5, 8] {
        let r = recipe(seats, seats % 2 == 1, 30);
        let made = make_one(&r, 0).unwrap();
        let map: serde_json::Value = serde_json::from_str(&made.text).unwrap();
        let c = crate::play::congruent(&map, 11, 120).unwrap_or_else(|e| {
            panic!("{seats} seats, shift {:?}: {e}", (made.board.s.dr, made.board.s.dc))
        });
        assert!(c.turns > 3, "{seats} seats: the match must be played");

        let caught = crate::play::congruent_with(&map, 11, 120, Some((3, seats as usize - 1)));
        let e = caught.expect_err("a seat that played differently went unnoticed");
        assert!(e.contains("turn 4") && e.contains("not seat 0's"), "{seats} seats: {e}");
    }
}
