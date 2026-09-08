//! What a model sees — docs/protocol.md §1 and the rules of Ants in the book §3.
//!
//! ```json
//! { "size":  [64, 96],
//!   "mine":  [[12,30], [13,30], [41,77]],
//!   "foes":  [[12,33,1], [11,34,1]],
//!   "food":  [[11,31], [40,80]],
//!   "hills": [[20,20,0], [44,76,1]],
//!   "water": { "rle": [0,812, 1,6, 0,4110, 1,12, 0,1204] } }
//! ```
//!
//! # What `water` carries, and why
//!
//! docs/protocol.md §8.3 left this open and docs/docs/cartridge.md owns it. **Decided: known water** —
//! `water AND seen`, per player, which is what rule 16 says the field means: water never changes,
//! so anything already seen stays true.
//!
//! The alternatives were rejected for the same reason and it is not the obvious one. Sending the
//! **whole map** breaks fog for the one thing worth scouting for. Sending **only what is visible
//! now** is defensible from rules 12-14 — and it would make exploration pointless, because a model
//! here is a pure function of one observation with no channel for state between turns. Under that
//! reading nothing a model discovers can ever be kept, by it or for it. Known water is the only
//! option in which scouting buys anything at all: the engine remembers on the model's behalf,
//! which is exactly what a stateless contract requires of the engine.
//!
//! It is the expensive choice and the numbers are known. The per-player seen-masks are **60% of
//! `wave_state`** — a fixed cost, because a bitmap does not grow with what is set in it — and
//! because a *partially* explored map is more fragmented than either an empty or a full one, the
//! run-length encoding grows over a match. Measured on `cell-04`, greedy play: **38 runs at turn 1,
//! 250 by turn 100, 574 by turn 300 and 1,206 by turn 600**, taking one observation from about
//! 0.6 KB to 3.0 KB. A wave of 16 at that size is 9% of the plugin response ceiling. Paying that so
//! that exploring means something is the right trade.
//!
//! **Those are the first real measurements of it.** `reveal` ran once, in `worldgen`, and was never
//! called again — so `known` was frozen at turn-zero vision for the whole match and exploring
//! recorded nothing. Every test passed, replays re-simulated exactly, and the only symptom was
//! observations smaller than the design said they would be. `turn.rs` folds vision in at the end of
//! every turn now; the earlier figures in this comment were an estimate that the code never met.
//!
//! Two consequences worth stating because they are visible to a model. A `0` in `water` conflates
//! *known empty* with *never seen*, which docs/protocol.md §1 already names as the cost of dropping
//! the `vis` mask. And `water` is the only field with memory — `foes`, `food` and `hills` are all
//! strictly what is visible this turn — which is not an inconsistency but the rules: only water is
//! permanent.

use serde_json::{json, Value};

use crate::map::Bits;
use crate::state::Match;

/// One seat's view. `refs` is an opaque handle the platform hands in and the engine echoes back
/// without looking inside it — see `lib.rs`.
pub fn view(m: &Match, seat: u8) -> Value {
    let vis = m.visible(seat);
    let known = &m.known[seat as usize];

    let mut seen_water = Bits::zeros(m.cells());
    for i in 0..m.cells() {
        if m.water.get(i) && known.get(i) {
            seen_water.set(i);
        }
    }

    let rc = |p: u16| {
        let (r, c) = m.g.rc(p);
        json!([r, c])
    };
    let rco = |p: u16, o: u8| {
        let (r, c) = m.g.rc(p);
        json!([r, c, o])
    };

    json!({
        "size":  [m.g.rows, m.g.cols],
        // Rule 24: your own ants, all of them, whether or not another of yours can see them.
        "mine":  m.mine(seat).into_iter().map(rc).collect::<Vec<_>>(),
        // Rules 13-14: on visible squares you see everything; on every other square, nothing.
        "foes":  m.ants.iter().filter(|a| a.owner != seat && vis.get(a.pos as usize))
                   .map(|a| rco(a.pos, a.owner)).collect::<Vec<_>>(),
        "food":  m.food.iter().copied().filter(|&f| vis.get(f as usize))
                   .map(rc).collect::<Vec<_>>(),
        // A razed hill is gone from the map (rule 42), so it is not in the view.
        "hills": m.hills.iter().filter(|h| !h.razed && vis.get(h.pos as usize))
                   .map(|h| rco(h.pos, h.owner)).collect::<Vec<_>>(),
        "water": { "rle": seen_water.rle() },
    })
}
