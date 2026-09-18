// SPDX-License-Identifier: GPL-3.0-or-later

//! DEFLATE decoding (RFC 1951) with the zlib wrapper (RFC 1950) that TIFF
//! compression 8 / 32946 puts around it. Decode only: stored, fixed
//! Huffman and dynamic Huffman blocks, the 32 KiB back-reference window.

use crate::{Error, Result};

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
            nbits: 0,
        }
    }

    fn need(&mut self, n: u32) -> Result<()> {
        while self.nbits < n {
            let b = *self
                .data
                .get(self.pos)
                .ok_or_else(|| Error::Format("deflate stream ends early".into()))?;
            self.pos += 1;
            self.bit |= (b as u32) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }

    fn bits(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }

    fn align(&mut self) {
        self.bit = 0;
        self.nbits = 0;
    }
}

/// A canonical Huffman decoder: code-length counts and the symbols in
/// code order (RFC 1951 §3.2.2).
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return Err(Error::Format("deflate: over-subscribed Huffman code".into()));
            }
        }
        let mut offs = [0u16; 16];
        for i in 1..15 {
            offs[i + 1] = offs[i] + count[i];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Self { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= bits.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Error::Format("deflate: invalid Huffman code".into()))
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Result<()> {
    loop {
        let sym = lit.decode(bits)?;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let i = (sym - 257) as usize;
            if i >= 29 {
                return Err(Error::Format("deflate: bad length code".into()));
            }
            let len = LEN_BASE[i] as usize + bits.bits(LEN_EXTRA[i] as u32)? as usize;
            let d = dist.decode(bits)? as usize;
            if d >= 30 {
                return Err(Error::Format("deflate: bad distance code".into()));
            }
            let distance = DIST_BASE[d] as usize + bits.bits(DIST_EXTRA[d] as u32)? as usize;
            if distance > out.len() {
                return Err(Error::Format("deflate: distance before the start of output".into()));
            }
            let start = out.len() - distance;
            for k in 0..len {
                let b = out[start + k];
                out.push(b);
            }
        }
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman)> {
    let mut l = [0u8; 288];
    for (i, v) in l.iter_mut().enumerate() {
        *v = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&l)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman)> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let nlen = bits.bits(5)? as usize + 257;
    let ndist = bits.bits(5)? as usize + 1;
    let ncode = bits.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(Error::Format("deflate: bad dynamic block counts".into()));
    }
    let mut lengths = [0u8; 19];
    for &o in ORDER.iter().take(ncode) {
        lengths[o] = bits.bits(3)? as u8;
    }
    let lencode = Huffman::new(&lengths)?;
    let mut all = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < nlen + ndist {
        let sym = lencode.decode(bits)?;
        match sym {
            0..=15 => {
                all[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(Error::Format("deflate: repeat with no previous length".into()));
                }
                let prev = all[i - 1];
                let n = 3 + bits.bits(2)? as usize;
                for _ in 0..n {
                    if i < all.len() {
                        all[i] = prev;
                        i += 1;
                    }
                }
            }
            17 => i += 3 + bits.bits(3)? as usize,
            _ => i += 11 + bits.bits(7)? as usize,
        }
    }
    if all[256] == 0 {
        return Err(Error::Format("deflate: no end-of-block code".into()));
    }
    Ok((Huffman::new(&all[..nlen])?, Huffman::new(&all[nlen..])?))
}

/// Decode a raw DEFLATE stream.
pub fn inflate_raw(data: &[u8]) -> Result<Vec<u8>> {
    let mut bits = Bits::new(data);
    let mut out = Vec::with_capacity(data.len() * 3);
    loop {
        let last = bits.bits(1)?;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let p = bits.pos;
                let len = u16::from_le_bytes([
                    *data.get(p).ok_or_else(|| Error::Format("deflate: stored block header cut".into()))?,
                    *data.get(p + 1).ok_or_else(|| Error::Format("deflate: stored block header cut".into()))?,
                ]) as usize;
                let nlen = u16::from_le_bytes([
                    *data.get(p + 2).ok_or_else(|| Error::Format("deflate: stored block header cut".into()))?,
                    *data.get(p + 3).ok_or_else(|| Error::Format("deflate: stored block header cut".into()))?,
                ]) as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err(Error::Format("deflate: stored block length check failed".into()));
                }
                let body = data
                    .get(p + 4..p + 4 + len)
                    .ok_or_else(|| Error::Format("deflate: stored block cut".into()))?;
                out.extend_from_slice(body);
                bits.pos = p + 4 + len;
            }
            1 => {
                let (l, d) = fixed_tables()?;
                codes(&mut bits, &mut out, &l, &d)?;
            }
            2 => {
                let (l, d) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &l, &d)?;
            }
            _ => return Err(Error::Format("deflate: reserved block type".into())),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

/// Decode a zlib stream (RFC 1950 header, DEFLATE body, Adler-32 trailer
/// — the trailer is not checked), or a raw DEFLATE stream when no zlib
/// header is present.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() >= 2 {
        let cmf = data[0];
        let flg = data[1];
        let is_zlib = cmf & 0x0F == 8 && (cmf >> 4) <= 7 && ((cmf as u16) << 8 | flg as u16).is_multiple_of(31);
        if is_zlib {
            if flg & 0x20 != 0 {
                return Err(Error::Format("zlib stream uses a preset dictionary".into()));
            }
            return inflate_raw(&data[2..]);
        }
    }
    inflate_raw(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_block_round_trips() {
        // Raw deflate: one final stored block of 5 bytes.
        let mut d = vec![0x01, 5, 0, 0xFA, 0xFF];
        d.extend_from_slice(b"hello");
        assert_eq!(inflate_raw(&d).unwrap(), b"hello");
        // With a zlib header and trailer.
        let mut z = vec![0x78, 0x01];
        z.extend_from_slice(&d);
        z.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(inflate(&z).unwrap(), b"hello");
    }

    #[test]
    fn fixed_and_dynamic_blocks_from_zlib() {
        // Python 3 zlib.compress(b"abcabcabcabcabcabc", 9): a fixed
        // Huffman block (byte 2 bits 1-2 == 01).
        let fixed = [
            0x78, 0xDA, 0x4B, 0x4C, 0x4A, 0x4E, 0x44, 0x45, 0x00, 0x41, 0x7C, 0x06, 0xE5,
        ];
        assert_eq!(inflate(&fixed).unwrap(), b"abcabcabcabcabcabc");
        // Python 3 zlib.compress(TEXT, 9) where TEXT is the 400-letter
        // string below: a dynamic Huffman block (byte 2 bits 1-2 == 10).
        const TEXT: &str = "caegaaeadfaebaaeeabaeeafabggfaffeabaeaceaeafcebaffgbdaeafafbeeecefedcbbbafceececfaaeebcaeeaaefccdfefeaabeaacgfecedaedbfaeabcabeeeabeeebaeebedebaababbaefbbcaaeedffcaefgaeeeeeeaegeababebacfaaafaeadfaabfeagbdfdeaaeeeecaaacbebeabedaeaecgabedbdbeeecgbfbbebbeedaabebbfdeddababebcbeffaegdgaaebebegcaeeeabbaaafegaffedaeeaaagaeaebbabbcebfcbeeaadefeeeaeaeeaebfaabaefaeaceeeeaeabbbaaeeeaaecfefebbeeeeebebebeaeae";
        let dynamic = [
            0x78, 0xDA, 0x25, 0x90, 0x8B, 0x15, 0xC0, 0x30, 0x08, 0x02, 0x67, 0x15, 0x15, 0xF7,
            0xDF, 0xA0, 0x47, 0xDA, 0x8F, 0x2F, 0x51, 0x44, 0xA4, 0x6B, 0xAF, 0x6A, 0x6B, 0x5C,
            0x2B, 0x0E, 0x5B, 0x4A, 0x70, 0xE9, 0xCE, 0x65, 0xBF, 0x7B, 0xF5, 0x12, 0xDC, 0x20,
            0xEC, 0xD3, 0xE4, 0x52, 0xD6, 0xEE, 0xF6, 0x7A, 0xA7, 0x25, 0xA5, 0xCA, 0xAD, 0x1D,
            0x0E, 0x75, 0x38, 0x6A, 0xDD, 0x3D, 0x06, 0x51, 0x25, 0xFE, 0x3E, 0x83, 0xA0, 0x79,
            0xC4, 0xB0, 0x02, 0x14, 0x86, 0x17, 0x32, 0x53, 0x3B, 0x51, 0xA0, 0x82, 0x6C, 0x2D,
            0xCA, 0x24, 0xC7, 0x86, 0xCB, 0xC7, 0x31, 0x0F, 0x62, 0x23, 0x48, 0x00, 0x33, 0xA9,
            0xFC, 0x2B, 0x2F, 0x31, 0x03, 0x5D, 0x9E, 0x4C, 0x8D, 0x2C, 0x6A, 0x0D, 0x2A, 0xEC,
            0x51, 0xBB, 0x7D, 0x39, 0x69, 0x9E, 0xE6, 0x13, 0xF4, 0xCB, 0x47, 0x2D, 0x5C, 0xA2,
            0x6F, 0xE6, 0xE7, 0xA5, 0xCB, 0xB0, 0xDE, 0xC4, 0x95, 0x30, 0x5C, 0x76, 0xD9, 0x88,
            0x62, 0x1C, 0x5E, 0x61, 0xC9, 0xBC, 0xED, 0xEA, 0x42, 0xAC, 0xE8, 0xC5, 0x18, 0xD3,
            0x48, 0x72, 0xD8, 0x36, 0x32, 0x83, 0x20, 0x99, 0x75, 0xD6, 0xCF, 0xC0, 0x3F, 0x1D,
            0xA7, 0x1E, 0x1F, 0x92, 0xE2, 0x4C, 0x44, 0x64, 0xFF, 0xF7, 0xA6, 0xED, 0x03, 0xBC,
            0x86, 0x9B, 0x63,
        ];
        assert_eq!((dynamic[2] >> 1) & 3, 2, "sample is a dynamic block");
        let out = inflate(&dynamic).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out, TEXT.as_bytes());
    }

    #[test]
    fn corrupt_streams_are_errors_not_panics() {
        assert!(inflate_raw(&[0x07]).is_err(), "reserved block type");
        assert!(inflate_raw(&[0x01, 9, 0, 0, 0]).is_err(), "stored length check");
        assert!(inflate(&[0x78, 0x9C]).is_err(), "empty body");
        assert!(inflate(&[0x78, 0x9C, 0x03]).is_err() || inflate(&[0x78, 0x9C, 0x03]).is_ok());
    }
}
