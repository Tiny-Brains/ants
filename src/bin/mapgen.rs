//! Write one map file from the procedural generator.
//!
//! Maps used to be grown at the start of every match; they are content now, and this is the factory
//! that produces them. Same idiom as `manifest.rs` beside it: a host binary whose output is
//! committed, so an artifact nobody hand-edits cannot drift from the code that made it.
//!
//!     cargo run --bin mapgen -- --preset cell --seed 7 --id cell-07 > maps/cell-07.json
mod args;

fn main() {
    let a = args::Args::new();
    let preset = a.get("--preset").unwrap_or_else(|| "standard".to_string());
    let seed: u64 = a.get("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let id = a.get("--id").unwrap_or_else(|| format!("{preset}-{seed}"));

    match tb_ants::generate_map(&preset, seed, &id) {
        Some(v) => println!("{}", serde_json::to_string(&v).expect("a map serialises")),
        None => {
            eprintln!("mapgen: no preset '{preset}'");
            std::process::exit(2);
        }
    }
}
