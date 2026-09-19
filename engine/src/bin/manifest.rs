//! Emit `cartridge.json`, the registration manifest -- the half of it the engine knows.
//!
//! Generated rather than hand-written, because the manifest is read once at registration and a
//! manifest that said something the engine refuses would fail per match, in production.
//! `tools/package.py` folds in what only the tree knows: the basic boards' catalogue, the
//! `limits.boards` envelope derived from them, and about.json.
//!
//! **No presets.** The component carries no boards, so it has no pools to declare and no seat
//! counts to publish: seats are a property of each board, which a season's upload or a registry's
//! file states for itself.
fn main() {
    let m = serde_json::json!({
        "game": "ants", "version": "1.0.0", "abi": 1,
        "limits":  { "max_turns": tb_ants::MAX_TURNS, "turn_ms": 1000 },
        // One budget, and it prices marshalling. There is no compute cap: measurement showed one
        // shadowed by S below and by turn_ms above, and unable to catch the exploit it was meant
        // for. The turn deadline is the compute bound, and each seat's model call has its own.
        "budgets": {
            // Measured, not argued. A shipped seven-plane manifest costs 86,051 operations at
            // 64x96 and 229,415 at 128x128 -- about 14 a cell -- so a million is 4.4x headroom on
            // a board of 128x128, above the largest an upload may be (`limits.boards.cells_max`).
            // The number must equal the Kalam node's `engine.ops_budget`, which is what actually
            // refuses one (web's configs.sh checks it).
            "adapter_ops_max": 1000000
        }
    });
    println!("{}", serde_json::to_string_pretty(&m).unwrap());
}
