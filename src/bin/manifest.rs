//! Emit `cartridge.json`, the registration manifest — `PROTOCOL.md` §4.
//!
//! Generated from the preset table rather than hand-written, because the two must not be able to
//! disagree: the manifest is read once at registration and decides how many seats a preset is
//! played at, and a manifest that said something the engine refuses would fail at pairing time,
//! per match, in production.
fn main() {
    let presets: Vec<serde_json::Value> = tb_ants::presets()
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "players": p.players }))
        .collect();
    let m = serde_json::json!({
        "game": "ants", "version": "1.0.0", "abi": 1,
        // No top-level `players`: the preset carries it (decision 14, 7 September 2026). Seats are
        // a property of the map, so a four-seat map is content rather than code.
        "presets": presets,
        "limits":  { "max_turns": tb_ants::MAX_TURNS, "turn_ms": 1000 },
        "budgets": {
            "flop_caps": { "nano": 2.5e8, "micro": 1e9, "mini": 4e9,
                           "small": 1.6e10, "large": 1.28e11 },
            // Measured, not argued: a reference six-plane adapter costs 197,272 operations against
            // a real worst-case observation, and a visibility-deriving one did not fit 200,000 at
            // all. axon/tests/ants_adapter.rs, and v2/04-model-loader.md §4.4.
            "adapter_ops_max": 1000000
        }
    });
    println!("{}", serde_json::to_string_pretty(&m).unwrap());
}
