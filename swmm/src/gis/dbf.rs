// SPDX-License-Identifier: GPL-3.0-or-later

//! dBASE III/IV `.dbf` attribute tables, as shapefiles carry them.
//!
//! Layout (public dBASE III file structure): a 32-byte header (version,
//! date, record count, header length, record length, language driver ID
//! at byte 29), one 32-byte descriptor per field (11-byte name, type
//! letter, length, decimals) ended by `0x0D`, then fixed-width records
//! each led by a deletion flag (`0x20` live, `0x2A` deleted).
//!
//! Text is decoded by the language driver ID and the `.cpg` sidecar: UTF-8
//! when either says so, Windows-1252 for the ANSI drivers, and Latin-1
//! (one byte, one character) for everything else — a multi-byte East
//! Asian code page therefore comes through as mojibake rather than an
//! error, which the import dialog can still work with.

use std::path::Path;

use crate::gis::vector::{Field, FieldKind, FieldValue};
use crate::{Error, Result};

/// How the text bytes are decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Cp1252,
    Latin1,
}

impl Encoding {
    /// From the language driver ID byte (header byte 29). Only the IDs
    /// that name a single-byte Windows/ISO page are recognised.
    pub fn from_ldid(ldid: u8) -> Option<Self> {
        match ldid {
            0x03 | 0x57 => Some(Self::Cp1252),
            // 0x00 means "not stated" (DBF writers that put UTF-8 there
            // rely on the .cpg file).
            0x00 => None,
            _ => Some(Self::Latin1),
        }
    }

    /// From the text of a `.cpg` sidecar (`UTF-8`, `ISO-8859-1`, `1252`, ...).
    pub fn from_cpg(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_uppercase().replace(['-', '_', ' '], "");
        if t.contains("UTF8") {
            Some(Self::Utf8)
        } else if t.contains("1252") || t == "ANSI" {
            Some(Self::Cp1252)
        } else if t.contains("8859") || t.contains("LATIN1") {
            Some(Self::Latin1)
        } else {
            None
        }
    }

    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                Err(_) => Self::Latin1.decode(bytes),
            },
            Self::Cp1252 => bytes.iter().map(|&b| cp1252_char(b)).collect(),
            Self::Latin1 => bytes.iter().map(|&b| b as char).collect(),
        }
    }
}

/// Windows-1252's 0x80–0x9F block; the rest is Latin-1.
fn cp1252_char(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}',
        '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
    ];
    if (0x80..=0x9F).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        b as char
    }
}

/// One field's on-disk description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbfField {
    pub name: String,
    pub kind: u8,
    pub length: u8,
    pub decimals: u8,
}

impl DbfField {
    pub fn to_field(&self) -> Field {
        let kind = match self.kind {
            b'N' | b'F' if self.decimals == 0 => FieldKind::Integer,
            b'N' | b'F' | b'B' | b'Y' => FieldKind::Number,
            b'L' => FieldKind::Bool,
            b'D' => FieldKind::Date,
            _ => FieldKind::Text,
        };
        Field::new(&self.name, kind)
    }
}

/// A read table.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub fields: Vec<DbfField>,
    /// Live records only (deleted ones are skipped), one value per field.
    pub records: Vec<Vec<FieldValue>>,
    pub encoding: Encoding,
}

impl Table {
    pub fn fields(&self) -> Vec<Field> {
        self.fields.iter().map(DbfField::to_field).collect()
    }
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Read a `.dbf` file; a `.cpg` beside it, when present, names the text
/// encoding.
pub fn read(path: &Path) -> Result<Table> {
    let bytes = std::fs::read(path)?;
    let cpg = path.with_extension("cpg");
    let hint = std::fs::read_to_string(&cpg)
        .ok()
        .and_then(|t| Encoding::from_cpg(&t));
    parse(&bytes, hint)
}

/// Parse a `.dbf` image. `hint` (from a `.cpg`) wins over the header's
/// language driver ID; with neither, text that is valid UTF-8 is taken as
/// UTF-8 and anything else as Latin-1.
pub fn parse(bytes: &[u8], hint: Option<Encoding>) -> Result<Table> {
    if bytes.len() < 33 {
        return Err(Error::Format("dBASE file is too short for a header".into()));
    }
    let n_records = u32le(bytes, 4) as usize;
    let header_len = u16le(bytes, 8) as usize;
    let record_len = u16le(bytes, 10) as usize;
    let ldid = bytes[29];
    if header_len < 33 || header_len > bytes.len() {
        return Err(Error::Format(format!("dBASE header length {header_len} is impossible")));
    }
    let mut fields = Vec::new();
    let mut off = 32;
    while off + 32 <= header_len && bytes[off] != 0x0D {
        let raw = &bytes[off..off + 32];
        let name_end = raw[..11].iter().position(|&b| b == 0).unwrap_or(11);
        let name = String::from_utf8_lossy(&raw[..name_end]).trim().to_string();
        fields.push(DbfField {
            name,
            kind: raw[11],
            length: raw[16],
            decimals: raw[17],
        });
        off += 32;
    }
    let width: usize = 1 + fields.iter().map(|f| f.length as usize).sum::<usize>();
    if record_len != 0 && record_len != width {
        // Some writers pad the record; trust the header's record length
        // for stepping, the descriptors for slicing.
        if record_len < width {
            return Err(Error::Format(format!(
                "dBASE record length {record_len} is shorter than its fields ({width})"
            )));
        }
    }
    let step = if record_len == 0 { width } else { record_len };
    let encoding = hint.or_else(|| Encoding::from_ldid(ldid)).unwrap_or_else(|| {
        let body = &bytes[header_len..];
        if std::str::from_utf8(body).is_ok() {
            Encoding::Utf8
        } else {
            Encoding::Latin1
        }
    });
    let mut records = Vec::with_capacity(n_records);
    let mut pos = header_len;
    for _ in 0..n_records {
        if pos + step > bytes.len() {
            // A truncated file: keep what was complete.
            if pos + width > bytes.len() {
                break;
            }
        }
        let rec = &bytes[pos..(pos + step).min(bytes.len())];
        pos += step;
        if rec[0] == 0x2A {
            continue;
        }
        let mut values = Vec::with_capacity(fields.len());
        let mut fo = 1;
        for f in &fields {
            let len = f.length as usize;
            let raw = &rec[fo..(fo + len).min(rec.len())];
            fo += len;
            values.push(decode_value(f, raw, encoding));
        }
        records.push(values);
    }
    Ok(Table {
        fields,
        records,
        encoding,
    })
}

fn decode_value(f: &DbfField, raw: &[u8], enc: Encoding) -> FieldValue {
    let text = || enc.decode(raw);
    match f.kind {
        b'N' | b'F' | b'B' | b'Y' => {
            let t = String::from_utf8_lossy(raw);
            let t = t.trim();
            if t.is_empty() || t.starts_with('*') {
                FieldValue::Null
            } else {
                t.parse::<f64>().map_or(FieldValue::Null, FieldValue::Number)
            }
        }
        b'L' => match raw.first().copied().unwrap_or(b'?') {
            b'T' | b't' | b'Y' | b'y' => FieldValue::Bool(true),
            b'F' | b'f' | b'N' | b'n' => FieldValue::Bool(false),
            _ => FieldValue::Null,
        },
        b'D' => {
            let t = String::from_utf8_lossy(raw).trim().to_string();
            if t.is_empty() {
                FieldValue::Null
            } else {
                FieldValue::Date(t)
            }
        }
        _ => {
            let t = text();
            let t = t.trim_end_matches(['\0', ' ']).to_string();
            if t.is_empty() {
                FieldValue::Null
            } else {
                FieldValue::Text(t)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// A dBASE-safe field name: ASCII letters, digits and underscore, at most
/// 10 characters, unique within `taken` (a numeric suffix is appended).
pub fn safe_name(name: &str, taken: &[String]) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c.to_ascii_uppercase() } else { '_' })
        .collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, 'F');
    }
    s.truncate(10);
    if !taken.iter().any(|t| t == &s) {
        return s;
    }
    for n in 1..1000 {
        let suffix = n.to_string();
        let mut c = s.clone();
        c.truncate(10 - suffix.len());
        c.push_str(&suffix);
        if !taken.iter().any(|t| t == &c) {
            return c;
        }
    }
    s
}

/// Field widths chosen from the values so nothing is clipped.
fn plan(fields: &[Field], records: &[Vec<FieldValue>]) -> Vec<DbfField> {
    let mut names: Vec<String> = Vec::new();
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let name = safe_name(&f.name, &names);
            names.push(name.clone());
            let col = records.iter().filter_map(|r| r.get(i));
            match f.kind {
                FieldKind::Text => {
                    let w = col
                        .map(|v| v.to_field().len())
                        .max()
                        .unwrap_or(1)
                        .clamp(1, 254);
                    DbfField {
                        name,
                        kind: b'C',
                        length: w as u8,
                        decimals: 0,
                    }
                }
                FieldKind::Integer => DbfField {
                    name,
                    kind: b'N',
                    length: 18,
                    decimals: 0,
                },
                FieldKind::Number => DbfField {
                    name,
                    kind: b'N',
                    length: 24,
                    decimals: 8,
                },
                FieldKind::Bool => DbfField {
                    name,
                    kind: b'L',
                    length: 1,
                    decimals: 0,
                },
                FieldKind::Date => DbfField {
                    name,
                    kind: b'D',
                    length: 8,
                    decimals: 0,
                },
            }
        })
        .collect()
}

/// Encode a table (UTF-8 text; write a `.cpg` saying so beside it, which
/// [`write`] does). Numbers are right-justified in a fixed width.
pub fn encode(fields: &[Field], records: &[Vec<FieldValue>]) -> Vec<u8> {
    let descs = plan(fields, records);
    let record_len: usize = 1 + descs.iter().map(|d| d.length as usize).sum::<usize>();
    let header_len = 32 + 32 * descs.len() + 1;
    let mut out = Vec::with_capacity(header_len + record_len * records.len() + 1);
    out.push(0x03);
    out.extend_from_slice(&[124, 1, 1]); // 2024-01-01: the date is not meaningful here
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    out.extend_from_slice(&(header_len as u16).to_le_bytes());
    out.extend_from_slice(&(record_len as u16).to_le_bytes());
    out.extend_from_slice(&[0; 17]);
    out.push(0x57); // ANSI language driver; the .cpg overrides it with UTF-8
    out.extend_from_slice(&[0; 2]);
    for d in &descs {
        let mut name = [0u8; 11];
        for (i, b) in d.name.bytes().take(10).enumerate() {
            name[i] = b;
        }
        out.extend_from_slice(&name);
        out.push(d.kind);
        out.extend_from_slice(&[0; 4]);
        out.push(d.length);
        out.push(d.decimals);
        out.extend_from_slice(&[0; 14]);
    }
    out.push(0x0D);
    for rec in records {
        out.push(0x20);
        for (i, d) in descs.iter().enumerate() {
            let v = rec.get(i).unwrap_or(&FieldValue::Null);
            let w = d.length as usize;
            let cell: Vec<u8> = match d.kind {
                b'N' => {
                    let s = match v.as_f64() {
                        Some(x) if d.decimals == 0 => format!("{}", x.round() as i64),
                        Some(x) => {
                            let s = format!("{x:.*}", d.decimals as usize);
                            if s.len() > w {
                                crate::doc::format_number(x)
                            } else {
                                s
                            }
                        }
                        None => String::new(),
                    };
                    let mut c = vec![b' '; w];
                    let b = s.as_bytes();
                    let take = b.len().min(w);
                    c[w - take..].copy_from_slice(&b[b.len() - take..]);
                    c
                }
                b'L' => vec![match v {
                    FieldValue::Bool(true) => b'T',
                    FieldValue::Bool(false) => b'F',
                    _ => b'?',
                }],
                b'D' => {
                    let s = v.to_field().replace('-', "");
                    let mut c = vec![b' '; 8];
                    let b = s.as_bytes();
                    let take = b.len().min(8);
                    c[..take].copy_from_slice(&b[..take]);
                    c
                }
                _ => {
                    let s = v.to_field();
                    let mut c = vec![b' '; w];
                    // Cut on a character boundary so a clipped UTF-8 cell
                    // stays decodable.
                    let mut end = s.len().min(w);
                    while end > 0 && !s.is_char_boundary(end) {
                        end -= 1;
                    }
                    c[..end].copy_from_slice(&s.as_bytes()[..end]);
                    c
                }
            };
            out.extend_from_slice(&cell);
        }
    }
    out.push(0x1A);
    out
}

/// Write `path` (the `.dbf`) and a `.cpg` beside it declaring UTF-8.
pub fn write(path: &Path, fields: &[Field], records: &[Vec<FieldValue>]) -> Result<()> {
    std::fs::write(path, encode(fields, records))?;
    std::fs::write(path.with_extension("cpg"), "UTF-8\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<Field>, Vec<Vec<FieldValue>>) {
        let fields = vec![
            Field::new("Name", FieldKind::Text),
            Field::new("Invert", FieldKind::Number),
            Field::new("Count", FieldKind::Integer),
            Field::new("Gated", FieldKind::Bool),
            Field::new("Built", FieldKind::Date),
        ];
        let records = vec![
            vec![
                FieldValue::Text("J1".into()),
                FieldValue::Number(101.25),
                FieldValue::Number(3.0),
                FieldValue::Bool(true),
                FieldValue::Date("2020-05-17".into()),
            ],
            vec![
                FieldValue::Text("Café".into()),
                FieldValue::Null,
                FieldValue::Number(-2.0),
                FieldValue::Null,
                FieldValue::Null,
            ],
        ];
        (fields, records)
    }

    #[test]
    fn round_trips_char_numeric_logical_and_date_fields() {
        let (fields, records) = sample();
        let bytes = encode(&fields, &records);
        let t = parse(&bytes, Some(Encoding::Utf8)).unwrap();
        assert_eq!(t.fields.len(), 5);
        assert_eq!(t.fields[0].name, "NAME");
        assert_eq!(t.fields[1].kind, b'N');
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[0][0], FieldValue::Text("J1".into()));
        assert_eq!(t.records[0][1], FieldValue::Number(101.25));
        assert_eq!(t.records[0][2], FieldValue::Number(3.0));
        assert_eq!(t.records[0][3], FieldValue::Bool(true));
        assert_eq!(t.records[0][4], FieldValue::Date("20200517".into()));
        assert_eq!(t.records[1][0], FieldValue::Text("Café".into()));
        assert_eq!(t.records[1][1], FieldValue::Null);
        assert_eq!(t.records[1][3], FieldValue::Null);
        let f = t.fields();
        assert_eq!(f[2].kind, FieldKind::Integer);
        assert_eq!(f[1].kind, FieldKind::Number);
    }

    #[test]
    fn without_a_hint_utf8_is_sniffed_and_ansi_ldid_means_cp1252() {
        let (fields, records) = sample();
        let mut bytes = encode(&fields, &records);
        bytes[29] = 0x00;
        let t = parse(&bytes, None).unwrap();
        assert_eq!(t.encoding, Encoding::Utf8);
        assert_eq!(t.records[1][0], FieldValue::Text("Café".into()));
        // Re-encode the é as a single 0xE9 byte with the ANSI driver id.
        let mut latin = encode(
            &fields[..1],
            &[vec![FieldValue::Text("Caf\u{e9}\u{2122}".into())]],
        );
        let hl = u16le(&latin, 8) as usize;
        latin.truncate(hl);
        latin.extend_from_slice(&[0x20, b'C', b'a', b'f', 0xE9, 0x99, 0x20]);
        // Fix the record length to 7 bytes with one 6-wide field.
        latin[10] = 7;
        latin[11] = 0;
        latin[32 + 16] = 6;
        latin[4] = 1;
        latin[29] = 0x57;
        let t = parse(&latin, None).unwrap();
        assert_eq!(t.encoding, Encoding::Cp1252);
        assert_eq!(t.records[0][0], FieldValue::Text("Caf\u{e9}\u{2122}".into()));
        latin[29] = 0x64; // an East-European DOS page: falls back to Latin-1
        let t = parse(&latin, None).unwrap();
        assert_eq!(t.encoding, Encoding::Latin1);
        assert_eq!(t.records[0][0], FieldValue::Text("Caf\u{e9}\u{99}".into()));
    }

    #[test]
    fn deleted_records_are_skipped_and_names_are_made_safe() {
        let (fields, records) = sample();
        let mut bytes = encode(&fields, &records);
        let hl = u16le(&bytes, 8) as usize;
        bytes[hl] = 0x2A;
        let t = parse(&bytes, Some(Encoding::Utf8)).unwrap();
        assert_eq!(t.records.len(), 1);
        assert_eq!(safe_name("max depth (ft)", &[]), "MAX_DEPTH_");
        assert_eq!(safe_name("MAX_DEPTH_", &["MAX_DEPTH_".into()]), "MAX_DEPTH1");
        assert_eq!(safe_name("1st", &[]), "F1ST");
        assert_eq!(Encoding::from_cpg("utf-8"), Some(Encoding::Utf8));
        assert_eq!(Encoding::from_cpg("ISO-8859-1"), Some(Encoding::Latin1));
    }
}
