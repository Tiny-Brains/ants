//! Emit `cartridge.json`, the registration manifest.
//!
//! Generated from the preset table rather than hand-written, because the two must not be able to
//! disagree: the manifest is read once at registration and decides how many seats a preset is
//! played at, and a manifest that said something the engine refuses would fail at pairing time, per
//! match, in production.
//!
//! `tools/cartridge.py` folds the board catalogue and about.json into what this prints.
fn main() {
    let presets: Vec<serde_json::Value> = tb_ants::presets()
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "players": p.players }))
        .collect();
    let m = serde_json::json!({
        "game": "ants", "version": "1.0.0", "abi": 1,
        // No top-level `players`: the preset carries it, because seats are a property of the map.
        "presets": presets,
        "limits":  { "max_turns": tb_ants::MAX_TURNS, "turn_ms": 1000 },
        // One budget, and it prices marshalling. There is no compute cap: it was removed on
        // 10 September 2026 (devops decision 46) after measurement showed it shadowed by S below
        // and by turn_ms above, and unable to catch the exploit it was introduced for. The turn
        // deadline is the compute bound, and the loader gives each row its own share of it.
        "budgets": {
            // Measured, not argued: a reference six-plane adapter costs 197,272 operations against
            // a real worst-case observation, and a visibility-deriving one did not fit 200,000 at
            // all. See axon/tests/ants_adapter.rs.
            "adapter_ops_max": 1000000
        }
    });
    println!("{}", serde_json::to_string_pretty(&m).unwrap());
}
