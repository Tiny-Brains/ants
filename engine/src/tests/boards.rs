//! The boards the tests play: the basic boards under `../maps/`, read from disk when a test runs.
//!
//! The component carries none, so a test that needs a real board loads one exactly as every
//! caller now does and hands it to `worldgen` whole. Read at run time rather than compiled in with
//! `include_str!`, so the board `mapgen generate` just wrote is the board the next `cargo test` plays.

use std::path::PathBuf;

use serde_json::Value;

use crate::maps::MapFile;

/// The two-seat basic board, for a test that needs two sides and nothing else.
pub const DUEL: &str = "basic-tiny-2p";

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("maps")
}

/// One basic board as the JSON a caller sends.
pub fn json(id: &str) -> Value {
    let path = dir().join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("{}: {e} -- run `cd mapgen && cargo run -- generate`", path.display())
    });
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One basic board, parsed and validated.
pub fn board(id: &str) -> MapFile {
    MapFile::from_json(&json(id)).unwrap_or_else(|e| panic!("{id}: {} {}", e.code, e.message))
}

/// Every basic board, in id order. Panics on an empty directory: a suite that played no board
/// would pass while proving nothing.
pub fn all() -> Vec<MapFile> {
    let mut ids: Vec<String> = std::fs::read_dir(dir())
        .expect("../maps/ exists")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .collect();
    ids.sort();
    assert!(
        !ids.is_empty(),
        "no boards under ../maps/ -- run `cd mapgen && cargo run -- generate`"
    );
    ids.iter().map(|id| board(id)).collect()
}
