// SPDX-License-Identifier: GPL-3.0-or-later

//! Lossless round-trip of the EPA sample models and of synthetic edge cases
//! through `stormsewer_swmm::doc::InpDoc`.

use std::fs;
use std::path::PathBuf;

use stormsewer_swmm::doc::InpDoc;

fn fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epa-samples");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).expect("fixture dir") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "inp") {
            let bytes = fs::read(&path).unwrap();
            let text = String::from_utf8(bytes).expect("fixture is UTF-8");
            out.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                text,
            ));
        }
    }
    out.sort();
    assert_eq!(out.len(), 7, "expected the seven EPA samples");
    out
}

fn assert_round_trip(label: &str, text: &str) {
    let doc = InpDoc::parse(text);
    let back = doc.to_string();
    if back != text {
        let diff = back
            .bytes()
            .zip(text.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(back.len().min(text.len()));
        panic!(
            "{label}: round trip differs at byte {diff} (lengths {} vs {}):\n  got  {:?}\n  want {:?}",
            back.len(),
            text.len(),
            &back[diff.saturating_sub(40)..(diff + 40).min(back.len())],
            &text[diff.saturating_sub(40)..(diff + 40).min(text.len())],
        );
    }
}

#[test]
fn epa_samples_round_trip_byte_for_byte() {
    for (name, text) in fixtures() {
        assert!(
            text.contains("\r\n"),
            "{name}: the EPA samples are CRLF; a checkout normalised it"
        );
        assert_round_trip(&name, &text);
    }
}

#[test]
fn epa_samples_keep_every_section_and_row() {
    for (name, text) in fixtures() {
        let doc = InpDoc::parse(&text);
        let headers = text
            .lines()
            .filter(|l| l.trim_start().starts_with('['))
            .count();
        assert_eq!(doc.sections().len(), headers, "{name}: section count");
        // Every non-blank, non-comment line inside a section is a data row.
        let data = text
            .lines()
            .filter(|l| {
                !l.trim().is_empty()
                    && !l.trim_start().starts_with(';')
                    && !l.trim_start().starts_with('[')
            })
            .count();
        let rows: usize = doc.sections().iter().map(|s| s.rows().count()).sum();
        assert_eq!(rows, data, "{name}: data row count");
    }
}

/// Deterministic shuffle so the test is reproducible.
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    for i in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        items.swap(i, j);
    }
}

#[test]
fn shuffled_sections_still_round_trip() {
    for (name, text) in fixtures() {
        let doc = InpDoc::parse(&text);
        // Rebuild the text from the sections in a different order, several
        // times, and check each ordering survives parse → to_string.
        for seed in 1..=5u64 {
            let mut order: Vec<usize> = (0..doc.sections().len()).collect();
            shuffle(&mut order, seed);
            let mut rebuilt = String::new();
            for i in order {
                let s = &doc.sections()[i];
                rebuilt.push_str(&s.header);
                rebuilt.push_str(s.ending.as_str());
                for l in &s.lines {
                    rebuilt.push_str(&l.text);
                    rebuilt.push_str(l.ending.as_str());
                }
                // A section that ended the file without a newline now sits
                // in the middle; give it one so headers stay on their own line.
                if !rebuilt.ends_with('\n') {
                    rebuilt.push_str("\r\n");
                }
            }
            assert_round_trip(&format!("{name} seed {seed}"), &rebuilt);
            let again = InpDoc::parse(&rebuilt);
            assert_eq!(
                again.sections().len(),
                doc.sections().len(),
                "{name}: shuffled section count"
            );
            assert_eq!(
                again.names("JUNCTIONS"),
                doc.names("JUNCTIONS"),
                "{name}: shuffled junctions"
            );
            assert_eq!(
                again.names("CONDUITS"),
                doc.names("CONDUITS"),
                "{name}: shuffled conduits"
            );
        }
    }
}

#[test]
fn synthetic_edge_cases_round_trip() {
    let cases: &[(&str, &str)] = &[
        ("bom + lf", "\u{feff}[TITLE]\nA model\n\n[JUNCTIONS]\nJ1 10 0\n"),
        ("no trailing newline", "[JUNCTIONS]\nJ1 10 0"),
        ("header without newline", "[JUNCTIONS]\nJ1 10 0\n[TAGS]"),
        ("mixed endings", "[TITLE]\r\nline one\nline two\r\n[OPTIONS]\nFLOW_UNITS CFS\r\n"),
        ("tabs and trailing whitespace", "[OPTIONS]\r\n;;Option\t\tValue   \r\nFLOW_UNITS\t\tCFS  \r\nROUTING_STEP        \t0:00:15 \r\n"),
        ("header case and trailing text", "[Polygons] ; outlines\nS1 0 0\nS1 1 1\n[coordinates]\nJ1 0 0\n"),
        ("unknown future section", "[JUNCTIONS]\nJ1 10 0\n\n[QUANTUM_FLUX]\n;;Name   Spin\nQ1   up   ; not in 5.2\n"),
        ("preamble text", "Some text before the first header\r\n\r\n[JUNCTIONS]\r\nJ1 10 0\r\n"),
        ("inline comments", "[JUNCTIONS]\nJ1 10 0 ; the inlet\nJ2 9 0;no space\n;\n;; ruler follows\n;;--------\n"),
        ("quoted label with semicolon", "[LABELS]\n10 20 \"Outfall; main\" \"\" \"Arial\" 12 0 0\n"),
        ("blank lines inside sections", "[OPTIONS]\nFLOW_UNITS CFS\n\n\nSTART_DATE 01/01/2020\n\n"),
        ("empty sections back to back", "[TAGS]\n[MAP]\n[COORDINATES]\n"),
        ("bare cr inside line", "[TITLE]\nweird\rline\n"),
        ("empty file", ""),
        ("only a header", "[JUNCTIONS]"),
        ("only newlines", "\r\n\n\r\n"),
        ("leading whitespace before header", "  [JUNCTIONS]\nJ1 10 0\n"),
        ("crlf everywhere", "[TITLE]\r\n\r\n[OPTIONS]\r\nFLOW_UNITS CFS\r\n"),
    ];
    for (label, text) in cases {
        assert_round_trip(label, text);
    }
}

#[test]
fn parse_classifies_lines() {
    let doc =
        InpDoc::parse("\u{feff}pre\n[Polygons] ; outlines\n;;Sub X Y\nS1 0 0 ; corner\n\nS1 1 1\n");
    assert!(doc.has_bom());
    assert_eq!(doc.preamble().len(), 1);
    let s = doc.section("polygons").expect("case-insensitive lookup");
    assert_eq!(s.name, "POLYGONS");
    assert_eq!(s.header, "[Polygons] ; outlines");
    assert_eq!(s.lines.len(), 4);
    assert!(s.lines[0].is_comment());
    let row = s.lines[1].row.as_ref().unwrap();
    assert_eq!(row.fields, vec!["S1", "0", "0"]);
    assert_eq!(row.comment.as_deref(), Some("; corner"));
    assert!(s.lines[2].is_blank());
    assert_eq!(doc.polygon("s1"), vec![(0.0, 0.0), (1.0, 1.0)]);
}
