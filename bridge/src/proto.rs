// SPDX-License-Identifier: GPL-3.0-or-later

//! The line protocol's lexical layer, plus the bits of `.inp` reading the
//! bridge needs that the engine API does not expose.
//!
//! One request per line, one reply per line. Tokens are separated by
//! spaces; a token holding a space, a quote or a backslash is written in
//! double quotes with `\"` and `\\` escapes. Replies start with `ok`,
//! `done`, `err <code> <message>` or, while a long `until` runs, a
//! heartbeat `.. t=<seconds>` that the client ignores except to reset its
//! silence timer.
//!
//! The client in `stormsewer_swmm::bridge` carries an identical tokenizer;
//! this crate has no dependencies by design, so the forty lines are
//! duplicated rather than shared.

/// Split a request line into tokens, honouring double quotes.
pub fn tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '\\') => match chars.next() {
                Some(e @ ('"' | '\\')) => cur.push(e),
                Some(other) => {
                    cur.push('\\');
                    cur.push(other);
                }
                None => return Err("dangling backslash".into()),
            },
            (true, '"') => quoted = false,
            (true, c) => cur.push(c),
            (false, '"') => {
                quoted = true;
                in_token = true;
            }
            (false, c) if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            (false, c) => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if quoted {
        return Err("unterminated quote".into());
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

/// A token as it goes on the wire: quoted only when it has to be.
pub fn quote(s: &str) -> String {
    let plain = !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\\');
    if plain {
        return s.to_string();
    }
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// Days since 1899-12-30 for a civil date (SWMM's own epoch). Howard
/// Hinnant's `days_from_civil`, exact over the proleptic Gregorian range.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let unix = era * 146_097 + doe as i64 - 719_468;
    // 1899-12-30 is 25 569 days before 1970-01-01.
    unix + 25_569
}

/// `MM/DD/YYYY` (or with `-`), as SWMM writes dates.
fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(['/', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let m: u32 = parts[0].trim().parse().ok()?;
    let d: u32 = parts[1].trim().parse().ok()?;
    let y: i64 = parts[2].trim().parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// `HH:MM:SS`, `HH:MM`, or plain decimal hours, as seconds.
fn parse_time(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    let mut secs = 0.0;
    let mult = [3600.0, 60.0, 1.0];
    if parts.len() > 3 {
        return None;
    }
    for (p, m) in parts.iter().zip(mult) {
        let v: f64 = p.trim().parse().ok()?;
        secs += v * m;
    }
    Some(secs)
}

/// Simulation length in seconds from the `[OPTIONS]` of an `.inp`, the way
/// the engine computes `TotalDuration`: whole seconds between the start and
/// end date-times. The engine API has no property for it, so the bridge
/// reads it from the file it was asked to open. Missing options take the
/// engine's own defaults (a zero-length run); a value that will not parse
/// gives `None`.
pub fn duration_from_inp(text: &str) -> Option<f64> {
    let mut in_options = false;
    let (mut sd, mut st, mut ed, mut et) = (None, None, None, None);
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_options = line.to_ascii_uppercase().starts_with("[OPTIONS]");
            continue;
        }
        if !in_options {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(key), Some(value)) = (it.next(), it.next()) else { continue };
        match key.to_ascii_uppercase().as_str() {
            "START_DATE" => sd = Some(parse_date(value)?),
            "START_TIME" => st = Some(parse_time(value)?),
            "END_DATE" => ed = Some(parse_date(value)?),
            "END_TIME" => et = Some(parse_time(value)?),
            _ => {}
        }
    }
    let sd = sd.unwrap_or_else(|| days_from_civil(2004, 1, 1));
    let start = sd as f64 * 86_400.0 + st.unwrap_or(0.0);
    let end = ed.unwrap_or(sd) as f64 * 86_400.0 + et.unwrap_or(0.0);
    Some((end - start).floor().max(0.0))
}

/// `52004` → `5.2.4`, the engine's `xyzzz` version packing.
pub fn version_string(v: i32) -> String {
    if v <= 0 {
        return "unknown".into();
    }
    format!("{}.{}.{}", v / 10_000, (v / 1_000) % 10, v % 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_split_on_spaces_and_honour_quotes() {
        let t = tokens(r#"open "C:\Models\my pond.inp" out.rpt "a \"q\" b""#).unwrap();
        assert_eq!(t, vec!["open", r"C:\Models\my pond.inp", "out.rpt", r#"a "q" b"#]);
        assert_eq!(tokens("   ").unwrap(), Vec::<String>::new());
        assert_eq!(tokens("a  b\tc").unwrap(), vec!["a", "b", "c"]);
        assert!(tokens("\"open").is_err());
        assert_eq!(tokens(r#""\\server\share""#).unwrap(), vec![r"\server\share"]);
    }

    #[test]
    fn quote_round_trips() {
        for s in ["plain", "with space", r#"q"uote"#, r"back\slash", "", "tab\there"] {
            let line = format!("x {} y", quote(s));
            let t = tokens(&line).unwrap();
            assert_eq!(t, vec!["x", s, "y"], "{line}");
        }
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn duration_reads_the_options_block() {
        let inp = "[TITLE]\nx\n[OPTIONS]\nSTART_DATE 01/01/2007\nSTART_TIME 00:00:00\nEND_DATE 01/01/2007\nEND_TIME 12:00:00\n[JUNCTIONS]\n";
        assert_eq!(duration_from_inp(inp), Some(43_200.0));
        let two_days = "[OPTIONS]\nSTART_DATE 12/31/2019\nSTART_TIME 6\nEND_DATE 01/02/2020\nEND_TIME 06:30\n";
        assert_eq!(duration_from_inp(two_days), Some(2.0 * 86_400.0 + 1_800.0));
        assert_eq!(duration_from_inp("[OPTIONS]\nFLOW_UNITS CFS\n"), Some(0.0));
        assert_eq!(duration_from_inp("[OPTIONS]\nSTART_DATE nonsense\n"), None);
    }

    #[test]
    fn epoch_matches_swmm() {
        assert_eq!(days_from_civil(1899, 12, 30), 0);
        assert_eq!(days_from_civil(1970, 1, 1), 25_569);
        assert_eq!(days_from_civil(2020, 2, 29), 43_890);
    }

    #[test]
    fn version_unpacks() {
        assert_eq!(version_string(52_004), "5.2.4");
        assert_eq!(version_string(51_015), "5.1.15");
        assert_eq!(version_string(0), "unknown");
    }
}
