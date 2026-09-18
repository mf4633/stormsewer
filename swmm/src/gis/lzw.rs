// SPDX-License-Identifier: GPL-3.0-or-later

//! LZW as TIFF 6.0 §13 specifies it (compression 5): codes packed
//! MSB-first, 9-bit initial code width growing to 12, `256` = clear,
//! `257` = end of information, and the "early change" the revised TIFF
//! spec (and every writer since) uses — the width grows one code earlier
//! than in the original 1987 scheme, when the next free code reaches
//! `2^width - 1`.
//!
//! A small encoder is included so tests can build compressed strips
//! without another tool.

use crate::{Error, Result};

const CLEAR: usize = 256;
const EOI: usize = 257;

struct MsbBits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    nbits: u32,
}

impl<'a> MsbBits<'a> {
    fn read(&mut self, width: u32) -> Option<usize> {
        while self.nbits < width {
            let b = *self.data.get(self.pos)?;
            self.pos += 1;
            self.acc = (self.acc << 8) | b as u32;
            self.nbits += 8;
        }
        let v = (self.acc >> (self.nbits - width)) & ((1 << width) - 1);
        self.nbits -= width;
        self.acc &= (1 << self.nbits) - 1;
        Some(v as usize)
    }
}

/// Decode one TIFF LZW strip or tile. `expected` bounds the output so a
/// corrupt stream cannot run away.
pub fn decode(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut bits = MsbBits {
        data,
        pos: 0,
        acc: 0,
        nbits: 0,
    };
    let mut out = Vec::with_capacity(expected);
    // Each table entry is (prefix code, last byte, length); strings are
    // rebuilt by walking prefixes, which keeps the table small.
    let mut prefix: Vec<u32> = Vec::with_capacity(4096);
    let mut suffix: Vec<u8> = Vec::with_capacity(4096);
    let mut length: Vec<u32> = Vec::with_capacity(4096);
    let reset = |prefix: &mut Vec<u32>, suffix: &mut Vec<u8>, length: &mut Vec<u32>| {
        prefix.clear();
        suffix.clear();
        length.clear();
        for i in 0..258u32 {
            prefix.push(u32::MAX);
            suffix.push(i as u8);
            length.push(1);
        }
    };
    reset(&mut prefix, &mut suffix, &mut length);
    let mut width = 9;
    let mut prev: Option<usize> = None;
    let mut scratch: Vec<u8> = Vec::new();
    let emit = |code: usize, prefix: &[u32], suffix: &[u8], length: &[u32], scratch: &mut Vec<u8>| {
        let n = length[code] as usize;
        scratch.resize(n, 0);
        let mut c = code;
        for i in (0..n).rev() {
            scratch[i] = suffix[c];
            c = prefix[c] as usize;
        }
    };
    while out.len() < expected {
        let Some(code) = bits.read(width) else { break };
        if code == CLEAR {
            reset(&mut prefix, &mut suffix, &mut length);
            width = 9;
            prev = None;
            continue;
        }
        if code == EOI {
            break;
        }
        let next = prefix.len();
        if code < next {
            emit(code, &prefix, &suffix, &length, &mut scratch);
        } else if code == next && prev.is_some() {
            let p = prev.unwrap();
            emit(p, &prefix, &suffix, &length, &mut scratch);
            let first = scratch[0];
            scratch.push(first);
        } else {
            return Err(Error::Format(format!("LZW: code {code} is not in the table (size {next})")));
        }
        out.extend_from_slice(&scratch);
        if let Some(p) = prev {
            if next < 4096 {
                prefix.push(p as u32);
                suffix.push(scratch[0]);
                length.push(length[p] + 1);
            }
        }
        prev = Some(code);
        if prefix.len() + 1 >= (1 << width) && width < 12 {
            width += 1;
        }
    }
    out.truncate(expected);
    Ok(out)
}

struct MsbWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl MsbWriter {
    fn put(&mut self, code: usize, width: u32) {
        self.acc = (self.acc << width) | code as u64;
        self.nbits += width;
        while self.nbits >= 8 {
            self.out.push((self.acc >> (self.nbits - 8)) as u8);
            self.nbits -= 8;
            self.acc &= (1 << self.nbits) - 1;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push((self.acc << (8 - self.nbits)) as u8);
        }
        self.out
    }
}

/// Encode bytes as a TIFF LZW stream (with the early change).
pub fn encode(data: &[u8]) -> Vec<u8> {
    use std::collections::HashMap;
    let mut w = MsbWriter {
        out: Vec::with_capacity(data.len()),
        acc: 0,
        nbits: 0,
    };
    let mut table: HashMap<(usize, u8), usize> = HashMap::new();
    let mut next = 258;
    let mut width = 9;
    w.put(CLEAR, width);
    let mut it = data.iter().copied();
    let Some(first) = it.next() else {
        w.put(EOI, width);
        return w.finish();
    };
    let mut cur = first as usize;
    for b in it {
        if let Some(&code) = table.get(&(cur, b)) {
            cur = code;
            continue;
        }
        w.put(cur, width);
        table.insert((cur, b), next);
        next += 1;
        // The decoder adds nothing for the first code after a clear, so
        // its table is one entry behind this one: it grows its width when
        // its size reaches 2^width - 1, which is when ours reaches 2^width.
        if next >= 4094 {
            w.put(CLEAR, width);
            table.clear();
            next = 258;
            width = 9;
        } else if next >= (1 << width) && width < 12 {
            width += 1;
        }
        cur = b as usize;
    }
    w.put(cur, width);
    next += 1;
    if next >= (1 << width) && width < 12 {
        width += 1;
    }
    w.put(EOI, width);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example_decodes() {
        // TIFF 6.0 §13 worked example: the input 7 7 7 8 8 7 7 6 6 encodes
        // to the codes Clear 7 258 8 8 258 6 6 EOI (early-change widths are
        // all 9 bits here).
        let mut w = MsbWriter { out: vec![], acc: 0, nbits: 0 };
        for c in [256usize, 7, 258, 8, 8, 258, 6, 6, 257] {
            w.put(c, 9);
        }
        let bytes = w.finish();
        assert_eq!(decode(&bytes, 9).unwrap(), [7, 7, 7, 8, 8, 7, 7, 6, 6]);
        assert_eq!(encode(&[7, 7, 7, 8, 8, 7, 7, 6, 6]), bytes);
    }

    #[test]
    fn long_streams_round_trip_across_every_width_and_a_clear() {
        let mut data = Vec::new();
        for i in 0..20000u32 {
            data.push(((i * 7919) % 251) as u8);
            data.push((i % 3) as u8);
        }
        let enc = encode(&data);
        assert!(enc.len() < data.len());
        assert_eq!(decode(&enc, data.len()).unwrap(), data);
        let runs: Vec<u8> = (0..6000).map(|i| (i / 500) as u8).collect();
        let enc = encode(&runs);
        assert_eq!(decode(&enc, runs.len()).unwrap(), runs);
        assert_eq!(decode(&encode(&[]), 0).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        // A code far past the table right after a clear.
        let mut w = MsbWriter { out: vec![], acc: 0, nbits: 0 };
        w.put(256, 9);
        w.put(400, 9);
        assert!(decode(&w.finish(), 10).is_err());
    }
}
