//! The hot loops, each checked against the plain form it replaced.
//!
//! These run on every plugin call — the codec over the whole wave in and out, the bitmaps once per
//! seat, the food scan whenever food falls due — so each is written for speed. A faster form that
//! answered differently would be a different game, and on the platform a silent one: every replay
//! would still re-simulate, just not the match that was played. So the original of each is kept
//! here, written for clarity, and the two are compared on real boards and awkward ones.

use super::*;

// ---------------------------------------------------------------- the food scan

/// `state::food_sets` as first written: every square, its whole orbit, a walk over the hills.
fn food_sets_by_definition(m: &Match) -> Vec<u16> {
    let mut out = Vec::new();
    for pos in 0..m.cells() as u16 {
        let mut orbit: Vec<u16> =
            (0..m.players as i32).map(|k| m.sym.image(&m.g, pos, k)).collect();
        orbit.sort_unstable();
        orbit.dedup();
        let usable = orbit[0] == pos
            && !orbit.iter().any(|&p| m.water.get(p as usize))
            && !orbit.iter().any(|&p| m.hills.iter().any(|h| h.pos == p))
            && !orbit[1..].iter().any(|&p| m.g.dist2(orbit[0], p) == 1);
        if usable {
            out.push(pos);
        }
    }
    out
}

#[test]
fn the_food_scan_finds_the_sets_the_definition_does_on_every_board() {
    for mf in crate::mapfile::catalogue() {
        let mut m = mf.build(7, 1000).unwrap();
        assert_eq!(crate::state::food_sets(&m), food_sets_by_definition(&m), "map {}", mf.id);
        // A razed hill keeps its square, and neither form may give it back to the food.
        for h in m.hills.iter_mut() {
            h.razed = true;
        }
        assert_eq!(crate::state::food_sets(&m), food_sets_by_definition(&m), "{} razed", mf.id);
    }
    for p in crate::map::PRESETS {
        for seed in [1, 99, 0xBEEF] {
            let m = worldgen(seed, p, 1000);
            assert_eq!(
                crate::state::food_sets(&m),
                food_sets_by_definition(&m),
                "{} seed {seed}",
                p.name
            );
        }
    }
    // Orbits of three and four, where images can coincide and deduplication has work to do.
    for (rows, cols, players) in [(40, 40, 4), (9, 7, 3), (12, 12, 4)] {
        let mut m = bare(rows, cols, players);
        let mut rng = Rng(rows as u64 * 31 + players as u64);
        for _ in 0..m.cells() / 6 {
            m.water.set(rng.below(m.cells() as u32) as usize);
        }
        m.hills.push(Hill { pos: m.g.at(1, 1), owner: 0, razed: false, last_touched: 0 });
        assert_eq!(
            crate::state::food_sets(&m),
            food_sets_by_definition(&m),
            "{rows}x{cols}, {players} seats"
        );
    }
}

// ---------------------------------------------------------------- vision

/// `Match::visible` as first written: every offset of the disk, each wrapped on its own.
fn visible_by_definition(m: &Match, owner: u8) -> Bits {
    let mut v = Bits::zeros(m.cells());
    let disk = m.g.disk(crate::map::VIEW_RADIUS2);
    for a in m.ants_of(owner) {
        let (r, c) = m.g.rc(a.pos);
        for (dr, dc) in &disk {
            v.set(m.g.at(r + dr, c + dc) as usize);
        }
    }
    v
}

#[test]
fn vision_stamped_by_rows_sees_the_disk_it_replaces() {
    // Each preset's size, and two boards narrower than the disk is wide, where a row wraps onto
    // itself more than once.
    for (rows, cols) in [(64, 96), (96, 96), (128, 128), (12, 10), (5, 17)] {
        let mut m = bare(rows, cols, 2);
        let mut rng = Rng(rows as u64 * 1000 + cols as u64);
        let (lr, lc) = (rows as i32 - 1, cols as i32 - 1);
        for (r, c) in [(0, 0), (0, lc), (lr, 0), (lr, lc)] {
            let pos = m.g.at(r, c);
            m.ants.push(Ant { pos, owner: 0 });
        }
        for _ in 0..40 {
            let pos = rng.below(m.cells() as u32) as u16;
            m.ants.push(Ant { pos, owner: rng.below(2) as u8 });
        }
        for owner in 0..2u8 {
            let want = visible_by_definition(&m, owner);
            assert_eq!(m.visible(owner).bits, want.bits, "{rows}x{cols} seat {owner}");

            // `reveal` ORs a byte at a time into what the seat already knew.
            for i in 0..m.cells() {
                if rng.below(3) == 0 {
                    m.known[owner as usize].set(i);
                }
            }
            let mut known = m.known[owner as usize].clone();
            for i in 0..want.len {
                if want.get(i) {
                    known.set(i);
                }
            }
            m.reveal(owner);
            assert_eq!(m.known[owner as usize].bits, known.bits, "{rows}x{cols} reveal {owner}");
        }
    }
}

// ---------------------------------------------------------------- run lengths

/// `Bits::rle` as first written: a square at a time.
fn rle_by_definition(b: &Bits) -> Vec<u32> {
    let mut out = Vec::new();
    if b.len == 0 {
        return out;
    }
    let mut cur = b.get(0) as u32;
    let mut run = 0u32;
    for i in 0..b.len {
        let v = b.get(i) as u32;
        if v == cur {
            run += 1;
        } else {
            out.push(cur);
            out.push(run);
            cur = v;
            run = 1;
        }
    }
    out.push(cur);
    out.push(run);
    out
}

#[test]
fn run_lengths_a_byte_at_a_time_are_the_run_lengths_a_square_at_a_time() {
    let mut rng = Rng(0x5EED);
    for len in (0..=67).chain([6144, 9216, 16384]) {
        // From all open to all water through sparse and patchy, so the byte path and the square
        // path are both taken and the switch between them falls on and off a byte boundary.
        for density in [0, 1, 8, 50, 92, 99, 100] {
            let mut b = Bits::zeros(len);
            let mut i = 0;
            while i < len {
                // Runs rather than independent squares, like terrain and fog.
                let n = 1 + rng.below(24) as usize;
                if rng.below(100) < density {
                    for j in i..(i + n).min(len) {
                        b.set(j);
                    }
                }
                i += n;
            }
            assert_eq!(b.rle(), rle_by_definition(&b), "len {len} density {density}");
        }
    }
}

// ---------------------------------------------------------------- base64

/// The codec's base64 as first written.
fn b64_encode_by_definition(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
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

fn b64_decode_by_definition(s: &str) -> Option<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in A.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let src: Vec<u8> = s.bytes().filter(|&b| b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
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

#[test]
fn base64_writes_and_reads_the_bytes_it_always_did() {
    let mut rng = Rng(0xB64);
    let mut cases: Vec<Vec<u8>> =
        (0..=40).map(|n| (0..n).map(|_| rng.below(256) as u8).collect()).collect();
    cases.push((0..20_000).map(|_| rng.below(256) as u8).collect());
    for data in &cases {
        let enc = crate::codec::b64_encode(data);
        assert_eq!(enc, b64_encode_by_definition(data), "encode {} bytes", data.len());
        assert_eq!(crate::codec::b64_decode(&enc).as_deref(), Some(&data[..]), "round trip");

        // Padding and whitespace wherever they fall, and a dangling character, read as before.
        let noisy: String = enc
            .chars()
            .enumerate()
            .flat_map(|(i, ch)| {
                let mut v = vec![ch];
                if i % 7 == 3 {
                    v.push(' ');
                }
                if i % 11 == 5 {
                    v.push('\n');
                }
                if i % 13 == 1 {
                    v.push('=');
                }
                v
            })
            .collect();
        assert_eq!(crate::codec::b64_decode(&noisy), b64_decode_by_definition(&noisy));
        let dangling = format!("{enc}Q");
        assert_eq!(crate::codec::b64_decode(&dangling), b64_decode_by_definition(&dangling));
    }
    assert_eq!(crate::codec::b64_decode("AB*C"), None);
    assert_eq!(b64_decode_by_definition("AB*C"), None);
}
