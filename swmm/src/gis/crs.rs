// SPDX-License-Identifier: GPL-3.0-or-later

//! Coordinate reference systems: a [`Crs`] built from WKT (`prj`), from an
//! EPSG code, or from the State Plane table, with forward (longitude,
//! latitude → easting, northing) and inverse projection, and
//! [`transform`] between two systems.
//!
//! Formulas are from Snyder (1987), *Map Projections — A Working Manual*,
//! USGS Professional Paper 1395, except the transverse Mercator, which
//! uses the Krüger series (Krüger 1912, as set out in Karney 2011,
//! "Transverse Mercator with an accuracy of a few nanometers") because
//! Snyder's USGS series loses millimetres at the edge of a UTM zone and
//! does not round-trip to the micrometre.
//!
//! **Datums.** NAD83 and WGS84 are treated as the same datum: the two
//! differ by about 1–2 m across the conterminous US (the NAD83(2011) to
//! WGS84(G1762) shift), which is inside the positional accuracy of the
//! data a drainage model is built from, but it is a real offset — a
//! survey-grade transformation is not attempted. NAD27 is recognised so
//! a `.prj` naming it can be reported, but [`transform`] refuses it:
//! NAD27 → NAD83 needs the NADCON grid shift (up to ~100 m), which this
//! crate does not carry.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use crate::{Error, Result};

/// A reference ellipsoid: semi-major axis in metres and inverse
/// flattening (0 for a sphere).
#[derive(Clone, Debug, PartialEq)]
pub struct Ellipsoid {
    pub name: String,
    pub a: f64,
    pub inv_f: f64,
}

impl Ellipsoid {
    pub const WGS84: (&'static str, f64, f64) = ("WGS 84", 6378137.0, 298.257223563);
    pub const GRS80: (&'static str, f64, f64) = ("GRS 1980", 6378137.0, 298.257222101);
    pub const CLARKE_1866: (&'static str, f64, f64) = ("Clarke 1866", 6378206.4, 294.9786982);
    pub const SPHERE_WEB: (&'static str, f64, f64) = ("WGS 84 auxiliary sphere", 6378137.0, 0.0);

    pub fn new(spec: (&str, f64, f64)) -> Self {
        Self {
            name: spec.0.to_string(),
            a: spec.1,
            inv_f: spec.2,
        }
    }

    pub fn wgs84() -> Self {
        Self::new(Self::WGS84)
    }

    pub fn grs80() -> Self {
        Self::new(Self::GRS80)
    }

    pub fn clarke_1866() -> Self {
        Self::new(Self::CLARKE_1866)
    }

    pub fn f(&self) -> f64 {
        if self.inv_f == 0.0 {
            0.0
        } else {
            1.0 / self.inv_f
        }
    }

    pub fn e2(&self) -> f64 {
        let f = self.f();
        2.0 * f - f * f
    }

    pub fn e(&self) -> f64 {
        self.e2().sqrt()
    }
}

/// The horizontal datum, as far as this crate distinguishes them.
#[derive(Clone, Debug, PartialEq)]
pub enum Datum {
    Wgs84,
    Nad83,
    Nad27,
    /// A sphere (Web Mercator); transforms treat it as WGS84 positions.
    Sphere,
    Other(String),
}

impl Datum {
    /// Classify a WKT `DATUM[...]` name.
    pub fn from_name(name: &str) -> Self {
        let u = name.to_ascii_uppercase().replace(['_', ' ', '-'], "");
        if u.contains("1927") || u.contains("NAD27") {
            Self::Nad27
        } else if u.contains("1983") || u.contains("NAD83") {
            Self::Nad83
        } else if u.contains("WGS1984") || u.contains("WGS84") {
            Self::Wgs84
        } else if u.contains("SPHERE") {
            Self::Sphere
        } else {
            Self::Other(name.to_string())
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Wgs84 => "WGS 84".into(),
            Self::Nad83 => "NAD83".into(),
            Self::Nad27 => "NAD27".into(),
            Self::Sphere => "sphere".into(),
            Self::Other(n) => n.clone(),
        }
    }

    pub fn wkt_name(&self) -> String {
        match self {
            Self::Wgs84 => "D_WGS_1984".into(),
            Self::Nad83 => "D_North_American_1983".into(),
            Self::Nad27 => "D_North_American_1927".into(),
            Self::Sphere => "D_WGS_1984_Major_Auxiliary_Sphere".into(),
            Self::Other(n) => n.clone(),
        }
    }

    /// Whether positions in the two datums may be exchanged without a
    /// shift (see the module notes on NAD83 vs WGS84).
    pub fn compatible(&self, other: &Datum) -> bool {
        let modern = |d: &Datum| matches!(d, Self::Wgs84 | Self::Nad83 | Self::Sphere);
        match (self, other) {
            (a, b) if modern(a) && modern(b) => true,
            (Self::Other(a), Self::Other(b)) => a.eq_ignore_ascii_case(b),
            (Self::Nad27, Self::Nad27) => true,
            _ => false,
        }
    }
}

/// A linear unit: metres per unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub name: String,
    pub to_metre: f64,
}

impl Unit {
    pub const US_FOOT: f64 = 1200.0 / 3937.0; // 0.304800609601219...
    pub const INTL_FOOT: f64 = 0.3048;

    pub fn metre() -> Self {
        Self {
            name: "metre".into(),
            to_metre: 1.0,
        }
    }

    pub fn us_foot() -> Self {
        Self {
            name: "US survey foot".into(),
            to_metre: Self::US_FOOT,
        }
    }

    pub fn intl_foot() -> Self {
        Self {
            name: "foot".into(),
            to_metre: Self::INTL_FOOT,
        }
    }

    pub fn degree() -> Self {
        Self {
            name: "degree".into(),
            to_metre: 0.0,
        }
    }

    /// Recognise a WKT `UNIT["name", factor]`.
    pub fn from_wkt(name: &str, factor: f64) -> Self {
        let u = name.to_ascii_uppercase().replace(['_', ' ', '-'], "");
        let known = if (factor - 1.0).abs() < 1e-12 || u == "METRE" || u == "METER" {
            Some(Self::metre())
        } else if (factor - Self::US_FOOT).abs() < 1e-9 || u.contains("FOOTUS") || u.contains("USSURVEY") {
            Some(Self::us_foot())
        } else if (factor - Self::INTL_FOOT).abs() < 1e-12 || u == "FOOT" || u == "FEET" || u == "INTERNATIONALFOOT" {
            Some(Self::intl_foot())
        } else {
            None
        };
        known.unwrap_or(Self {
            name: name.to_string(),
            to_metre: factor,
        })
    }

    pub fn wkt(&self) -> String {
        let name = match self.name.as_str() {
            "metre" => "Meter",
            "US survey foot" => "Foot_US",
            "foot" => "Foot",
            n => n,
        };
        format!("UNIT[\"{name}\",{}]", fmt(self.to_metre))
    }
}

/// The projection and its parameters. Angles in degrees, false origin in
/// the CRS's linear unit is *not* assumed: false easting/northing are in
/// metres here, and are converted to the unit on output.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// Longitude/latitude in degrees.
    Geographic,
    TransverseMercator {
        lat0: f64,
        lon0: f64,
        k0: f64,
        fe: f64,
        fn_: f64,
    },
    LambertConformal2Sp {
        lat0: f64,
        lon0: f64,
        sp1: f64,
        sp2: f64,
        fe: f64,
        fn_: f64,
    },
    LambertConformal1Sp {
        lat0: f64,
        lon0: f64,
        k0: f64,
        fe: f64,
        fn_: f64,
    },
    AlbersEqualArea {
        lat0: f64,
        lon0: f64,
        sp1: f64,
        sp2: f64,
        fe: f64,
        fn_: f64,
    },
    /// Ellipsoidal Mercator; `k0` at the equator (a latitude of true scale
    /// is folded into `k0` when parsed).
    Mercator {
        lon0: f64,
        k0: f64,
        fe: f64,
        fn_: f64,
    },
    /// Spherical Mercator on the WGS 84 semi-major axis (EPSG 3857).
    WebMercator { lon0: f64, fe: f64, fn_: f64 },
    /// Polar stereographic, variant B (latitude of true scale) — north
    /// when `lat_ts > 0`, south otherwise.
    PolarStereographic {
        lat_ts: f64,
        lon0: f64,
        fe: f64,
        fn_: f64,
    },
    /// Hotine oblique Mercator. `at_centre` is EPSG's variant B (false
    /// origin at the projection centre, as State Plane Alaska 1 uses).
    HotineObliqueMercator {
        latc: f64,
        lonc: f64,
        azimuth: f64,
        gamma: f64,
        k0: f64,
        fe: f64,
        fn_: f64,
        at_centre: bool,
    },
}

impl Projection {
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::Geographic => "Geographic (longitude/latitude)",
            Self::TransverseMercator { .. } => "Transverse Mercator",
            Self::LambertConformal2Sp { .. } => "Lambert Conformal Conic (2SP)",
            Self::LambertConformal1Sp { .. } => "Lambert Conformal Conic (1SP)",
            Self::AlbersEqualArea { .. } => "Albers Equal Area",
            Self::Mercator { .. } => "Mercator",
            Self::WebMercator { .. } => "Web Mercator",
            Self::PolarStereographic { .. } => "Polar Stereographic",
            Self::HotineObliqueMercator { .. } => "Hotine Oblique Mercator",
        }
    }
}

/// A coordinate reference system.
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    pub name: String,
    pub datum: Datum,
    pub ellipsoid: Ellipsoid,
    pub projection: Projection,
    /// The linear unit of projected coordinates (degrees for geographic).
    pub unit: Unit,
    pub epsg: Option<u32>,
    /// The WKT this was parsed from, verbatim, when it came from text.
    pub wkt: Option<String>,
}

fn fmt(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        let s = format!("{v:.12}");
        s.trim_end_matches('0').to_string()
    }
}

impl Crs {
    pub fn wgs84() -> Self {
        Self {
            name: "WGS 84".into(),
            datum: Datum::Wgs84,
            ellipsoid: Ellipsoid::wgs84(),
            projection: Projection::Geographic,
            unit: Unit::degree(),
            epsg: Some(4326),
            wkt: None,
        }
    }

    pub fn nad83() -> Self {
        Self {
            name: "NAD83".into(),
            datum: Datum::Nad83,
            ellipsoid: Ellipsoid::grs80(),
            projection: Projection::Geographic,
            unit: Unit::degree(),
            epsg: Some(4269),
            wkt: None,
        }
    }

    pub fn nad27() -> Self {
        Self {
            name: "NAD27".into(),
            datum: Datum::Nad27,
            ellipsoid: Ellipsoid::clarke_1866(),
            projection: Projection::Geographic,
            unit: Unit::degree(),
            epsg: Some(4267),
            wkt: None,
        }
    }

    pub fn web_mercator() -> Self {
        Self {
            name: "WGS 84 / Pseudo-Mercator".into(),
            datum: Datum::Sphere,
            ellipsoid: Ellipsoid::new(Ellipsoid::SPHERE_WEB),
            projection: Projection::WebMercator {
                lon0: 0.0,
                fe: 0.0,
                fn_: 0.0,
            },
            unit: Unit::metre(),
            epsg: Some(3857),
            wkt: None,
        }
    }

    /// A UTM zone on WGS 84 (`datum` `Wgs84`), NAD83 (GRS 80) or NAD27
    /// (Clarke 1866).
    pub fn utm(zone: u8, north: bool, datum: Datum) -> Result<Self> {
        if !(1..=60).contains(&zone) {
            return Err(Error::Format(format!("UTM zone {zone} is not between 1 and 60")));
        }
        let (ellipsoid, epsg, dname) = match &datum {
            Datum::Wgs84 | Datum::Sphere => (
                Ellipsoid::wgs84(),
                Some(if north { 32600 } else { 32700 } + zone as u32),
                "WGS 84",
            ),
            Datum::Nad83 => (
                Ellipsoid::grs80(),
                if north && zone <= 23 { Some(26900 + zone as u32) } else { None },
                "NAD83",
            ),
            Datum::Nad27 => (
                Ellipsoid::clarke_1866(),
                if north && zone <= 22 { Some(26700 + zone as u32) } else { None },
                "NAD27",
            ),
            Datum::Other(n) => return Err(Error::Format(format!("UTM on datum {n:?} is not supported"))),
        };
        Ok(Self {
            name: format!("{dname} / UTM zone {zone}{}", if north { "N" } else { "S" }),
            datum: if datum == Datum::Sphere { Datum::Wgs84 } else { datum },
            ellipsoid,
            projection: Projection::TransverseMercator {
                lat0: 0.0,
                lon0: -183.0 + 6.0 * zone as f64,
                k0: 0.9996,
                fe: 500000.0,
                fn_: if north { 0.0 } else { 10_000_000.0 },
            },
            unit: Unit::metre(),
            epsg,
            wkt: None,
        })
    }

    /// The UTM zone containing a longitude/latitude.
    pub fn utm_zone_for(lon: f64, lat: f64) -> (u8, bool) {
        let z = (((lon + 180.0) / 6.0).floor() as i32 + 1).clamp(1, 60) as u8;
        (z, lat >= 0.0)
    }

    /// A CRS by EPSG code: 4326/4269/4267, 3857, UTM (326xx, 327xx,
    /// 269xx, 267xx), the State Plane 1983 zones, 5070 (CONUS Albers),
    /// 3031/3413/3995 (polar stereographic).
    pub fn from_epsg(code: u32) -> Result<Self> {
        match code {
            4326 => return Ok(Self::wgs84()),
            4269 => return Ok(Self::nad83()),
            4267 => return Ok(Self::nad27()),
            3857 | 900913 | 3785 => return Ok(Self::web_mercator()),
            32601..=32660 => return Self::utm((code - 32600) as u8, true, Datum::Wgs84),
            32701..=32760 => return Self::utm((code - 32700) as u8, false, Datum::Wgs84),
            26901..=26923 => return Self::utm((code - 26900) as u8, true, Datum::Nad83),
            26701..=26722 => return Self::utm((code - 26700) as u8, true, Datum::Nad27),
            5070 | 102003 => {
                return Ok(Self {
                    name: "NAD83 / Conus Albers".into(),
                    datum: Datum::Nad83,
                    ellipsoid: Ellipsoid::grs80(),
                    projection: Projection::AlbersEqualArea {
                        lat0: 23.0,
                        lon0: -96.0,
                        sp1: 29.5,
                        sp2: 45.5,
                        fe: 0.0,
                        fn_: 0.0,
                    },
                    unit: Unit::metre(),
                    epsg: Some(5070),
                    wkt: None,
                })
            }
            3031 | 3413 | 3995 => {
                let (name, lat_ts, lon0) = match code {
                    3031 => ("WGS 84 / Antarctic Polar Stereographic", -71.0, 0.0),
                    3413 => ("WGS 84 / NSIDC Sea Ice Polar Stereographic North", 70.0, -45.0),
                    _ => ("WGS 84 / Arctic Polar Stereographic", 71.0, 0.0),
                };
                return Ok(Self {
                    name: name.into(),
                    datum: Datum::Wgs84,
                    ellipsoid: Ellipsoid::wgs84(),
                    projection: Projection::PolarStereographic {
                        lat_ts,
                        lon0,
                        fe: 0.0,
                        fn_: 0.0,
                    },
                    unit: Unit::metre(),
                    epsg: Some(code),
                    wkt: None,
                });
            }
            _ => {}
        }
        crate::gis::stateplane::from_epsg(code)
            .ok_or_else(|| Error::NotFound(format!("EPSG:{code} is not in the built-in CRS table")))
    }

    /// Parse WKT (OGC WKT1 or ESRI `.prj` text).
    pub fn from_wkt(text: &str) -> Result<Self> {
        crate::gis::prj::parse_wkt(text)
    }

    /// Parse `EPSG:nnnn`, a bare number, or WKT.
    pub fn parse(text: &str) -> Result<Self> {
        let t = text.trim();
        if let Some(code) = t
            .strip_prefix("EPSG:")
            .or_else(|| t.strip_prefix("epsg:"))
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            return Self::from_epsg(code);
        }
        if let Ok(code) = t.parse::<u32>() {
            return Self::from_epsg(code);
        }
        Self::from_wkt(t)
    }

    pub fn is_geographic(&self) -> bool {
        matches!(self.projection, Projection::Geographic)
    }

    /// Short label for the UI: name and code.
    pub fn label(&self) -> String {
        match self.epsg {
            Some(c) => format!("{} (EPSG:{c})", self.name),
            None => self.name.clone(),
        }
    }

    /// A one-line summary of what the system is.
    pub fn describe(&self) -> String {
        format!(
            "{} — {}, {} datum, {}",
            self.label(),
            self.projection.method_name(),
            self.datum.label(),
            if self.is_geographic() { "degrees".to_string() } else { self.unit.name.clone() }
        )
    }

    /// Whether two systems produce the same coordinates.
    pub fn same_as(&self, other: &Crs) -> bool {
        if let (Some(a), Some(b)) = (self.epsg, other.epsg) {
            return a == b;
        }
        self.projection == other.projection
            && self.datum.compatible(&other.datum)
            && (self.ellipsoid.a - other.ellipsoid.a).abs() < 1e-6
            && (self.unit.to_metre - other.unit.to_metre).abs() < 1e-12
    }

    /// Project longitude/latitude (degrees) to coordinates in this
    /// system's unit.
    pub fn forward(&self, lon: f64, lat: f64) -> Result<(f64, f64)> {
        if !(-90.0..=90.0).contains(&lat) || !lon.is_finite() {
            return Err(Error::Format(format!("latitude {lat} / longitude {lon} is out of range")));
        }
        if self.is_geographic() {
            return Ok((lon, lat));
        }
        let (x, y) = self.forward_metres(lon.to_radians(), lat.to_radians())?;
        Ok((x / self.unit.to_metre, y / self.unit.to_metre))
    }

    /// Unproject coordinates in this system's unit to longitude/latitude
    /// (degrees).
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        if !x.is_finite() || !y.is_finite() {
            return Err(Error::Format("coordinate is not finite".into()));
        }
        if self.is_geographic() {
            return Ok((x, y));
        }
        let (lon, lat) = self.inverse_metres(x * self.unit.to_metre, y * self.unit.to_metre)?;
        Ok((normalize_lon(lon.to_degrees()), lat.to_degrees()))
    }

    fn forward_metres(&self, lon: f64, lat: f64) -> Result<(f64, f64)> {
        let el = &self.ellipsoid;
        let a = el.a;
        let e2 = el.e2();
        let e = e2.sqrt();
        let d = f64::to_radians;
        Ok(match &self.projection {
            Projection::Geographic => (lon, lat),
            Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ } => {
                let (x, y) = tm_forward(a, el.f(), *k0, d(*lon0), lon, lat);
                let (_, y0) = tm_forward(a, el.f(), *k0, d(*lon0), d(*lon0), d(*lat0));
                (x + fe, y - y0 + fn_)
            }
            Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ } => {
                let (n, f, rho0) = lcc_2sp_constants(a, e, d(*lat0), d(*sp1), d(*sp2));
                lcc_forward(a, e, n, f, rho0, d(*lon0), *fe, *fn_, lon, lat)
            }
            Projection::LambertConformal1Sp { lat0, lon0, k0, fe, fn_ } => {
                let (n, f, rho0) = lcc_1sp_constants(a, e, d(*lat0), *k0);
                lcc_forward(a, e, n, f, rho0, d(*lon0), *fe, *fn_, lon, lat)
            }
            Projection::AlbersEqualArea { lat0, lon0, sp1, sp2, fe, fn_ } => {
                let (n, c, rho0) = albers_constants(a, e, d(*lat0), d(*sp1), d(*sp2));
                let q = albers_q(e, lat);
                let rho = a * (c - n * q).max(0.0).sqrt() / n;
                let theta = n * (lon - d(*lon0));
                (rho * theta.sin() + fe, rho0 - rho * theta.cos() + fn_)
            }
            Projection::Mercator { lon0, k0, fe, fn_ } => {
                if lat.abs() >= FRAC_PI_2 - 1e-10 {
                    return Err(Error::Format("Mercator is undefined at the poles".into()));
                }
                let t = iso_t(e, lat);
                (a * k0 * (lon - d(*lon0)) + fe, -a * k0 * t.ln() + fn_)
            }
            Projection::WebMercator { lon0, fe, fn_ } => {
                if lat.abs() >= FRAC_PI_2 - 1e-10 {
                    return Err(Error::Format("Web Mercator is undefined at the poles".into()));
                }
                (a * (lon - d(*lon0)) + fe, a * (FRAC_PI_4 + lat / 2.0).tan().ln() + fn_)
            }
            Projection::PolarStereographic { lat_ts, lon0, fe, fn_ } => {
                let south = *lat_ts < 0.0;
                let (phi, lam, lam0) = if south { (-lat, -lon, -d(*lon0)) } else { (lat, lon, d(*lon0)) };
                let tsr = d(lat_ts.abs());
                let mc = tsr.cos() / (1.0 - e2 * tsr.sin().powi(2)).sqrt();
                let tc = iso_t(e, tsr);
                let rho = a * mc * iso_t(e, phi) / tc;
                let (mut x, mut y) = (rho * (lam - lam0).sin(), -rho * (lam - lam0).cos());
                if south {
                    x = -x;
                    y = -y;
                }
                (x + fe, y + fn_)
            }
            Projection::HotineObliqueMercator { latc, lonc, azimuth, gamma, k0, fe, fn_, at_centre } => {
                let h = Hom::new(a, e, d(*latc), d(*lonc), d(*azimuth), d(*gamma), *k0, *at_centre);
                let (x, y) = h.forward(lon, lat);
                (x + fe, y + fn_)
            }
        })
    }

    fn inverse_metres(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let el = &self.ellipsoid;
        let a = el.a;
        let e2 = el.e2();
        let e = e2.sqrt();
        let d = f64::to_radians;
        Ok(match &self.projection {
            Projection::Geographic => (x, y),
            Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ } => {
                let (_, y0) = tm_forward(a, el.f(), *k0, d(*lon0), d(*lon0), d(*lat0));
                tm_inverse(a, el.f(), *k0, d(*lon0), x - fe, y - fn_ + y0)
            }
            Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ } => {
                let (n, f, rho0) = lcc_2sp_constants(a, e, d(*lat0), d(*sp1), d(*sp2));
                lcc_inverse(a, e, n, f, rho0, d(*lon0), x - fe, y - fn_)
            }
            Projection::LambertConformal1Sp { lat0, lon0, k0, fe, fn_ } => {
                let (n, f, rho0) = lcc_1sp_constants(a, e, d(*lat0), *k0);
                lcc_inverse(a, e, n, f, rho0, d(*lon0), x - fe, y - fn_)
            }
            Projection::AlbersEqualArea { lat0, lon0, sp1, sp2, fe, fn_ } => {
                let (n, c, rho0) = albers_constants(a, e, d(*lat0), d(*sp1), d(*sp2));
                let (x, y) = (x - fe, y - fn_);
                let sgn = n.signum();
                let rho = (x * x + (rho0 - y).powi(2)).sqrt();
                let theta = (sgn * x).atan2(sgn * (rho0 - y));
                let q = (c - rho * rho * n * n / (a * a)) / n;
                let qp = 1.0 - (1.0 - e2) / (2.0 * e) * ((1.0 - e) / (1.0 + e)).ln();
                let beta = (q / qp).clamp(-1.0, 1.0).asin();
                let e4 = e2 * e2;
                let e6 = e4 * e2;
                let mut lat = beta
                    + (e2 / 3.0 + 31.0 * e4 / 180.0 + 517.0 * e6 / 5040.0) * (2.0 * beta).sin()
                    + (23.0 * e4 / 360.0 + 251.0 * e6 / 3780.0) * (4.0 * beta).sin()
                    + (761.0 * e6 / 45360.0) * (6.0 * beta).sin();
                // Snyder's series (3-18) is truncated at e^6; a few Newton
                // steps on q(φ) (3-16) take the residual below a micrometre.
                for _ in 0..6 {
                    let s = lat.sin();
                    let dq = 2.0 * (1.0 - e2) * lat.cos() / (1.0 - e2 * s * s).powi(2);
                    if dq.abs() < 1e-30 {
                        break;
                    }
                    let step = (q - albers_q(e, lat)) / dq;
                    lat += step;
                    if step.abs() < 1e-15 {
                        break;
                    }
                }
                (d(*lon0) + theta / n, lat)
            }
            Projection::Mercator { lon0, k0, fe, fn_ } => {
                let t = (-(y - fn_) / (a * k0)).exp();
                (d(*lon0) + (x - fe) / (a * k0), lat_from_t(e, t))
            }
            Projection::WebMercator { lon0, fe, fn_ } => (
                d(*lon0) + (x - fe) / a,
                2.0 * ((y - fn_) / a).exp().atan() - FRAC_PI_2,
            ),
            Projection::PolarStereographic { lat_ts, lon0, fe, fn_ } => {
                let south = *lat_ts < 0.0;
                let (mut x, mut y) = (x - fe, y - fn_);
                let lam0 = if south { -d(*lon0) } else { d(*lon0) };
                if south {
                    x = -x;
                    y = -y;
                }
                let tsr = d(lat_ts.abs());
                let mc = tsr.cos() / (1.0 - e2 * tsr.sin().powi(2)).sqrt();
                let tc = iso_t(e, tsr);
                let rho = (x * x + y * y).sqrt();
                let t = rho * tc / (a * mc);
                let phi = if rho == 0.0 { FRAC_PI_2 } else { lat_from_t(e, t) };
                let lam = if rho == 0.0 { lam0 } else { lam0 + x.atan2(-y) };
                if south {
                    (-lam, -phi)
                } else {
                    (lam, phi)
                }
            }
            Projection::HotineObliqueMercator { latc, lonc, azimuth, gamma, k0, fe, fn_, at_centre } => {
                let h = Hom::new(a, e, d(*latc), d(*lonc), d(*azimuth), d(*gamma), *k0, *at_centre);
                h.inverse(x - fe, y - fn_)
            }
        })
    }

    /// ESRI-style WKT for this system (what a `.prj` holds), with an
    /// `AUTHORITY` node when the EPSG code is known.
    pub fn to_wkt(&self) -> String {
        if let Some(w) = &self.wkt {
            return w.clone();
        }
        let el = &self.ellipsoid;
        let geogcs_name = match self.datum {
            Datum::Wgs84 => "GCS_WGS_1984".to_string(),
            Datum::Nad83 => "GCS_North_American_1983".to_string(),
            Datum::Nad27 => "GCS_North_American_1927".to_string(),
            Datum::Sphere => "GCS_WGS_1984_Major_Auxiliary_Sphere".to_string(),
            Datum::Other(ref n) => format!("GCS_{n}"),
        };
        let spheroid_name = el.name.replace(' ', "_");
        let mut geogcs = format!(
            "GEOGCS[\"{geogcs_name}\",DATUM[\"{}\",SPHEROID[\"{spheroid_name}\",{},{}]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]",
            self.datum.wkt_name(),
            fmt(el.a),
            fmt(el.inv_f)
        );
        if self.is_geographic() {
            if let Some(c) = self.epsg {
                geogcs.push_str(&format!(",AUTHORITY[\"EPSG\",\"{c}\"]"));
            }
            geogcs.push(']');
            return geogcs;
        }
        geogcs.push(']');
        let u = self.unit.to_metre;
        let p = |name: &str, v: f64| format!(",PARAMETER[\"{name}\",{}]", fmt(v));
        let (method, params) = match &self.projection {
            Projection::Geographic => unreachable!(),
            Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ } => (
                "Transverse_Mercator",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Scale_Factor", *k0)
                    + &p("Latitude_Of_Origin", *lat0),
            ),
            Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ } => (
                "Lambert_Conformal_Conic",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Standard_Parallel_1", *sp1)
                    + &p("Standard_Parallel_2", *sp2)
                    + &p("Latitude_Of_Origin", *lat0),
            ),
            Projection::LambertConformal1Sp { lat0, lon0, k0, fe, fn_ } => (
                "Lambert_Conformal_Conic",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Standard_Parallel_1", *lat0)
                    + &p("Scale_Factor", *k0)
                    + &p("Latitude_Of_Origin", *lat0),
            ),
            Projection::AlbersEqualArea { lat0, lon0, sp1, sp2, fe, fn_ } => (
                "Albers",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Standard_Parallel_1", *sp1)
                    + &p("Standard_Parallel_2", *sp2)
                    + &p("Latitude_Of_Origin", *lat0),
            ),
            Projection::Mercator { lon0, k0, fe, fn_ } => (
                "Mercator_1SP",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Scale_Factor", *k0),
            ),
            Projection::WebMercator { lon0, fe, fn_ } => (
                "Mercator_Auxiliary_Sphere",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Standard_Parallel_1", 0.0)
                    + &p("Auxiliary_Sphere_Type", 0.0),
            ),
            Projection::PolarStereographic { lat_ts, lon0, fe, fn_ } => (
                "Polar_Stereographic",
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Central_Meridian", *lon0)
                    + &p("Standard_Parallel_1", *lat_ts),
            ),
            Projection::HotineObliqueMercator { latc, lonc, azimuth, gamma, k0, fe, fn_, at_centre } => (
                if *at_centre { "Hotine_Oblique_Mercator_Azimuth_Center" } else { "Hotine_Oblique_Mercator_Azimuth_Natural_Origin" },
                p("False_Easting", fe / u)
                    + &p("False_Northing", fn_ / u)
                    + &p("Scale_Factor", *k0)
                    + &p("Azimuth", *azimuth)
                    + &p("Longitude_Of_Center", *lonc)
                    + &p("Latitude_Of_Center", *latc)
                    + &p("Rectified_Grid_Angle", *gamma),
            ),
        };
        let mut out = format!(
            "PROJCS[\"{}\",{geogcs},PROJECTION[\"{method}\"]{params},{}",
            self.name.replace(' ', "_").replace('/', "_"),
            self.unit.wkt()
        );
        if let Some(c) = self.epsg {
            out.push_str(&format!(",AUTHORITY[\"EPSG\",\"{c}\"]"));
        }
        out.push(']');
        out
    }
}

fn normalize_lon(mut lon: f64) -> f64 {
    while lon > 180.0 {
        lon -= 360.0;
    }
    while lon < -180.0 {
        lon += 360.0;
    }
    lon
}

/// Transform a coordinate from one system to another. Errors when either
/// datum needs a shift this crate does not carry (NAD27).
pub fn transform(from: &Crs, to: &Crs, x: f64, y: f64) -> Result<(f64, f64)> {
    if from.same_as(to) {
        return Ok((x, y));
    }
    if !from.datum.compatible(&to.datum) {
        return Err(Error::Format(format!(
            "unsupported datum shift: {} to {} (only NAD83 and WGS 84, treated as one datum, are supported)",
            from.datum.label(),
            to.datum.label()
        )));
    }
    let (lon, lat) = from.inverse(x, y)?;
    to.forward(lon, lat)
}

// ---------------------------------------------------------------------------
// Shared ellipsoidal helpers (Snyder 1987)
// ---------------------------------------------------------------------------

/// Snyder's `t` (eq. 15-9): the isometric-latitude quantity.
fn iso_t(e: f64, phi: f64) -> f64 {
    let es = e * phi.sin();
    (FRAC_PI_4 - phi / 2.0).tan() / ((1.0 - es) / (1.0 + es)).powf(e / 2.0)
}

/// Latitude from `t` by Snyder's iteration (eq. 7-9), to 1e-15 rad.
fn lat_from_t(e: f64, t: f64) -> f64 {
    let mut phi = FRAC_PI_2 - 2.0 * t.atan();
    for _ in 0..30 {
        let es = e * phi.sin();
        let next = FRAC_PI_2 - 2.0 * (t * ((1.0 - es) / (1.0 + es)).powf(e / 2.0)).atan();
        if (next - phi).abs() < 1e-15 {
            return next;
        }
        phi = next;
    }
    phi
}

/// Snyder's `m` (eq. 14-15).
fn iso_m(e: f64, phi: f64) -> f64 {
    phi.cos() / (1.0 - e * e * phi.sin().powi(2)).sqrt()
}

// -- Transverse Mercator: Krüger series (Karney 2011, eqs. 7–12, 35) ------------

struct Kruger {
    a_rect: f64,
    alpha: [f64; 6],
    beta: [f64; 6],
    e: f64,
}

impl Kruger {
    fn new(a: f64, f: f64) -> Self {
        let n = f / (2.0 - f);
        let n2 = n * n;
        let n3 = n2 * n;
        let n4 = n3 * n;
        let n5 = n4 * n;
        let n6 = n5 * n;
        let a_rect = a / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
        let alpha = [
            n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
                + 7891.0 * n6 / 37800.0,
            13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
                - 1983433.0 * n6 / 1935360.0,
            61.0 * n3 / 240.0 - 103.0 * n4 / 140.0 + 15061.0 * n5 / 26880.0 + 167603.0 * n6 / 181440.0,
            49561.0 * n4 / 161280.0 - 179.0 * n5 / 168.0 + 6601661.0 * n6 / 7257600.0,
            34729.0 * n5 / 80640.0 - 3418889.0 * n6 / 1995840.0,
            212378941.0 * n6 / 319334400.0,
        ];
        let beta = [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0 - 81.0 * n5 / 512.0
                + 96199.0 * n6 / 604800.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0 + 46.0 * n5 / 105.0 - 1118711.0 * n6 / 3870720.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0 - 209.0 * n5 / 4480.0 + 5569.0 * n6 / 90720.0,
            4397.0 * n4 / 161280.0 - 11.0 * n5 / 504.0 - 830251.0 * n6 / 7257600.0,
            4583.0 * n5 / 161280.0 - 108847.0 * n6 / 3991680.0,
            20648693.0 * n6 / 638668800.0,
        ];
        let e = (2.0 * f - f * f).sqrt();
        Self {
            a_rect,
            alpha,
            beta,
            e,
        }
    }
}

/// Forward TM about `lon0` (radians): x east of the meridian, y from the
/// equator, both in metres, scaled by `k0`.
fn tm_forward(a: f64, f: f64, k0: f64, lon0: f64, lon: f64, lat: f64) -> (f64, f64) {
    let k = Kruger::new(a, f);
    let e = k.e;
    let dl = lon - lon0;
    // Conformal latitude via tau' (Karney eq. 8).
    let tau = lat.tan();
    let sigma = (e * (e * tau / (1.0 + tau * tau).sqrt()).atanh()).sinh();
    let taup = tau * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + tau * tau).sqrt();
    let xi_p = taup.atan2(dl.cos());
    let eta_p = (dl.sin() / (taup * taup + dl.cos() * dl.cos()).sqrt()).asinh();
    let mut xi = xi_p;
    let mut eta = eta_p;
    for (j, al) in k.alpha.iter().enumerate() {
        let m = 2.0 * (j + 1) as f64;
        xi += al * (m * xi_p).sin() * (m * eta_p).cosh();
        eta += al * (m * xi_p).cos() * (m * eta_p).sinh();
    }
    (k0 * k.a_rect * eta, k0 * k.a_rect * xi)
}

/// Inverse of [`tm_forward`]: longitude and latitude in radians.
fn tm_inverse(a: f64, f: f64, k0: f64, lon0: f64, x: f64, y: f64) -> (f64, f64) {
    let k = Kruger::new(a, f);
    let e = k.e;
    let xi = y / (k0 * k.a_rect);
    let eta = x / (k0 * k.a_rect);
    let mut xi_p = xi;
    let mut eta_p = eta;
    for (j, be) in k.beta.iter().enumerate() {
        let m = 2.0 * (j + 1) as f64;
        xi_p -= be * (m * xi).sin() * (m * eta).cosh();
        eta_p -= be * (m * xi).cos() * (m * eta).sinh();
    }
    let taup = xi_p.sin() / (eta_p.sinh().powi(2) + xi_p.cos().powi(2)).sqrt();
    let lon = lon0 + eta_p.sinh().atan2(xi_p.cos());
    // tau from tau' by Newton (Karney eqs. 19–21).
    let mut tau = taup;
    for _ in 0..20 {
        let sigma = (e * (e * tau / (1.0 + tau * tau).sqrt()).atanh()).sinh();
        let taup_i = tau * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + tau * tau).sqrt();
        let dtaup = (1.0 - e * e) * (1.0 + taup_i * taup_i).sqrt() * (1.0 + tau * tau).sqrt()
            / (1.0 + (1.0 - e * e) * tau * tau);
        let step = (taup - taup_i) / dtaup;
        tau += step;
        if step.abs() < 1e-16 * (1.0 + tau.abs()) {
            break;
        }
    }
    (lon, tau.atan())
}

// -- Lambert conformal conic (Snyder eqs. 15-1 .. 15-11) ----------------------

fn lcc_2sp_constants(a: f64, e: f64, lat0: f64, sp1: f64, sp2: f64) -> (f64, f64, f64) {
    let m1 = iso_m(e, sp1);
    let m2 = iso_m(e, sp2);
    let t0 = iso_t(e, lat0);
    let t1 = iso_t(e, sp1);
    let t2 = iso_t(e, sp2);
    let n = if (sp1 - sp2).abs() < 1e-10 {
        sp1.sin()
    } else {
        (m1.ln() - m2.ln()) / (t1.ln() - t2.ln())
    };
    let f = m1 / (n * t1.powf(n));
    let rho0 = a * f * t0.powf(n);
    (n, f, rho0)
}

fn lcc_1sp_constants(a: f64, e: f64, lat0: f64, k0: f64) -> (f64, f64, f64) {
    let m0 = iso_m(e, lat0);
    let t0 = iso_t(e, lat0);
    let n = lat0.sin();
    let f = k0 * m0 / (n * t0.powf(n));
    let rho0 = a * f * t0.powf(n);
    (n, f, rho0)
}

#[allow(clippy::too_many_arguments)]
fn lcc_forward(a: f64, e: f64, n: f64, f: f64, rho0: f64, lon0: f64, fe: f64, fn_: f64, lon: f64, lat: f64) -> (f64, f64) {
    let rho = if lat.abs() >= FRAC_PI_2 - 1e-12 {
        0.0
    } else {
        a * f * iso_t(e, lat).powf(n)
    };
    let theta = n * (lon - lon0);
    (rho * theta.sin() + fe, rho0 - rho * theta.cos() + fn_)
}

#[allow(clippy::too_many_arguments)]
fn lcc_inverse(a: f64, e: f64, n: f64, f: f64, rho0: f64, lon0: f64, x: f64, y: f64) -> (f64, f64) {
    let sgn = n.signum();
    let rho = sgn * (x * x + (rho0 - y).powi(2)).sqrt();
    let theta = (sgn * x).atan2(sgn * (rho0 - y));
    if rho == 0.0 {
        return (lon0, sgn * FRAC_PI_2);
    }
    let t = (rho / (a * f)).powf(1.0 / n);
    (lon0 + theta / n, lat_from_t(e, t))
}

// -- Albers equal area (Snyder eqs. 14-1 .. 14-21) ----------------------------

fn albers_q(e: f64, phi: f64) -> f64 {
    let s = phi.sin();
    let es = e * s;
    (1.0 - e * e) * (s / (1.0 - es * es) - (1.0 / (2.0 * e)) * ((1.0 - es) / (1.0 + es)).ln())
}

fn albers_constants(a: f64, e: f64, lat0: f64, sp1: f64, sp2: f64) -> (f64, f64, f64) {
    let m1 = iso_m(e, sp1);
    let m2 = iso_m(e, sp2);
    let q0 = albers_q(e, lat0);
    let q1 = albers_q(e, sp1);
    let q2 = albers_q(e, sp2);
    let n = if (sp1 - sp2).abs() < 1e-10 {
        sp1.sin()
    } else {
        (m1 * m1 - m2 * m2) / (q2 - q1)
    };
    let c = m1 * m1 + n * q1;
    let rho0 = a * (c - n * q0).sqrt() / n;
    (n, c, rho0)
}

// -- Hotine oblique Mercator (Snyder eqs. 9-6 .. 9-48; EPSG 9812/9815) --------

struct Hom {
    a_: f64,
    b: f64,
    e_: f64,
    e: f64,
    gamma0: f64,
    lam0: f64,
    gamma_c: f64,
    uc: f64,
}

impl Hom {
    #[allow(clippy::too_many_arguments)]
    fn new(a: f64, e: f64, latc: f64, lonc: f64, azimuth: f64, gamma_c: f64, k0: f64, at_centre: bool) -> Self {
        let e2 = e * e;
        let s0 = latc.sin();
        let c0 = latc.cos();
        let b = (1.0 + e2 * c0.powi(4) / (1.0 - e2)).sqrt();
        let a_ = a * b * k0 * (1.0 - e2).sqrt() / (1.0 - e2 * s0 * s0);
        let t0 = iso_t(e, latc);
        let d = b * (1.0 - e2).sqrt() / (c0 * (1.0 - e2 * s0 * s0).sqrt());
        let d2 = if d < 1.0 { 1.0 } else { d * d };
        let f = d + (d2 - 1.0).sqrt() * latc.signum();
        let e_ = f * t0.powf(b);
        let g = (f - 1.0 / f) / 2.0;
        let gamma0 = (azimuth.sin() / d).clamp(-1.0, 1.0).asin();
        let lam0 = lonc - ((g * gamma0.tan()).clamp(-1.0, 1.0)).asin() / b;
        let uc = if at_centre {
            if (azimuth - FRAC_PI_2).abs() < 1e-12 {
                a_ * (lonc - lam0)
            } else {
                (a_ / b) * ((d2 - 1.0).sqrt() / azimuth.cos()).atan() * latc.signum()
            }
        } else {
            0.0
        };
        Self {
            a_,
            b,
            e_,
            e,
            gamma0,
            lam0,
            gamma_c,
            uc,
        }
    }

    fn forward(&self, lon: f64, lat: f64) -> (f64, f64) {
        let t = iso_t(self.e, lat);
        let q = self.e_ / t.powf(self.b);
        let s = (q - 1.0 / q) / 2.0;
        let tt = (q + 1.0 / q) / 2.0;
        let dl = self.b * (lon - self.lam0);
        let v = dl.sin();
        let u_ = (-v * self.gamma0.cos() + s * self.gamma0.sin()) / tt;
        let v_ = self.a_ * ((1.0 - u_) / (1.0 + u_)).ln() / (2.0 * self.b);
        let u = self.a_ * (s * self.gamma0.cos() + v * self.gamma0.sin()).atan2(dl.cos()) / self.b - self.uc;
        (
            v_ * self.gamma_c.cos() + u * self.gamma_c.sin(),
            u * self.gamma_c.cos() - v_ * self.gamma_c.sin(),
        )
    }

    fn inverse(&self, x: f64, y: f64) -> (f64, f64) {
        let v_ = x * self.gamma_c.cos() - y * self.gamma_c.sin();
        let u = y * self.gamma_c.cos() + x * self.gamma_c.sin() + self.uc;
        let q = (-self.b * v_ / self.a_).exp();
        let s = (q - 1.0 / q) / 2.0;
        let tt = (q + 1.0 / q) / 2.0;
        let bu = self.b * u / self.a_;
        let v = bu.sin();
        let u_ = (v * self.gamma0.cos() + s * self.gamma0.sin()) / tt;
        let t = (self.e_ / ((1.0 + u_) / (1.0 - u_)).sqrt()).powf(1.0 / self.b);
        let lat = lat_from_t(self.e, t);
        let lon = self.lam0 - (s * self.gamma0.cos() - v * self.gamma0.sin()).atan2(bu.cos()) / self.b;
        (lon, lat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        (a.0 - b.0).abs() < tol && (a.1 - b.1).abs() < tol
    }

    #[test]
    fn utm_zone_lookup_and_epsg_codes() {
        assert_eq!(Crs::utm_zone_for(-80.8431, 35.2271), (17, true));
        assert_eq!(Crs::utm_zone_for(151.2, -33.9), (56, false));
        let z = Crs::from_epsg(32617).unwrap();
        assert_eq!(z.name, "WGS 84 / UTM zone 17N");
        assert_eq!(Crs::from_epsg(26917).unwrap().datum, Datum::Nad83);
        assert_eq!(Crs::from_epsg(32756).unwrap().projection, Projection::TransverseMercator { lat0: 0.0, lon0: 153.0, k0: 0.9996, fe: 500000.0, fn_: 10_000_000.0 });
        assert!(Crs::from_epsg(99999).is_err());
        assert_eq!(Crs::parse("EPSG:4326").unwrap().epsg, Some(4326));
        assert_eq!(Crs::parse("3857").unwrap().epsg, Some(3857));
    }

    #[test]
    fn nad27_transforms_are_refused_and_nad83_wgs84_are_one_datum() {
        let nad27 = Crs::from_epsg(26717).unwrap();
        let wgs = Crs::wgs84();
        let e = transform(&nad27, &wgs, 500000.0, 3900000.0).unwrap_err();
        assert!(e.to_string().contains("unsupported datum shift"), "{e}");
        let nad83 = Crs::from_epsg(26917).unwrap();
        let (lon, lat) = transform(&nad83, &wgs, 514277.72, 3898239.34).unwrap();
        assert!(close((lon, lat), (-80.8431, 35.2271), 1e-5));
        // Same system: identity even for odd values.
        assert_eq!(transform(&nad83, &nad83, 1.0, 2.0).unwrap(), (1.0, 2.0));
    }

    #[test]
    fn every_projection_round_trips_to_a_micrometre() {
        let systems = [
            Crs::from_epsg(32617).unwrap(),
            Crs::from_epsg(2264).unwrap(),
            Crs::from_epsg(26931).unwrap(),
            Crs::from_epsg(5070).unwrap(),
            Crs::from_epsg(3857).unwrap(),
            Crs::from_epsg(3031).unwrap(),
            Crs::from_epsg(3413).unwrap(),
            Crs::from_epsg(26948).unwrap(),
            Crs {
                name: "merc".into(),
                datum: Datum::Wgs84,
                ellipsoid: Ellipsoid::wgs84(),
                projection: Projection::Mercator { lon0: -80.0, k0: 0.98, fe: 1000.0, fn_: 2000.0 },
                unit: Unit::metre(),
                epsg: None,
                wkt: None,
            },
            Crs {
                name: "lcc1".into(),
                datum: Datum::Nad83,
                ellipsoid: Ellipsoid::grs80(),
                projection: Projection::LambertConformal1Sp { lat0: 35.0, lon0: -80.0, k0: 0.9999, fe: 1000.0, fn_: 2000.0 },
                unit: Unit::us_foot(),
                epsg: None,
                wkt: None,
            },
        ];
        let points = [
            (-80.8431, 35.2271),
            (-134.4, 58.3),
            (-83.9, 33.5),
            (-96.0, 40.0),
            (0.0, -75.0),
            (-45.0, 80.0),
            (-110.0, 33.0),
            (-78.0, 34.5),
        ];
        for crs in &systems {
            for &(lon, lat) in &points {
                let (x, y) = crs.forward(lon, lat).unwrap();
                let (lon2, lat2) = crs.inverse(x, y).unwrap();
                let (x2, y2) = crs.forward(lon2, lat2).unwrap();
                let tol = 1e-6 / crs.unit.to_metre;
                assert!(
                    close((x, y), (x2, y2), tol),
                    "{}: ({lon},{lat}) -> ({x},{y}) -> ({lon2},{lat2}) -> ({x2},{y2})",
                    crs.name
                );
                assert!(close((lon, lat), (lon2, lat2), 1e-9), "{}: {lon},{lat} vs {lon2},{lat2}", crs.name);
            }
        }
    }

    #[test]
    fn wkt_generation_parses_back_to_the_same_system() {
        for code in [4326, 4269, 32617, 2264, 32119, 26931, 5070, 3857, 3031] {
            let crs = Crs::from_epsg(code).unwrap();
            let wkt = crs.to_wkt();
            let back = Crs::from_wkt(&wkt).unwrap_or_else(|e| panic!("{code}: {e}\n{wkt}"));
            assert_eq!(back.epsg, Some(code), "{wkt}");
            let (x, y) = crs.forward(-80.0, 35.0).unwrap();
            let (x2, y2) = back.forward(-80.0, 35.0).unwrap();
            assert!(close((x, y), (x2, y2), 1e-6), "{code}: {x},{y} vs {x2},{y2}");
        }
    }
}
