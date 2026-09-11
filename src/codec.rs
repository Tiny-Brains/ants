//! The packed `wave_state`, and base64 over it.
//!
//! Three rules, and this file is the whole of the compliance.
//!
//! **Opaque and compact.** Base64 over a packed binary encoding, not readable JSON. Nothing outside
//! this component decodes it, which is what makes "the platform never parses game state" a property
//! rather than a promise.
//!
//! **It round-trips exactly.** `step` returns the state its next call receives; anything not
//! encoded is information the match does not have next turn.
//!
//! **No version field.** A wave never spans two cartridge versions, so the encoding is versioned by
//! the digest that produced it.
//!
//! ```text
//! wave    u16 matches, then each match in order
//! match   u64 seed · u16 turn · u16 max_turns · u8 players · u8 done · u8 reason
//!         u8  rows · u8 cols
//!         u8  cutoff_bot · u16 cutoff_turns
//!         u16 food_rate · u16 food_turn · u32 food_extra
//!         u16 food_rotation · u16 food_cursor
//!         u16 n_pending, then n_pending x u16 pos
//!         u8  map_id_len, then that many bytes of UTF-8
//!         u16 n_food0, then n_food0 x u16 pos     (the board's turn-zero food)
//!         bitmap water            ceil(rows*cols/8) bytes
//!         bitmap known[player]    the same, once per player
//!         u16 n_ants,  then n_ants x (u16 pos, u8 owner)
//!         u16 n_food,  then n_food x u16 pos
//!         u8  n_hills, then n_hills x (u16 pos, u8 owner, u8 razed, u16 last_touched)
//!         per player: u16 hive, i16 score
//! ```

use crate::map::{Bits, Geom, Symmetry};
use crate::state::{Ant, Hill, Match};

pub struct Wave {
    pub matches: Vec<Match>,
}

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
    /// A count-prefixed list of positions.
    fn positions(&mut self, v: &[u16]) {
        self.u16(v.len() as u16);
        for &p in v {
            self.u16(p);
        }
    }
}

pub fn pack(w: &Wave) -> String {
    let mut o = W::default();
    o.u16(w.matches.len() as u16);
    for m in &w.matches {
        o.u64(m.seed);
        o.u16(m.turn);
        o.u16(m.max_turns);
        o.u8(m.players);
        o.u8(m.done as u8);
        o.u8(m.reason);
        o.u8(m.g.rows as u8);
        o.u8(m.g.cols as u8);
        o.u8(m.cutoff_bot);
        o.u16(m.cutoff_turns);
        o.u16(m.food_rate);
        o.u16(m.food_turn);
        o.u32(m.food_extra);
        o.u16(m.food_rotation);
        o.u16(m.food_cursor);
        o.positions(&m.pending_food);
        // The board, for the replay envelope — `state.rs` on why the turn-zero food cannot be read
        // back off a played match.
        let id = m.map_id.as_bytes();
        let id = &id[..id.len().min(255)];
        o.u8(id.len() as u8);
        o.bytes(id);
        o.positions(&m.food0);
        o.bytes(&m.water.bits);
        for k in &m.known {
            o.bytes(&k.bits);
        }
        o.u16(m.ants.len() as u16);
        for a in &m.ants {
            o.u16(a.pos);
            o.u8(a.owner);
        }
        o.positions(&m.food);
        o.u8(m.hills.len() as u8);
        for h in &m.hills {
            o.u16(h.pos);
            o.u8(h.owner);
            o.u8(h.razed as u8);
            o.u16(h.last_touched);
        }
        for i in 0..m.players as usize {
            o.u16(m.hive[i]);
            o.i16(m.score[i]);
        }
    }
    b64_encode(&o.0)
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        let s = self.bytes(2)?;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.bytes(4)?;
        Some(u32::from_le_bytes(s.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        let s = self.bytes(8)?;
        Some(u64::from_le_bytes(s.try_into().ok()?))
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn positions(&mut self) -> Option<Vec<u16>> {
        let n = self.u16()?;
        (0..n).map(|_| self.u16()).collect()
    }
}

pub fn unpack(s: &str) -> Option<Wave> {
    let buf = b64_decode(s)?;
    let mut r = R { b: &buf, at: 0 };
    let n = r.u16()?;
    let mut matches = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let seed = r.u64()?;
        let turn = r.u16()?;
        let max_turns = r.u16()?;
        let players = r.u8()?;
        let done = r.u8()? != 0;
        let reason = r.u8()?;
        let g = Geom::new(r.u8()?, r.u8()?);
        let cutoff_bot = r.u8()?;
        let cutoff_turns = r.u16()?;
        let food_rate = r.u16()?;
        let food_turn = r.u16()?;
        let food_extra = r.u32()?;
        let food_rotation = r.u16()?;
        let food_cursor = r.u16()?;
        let pending_food = r.positions()?;
        let idn = r.u8()? as usize;
        let map_id = String::from_utf8(r.bytes(idn)?.to_vec()).ok()?;
        let food0 = r.positions()?;

        let cells = g.cells();
        let nb = cells.div_ceil(8);
        let water = Bits { bits: r.bytes(nb)?.to_vec(), len: cells };
        let mut known = Vec::with_capacity(players as usize);
        for _ in 0..players {
            known.push(Bits { bits: r.bytes(nb)?.to_vec(), len: cells });
        }
        let na = r.u16()?;
        let mut ants = Vec::with_capacity(na as usize);
        for _ in 0..na {
            ants.push(Ant { pos: r.u16()?, owner: r.u8()? });
        }
        let food = r.positions()?;
        let nh = r.u8()?;
        let mut hills = Vec::with_capacity(nh as usize);
        for _ in 0..nh {
            hills.push(Hill {
                pos: r.u16()?,
                owner: r.u8()?,
                razed: r.u8()? != 0,
                last_touched: r.u16()?,
            });
        }
        let mut hive = Vec::with_capacity(players as usize);
        let mut score = Vec::with_capacity(players as usize);
        for _ in 0..players {
            hive.push(r.u16()?);
            score.push(r.i16()?);
        }

        matches.push(Match {
            seed,
            turn,
            max_turns,
            players,
            g,
            sym: Symmetry::for_preset(&g, players),
            done,
            reason,
            water,
            known,
            ants,
            food,
            hills,
            hive,
            score,
            cutoff_bot,
            cutoff_turns,
            map_id,
            food0,
            food_rate,
            food_turn,
            food_extra,
            food_rotation,
            food_cursor,
            pending_food,
        });
    }
    Some(Wave { matches })
}

// ---------------------------------------------------------------- base64
//
// Written out rather than pulled in: the component is built for wasm32 and every dependency is
// bytes in the artifact the plugin ceiling has to carry.

const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `A` inverted, at compile time; 255 marks a byte outside the alphabet.
const REV: [u8; 256] = {
    let mut rev = [255u8; 256];
    let mut i = 0;
    while i < 64 {
        rev[A[i] as usize] = i as u8;
        i += 1;
    }
    rev
};

/// Every call carries the whole wave in and out through here, so both directions run once per
/// plugin invocation over tens of kilobytes. They are written for that: one pass, no copy of the
/// input, output sized up front.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    let (whole, rest) = data.as_chunks::<3>();
    for c in whole {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32;
        out.extend_from_slice(&[
            A[(n >> 18) as usize & 63],
            A[(n >> 12) as usize & 63],
            A[(n >> 6) as usize & 63],
            A[n as usize & 63],
        ]);
    }
    if !rest.is_empty() {
        let n = ((rest[0] as u32) << 16) | ((*rest.get(1).unwrap_or(&0) as u32) << 8);
        out.push(A[(n >> 18) as usize & 63]);
        out.push(A[(n >> 12) as usize & 63]);
        out.push(if rest.len() > 1 { A[(n >> 6) as usize & 63] } else { b'=' });
        out.push(b'=');
    }
    String::from_utf8(out).expect("the alphabet is ASCII")
}

/// Padding and whitespace are skipped wherever they fall, and a short final group decodes as a
/// whole one would have been cut: two characters carry one byte, three carry two.
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let src = s.as_bytes();
    let mut out = Vec::with_capacity(src.len() / 4 * 3);

    // Whole groups of four alphabet characters first, which is everything `b64_encode` writes but
    // its last group. Any byte outside the alphabet — padding and whitespace included — maps to
    // 255, so one OR over the four finds it and hands the rest to the loop below, which then starts
    // on a group boundary exactly where this stopped.
    let mut at = 0;
    for g in src.as_chunks::<4>().0 {
        let v = [REV[g[0] as usize], REV[g[1] as usize], REV[g[2] as usize], REV[g[3] as usize]];
        if (v[0] | v[1] | v[2] | v[3]) >= 64 {
            break;
        }
        let n = (v[0] as u32) << 18 | (v[1] as u32) << 12 | (v[2] as u32) << 6 | v[3] as u32;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        at += 4;
    }

    let (mut n, mut k) = (0u32, 0u32);
    for &b in &src[at..] {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        let v = REV[b as usize];
        if v == 255 {
            return None;
        }
        n |= (v as u32) << (18 - 6 * k);
        k += 1;
        if k == 4 {
            out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            (n, k) = (0, 0);
        }
    }
    if k > 0 {
        out.push((n >> 16) as u8);
        if k > 2 {
            out.push((n >> 8) as u8);
        }
    }
    Some(out)
}
