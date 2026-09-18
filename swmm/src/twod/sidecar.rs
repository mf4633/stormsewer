// SPDX-License-Identifier: GPL-3.0-or-later

//! The `<model>.2d` sidecar: a plain-text, INI-like file holding the
//! [`Config`] beside the `.inp` (which stays EPA's format untouched).
//! Sections and keywords are case-insensitive; `;` starts a comment;
//! paths with spaces are double-quoted. The full grammar is in the
//! manual's 2D methods chapter.
//!
//! ```text
//! [GRID]           DEM "path"  |  CELL size  |  WINDOW xmin ymin xmax ymax
//! [ROUGHNESS]      UNIFORM n  |  GRID "path"  |  CLASSES "path"  +  CLASS id n ...
//! [RAIN]           NONE  |  GAGE name  |  CONSTANT intensity
//! [INFILTRATION]   NONE  |  CONSTANT rate  |  HORTON f0 fc k
//! [BOUNDARY]       CLOSED  |  OPEN  |  FIXED_HEAD elevation
//! [RUN]            DURATION s | OUTPUT_STEP s | DRY_DEPTH d | COURANT c | MAX_DT s | THREADS n
//! [SOURCES]        name x y series
//! [NODES]          node MANHOLE [coeff|*] [OPEN]  |  node INLET perimeter area [coeff]  |  node SEALED
//! [BANKS]          link LEFT|RIGHT crest|DEM coeff|* x1 y1 x2 y2 ...
//! [SEALED]         node
//! ```

use std::path::PathBuf;

use crate::doc::{format_number, tokenize};
use crate::{Error, Result};

use super::{
    BankInterface, Boundary, Config, Infiltration, InterfaceKind, NodeInterface, RainOnGrid,
    Roughness, Source,
};

fn num(tok: Option<&String>, what: &str, line: usize) -> Result<f64> {
    tok.and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Format(format!("2D sidecar line {line}: {what} needs a number")))
}

fn unq(s: &str) -> String {
    crate::doc::unquote(s).to_string()
}

fn quoted(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    if s.contains(' ') || s.is_empty() {
        format!("\"{s}\"")
    } else {
        s.into_owned()
    }
}

/// Parse sidecar text.
pub fn parse(text: &str) -> Result<Config> {
    let mut cfg = Config::default();
    let mut section = String::new();
    let mut class_table: Vec<(i64, f64)> = Vec::new();
    let mut classes_raster: Option<PathBuf> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() || t.starts_with(';') {
            continue;
        }
        if t.starts_with('[') {
            section = t
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_uppercase();
            continue;
        }
        let (fields, _) = tokenize(t);
        if fields.is_empty() {
            continue;
        }
        let key = fields[0].to_ascii_uppercase();
        let f = |k: usize| fields.get(k);
        match section.as_str() {
            "GRID" => match key.as_str() {
                "DEM" => cfg.dem = f(1).map(|s| PathBuf::from(unq(s))),
                "CELL" => cfg.cell = Some(num(f(1), "CELL", line)?),
                "WINDOW" => {
                    cfg.window = Some((
                        num(f(1), "WINDOW", line)?,
                        num(f(2), "WINDOW", line)?,
                        num(f(3), "WINDOW", line)?,
                        num(f(4), "WINDOW", line)?,
                    ))
                }
                _ => return Err(unknown(&section, &key, line)),
            },
            "ROUGHNESS" => match key.as_str() {
                "UNIFORM" => cfg.roughness = Roughness::Uniform(num(f(1), "UNIFORM", line)?),
                "GRID" => {
                    cfg.roughness = Roughness::Grid(PathBuf::from(unq(f(1).ok_or_else(|| {
                        Error::Format(format!("2D sidecar line {line}: GRID needs a path"))
                    })?)))
                }
                "CLASSES" => {
                    classes_raster = Some(PathBuf::from(unq(f(1).ok_or_else(|| {
                        Error::Format(format!("2D sidecar line {line}: CLASSES needs a path"))
                    })?)))
                }
                "CLASS" => {
                    let id = num(f(1), "CLASS", line)? as i64;
                    class_table.push((id, num(f(2), "CLASS", line)?));
                }
                _ => return Err(unknown(&section, &key, line)),
            },
            "RAIN" => match key.as_str() {
                "NONE" => cfg.rain = RainOnGrid::None,
                "GAGE" => {
                    cfg.rain = RainOnGrid::Gage(unq(f(1).ok_or_else(|| {
                        Error::Format(format!("2D sidecar line {line}: GAGE needs a name"))
                    })?))
                }
                "CONSTANT" => cfg.rain = RainOnGrid::Constant(num(f(1), "CONSTANT", line)?),
                _ => return Err(unknown(&section, &key, line)),
            },
            "INFILTRATION" => match key.as_str() {
                "NONE" => cfg.infiltration = Infiltration::None,
                "CONSTANT" => cfg.infiltration = Infiltration::Constant(num(f(1), "CONSTANT", line)?),
                "HORTON" => {
                    cfg.infiltration = Infiltration::Horton {
                        f0: num(f(1), "HORTON f0", line)?,
                        fc: num(f(2), "HORTON fc", line)?,
                        k: num(f(3), "HORTON k", line)?,
                    }
                }
                _ => return Err(unknown(&section, &key, line)),
            },
            "BOUNDARY" => match key.as_str() {
                "CLOSED" => cfg.boundary = Boundary::Closed,
                "OPEN" => cfg.boundary = Boundary::Open,
                "FIXED_HEAD" | "FIXED" => {
                    cfg.boundary = Boundary::FixedHead(num(f(1), "FIXED_HEAD", line)?)
                }
                _ => return Err(unknown(&section, &key, line)),
            },
            "RUN" => match key.as_str() {
                "DURATION" => cfg.duration_s = Some(num(f(1), "DURATION", line)?),
                "OUTPUT_STEP" => cfg.output_step_s = num(f(1), "OUTPUT_STEP", line)?,
                "DRY_DEPTH" => cfg.dry_depth = num(f(1), "DRY_DEPTH", line)?,
                "COURANT" => cfg.courant = num(f(1), "COURANT", line)?,
                "MAX_DT" => cfg.max_dt_s = Some(num(f(1), "MAX_DT", line)?),
                "THREADS" => cfg.threads = num(f(1), "THREADS", line)? as usize,
                _ => return Err(unknown(&section, &key, line)),
            },
            "SOURCES" => {
                cfg.sources.push(Source {
                    name: unq(&fields[0]),
                    x: num(f(1), "source x", line)?,
                    y: num(f(2), "source y", line)?,
                    series: unq(f(3).ok_or_else(|| {
                        Error::Format(format!("2D sidecar line {line}: source needs a series"))
                    })?),
                });
            }
            "NODES" => {
                let kind_tok = f(1)
                    .map(|s| s.to_ascii_uppercase())
                    .ok_or_else(|| Error::Format(format!("2D sidecar line {line}: node needs a kind")))?;
                let coeff_of = |k: usize| -> Result<Option<f64>> {
                    match f(k).map(|s| s.as_str()) {
                        None | Some("*") => Ok(None),
                        Some(s) if s.eq_ignore_ascii_case("OPEN") => Ok(None),
                        Some(_) => Ok(Some(num(f(k), "coefficient", line)?)),
                    }
                };
                let (kind, coeff, lid_open) = match kind_tok.as_str() {
                    "MANHOLE" => (
                        InterfaceKind::Manhole,
                        coeff_of(2)?,
                        fields.iter().skip(2).any(|s| s.eq_ignore_ascii_case("OPEN")),
                    ),
                    "INLET" => (
                        InterfaceKind::Inlet {
                            perimeter: num(f(2), "inlet perimeter", line)?,
                            area: num(f(3), "inlet area", line)?,
                        },
                        coeff_of(4)?,
                        false,
                    ),
                    "SEALED" => (InterfaceKind::Sealed, None, false),
                    other => {
                        return Err(Error::Format(format!(
                            "2D sidecar line {line}: unknown node kind {other}"
                        )))
                    }
                };
                cfg.nodes.push(NodeInterface {
                    node: unq(&fields[0]),
                    kind,
                    weir_coeff: coeff,
                    lid_open,
                });
            }
            "BANKS" => {
                let side = f(1).map(|s| s.to_ascii_uppercase()).unwrap_or_default();
                let right = match side.as_str() {
                    "LEFT" => false,
                    "RIGHT" => true,
                    _ => {
                        return Err(Error::Format(format!(
                            "2D sidecar line {line}: bank side must be LEFT or RIGHT"
                        )))
                    }
                };
                let crest = match f(2).map(|s| s.as_str()) {
                    None | Some("DEM") | Some("*") => None,
                    Some(_) => Some(num(f(2), "crest", line)?),
                };
                let weir_coeff = match f(3).map(|s| s.as_str()) {
                    None | Some("*") => None,
                    Some(_) => Some(num(f(3), "coefficient", line)?),
                };
                let mut polyline = Vec::new();
                let mut k = 4;
                while fields.len() >= k + 2 {
                    polyline.push((num(f(k), "bank x", line)?, num(f(k + 1), "bank y", line)?));
                    k += 2;
                }
                cfg.banks.push(BankInterface {
                    link: unq(&fields[0]),
                    right,
                    polyline,
                    crest,
                    weir_coeff,
                });
            }
            "SEALED" => cfg.sealed.push(unq(&fields[0])),
            "" => {
                return Err(Error::Format(format!(
                    "2D sidecar line {line}: data before the first [SECTION]"
                )))
            }
            other => {
                return Err(Error::Format(format!(
                    "2D sidecar line {line}: unknown section [{other}]"
                )))
            }
        }
    }
    if let Some(raster) = classes_raster {
        cfg.roughness = Roughness::Classes {
            raster,
            table: class_table,
        };
    }
    Ok(cfg)
}

fn unknown(section: &str, key: &str, line: usize) -> Error {
    Error::Format(format!(
        "2D sidecar line {line}: unknown keyword {key} in [{section}]"
    ))
}

fn n(v: f64) -> String {
    format_number(v)
}

/// Render a config as sidecar text.
pub fn to_text(cfg: &Config) -> String {
    let mut s = String::new();
    s.push_str("; StormSewer 2D overland-flow sidecar (see manual chapter 20b)\n");
    s.push_str("[GRID]\n");
    if let Some(dem) = &cfg.dem {
        s.push_str(&format!("DEM          {}\n", quoted(dem)));
    }
    if let Some(c) = cfg.cell {
        s.push_str(&format!("CELL         {}\n", n(c)));
    }
    if let Some((a, b, c, d)) = cfg.window {
        s.push_str(&format!("WINDOW       {} {} {} {}\n", n(a), n(b), n(c), n(d)));
    }
    s.push_str("\n[ROUGHNESS]\n");
    match &cfg.roughness {
        Roughness::Uniform(v) => s.push_str(&format!("UNIFORM      {}\n", n(*v))),
        Roughness::Grid(p) => s.push_str(&format!("GRID         {}\n", quoted(p))),
        Roughness::Classes { raster, table } => {
            s.push_str(&format!("CLASSES      {}\n", quoted(raster)));
            for (id, v) in table {
                s.push_str(&format!("CLASS        {id} {}\n", n(*v)));
            }
        }
    }
    s.push_str("\n[RAIN]\n");
    match &cfg.rain {
        RainOnGrid::None => s.push_str("NONE\n"),
        RainOnGrid::Gage(g) => s.push_str(&format!("GAGE         {}\n", quote_name(g))),
        RainOnGrid::Constant(v) => s.push_str(&format!("CONSTANT     {}\n", n(*v))),
    }
    s.push_str("\n[INFILTRATION]\n");
    match cfg.infiltration {
        Infiltration::None => s.push_str("NONE\n"),
        Infiltration::Constant(v) => s.push_str(&format!("CONSTANT     {}\n", n(v))),
        Infiltration::Horton { f0, fc, k } => {
            s.push_str(&format!("HORTON       {} {} {}\n", n(f0), n(fc), n(k)))
        }
    }
    s.push_str("\n[BOUNDARY]\n");
    match cfg.boundary {
        Boundary::Closed => s.push_str("CLOSED\n"),
        Boundary::Open => s.push_str("OPEN\n"),
        Boundary::FixedHead(h) => s.push_str(&format!("FIXED_HEAD   {}\n", n(h))),
    }
    s.push_str("\n[RUN]\n");
    if let Some(d) = cfg.duration_s {
        s.push_str(&format!("DURATION     {}\n", n(d)));
    }
    s.push_str(&format!("OUTPUT_STEP  {}\n", n(cfg.output_step_s)));
    s.push_str(&format!("DRY_DEPTH    {}\n", n(cfg.dry_depth)));
    s.push_str(&format!("COURANT      {}\n", n(cfg.courant)));
    if let Some(m) = cfg.max_dt_s {
        s.push_str(&format!("MAX_DT       {}\n", n(m)));
    }
    s.push_str(&format!("THREADS      {}\n", cfg.threads));
    s.push_str("\n[SOURCES]\n;;Name  X  Y  Series\n");
    for src in &cfg.sources {
        s.push_str(&format!(
            "{} {} {} {}\n",
            quote_name(&src.name),
            n(src.x),
            n(src.y),
            quote_name(&src.series)
        ));
    }
    s.push_str("\n[NODES]\n;;Node  MANHOLE [coeff] [OPEN] | INLET perimeter area [coeff] | SEALED\n");
    for nd in &cfg.nodes {
        let c = nd.weir_coeff.map(n).unwrap_or_else(|| "*".into());
        match nd.kind {
            InterfaceKind::Manhole => {
                s.push_str(&format!("{} MANHOLE {c}", quote_name(&nd.node)));
                if nd.lid_open {
                    s.push_str(" OPEN");
                }
                s.push('\n');
            }
            InterfaceKind::Inlet { perimeter, area } => s.push_str(&format!(
                "{} INLET {} {} {c}\n",
                quote_name(&nd.node),
                n(perimeter),
                n(area)
            )),
            InterfaceKind::Sealed => s.push_str(&format!("{} SEALED\n", quote_name(&nd.node))),
        }
    }
    s.push_str("\n[BANKS]\n;;Link  LEFT|RIGHT  crest|DEM  coeff|*  x1 y1 x2 y2 ...\n");
    for b in &cfg.banks {
        s.push_str(&format!(
            "{} {} {} {}",
            quote_name(&b.link),
            if b.right { "RIGHT" } else { "LEFT" },
            b.crest.map(n).unwrap_or_else(|| "DEM".into()),
            b.weir_coeff.map(n).unwrap_or_else(|| "*".into())
        ));
        for (x, y) in &b.polyline {
            s.push_str(&format!(" {} {}", n(*x), n(*y)));
        }
        s.push('\n');
    }
    s.push_str("\n[SEALED]\n");
    for name in &cfg.sealed {
        s.push_str(&format!("{}\n", quote_name(name)));
    }
    s
}

fn quote_name(name: &str) -> String {
    if name.contains(' ') || name.is_empty() {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_config_round_trips() {
        let cfg = Config {
            dem: Some(PathBuf::from("C:/data/my dem.asc")),
            cell: Some(2.5),
            window: Some((0.0, 10.0, 200.0, 310.5)),
            roughness: Roughness::Classes {
                raster: PathBuf::from("lc.asc"),
                table: vec![(11, 0.03), (22, 0.1)],
            },
            rain: RainOnGrid::Gage("RainGage".into()),
            infiltration: Infiltration::Horton {
                f0: 3.0,
                fc: 0.5,
                k: 4.14,
            },
            boundary: Boundary::FixedHead(98.25),
            duration_s: Some(7200.0),
            output_step_s: 60.0,
            dry_depth: 0.01,
            courant: 0.5,
            max_dt_s: Some(2.0),
            sources: vec![Source {
                name: "culvert out".into(),
                x: 12.5,
                y: 40.0,
                series: "Q1".into(),
            }],
            nodes: vec![
                NodeInterface {
                    node: "J1".into(),
                    kind: InterfaceKind::Manhole,
                    weir_coeff: Some(0.5),
                    lid_open: true,
                },
                NodeInterface {
                    node: "J2".into(),
                    kind: InterfaceKind::Inlet {
                        perimeter: 3.0,
                        area: 0.2,
                    },
                    weir_coeff: None,
                    lid_open: false,
                },
                NodeInterface {
                    node: "J3".into(),
                    kind: InterfaceKind::Sealed,
                    weir_coeff: None,
                    lid_open: false,
                },
            ],
            banks: vec![BankInterface {
                link: "C1".into(),
                right: true,
                polyline: vec![(0.0, 0.0), (10.0, 5.0)],
                crest: None,
                weir_coeff: Some(0.7),
            }],
            sealed: vec!["O1".into()],
            threads: 4,
        };
        let text = to_text(&cfg);
        let back = parse(&text).unwrap();
        assert_eq!(back, cfg, "{text}");
        let again = to_text(&back);
        assert_eq!(again, text);
    }

    #[test]
    fn defaults_and_errors() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(
            parse("[RAIN]\nCONSTANT 25\n[BOUNDARY]\nOPEN\n").unwrap().rain,
            RainOnGrid::Constant(25.0)
        );
        assert!(parse("[GRID]\nCELL abc\n").is_err());
        assert!(parse("[NOPE]\nX 1\n").is_err());
        assert!(parse("DEM x.asc\n").is_err());
        assert!(parse("[NODES]\nJ1 INLET 3\n").is_err());
    }
}
