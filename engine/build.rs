//! Compile every board under `../maps/` into the component.
//!
//! A cartridge imports nothing -- no filesystem, no clock, no sockets -- so a board cannot be read at
//! run time. This writes one `include_str!` per file into `OUT_DIR`, which `src/maps.rs` includes
//! as `MAPS`. Cargo re-runs it whenever anything under `../maps/` changes, so `cargo test` always
//! sees the boards that are committed rather than the ones some earlier step happened to embed.
//!
//! The boards sit at the repository root rather than in this crate because they are content, not
//! source: `mapgen/` writes them, `dist/maps/` ships them, and the CLI exports copies of them.
//!
//! It is also what makes a map edit an engine-digest change, and therefore refused while a season
//! is live, on the same rails a rules change already runs on.
//!
//! **An empty `maps/` builds, with a warning.** `mapgen` links this crate to validate what it writes,
//! so refusing to build without boards would leave no way to make the first ones. The refusal that
//! matters is at packaging: `tools/package.py` will not write a cartridge with no boards in it, and
//! `every_committed_map_is_valid_and_symmetric` fails on an empty catalogue.

use std::path::Path;
use std::{env, fs};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let dir = Path::new(&manifest_dir).parent().expect("the crate is engine/").join("maps");
    println!("cargo::rerun-if-changed={}", dir.display());

    // Sorted, so the table -- and so the component -- does not depend on directory order.
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        println!(
            "cargo::warning=no boards in maps/ -- make them with `cd mapgen && cargo run -- generate`"
        );
    }

    let rows: String = names
        .iter()
        .map(|n| {
            let file = dir.join(format!("{n}.json"));
            format!("    ({n:?}, include_str!({:?})),\n", file.display().to_string())
        })
        .collect();
    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets it")).join("maps.rs");
    fs::write(out, format!("[\n{rows}]\n")).expect("OUT_DIR is writable");
}
