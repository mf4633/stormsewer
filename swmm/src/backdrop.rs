// SPDX-License-Identifier: GPL-3.0-or-later

//! The `[BACKDROP]` section and the world files that georeference an
//! image, plus `[MAP] DIMENSIONS`.
//!
//! SWMM's own section (EPA SWMM 5.2 User's Manual, Appendix D.1):
//!
//! ```text
//! [BACKDROP]
//! FILE       "site.png"
//! DIMENSIONS x1 y1 x2 y2      ; lower-left and upper-right map coordinates
//! UNITS      NONE | FEET | METERS | DEGREES
//! OFFSET     x y
//! SCALING    xfactor yfactor
//! ```
//!
//! A world file (`.pgw` for PNG, `.jgw` for JPEG, or `.wld`) is six lines
//! — the pixel width `A`, the two rotation terms `D` and `B`, the pixel
//! height `E` (negative: rows run down), and the centre of the top-left
//! pixel `C`, `F` — the ESRI convention every GIS writes. With it, the
//! image's DIMENSIONS follow from its pixel size; rotation is ignored with
//! a note, since SWMM cannot draw a rotated backdrop.

use std::path::{Path, PathBuf};

use crate::doc::{format_number, Command, InpDoc};

/// The six-parameter affine of a world file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldFile {
    /// Pixel width in map units (x change per column).
    pub a: f64,
    /// Row rotation (y change per column).
    pub d: f64,
    /// Column rotation (x change per row).
    pub b: f64,
    /// Pixel height in map units, negative for north-up images.
    pub e: f64,
    /// Map x of the centre of the top-left pixel.
    pub c: f64,
    /// Map y of the centre of the top-left pixel.
    pub f: f64,
}

impl WorldFile {
    /// Parse the six lines. Blank lines and `;`/`#` comments are skipped.
    pub fn parse(text: &str) -> Result<Self, String> {
        let nums: Vec<f64> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with(';') && !l.starts_with('#'))
            .map(|l| {
                l.parse::<f64>()
                    .map_err(|_| format!("world file: {l:?} is not a number"))
            })
            .collect::<Result<_, _>>()?;
        if nums.len() != 6 {
            return Err(format!(
                "world file: expected 6 numbers, found {}",
                nums.len()
            ));
        }
        let w = Self {
            a: nums[0],
            d: nums[1],
            b: nums[2],
            e: nums[3],
            c: nums[4],
            f: nums[5],
        };
        if w.a == 0.0 || w.e == 0.0 {
            return Err("world file: pixel size is zero".into());
        }
        Ok(w)
    }

    /// Whether the file carries rotation, which SWMM cannot show.
    pub fn is_rotated(&self) -> bool {
        self.b.abs() > 1e-12 || self.d.abs() > 1e-12
    }

    /// The image's extent `(x_min, y_min, x_max, y_max)` in map units for
    /// an image `width × height` pixels: the outer edges of the corner
    /// pixels, rotation ignored.
    pub fn extent(&self, width: u32, height: u32) -> (f64, f64, f64, f64) {
        let x_left = self.c - self.a / 2.0;
        let x_right = self.c + self.a * (width as f64 - 0.5);
        let y_top = self.f - self.e / 2.0;
        let y_bottom = self.f + self.e * (height as f64 - 0.5);
        (
            x_left.min(x_right),
            y_top.min(y_bottom),
            x_left.max(x_right),
            y_top.max(y_bottom),
        )
    }

    /// The world file for a given extent and pixel size (for writing one
    /// back out beside a georeferenced image).
    pub fn from_extent(extent: (f64, f64, f64, f64), width: u32, height: u32) -> Self {
        let (x0, y0, x1, y1) = extent;
        let a = (x1 - x0) / width.max(1) as f64;
        let e = -(y1 - y0) / height.max(1) as f64;
        Self {
            a,
            d: 0.0,
            b: 0.0,
            e,
            c: x0 + a / 2.0,
            f: y1 + e / 2.0,
        }
    }

    pub fn to_text(&self) -> String {
        [self.a, self.d, self.b, self.e, self.c, self.f]
            .iter()
            .map(|v| format!("{v:.6}\n"))
            .collect()
    }
}

/// The world-file names GIS tools write beside `image`, most specific
/// first: `name.pgw` / `name.jgw` (first, last letter of the extension +
/// `w`), `name.pngw` / `name.jpgw`, then `name.wld`.
pub fn world_file_candidates(image: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let ext = image
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext.len() >= 2 {
        let first = ext.chars().next().unwrap_or('p');
        let last = ext.chars().last().unwrap_or('g');
        out.push(image.with_extension(format!("{first}{last}w")));
        out.push(image.with_extension(format!("{ext}w")));
    }
    out.push(image.with_extension("wld"));
    out
}

/// The first sibling world file that exists and parses.
pub fn find_world_file(image: &Path) -> Option<(PathBuf, WorldFile)> {
    world_file_candidates(image).into_iter().find_map(|p| {
        let text = std::fs::read_to_string(&p).ok()?;
        WorldFile::parse(&text).ok().map(|w| (p, w))
    })
}

/// The `[BACKDROP]` section as values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Backdrop {
    pub file: String,
    pub dimensions: Option<(f64, f64, f64, f64)>,
    pub units: Option<String>,
    pub offset: Option<(f64, f64)>,
    pub scaling: Option<(f64, f64)>,
}

fn pair(v: &str) -> Option<(f64, f64)> {
    let mut it = v.split_whitespace().map(|s| s.parse::<f64>().ok());
    Some((it.next()??, it.next()??))
}

fn quad(v: &str) -> Option<(f64, f64, f64, f64)> {
    let nums: Vec<f64> = v
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    match nums.as_slice() {
        [a, b, c, d] => Some((*a, *b, *c, *d)),
        _ => None,
    }
}

impl Backdrop {
    /// Read `[BACKDROP]`, if the document has one with a FILE.
    pub fn read(doc: &InpDoc) -> Option<Self> {
        let file = doc.field("BACKDROP", "FILE", "1")?.to_string();
        if file.is_empty() {
            return None;
        }
        let value = |key: &str| doc.key_value("BACKDROP", key);
        Some(Self {
            file,
            dimensions: value("DIMENSIONS").as_deref().and_then(quad),
            units: value("UNITS").filter(|s| !s.is_empty()),
            offset: value("OFFSET").as_deref().and_then(pair),
            scaling: value("SCALING").as_deref().and_then(pair),
        })
    }

    /// The batch that writes every key: FILE, DIMENSIONS, and UNITS,
    /// OFFSET, SCALING when set (an unset one already in the file is
    /// removed).
    pub fn command(&self, doc: &InpDoc) -> Command {
        let mut cmds = vec![Command::SetOption {
            section: "BACKDROP".into(),
            key: "FILE".into(),
            value: format!("\"{}\"", self.file.replace('"', "")),
        }];
        let mut set = |key: &str, value: Option<String>| match value {
            Some(v) => cmds.push(Command::SetOption {
                section: "BACKDROP".into(),
                key: key.into(),
                value: v,
            }),
            None => {
                if doc.contains("BACKDROP", key) {
                    cmds.push(Command::DeleteObject {
                        section: "BACKDROP".into(),
                        name: key.into(),
                    });
                }
            }
        };
        set(
            "DIMENSIONS",
            self.dimensions.map(|(x0, y0, x1, y1)| {
                format!(
                    "{} {} {} {}",
                    format_number(x0),
                    format_number(y0),
                    format_number(x1),
                    format_number(y1)
                )
            }),
        );
        set("UNITS", self.units.clone());
        set(
            "OFFSET",
            self.offset
                .map(|(x, y)| format!("{} {}", format_number(x), format_number(y))),
        );
        set(
            "SCALING",
            self.scaling
                .map(|(x, y)| format!("{} {}", format_number(x), format_number(y))),
        );
        Command::Batch(cmds)
    }

    /// The batch that removes every `[BACKDROP]` key (the empty section
    /// stays, as the EPA GUI leaves it).
    pub fn remove_command(doc: &InpDoc) -> Command {
        Command::Batch(
            ["FILE", "DIMENSIONS", "UNITS", "OFFSET", "SCALING"]
                .iter()
                .filter(|k| doc.contains("BACKDROP", k))
                .map(|k| Command::DeleteObject {
                    section: "BACKDROP".into(),
                    name: k.to_string(),
                })
                .collect(),
        )
    }

    /// The image path resolved against the model's folder when relative.
    pub fn resolve(&self, model: Option<&Path>) -> PathBuf {
        let p = PathBuf::from(&self.file);
        if p.is_absolute() {
            return p;
        }
        match model.and_then(Path::parent) {
            Some(dir) => dir.join(p),
            None => p,
        }
    }
}

/// `[MAP] DIMENSIONS` as `(x_min, y_min, x_max, y_max)`.
pub fn map_dimensions(doc: &InpDoc) -> Option<(f64, f64, f64, f64)> {
    doc.key_value("MAP", "DIMENSIONS").as_deref().and_then(quad)
}

/// `[MAP] UNITS` (`None`, `Feet`, `Meters`, `Degrees`), as written.
pub fn map_units(doc: &InpDoc) -> Option<String> {
    doc.key_value("MAP", "UNITS").filter(|s| !s.is_empty())
}

/// The command that sets `[MAP] DIMENSIONS`.
pub fn set_map_dimensions(x0: f64, y0: f64, x1: f64, y1: f64) -> Command {
    Command::SetOption {
        section: "MAP".into(),
        key: "DIMENSIONS".into(),
        value: format!(
            "{} {} {} {}",
            format_number(x0.min(x1)),
            format_number(y0.min(y1)),
            format_number(x0.max(x1)),
            format_number(y0.max(y1))
        ),
    }
}

/// The command that sets `[MAP] UNITS`.
pub fn set_map_units(units: &str) -> Command {
    Command::SetOption {
        section: "MAP".into(),
        key: "UNITS".into(),
        value: units.trim().to_string(),
    }
}

/// `bounds` grown by `pad` (a fraction of the larger side) on every side,
/// the way the EPA GUI sizes a map sheet around a model.
pub fn padded(bounds: (f64, f64, f64, f64), pad: f64) -> (f64, f64, f64, f64) {
    let (x0, y0, x1, y1) = bounds;
    let m = ((x1 - x0).max(y1 - y0) * pad).max(1.0);
    (x0 - m, y0 - m, x1 + m, y1 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_file_parses_and_gives_the_extent() {
        let w = WorldFile::parse("2.5\n0\n0\n-2.5\n1001.25\n2998.75\n").unwrap();
        assert_eq!(w.a, 2.5);
        assert_eq!(w.e, -2.5);
        assert!(!w.is_rotated());
        // 400 × 200 px: left edge is half a pixel left of the first centre.
        let (x0, y0, x1, y1) = w.extent(400, 200);
        assert!(
            (x0 - 1000.0).abs() < 1e-9 && (x1 - 2000.0).abs() < 1e-9,
            "{x0} {x1}"
        );
        assert!(
            (y1 - 3000.0).abs() < 1e-9 && (y0 - 2500.0).abs() < 1e-9,
            "{y0} {y1}"
        );
        // And back again.
        let back = WorldFile::from_extent((x0, y0, x1, y1), 400, 200);
        assert!((back.a - w.a).abs() < 1e-9 && (back.e - w.e).abs() < 1e-9);
        assert!((back.c - w.c).abs() < 1e-9 && (back.f - w.f).abs() < 1e-9);
        assert_eq!(WorldFile::parse(&back.to_text()).unwrap(), back);
        assert!(WorldFile::parse("1\n2\n3\n").is_err());
        assert!(WorldFile::parse("1\nx\n0\n-1\n0\n0\n").is_err());
        assert!(WorldFile::parse("1\n0.1\n0\n-1\n0\n0\n")
            .unwrap()
            .is_rotated());
        let c = world_file_candidates(Path::new("C:/maps/site.PNG"));
        assert_eq!(c[0], PathBuf::from("C:/maps/site.pgw"));
        assert_eq!(c[1], PathBuf::from("C:/maps/site.pngw"));
        assert_eq!(c[2], PathBuf::from("C:/maps/site.wld"));
        assert_eq!(
            world_file_candidates(Path::new("a.jpeg"))[0],
            PathBuf::from("a.jgw")
        );
    }

    #[test]
    fn backdrop_and_map_dimensions_are_written_and_round_trip() {
        let mut doc = InpDoc::parse("[MAP]\nDIMENSIONS 0 0 100 100\nUnits None\n");
        let bd = Backdrop {
            file: "Site-Post.jpg".into(),
            dimensions: Some((-23.36, -29.165, 1463.996, 1512.277)),
            units: Some("FEET".into()),
            offset: None,
            scaling: None,
        };
        doc.apply(bd.command(&doc)).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        assert_eq!(Backdrop::read(&doc), Some(bd.clone()));
        assert_eq!(doc.field("BACKDROP", "FILE", "1"), Some("Site-Post.jpg"));
        assert_eq!(
            doc.key_value("BACKDROP", "DIMENSIONS").as_deref(),
            Some("-23.36 -29.165 1463.996 1512.277")
        );
        // A second write with fewer keys removes the dropped one.
        let mut bd2 = bd.clone();
        bd2.units = None;
        bd2.offset = Some((1.0, 2.0));
        doc.apply(bd2.command(&doc)).unwrap();
        assert!(!doc.contains("BACKDROP", "UNITS"));
        assert_eq!(Backdrop::read(&doc).unwrap().offset, Some((1.0, 2.0)));
        // Map dimensions from the backdrop.
        doc.apply(set_map_dimensions(1463.996, 1512.277, -23.36, -29.165))
            .unwrap();
        assert_eq!(
            map_dimensions(&doc),
            Some((-23.36, -29.165, 1463.996, 1512.277))
        );
        assert_eq!(map_units(&doc).as_deref(), Some("None"));
        doc.apply(set_map_units("Feet")).unwrap();
        assert_eq!(map_units(&doc).as_deref(), Some("Feet"));
        doc.apply(Backdrop::remove_command(&doc)).unwrap();
        assert!(Backdrop::read(&doc).is_none());
        assert!(doc.section("BACKDROP").is_some(), "the section stays");
        // Undo everything back to the start.
        while doc.undo() {}
        assert_eq!(
            doc.to_string(),
            "[MAP]\nDIMENSIONS 0 0 100 100\nUnits None\n"
        );
        assert_eq!(
            padded((0.0, 0.0, 100.0, 50.0), 0.06),
            (-6.0, -6.0, 106.0, 56.0)
        );
    }

    #[test]
    fn epa_sample_backdrop_reads() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples/Site_Drainage_Model.inp");
        let doc = InpDoc::read(&path).unwrap();
        let bd = Backdrop::read(&doc).expect("the sample has a backdrop");
        assert_eq!(bd.file, "Site-Post.jpg");
        assert_eq!(bd.dimensions, Some((0.0, 0.0, 1423.0, 1475.0)));
        assert!(bd
            .resolve(Some(&path))
            .ends_with("epa-samples/Site-Post.jpg"));
        assert_eq!(map_units(&doc).as_deref(), Some("Feet"));
    }
}
