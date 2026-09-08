//! Write one map file from the procedural generator.
//!
//! Maps used to be grown at the start of every match; they are now content, and this is the
//! factory that produces them. Same idiom as `manifest.rs` beside it: a host binary whose output
//! is committed, so an artifact nobody hand-edits cannot drift from the code that made it.
//!
//! ```sh
//! cargo run --bin mapgen -- --preset cell --seed 7 --id cell-07 > maps/cell-07.json
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| -> Option<String> {
        args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
    };
    let preset = get("--preset").unwrap_or_else(|| "standard".to_string());
    let seed: u64 = get("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let id = get("--id").unwrap_or_else(|| format!("{preset}-{seed}"));

    match tb_ants::generate_map(&preset, seed, &id) {
        Some(v) => println!("{}", serde_json::to_string(&v).expect("a map serialises")),
        None => {
            eprintln!("mapgen: no preset '{preset}'");
            std::process::exit(2);
        }
    }
}
