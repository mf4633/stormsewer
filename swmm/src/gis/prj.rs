// SPDX-License-Identifier: GPL-3.0-or-later

//! `.prj` files: OGC WKT1 (and the ESRI dialect ArcGIS writes) parsed
//! into a [`Crs`].
//!
//! The grammar is a tree of `KEYWORD[arg, arg, ...]` where an argument is
//! a quoted string, a number, or another node. A `PROJCS` carries a
//! `GEOGCS` (datum and spheroid), a `PROJECTION`, `PARAMETER`s, a `UNIT`
//! and, in the OGC form, an `AUTHORITY`. ESRI's form has no authority; a
//! system that matches a built-in UTM or State Plane zone is identified
//! by its parameters instead. WKT2 (`PROJCRS[`, `GEOGCRS[`) is refused
//! with a clear message.

use crate::gis::crs::{Crs, Datum, Ellipsoid, Projection, Unit};
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum WktValue {
    Str(String),
    Num(f64),
    Node(WktNode),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WktNode {
    pub keyword: String,
    pub args: Vec<WktValue>,
}

impl WktNode {
    pub fn child(&self, keyword: &str) -> Option<&WktNode> {
        self.args.iter().find_map(|a| match a {
            WktValue::Node(n) if n.keyword.eq_ignore_ascii_case(keyword) => Some(n),
            _ => None,
        })
    }

    pub fn children(&self, keyword: &str) -> Vec<&WktNode> {
        self.args
            .iter()
            .filter_map(|a| match a {
                WktValue::Node(n) if n.keyword.eq_ignore_ascii_case(keyword) => Some(n),
                _ => None,
            })
            .collect()
    }

    pub fn str_arg(&self, i: usize) -> Option<&str> {
        match self.args.get(i)? {
            WktValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num_arg(&self, i: usize) -> Option<f64> {
        match self.args.get(i)? {
            WktValue::Num(v) => Some(*v),
            WktValue::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn node(&mut self) -> Result<WktNode> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        if start == self.i {
            return Err(Error::Format(format!("WKT: expected a keyword at byte {}", self.i)));
        }
        let keyword = String::from_utf8_lossy(&self.s[start..self.i]).to_ascii_uppercase();
        self.skip_ws();
        let open = self.s.get(self.i).copied();
        if open != Some(b'[') && open != Some(b'(') {
            return Err(Error::Format(format!("WKT: expected '[' after {keyword}")));
        }
        self.i += 1;
        let mut args = Vec::new();
        loop {
            self.skip_ws();
            match self.s.get(self.i).copied() {
                None => return Err(Error::Format(format!("WKT: {keyword}[ is not closed"))),
                Some(b']') | Some(b')') => {
                    self.i += 1;
                    break;
                }
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'"') => {
                    self.i += 1;
                    let start = self.i;
                    while self.i < self.s.len() && self.s[self.i] != b'"' {
                        self.i += 1;
                    }
                    args.push(WktValue::Str(String::from_utf8_lossy(&self.s[start..self.i]).to_string()));
                    self.i += 1;
                }
                Some(c) if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
                    let start = self.i;
                    while self.i < self.s.len()
                        && (self.s[self.i].is_ascii_alphanumeric() || matches!(self.s[self.i], b'-' | b'+' | b'.'))
                    {
                        self.i += 1;
                    }
                    let t = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
                    let v: f64 = t
                        .parse()
                        .map_err(|_| Error::Format(format!("WKT: bad number {t:?}")))?;
                    args.push(WktValue::Num(v));
                }
                Some(_) => {
                    // A bare word is a nested node when '[' follows
                    // (`DATUM[...]`), else a keyword value such as the
                    // `EAST` / `NORTH` in `AXIS["Easting",EAST]`.
                    let save = self.i;
                    let start = self.i;
                    while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                        self.i += 1;
                    }
                    let word = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
                    self.skip_ws();
                    if matches!(self.s.get(self.i), Some(b'[') | Some(b'(')) {
                        self.i = save;
                        args.push(WktValue::Node(self.node()?));
                    } else if word.is_empty() {
                        return Err(Error::Format(format!("WKT: unexpected character at byte {}", self.i)));
                    } else {
                        args.push(WktValue::Str(word));
                    }
                }
            }
        }
        Ok(WktNode { keyword, args })
    }
}

/// Parse WKT text into its tree.
pub fn parse_tree(text: &str) -> Result<WktNode> {
    let t = text.trim().trim_start_matches('\u{feff}');
    if t.is_empty() {
        return Err(Error::Format("WKT is empty".into()));
    }
    let mut p = Parser { s: t.as_bytes(), i: 0 };
    p.node()
}

/// A parameter name folded for matching: lowercase, no separators.
fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

struct Params(Vec<(String, f64)>);

impl Params {
    fn get(&self, names: &[&str]) -> Option<f64> {
        names.iter().find_map(|n| {
            let k = norm(n);
            self.0.iter().find(|(name, _)| *name == k).map(|(_, v)| *v)
        })
    }
}

fn geogcs(node: &WktNode) -> Result<(String, Datum, Ellipsoid, Option<u32>)> {
    let name = node.str_arg(0).unwrap_or("").to_string();
    let datum = node
        .child("DATUM")
        .ok_or_else(|| Error::Format("WKT GEOGCS has no DATUM".into()))?;
    let dname = datum.str_arg(0).unwrap_or("");
    let sph = datum
        .child("SPHEROID")
        .or_else(|| datum.child("ELLIPSOID"))
        .ok_or_else(|| Error::Format("WKT DATUM has no SPHEROID".into()))?;
    let a = sph.num_arg(1).ok_or_else(|| Error::Format("WKT SPHEROID has no semi-major axis".into()))?;
    let inv_f = sph.num_arg(2).unwrap_or(0.0);
    let mut d = Datum::from_name(dname);
    if inv_f == 0.0 && matches!(d, Datum::Wgs84 | Datum::Other(_)) {
        d = Datum::Sphere;
    }
    let el = Ellipsoid {
        name: sph.str_arg(0).unwrap_or("").to_string(),
        a,
        inv_f,
    };
    Ok((name, d, el, authority(node)))
}

fn authority(node: &WktNode) -> Option<u32> {
    let auth = node.child("AUTHORITY").or_else(|| node.child("ID"))?;
    if !auth.str_arg(0)?.eq_ignore_ascii_case("EPSG") {
        return None;
    }
    auth.num_arg(1).map(|v| v as u32)
}

/// Solve the latitude of true scale that gives the same radius as polar
/// stereographic variant A's scale factor at the pole (Snyder eq. 21-33
/// vs 21-34), by bisection on m/t.
fn lat_ts_from_k0(e: f64, k0: f64) -> f64 {
    let target = 2.0 * k0 / ((1.0 + e).powf(1.0 + e) * (1.0 - e).powf(1.0 - e)).sqrt();
    let ratio = |phi: f64| {
        let es = e * phi.sin();
        let m = phi.cos() / (1.0 - es * es).sqrt();
        let t = (std::f64::consts::FRAC_PI_4 - phi / 2.0).tan() / ((1.0 - es) / (1.0 + es)).powf(e / 2.0);
        m / t
    };
    let (mut lo, mut hi) = (0.0f64, std::f64::consts::FRAC_PI_2 - 1e-9);
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if ratio(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ((lo + hi) / 2.0).to_degrees()
}

/// Build a [`Crs`] from a parsed tree.
pub fn crs_from_tree(root: &WktNode, text: &str) -> Result<Crs> {
    match root.keyword.as_str() {
        "PROJCRS" | "GEOGCRS" | "BOUNDCRS" | "GEODCRS" | "ENGCRS" => {
            return Err(Error::Format(
                "WKT2 coordinate systems (PROJCRS/GEOGCRS) are not supported; save the .prj as WKT1 (ESRI) or give the EPSG code".into(),
            ))
        }
        "COMPD_CS" => {
            let inner = root
                .child("PROJCS")
                .or_else(|| root.child("GEOGCS"))
                .ok_or_else(|| Error::Format("WKT COMPD_CS has no horizontal system".into()))?;
            let mut c = crs_from_tree(inner, text)?;
            c.wkt = Some(text.trim().to_string());
            return Ok(c);
        }
        "GEOGCS" => {
            let (name, datum, ellipsoid, epsg) = geogcs(root)?;
            let epsg = epsg.or(match datum {
                Datum::Wgs84 => Some(4326),
                Datum::Nad83 => Some(4269),
                Datum::Nad27 => Some(4267),
                _ => None,
            });
            return Ok(Crs {
                name,
                datum,
                ellipsoid,
                projection: Projection::Geographic,
                unit: Unit::degree(),
                epsg,
                wkt: Some(text.trim().to_string()),
            });
        }
        "PROJCS" => {}
        "LOCAL_CS" => return Err(Error::Format("WKT LOCAL_CS has no geographic reference".into())),
        other => return Err(Error::Format(format!("WKT: {other} is not a coordinate system"))),
    }
    let name = root.str_arg(0).unwrap_or("").to_string();
    let g = root
        .child("GEOGCS")
        .ok_or_else(|| Error::Format("WKT PROJCS has no GEOGCS".into()))?;
    let (_, datum, ellipsoid, _) = geogcs(g)?;
    let method = root
        .child("PROJECTION")
        .and_then(|p| p.str_arg(0))
        .ok_or_else(|| Error::Format("WKT PROJCS has no PROJECTION".into()))?;
    let params = Params(
        root.children("PARAMETER")
            .iter()
            .filter_map(|p| Some((norm(p.str_arg(0)?), p.num_arg(1)?)))
            .collect(),
    );
    let unit = root
        .child("UNIT")
        .map(|u| Unit::from_wkt(u.str_arg(0).unwrap_or(""), u.num_arg(1).unwrap_or(1.0)))
        .unwrap_or_else(Unit::metre);
    let um = unit.to_metre;
    let fe = params.get(&["False_Easting", "Easting_At_False_Origin", "Easting_At_Projection_Centre"]).unwrap_or(0.0) * um;
    let fn_ = params.get(&["False_Northing", "Northing_At_False_Origin", "Northing_At_Projection_Centre"]).unwrap_or(0.0) * um;
    let lon0 = params
        .get(&["Central_Meridian", "Longitude_Of_Origin", "Longitude_Of_Natural_Origin", "Longitude_Of_Center", "Longitude_Of_False_Origin", "Longitude_Of_Projection_Centre"])
        .unwrap_or(0.0);
    let lat0 = params
        .get(&["Latitude_Of_Origin", "Latitude_Of_Natural_Origin", "Latitude_Of_Center", "Latitude_Of_False_Origin", "Latitude_Of_Projection_Centre"])
        .unwrap_or(0.0);
    let k0 = params.get(&["Scale_Factor", "Scale_Factor_At_Natural_Origin", "Scale_Factor_On_Initial_Line"]);
    let sp1 = params.get(&["Standard_Parallel_1", "Latitude_Of_1st_Standard_Parallel", "Latitude_Of_Standard_Parallel"]);
    let sp2 = params.get(&["Standard_Parallel_2", "Latitude_Of_2nd_Standard_Parallel"]);
    let m = norm(method);
    let e = ellipsoid.e();
    let projection = match m.as_str() {
        "transversemercator" | "gausskruger" | "gausskrueger" => Projection::TransverseMercator {
            lat0,
            lon0,
            k0: k0.unwrap_or(1.0),
            fe,
            fn_,
        },
        "lambertconformalconic" | "lambertconformalconic2sp" | "lambertconicconformal2sp" => {
            match (sp1, sp2) {
                (Some(a), Some(b)) => Projection::LambertConformal2Sp { lat0, lon0, sp1: a, sp2: b, fe, fn_ },
                (Some(a), None) if k0.is_none() => Projection::LambertConformal2Sp { lat0, lon0, sp1: a, sp2: a, fe, fn_ },
                (Some(a), None) => Projection::LambertConformal1Sp { lat0: a, lon0, k0: k0.unwrap_or(1.0), fe, fn_ },
                _ => Projection::LambertConformal1Sp { lat0, lon0, k0: k0.unwrap_or(1.0), fe, fn_ },
            }
        }
        "lambertconformalconic1sp" | "lambertconicconformal1sp" => Projection::LambertConformal1Sp {
            lat0: sp1.unwrap_or(lat0),
            lon0,
            k0: k0.unwrap_or(1.0),
            fe,
            fn_,
        },
        "albers" | "albersconicequalarea" | "albersequalarea" => Projection::AlbersEqualArea {
            lat0,
            lon0,
            sp1: sp1.unwrap_or(lat0),
            sp2: sp2.or(sp1).unwrap_or(lat0),
            fe,
            fn_,
        },
        "mercatorauxiliarysphere" | "popularvisualisationpseudomercator" | "webmercator" => {
            Projection::WebMercator { lon0, fe, fn_ }
        }
        "mercator" | "mercator1sp" | "mercator2sp" | "mercatorvarianta" | "mercatorvariantb" => {
            let k = match (k0, sp1) {
                (Some(k), _) if m != "mercator2sp" => k,
                (_, Some(ts)) => {
                    let p = ts.to_radians();
                    p.cos() / (1.0 - e * e * p.sin().powi(2)).sqrt()
                }
                _ => 1.0,
            };
            if ellipsoid.inv_f == 0.0 && (ellipsoid.a - 6378137.0).abs() < 1.0 {
                Projection::WebMercator { lon0, fe, fn_ }
            } else {
                Projection::Mercator { lon0, k0: k, fe, fn_ }
            }
        }
        "polarstereographic" | "polarstereographicvariantb" | "stereographicnorthpole" | "stereographicsouthpole"
        | "polarstereographicvarianta" => {
            let lat_ts = match (sp1, k0) {
                (Some(ts), _) if ts.abs() < 90.0 - 1e-9 => ts,
                (_, Some(k)) => lat_ts_from_k0(e, k) * lat0.signum(),
                _ if lat0.abs() < 90.0 - 1e-9 && lat0 != 0.0 => lat0,
                _ => 90.0 * if lat0 < 0.0 { -1.0 } else { 1.0 },
            };
            Projection::PolarStereographic { lat_ts, lon0, fe, fn_ }
        }
        "hotineobliquemercator" | "hotineobliquemercatorazimuthnaturalorigin" | "obliquemercator"
        | "hotineobliquemercatorazimuthcenter" | "hotineobliquemercatorvariantb" | "hotineobliquemercatorvarianta"
        | "rectifiedskeworthomorphiccentre" | "rectifiedskeworthomorphicnaturalorigin" => {
            let azimuth = params.get(&["Azimuth", "Azimuth_Of_Initial_Line"]).unwrap_or(0.0);
            let gamma = params.get(&["Rectified_Grid_Angle", "XY_Plane_Rotation", "Angle_From_Rectified_To_Skew_Grid"]).unwrap_or(azimuth);
            let at_centre = m.contains("center") || m.contains("centre") || m.contains("variantb") || m == "obliquemercator";
            Projection::HotineObliqueMercator {
                latc: lat0,
                lonc: lon0,
                azimuth,
                gamma,
                k0: k0.unwrap_or(1.0),
                fe,
                fn_,
                at_centre,
            }
        }
        _ => {
            return Err(Error::Format(format!(
                "projection {method:?} is not supported (supported: Transverse Mercator, Lambert Conformal Conic, Albers, Mercator, Web Mercator, Polar Stereographic, Hotine Oblique Mercator)"
            )))
        }
    };
    let mut crs = Crs {
        name,
        datum,
        ellipsoid,
        projection,
        unit,
        epsg: authority(root),
        wkt: Some(text.trim().to_string()),
    };
    if crs.epsg.is_none() {
        crs.epsg = crate::gis::stateplane::identify(&crs);
    }
    Ok(crs)
}

/// Parse `.prj` text into a [`Crs`].
pub fn parse_wkt(text: &str) -> Result<Crs> {
    let tree = parse_tree(text)?;
    crs_from_tree(&tree, text)
}

pub fn read(path: &std::path::Path) -> Result<Crs> {
    let text = std::fs::read_to_string(path)?;
    parse_wkt(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESRI_NC_FT: &str = r#"PROJCS["NAD_1983_StatePlane_North_Carolina_FIPS_3200_Feet",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Lambert_Conformal_Conic"],PARAMETER["False_Easting",2000000.002616666],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-79.0],PARAMETER["Standard_Parallel_1",34.33333333333334],PARAMETER["Standard_Parallel_2",36.16666666666666],PARAMETER["Latitude_Of_Origin",33.75],UNIT["Foot_US",0.3048006096012192]]"#;

    const OGC_UTM: &str = r#"PROJCS["WGS 84 / UTM zone 17N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",-81],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","32617"]]"#;

    #[test]
    fn esri_state_plane_prj_is_identified_with_us_feet() {
        let c = parse_wkt(ESRI_NC_FT).unwrap();
        assert_eq!(c.datum, Datum::Nad83);
        assert!((c.ellipsoid.inv_f - 298.257222101).abs() < 1e-9);
        assert_eq!(c.unit.name, "US survey foot");
        match c.projection {
            Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ } => {
                assert_eq!(lat0, 33.75);
                assert_eq!(lon0, -79.0);
                assert!((sp1 - 34.0 - 20.0 / 60.0).abs() < 1e-9);
                assert!((sp2 - 36.0 - 10.0 / 60.0).abs() < 1e-9);
                assert!((fe - 609601.22).abs() < 0.01, "{fe}");
                assert_eq!(fn_, 0.0);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(c.epsg, Some(2264), "identified from its parameters");
        assert!(c.wkt.as_deref().unwrap().starts_with("PROJCS"));
    }

    #[test]
    fn ogc_utm_prj_carries_its_authority_and_a_geogcs_parses_alone() {
        let c = parse_wkt(OGC_UTM).unwrap();
        assert_eq!(c.epsg, Some(32617));
        assert_eq!(c.datum, Datum::Wgs84);
        assert_eq!(c.unit.to_metre, 1.0);
        let g = parse_wkt(r#"GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#).unwrap();
        assert!(g.is_geographic());
        assert_eq!(g.epsg, Some(4269));
        let w = parse_wkt("PROJCRS[\"x\"]").unwrap_err();
        assert!(w.to_string().contains("WKT2"), "{w}");
        assert!(parse_wkt("").is_err());
        assert!(parse_wkt("PROJCS[\"x\"").is_err());
        let nad27 = parse_wkt(r#"PROJCS["NAD_1927_UTM_Zone_17N",GEOGCS["GCS_North_American_1927",DATUM["D_North_American_1927",SPHEROID["Clarke_1866",6378206.4,294.9786982]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-81.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#).unwrap();
        assert_eq!(nad27.datum, Datum::Nad27);
        assert_eq!(nad27.epsg, Some(26717));
    }

    #[test]
    fn esri_web_mercator_and_polar_forms_parse() {
        let wm = parse_wkt(r#"PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Mercator_Auxiliary_Sphere"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],PARAMETER["Auxiliary_Sphere_Type",0.0],UNIT["Meter",1.0]]"#).unwrap();
        assert!(matches!(wm.projection, Projection::WebMercator { .. }));
        assert_eq!(wm.epsg, Some(3857));
        // OGC polar stereographic variant A: scale factor at the pole.
        let ps = parse_wkt(r#"PROJCS["x",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Polar_Stereographic"],PARAMETER["latitude_of_origin",90],PARAMETER["central_meridian",0],PARAMETER["scale_factor",0.994],PARAMETER["false_easting",2000000],PARAMETER["false_northing",2000000],UNIT["metre",1]]"#).unwrap();
        match ps.projection {
            Projection::PolarStereographic { lat_ts, .. } => {
                // UPS-style k0 = 0.994 at the pole: on a sphere true scale
                // is where 2k0/(1+sin φ) = 1, φ = asin(0.988) = 81.11°; the
                // ellipsoid moves that by well under a tenth of a degree.
                assert!((lat_ts - 81.11).abs() < 0.1, "{lat_ts}");
            }
            other => panic!("{other:?}"),
        }
        let tree = parse_tree("A[\"s\",1,B[2]]").unwrap();
        assert_eq!(tree.keyword, "A");
        assert_eq!(tree.child("B").unwrap().num_arg(0), Some(2.0));
    }
}
