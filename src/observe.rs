//! What a model sees — `docs/protocol.md` §1.
//!
//! Owners are relative to the observer: **you are always `0`**, an opponent is `1` upward. So the
//! same position, asked of either seat, comes back as the same bytes — which is what
//! `docs/protocol.md` §3.1 requires and what a self-play trainer depends on.
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
//! `water` carries **known water** — `water AND seen`, per player. Water never changes, so anything
//! already seen stays true. The alternatives were rejected for one reason: a model here is a pure
//! function of one observation with no channel for state between turns, so sending only what is
//! visible now would make exploration pointless, and sending the whole map would break fog for the
//! one thing worth scouting for. Known water is the only option in which scouting buys anything at
//! all — the engine remembers on the model's behalf, which is what a stateless contract requires of
//! it.
//!
//! It is the expensive choice: the per-player seen-masks are about 60% of `wave_state`, and because
//! a partially explored map is more fragmented than either an empty or a full one, the run-length
//! encoding grows over a match.
//!
//! Two consequences are visible to a model. A `0` in `water` conflates *known empty* with *never
//! seen*, which is the cost of dropping the `vis` mask. And `water` is the only field with memory —
//! `foes`, `food` and `hills` are strictly what is visible this turn — which is not an
//! inconsistency but the rules: only water is permanent.

use serde_json::{Value, json};

use crate::map::{Bits, Geom};
use crate::state::Match;

/// A position as the protocol writes it: `[row, col]`.
pub fn rc(g: &Geom, p: u16) -> Value {
    let (r, c) = g.rc(p);
    json!([r, c])
}

/// A position with an owner: `[row, col, owner]`. The owner is **relative to the observer** —
/// see `relative`.
pub fn rc_owned(g: &Geom, p: u16, owner: u8) -> Value {
    let (r, c) = g.rc(p);
    json!([r, c, owner])
}

/// A seat number as the observer sees it: yourself is always `0`, and an opponent is `1..players`,
/// counting round from you.
///
/// **This is the observer-relative rule (`docs/protocol.md` §3.1), and for a long time this engine
/// did not obey it.** It emitted raw seat numbers, so seat 1's own hill arrived labelled `1` and
/// every adapter that read "owner 0 is mine" — including the reference one — had its own-hills and
/// enemy-hills planes swapped on one side of every match. `schema/state.schema.json` always said
/// what the field meant ("0 is yours; 1+ is an opponent's"); the code did not, and nothing caught
/// it because `validate.py` checked the property against a Python stand-in and the engine's own
/// tests only ever asked seat 0. Both gaps are closed with this change.
///
/// It is not a cosmetic relabel. A self-play trainer sees two seats of one match as two samples of
/// one distribution, and under absolute labels they are two different games — so the bug cost
/// nothing while every model held still, and would have cost half the training signal the moment
/// one did not.
fn relative(owner: u8, seat: u8, players: u8) -> u8 {
    (owner + players - seat) % players
}

/// One seat's view.
pub fn view(m: &Match, seat: u8) -> Value {
    let vis = m.visible(seat);
    let known = &m.known[seat as usize];

    let mut seen_water = Bits::zeros(m.cells());
    for i in 0..m.cells() {
        if m.water.get(i) && known.get(i) {
            seen_water.set(i);
        }
    }

    json!({
        "size":  [m.g.rows, m.g.cols],
        // Your own ants, all of them, whether or not another of yours can see them.
        "mine":  m.mine(seat).into_iter().map(|p| rc(&m.g, p)).collect::<Vec<_>>(),
        // On visible squares you see everything; on every other square, nothing.
        "foes":  m.ants.iter().filter(|a| a.owner != seat && vis.get(a.pos as usize))
                   .map(|a| rc_owned(&m.g, a.pos, relative(a.owner, seat, m.players)))
                   .collect::<Vec<_>>(),
        "food":  m.food.iter().copied().filter(|&f| vis.get(f as usize))
                   .map(|p| rc(&m.g, p)).collect::<Vec<_>>(),
        // A razed hill is gone from the map, so it is not in the view.
        "hills": m.hills.iter().filter(|h| !h.razed && vis.get(h.pos as usize))
                   .map(|h| rc_owned(&m.g, h.pos, relative(h.owner, seat, m.players)))
                   .collect::<Vec<_>>(),
        "water": { "rle": seen_water.rle() },
    })
}
