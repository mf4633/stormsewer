// SPDX-License-Identifier: GPL-3.0-or-later

//! The model's coordinate reference system lives in a sidecar file,
//! `<model>.crs`, beside the `.inp` — never inside it, because the EPA
//! engine rejects a section it does not know and the `.inp` must stay a
//! file every SWMM tool can read.
//!
//! The sidecar is plain text: comment lines start with `#`, an
//! `EPSG:nnnn` line gives the code when one is known, and the rest is
//! the WKT. Either the code or the WKT alone is enough to read it back.

use std::path::{Path, PathBuf};

use crate::gis::crs::Crs;
use crate::{Error, Result};

pub const EXTENSION: &str = "crs";

/// `model.inp` → `model.crs`.
pub fn path_for(model: &Path) -> PathBuf {
    model.with_extension(EXTENSION)
}

/// The sidecar text for a system.
pub fn encode(crs: &Crs) -> String {
    let mut s = String::from(
        "# StormSewer model coordinate reference system.\n# Sidecar to the .inp beside it; the engine never reads this file.\n",
    );
    if let Some(code) = crs.epsg {
        s.push_str(&format!("EPSG:{code}\n"));
    }
    s.push_str(&crs.to_wkt());
    s.push('\n');
    s
}

/// Parse sidecar text. WKT wins when both are present and parse; the
/// EPSG line is used when the WKT is missing or unreadable.
pub fn parse(text: &str) -> Result<Crs> {
    let mut epsg: Option<u32> = None;
    let mut wkt = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(code) = t.strip_prefix("EPSG:").and_then(|c| c.trim().parse::<u32>().ok()) {
            epsg = Some(code);
            continue;
        }
        wkt.push_str(t);
        wkt.push('\n');
    }
    let from_wkt = if wkt.trim().is_empty() {
        None
    } else {
        Some(Crs::from_wkt(&wkt))
    };
    match (from_wkt, epsg) {
        (Some(Ok(mut c)), code) => {
            if c.epsg.is_none() {
                c.epsg = code;
            }
            Ok(c)
        }
        (Some(Err(e)), Some(code)) => Crs::from_epsg(code).map_err(|_| e),
        (Some(Err(e)), None) => Err(e),
        (None, Some(code)) => Crs::from_epsg(code),
        (None, None) => Err(Error::Format("the .crs sidecar names no coordinate system".into())),
    }
}

/// Read the sidecar for a model path; `Ok(None)` when there is none.
pub fn read(model: &Path) -> Result<Option<Crs>> {
    let p = path_for(model);
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p)?;
    parse(&text).map(Some)
}

pub fn write(model: &Path, crs: &Crs) -> Result<PathBuf> {
    let p = path_for(model);
    std::fs::write(&p, encode(crs))?;
    Ok(p)
}

/// Delete the sidecar, if any.
pub fn remove(model: &Path) -> Result<()> {
    let p = path_for(model);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_round_trips_and_falls_back_to_the_code() {
        let dir = std::env::temp_dir().join("stormsewer-gis-tests").join("sidecar");
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("site.inp");
        assert_eq!(read(&model).unwrap(), None);
        let crs = Crs::from_epsg(2264).unwrap();
        let p = write(&model, &crs).unwrap();
        assert_eq!(p, dir.join("site.crs"));
        let back = read(&model).unwrap().unwrap();
        assert_eq!(back.epsg, Some(2264));
        assert_eq!(back.unit.name, "US survey foot");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("EPSG:2264") && text.contains("PROJCS["));
        assert_eq!(parse("EPSG:32617\n").unwrap().name, "WGS 84 / UTM zone 17N");
        assert_eq!(parse("# only a comment\nEPSG:4326\nPROJCRS[\"wkt2\"]\n").unwrap().epsg, Some(4326));
        assert!(parse("# nothing\n").is_err());
        remove(&model).unwrap();
        assert_eq!(read(&model).unwrap(), None);
    }
}
