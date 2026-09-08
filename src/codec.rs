//! The packed `wave_state`, and base64 over it.
//!
//! `cartridge.md` §2 states three rules, and this file is the whole of the compliance:
//!
//! **Opaque and compact.** Base64 over a packed binary encoding, not readable JSON. Nothing outside
//! this component decodes it and nothing outside it should be able to — that is what makes "the
//! platform never parses game state" a property rather than a promise.
//!
//! **It round-trips exactly.** `step` returns the state its next call receives; any information not
//! encoded is information the match does not have next turn. Tested against a played match, not
//! against a fresh one, because a fresh state exercises none of the fields that matter.
//!
//! **No version field.** A wave never spans two cartridge versions, so the encoding is versioned by
//! the digest that produced it.
//!
//! ```text
//! wave    u16 matches, then each match in order
//! match   u64 seed · u16 turn · u16 max_turns · u8 players · u8 done · u8 reason
//!         u8 rows · u8 cols
//!         u16 domination_turns · u16 idle_food_turns
//!         bitmap water            ceil(rows*cols/8) bytes
//!         bitmap known[player]    the same, once per player
//!         u16 n_ants,  then n_ants x (u16 pos, u8 owner)
//!         u16 n_food,  then n_food x u16 pos
//!         u8  n_hills, then n_hills x (u16 pos, u8 owner, u8 razed, u16 last_spawn)
//!         per player: u16 hive, i16 score
//! ```

use crate::map::{Bits, Geom, Symmetry};
use crate::state::{Hill, Match, Ant};

pub struct Wave {
    pub matches: Vec<Match>,
}

// ---------------------------------------------------------------- writing

fn put_u16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_i16(o: &mut Vec<u8>, v: i16) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_le_bytes());
}

pub fn pack(w: &Wave) -> String {
    let mut o: Vec<u8> = Vec::new();
    put_u16(&mut o, w.matches.len() as u16);
    for m in &w.matches {
        put_u64(&mut o, m.seed);
        put_u16(&mut o, m.turn);
        put_u16(&mut o, m.max_turns);
        o.push(m.players);
        o.push(m.done as u8);
        o.push(m.reason);
        o.push(m.g.rows as u8);
        o.push(m.g.cols as u8);
        put_u16(&mut o, m.domination_turns);
        put_u16(&mut o, m.idle_food_turns);
        o.extend_from_slice(&m.water.bits);
        for k in &m.known {
            o.extend_from_slice(&k.bits);
        }
        put_u16(&mut o, m.ants.len() as u16);
        for a in &m.ants {
            put_u16(&mut o, a.pos);
            o.push(a.owner);
        }
        put_u16(&mut o, m.food.len() as u16);
        for &f in &m.food {
            put_u16(&mut o, f);
        }
        o.push(m.hills.len() as u8);
        for h in &m.hills {
            put_u16(&mut o, h.pos);
            o.push(h.owner);
            o.push(h.razed as u8);
            put_u16(&mut o, h.last_spawn);
        }
        for i in 0..m.players as usize {
            put_u16(&mut o, m.hive[i]);
            put_i16(&mut o, m.score[i]);
        }
    }
    b64_encode(&o)
}

// ---------------------------------------------------------------- reading

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let s = self.b.get(self.at..self.at + 2)?;
        self.at += 2;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn u64(&mut self) -> Option<u64> {
        let s = self.b.get(self.at..self.at + 8)?;
        self.at += 8;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Some(u64::from_le_bytes(a))
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
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
        let rows = r.u8()?;
        let cols = r.u8()?;
        let domination_turns = r.u16()?;
        let idle_food_turns = r.u16()?;

        let g = Geom::new(rows, cols);
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
            let pos = r.u16()?;
            ants.push(Ant { pos, owner: r.u8()? });
        }
        let nf = r.u16()?;
        let mut food = Vec::with_capacity(nf as usize);
        for _ in 0..nf {
            food.push(r.u16()?);
        }
        let nh = r.u8()?;
        let mut hills = Vec::with_capacity(nh as usize);
        for _ in 0..nh {
            let pos = r.u16()?;
            let owner = r.u8()?;
            let razed = r.u8()? != 0;
            hills.push(Hill { pos, owner, razed, last_spawn: r.u16()? });
        }
        let mut hive = Vec::with_capacity(players as usize);
        let mut score = Vec::with_capacity(players as usize);
        for _ in 0..players {
            hive.push(r.u16()?);
            score.push(r.i16()?);
        }
        matches.push(Match {
            seed, turn, max_turns, players, g,
            sym: Symmetry::for_preset(&g, players),
            done, reason, water, known, ants, food, hills, hive, score,
            domination_turns, idle_food_turns,
        });
    }
    Some(Wave { matches })
}

// ---------------------------------------------------------------- base64
//
// Written out rather than pulled in: the component is built for wasm32 and every dependency is
// bytes in the artifact the plugin ceiling has to carry.

const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut rev = [255u8; 256];
    for (i, &c) in A.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let src: Vec<u8> = s.bytes().filter(|&b| b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(src.len() / 4 * 3);
    for c in src.chunks(4) {
        let mut n = 0u32;
        for (k, &b) in c.iter().enumerate() {
            let v = rev[b as usize];
            if v == 255 {
                return None;
            }
            n |= (v as u32) << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}
