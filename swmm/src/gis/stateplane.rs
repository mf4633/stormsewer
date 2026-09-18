// SPDX-License-Identifier: GPL-3.0-or-later

//! The US State Plane Coordinate System of 1983: every zone's projection
//! type, origin, standard parallels and false origin (in metres), with
//! the EPSG codes for the metre form (26929–26998 and 32100–32161) and
//! the foot form where one exists.
//!
//! Zone parameters are those published in NOAA Manual NOS NGS 5, *State
//! Plane Coordinate System of 1983* (James E. Stem, 1990), Appendix A —
//! the defining constants, which are also what the EPSG registry carries
//! for these codes. The foot-unit EPSG codes (2222–2289 and the Indiana
//! pair 2965/2966) are as this table's author recalls the registry; the
//! zone constants they resolve to are the same as the metre codes', so a
//! wrong foot code would mislabel a system, not misplace it. Check a code
//! against epsg.org when the number itself matters.
//!
//! Also the UTM zones are identified here (from their parameters) so an
//! ESRI `.prj`, which carries no EPSG authority, gets its code.

use std::sync::OnceLock;

use crate::gis::crs::{Crs, Datum, Ellipsoid, Projection, Unit};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoneProj {
    /// Transverse Mercator: latitude of origin, central meridian, scale,
    /// false easting, false northing (m).
    Tm(f64, f64, f64, f64, f64),
    /// Lambert conformal conic: latitude of origin, central meridian,
    /// standard parallels 1 and 2, false easting, false northing (m).
    Lcc(f64, f64, f64, f64, f64, f64),
    /// Hotine oblique Mercator (variant A, EPSG 9812): latitude and
    /// longitude of centre, azimuth (also the rectified grid angle),
    /// scale, false easting, false northing at the natural origin (m).
    Om(f64, f64, f64, f64, f64, f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: &'static str,
    /// EPSG code of the metre form.
    pub epsg: u32,
    /// EPSG code of the foot form, and whether that foot is the US survey
    /// foot (`true`) or the international foot (`false`).
    pub epsg_ft: Option<(u32, bool)>,
    pub proj: ZoneProj,
}

fn dm(d: f64, m: f64) -> f64 {
    if d < 0.0 {
        d - m / 60.0
    } else {
        d + m / 60.0
    }
}

/// Every zone, in EPSG order.
pub fn zones() -> &'static [Zone] {
    static ZONES: OnceLock<Vec<Zone>> = OnceLock::new();
    ZONES.get_or_init(build)
}

fn build() -> Vec<Zone> {
    use ZoneProj::{Lcc, Om, Tm};
    let us = |c: u32| Some((c, true));
    let intl = |c: u32| Some((c, false));
    let z = |name: &'static str, epsg: u32, epsg_ft: Option<(u32, bool)>, proj: ZoneProj| Zone {
        name,
        epsg,
        epsg_ft,
        proj,
    };
    // Colorado, Connecticut and the Carolinas define their false origins
    // in feet; the metre values below are those conversions.
    const FT3M: f64 = 3_000_000.0 * 1200.0 / 3937.0; // 914401.8289
    const FT1M: f64 = 1_000_000.0 * 1200.0 / 3937.0; // 304800.6096
    const FT500K: f64 = 500_000.0 * 1200.0 / 3937.0; // 152400.3048
    const NC_FE: f64 = 2_000_000.0 * 1200.0 / 3937.0; // 609601.2192 (EPSG rounds to 609601.22)
    const SC_FE: f64 = 2_000_000.0 * 0.3048; // 609600 (international feet)
    const AZ_FE: f64 = 700_000.0 * 0.3048; // 213360 (international feet)
    vec![
        z("Alabama East", 26929, None, Tm(30.5, dm(-85.0, 50.0), 0.99996, 200000.0, 0.0)),
        z("Alabama West", 26930, None, Tm(30.0, -87.5, 0.9999333333333333, 600000.0, 0.0)),
        z("Alaska zone 1", 26931, None, Om(57.0, dm(-133.0, 40.0), 323.0 + 7.0 / 60.0 + 48.3685 / 3600.0, 0.9999, 5000000.0, -5000000.0)),
        z("Alaska zone 2", 26932, None, Tm(54.0, -142.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 3", 26933, None, Tm(54.0, -146.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 4", 26934, None, Tm(54.0, -150.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 5", 26935, None, Tm(54.0, -154.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 6", 26936, None, Tm(54.0, -158.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 7", 26937, None, Tm(54.0, -162.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 8", 26938, None, Tm(54.0, -166.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 9", 26939, None, Tm(54.0, -170.0, 0.9999, 500000.0, 0.0)),
        z("Alaska zone 10", 26940, None, Lcc(51.0, -176.0, dm(51.0, 50.0), dm(53.0, 50.0), 1000000.0, 0.0)),
        z("California zone 1", 26941, us(2225), Lcc(dm(39.0, 20.0), -122.0, 40.0, dm(41.0, 40.0), 2000000.0, 500000.0)),
        z("California zone 2", 26942, us(2226), Lcc(dm(37.0, 40.0), -122.0, dm(38.0, 20.0), dm(39.0, 50.0), 2000000.0, 500000.0)),
        z("California zone 3", 26943, us(2227), Lcc(36.5, -120.5, dm(37.0, 4.0), dm(38.0, 26.0), 2000000.0, 500000.0)),
        z("California zone 4", 26944, us(2228), Lcc(dm(35.0, 20.0), -119.0, 36.0, dm(37.0, 15.0), 2000000.0, 500000.0)),
        z("California zone 5", 26945, us(2229), Lcc(33.5, -118.0, dm(34.0, 2.0), dm(35.0, 28.0), 2000000.0, 500000.0)),
        z("California zone 6", 26946, us(2230), Lcc(dm(32.0, 10.0), -116.25, dm(32.0, 47.0), dm(33.0, 53.0), 2000000.0, 500000.0)),
        z("Arizona East", 26948, intl(2222), Tm(31.0, dm(-110.0, 10.0), 0.9999, AZ_FE, 0.0)),
        z("Arizona Central", 26949, intl(2223), Tm(31.0, dm(-111.0, 55.0), 0.9999, AZ_FE, 0.0)),
        z("Arizona West", 26950, intl(2224), Tm(31.0, -113.75, 0.9999333333333333, AZ_FE, 0.0)),
        z("Arkansas North", 26951, None, Lcc(dm(34.0, 20.0), -92.0, dm(34.0, 56.0), dm(36.0, 14.0), 400000.0, 0.0)),
        z("Arkansas South", 26952, None, Lcc(dm(32.0, 40.0), -92.0, dm(33.0, 18.0), dm(34.0, 46.0), 400000.0, 400000.0)),
        z("Colorado North", 26953, us(2231), Lcc(dm(39.0, 20.0), -105.5, dm(39.0, 43.0), dm(40.0, 47.0), FT3M, FT1M)),
        z("Colorado Central", 26954, us(2232), Lcc(dm(37.0, 50.0), -105.5, dm(38.0, 27.0), dm(39.0, 45.0), FT3M, FT1M)),
        z("Colorado South", 26955, us(2233), Lcc(dm(36.0, 40.0), -105.5, dm(37.0, 14.0), dm(38.0, 26.0), FT3M, FT1M)),
        z("Connecticut", 26956, us(2234), Lcc(dm(40.0, 50.0), -72.75, dm(41.0, 12.0), dm(41.0, 52.0), FT1M, FT500K)),
        z("Delaware", 26957, us(2235), Tm(38.0, dm(-75.0, 25.0), 0.999995, 200000.0, 0.0)),
        z("Florida East", 26958, us(2236), Tm(dm(24.0, 20.0), -81.0, 0.9999411764705882, 200000.0, 0.0)),
        z("Florida West", 26959, us(2237), Tm(dm(24.0, 20.0), -82.0, 0.9999411764705882, 200000.0, 0.0)),
        z("Florida North", 26960, us(2238), Lcc(29.0, -84.5, dm(29.0, 35.0), 30.75, 600000.0, 0.0)),
        z("Hawaii zone 1", 26961, None, Tm(dm(18.0, 50.0), -155.5, 0.9999666666666667, 500000.0, 0.0)),
        z("Hawaii zone 2", 26962, None, Tm(dm(20.0, 20.0), dm(-156.0, 40.0), 0.9999666666666667, 500000.0, 0.0)),
        z("Hawaii zone 3", 26963, None, Tm(dm(21.0, 10.0), -158.0, 0.99999, 500000.0, 0.0)),
        z("Hawaii zone 4", 26964, None, Tm(dm(21.0, 50.0), -159.5, 0.99999, 500000.0, 0.0)),
        z("Hawaii zone 5", 26965, None, Tm(dm(21.0, 40.0), dm(-160.0, 10.0), 1.0, 500000.0, 0.0)),
        z("Georgia East", 26966, us(2239), Tm(30.0, dm(-82.0, 10.0), 0.9999, 200000.0, 0.0)),
        z("Georgia West", 26967, us(2240), Tm(30.0, dm(-84.0, 10.0), 0.9999, 700000.0, 0.0)),
        z("Idaho East", 26968, us(2241), Tm(dm(41.0, 40.0), dm(-112.0, 10.0), 0.9999473684210526, 200000.0, 0.0)),
        z("Idaho Central", 26969, us(2242), Tm(dm(41.0, 40.0), -114.0, 0.9999473684210526, 500000.0, 0.0)),
        z("Idaho West", 26970, us(2243), Tm(dm(41.0, 40.0), -115.75, 0.9999333333333333, 800000.0, 0.0)),
        z("Illinois East", 26971, None, Tm(dm(36.0, 40.0), dm(-88.0, 20.0), 0.999975, 300000.0, 0.0)),
        z("Illinois West", 26972, None, Tm(dm(36.0, 40.0), dm(-90.0, 10.0), 0.9999411764705882, 700000.0, 0.0)),
        z("Indiana East", 26973, us(2965), Tm(37.5, dm(-85.0, 40.0), 0.9999666666666667, 100000.0, 250000.0)),
        z("Indiana West", 26974, us(2966), Tm(37.5, dm(-87.0, 5.0), 0.9999666666666667, 900000.0, 250000.0)),
        z("Iowa North", 26975, None, Lcc(41.5, -93.5, dm(42.0, 4.0), dm(43.0, 16.0), 1500000.0, 1000000.0)),
        z("Iowa South", 26976, None, Lcc(40.0, -93.5, dm(40.0, 37.0), dm(41.0, 47.0), 500000.0, 0.0)),
        z("Kansas North", 26977, None, Lcc(dm(38.0, 20.0), -98.0, dm(38.0, 43.0), dm(39.0, 47.0), 400000.0, 0.0)),
        z("Kansas South", 26978, None, Lcc(dm(36.0, 40.0), -98.5, dm(37.0, 16.0), dm(38.0, 34.0), 400000.0, 400000.0)),
        z("Kentucky North", 26979, us(2246), Lcc(37.5, -84.25, dm(37.0, 58.0), dm(38.0, 58.0), 500000.0, 0.0)),
        z("Kentucky South", 26980, us(2247), Lcc(dm(36.0, 20.0), -85.75, dm(36.0, 44.0), dm(37.0, 56.0), 500000.0, 500000.0)),
        z("Louisiana North", 26981, None, Lcc(30.5, -92.5, dm(31.0, 10.0), dm(32.0, 40.0), 1000000.0, 0.0)),
        z("Louisiana South", 26982, None, Lcc(28.5, dm(-91.0, 20.0), dm(29.0, 18.0), dm(30.0, 42.0), 1000000.0, 0.0)),
        z("Maine East", 26983, None, Tm(dm(43.0, 40.0), -68.5, 0.9999, 300000.0, 0.0)),
        z("Maine West", 26984, None, Tm(dm(42.0, 50.0), dm(-70.0, 10.0), 0.9999666666666667, 900000.0, 0.0)),
        z("Maryland", 26985, us(2248), Lcc(dm(37.0, 40.0), -77.0, dm(38.0, 18.0), dm(39.0, 27.0), 400000.0, 0.0)),
        z("Massachusetts Mainland", 26986, us(2249), Lcc(41.0, -71.5, dm(41.0, 43.0), dm(42.0, 41.0), 200000.0, 750000.0)),
        z("Massachusetts Island", 26987, us(2250), Lcc(41.0, -70.5, dm(41.0, 17.0), dm(41.0, 29.0), 500000.0, 0.0)),
        z("Michigan North", 26988, intl(2251), Lcc(dm(44.0, 47.0), -87.0, dm(45.0, 29.0), dm(47.0, 5.0), 8000000.0, 0.0)),
        z("Michigan Central", 26989, intl(2252), Lcc(dm(43.0, 19.0), dm(-84.0, 22.0), dm(44.0, 11.0), dm(45.0, 42.0), 6000000.0, 0.0)),
        z("Michigan South", 26990, intl(2253), Lcc(41.5, dm(-84.0, 22.0), dm(42.0, 6.0), dm(43.0, 40.0), 4000000.0, 0.0)),
        z("Minnesota North", 26991, None, Lcc(46.5, dm(-93.0, 6.0), dm(47.0, 2.0), dm(48.0, 38.0), 800000.0, 100000.0)),
        z("Minnesota Central", 26992, None, Lcc(45.0, -94.25, dm(45.0, 37.0), dm(47.0, 3.0), 800000.0, 100000.0)),
        z("Minnesota South", 26993, None, Lcc(43.0, -94.0, dm(43.0, 47.0), dm(45.0, 13.0), 800000.0, 100000.0)),
        z("Mississippi East", 26994, us(2254), Tm(29.5, dm(-88.0, 50.0), 0.99995, 300000.0, 0.0)),
        z("Mississippi West", 26995, us(2255), Tm(29.5, dm(-90.0, 20.0), 0.99995, 700000.0, 0.0)),
        z("Missouri East", 26996, None, Tm(dm(35.0, 50.0), -90.5, 0.9999333333333333, 250000.0, 0.0)),
        z("Missouri Central", 26997, None, Tm(dm(35.0, 50.0), -92.5, 0.9999333333333333, 500000.0, 0.0)),
        z("Missouri West", 26998, None, Tm(dm(36.0, 10.0), -94.5, 0.9999411764705882, 850000.0, 0.0)),
        z("Montana", 32100, intl(2256), Lcc(44.25, -109.5, 45.0, 49.0, 600000.0, 0.0)),
        z("Nebraska", 32104, None, Lcc(dm(39.0, 50.0), -100.0, 40.0, 43.0, 500000.0, 0.0)),
        z("Nevada East", 32107, None, Tm(34.75, dm(-115.0, 35.0), 0.9999, 200000.0, 8000000.0)),
        z("Nevada Central", 32108, None, Tm(34.75, dm(-116.0, 40.0), 0.9999, 500000.0, 6000000.0)),
        z("Nevada West", 32109, None, Tm(34.75, dm(-118.0, 35.0), 0.9999, 800000.0, 4000000.0)),
        z("New Hampshire", 32110, None, Tm(42.5, dm(-71.0, 40.0), 0.9999666666666667, 300000.0, 0.0)),
        z("New Jersey", 32111, None, Tm(dm(38.0, 50.0), -74.5, 0.9999, 150000.0, 0.0)),
        z("New Mexico East", 32112, us(2257), Tm(31.0, dm(-104.0, 20.0), 0.9999090909090909, 165000.0, 0.0)),
        z("New Mexico Central", 32113, us(2258), Tm(31.0, -106.25, 0.9999, 500000.0, 0.0)),
        z("New Mexico West", 32114, us(2259), Tm(31.0, dm(-107.0, 50.0), 0.9999166666666667, 830000.0, 0.0)),
        z("New York East", 32115, us(2260), Tm(dm(38.0, 50.0), -74.5, 0.9999, 150000.0, 0.0)),
        z("New York Central", 32116, us(2261), Tm(40.0, dm(-76.0, 35.0), 0.9999375, 250000.0, 0.0)),
        z("New York West", 32117, us(2262), Tm(40.0, dm(-78.0, 35.0), 0.9999375, 350000.0, 0.0)),
        z("New York Long Island", 32118, us(2263), Lcc(dm(40.0, 10.0), -74.0, dm(40.0, 40.0), dm(41.0, 2.0), 300000.0, 0.0)),
        z("North Carolina", 32119, us(2264), Lcc(33.75, -79.0, dm(34.0, 20.0), dm(36.0, 10.0), NC_FE, 0.0)),
        z("North Dakota North", 32120, intl(2265), Lcc(47.0, -100.5, dm(47.0, 26.0), dm(48.0, 44.0), 600000.0, 0.0)),
        z("North Dakota South", 32121, intl(2266), Lcc(dm(45.0, 40.0), -100.5, dm(46.0, 11.0), dm(47.0, 29.0), 600000.0, 0.0)),
        z("Ohio North", 32122, None, Lcc(dm(39.0, 40.0), -82.5, dm(40.0, 26.0), dm(41.0, 42.0), 600000.0, 0.0)),
        z("Ohio South", 32123, None, Lcc(38.0, -82.5, dm(38.0, 44.0), dm(40.0, 2.0), 600000.0, 0.0)),
        z("Oklahoma North", 32124, us(2267), Lcc(35.0, -98.0, dm(35.0, 34.0), dm(36.0, 46.0), 600000.0, 0.0)),
        z("Oklahoma South", 32125, us(2268), Lcc(dm(33.0, 20.0), -98.0, dm(33.0, 56.0), dm(35.0, 14.0), 600000.0, 0.0)),
        z("Oregon North", 32126, intl(2269), Lcc(dm(43.0, 40.0), -120.5, dm(44.0, 20.0), 46.0, 2500000.0, 0.0)),
        z("Oregon South", 32127, intl(2270), Lcc(dm(41.0, 40.0), -120.5, dm(42.0, 20.0), 44.0, 1500000.0, 0.0)),
        z("Pennsylvania North", 32128, us(2271), Lcc(dm(40.0, 10.0), -77.75, dm(40.0, 53.0), dm(41.0, 57.0), 600000.0, 0.0)),
        z("Pennsylvania South", 32129, us(2272), Lcc(dm(39.0, 20.0), -77.75, dm(39.0, 56.0), dm(40.0, 58.0), 600000.0, 0.0)),
        z("Rhode Island", 32130, None, Tm(dm(41.0, 5.0), -71.5, 0.99999375, 100000.0, 0.0)),
        z("South Carolina", 32133, intl(2273), Lcc(dm(31.0, 50.0), -81.0, 32.5, dm(34.0, 50.0), SC_FE, 0.0)),
        z("South Dakota North", 32134, None, Lcc(dm(43.0, 50.0), -100.0, dm(44.0, 25.0), dm(45.0, 41.0), 600000.0, 0.0)),
        z("South Dakota South", 32135, None, Lcc(dm(42.0, 20.0), dm(-100.0, 20.0), dm(42.0, 50.0), dm(44.0, 24.0), 600000.0, 0.0)),
        z("Tennessee", 32136, us(2274), Lcc(dm(34.0, 20.0), -86.0, 35.25, dm(36.0, 25.0), 600000.0, 0.0)),
        z("Texas North", 32137, us(2275), Lcc(34.0, -101.5, dm(34.0, 39.0), dm(36.0, 11.0), 200000.0, 1000000.0)),
        z("Texas North Central", 32138, us(2276), Lcc(dm(31.0, 40.0), -98.5, dm(32.0, 8.0), dm(33.0, 58.0), 600000.0, 2000000.0)),
        z("Texas Central", 32139, us(2277), Lcc(dm(29.0, 40.0), dm(-100.0, 20.0), dm(30.0, 7.0), dm(31.0, 53.0), 700000.0, 3000000.0)),
        z("Texas South Central", 32140, us(2278), Lcc(dm(27.0, 50.0), -99.0, dm(28.0, 23.0), dm(30.0, 17.0), 600000.0, 4000000.0)),
        z("Texas South", 32141, us(2279), Lcc(dm(25.0, 40.0), -98.5, dm(26.0, 10.0), dm(27.0, 50.0), 300000.0, 5000000.0)),
        z("Utah North", 32142, intl(2280), Lcc(dm(40.0, 20.0), -111.5, dm(40.0, 43.0), dm(41.0, 47.0), 500000.0, 1000000.0)),
        z("Utah Central", 32143, intl(2281), Lcc(dm(38.0, 20.0), -111.5, dm(39.0, 1.0), dm(40.0, 39.0), 500000.0, 2000000.0)),
        z("Utah South", 32144, intl(2282), Lcc(dm(36.0, 40.0), -111.5, dm(37.0, 13.0), dm(38.0, 21.0), 500000.0, 3000000.0)),
        z("Vermont", 32145, None, Tm(42.5, -72.5, 0.9999642857142857, 500000.0, 0.0)),
        z("Virginia North", 32146, us(2283), Lcc(dm(37.0, 40.0), -78.5, dm(38.0, 2.0), dm(39.0, 12.0), 3500000.0, 2000000.0)),
        z("Virginia South", 32147, us(2284), Lcc(dm(36.0, 20.0), -78.5, dm(36.0, 46.0), dm(37.0, 58.0), 3500000.0, 1000000.0)),
        z("Washington North", 32148, us(2285), Lcc(47.0, dm(-120.0, 50.0), 47.5, dm(48.0, 44.0), 500000.0, 0.0)),
        z("Washington South", 32149, us(2286), Lcc(dm(45.0, 20.0), -120.5, dm(45.0, 50.0), dm(47.0, 20.0), 500000.0, 0.0)),
        z("West Virginia North", 32150, None, Lcc(38.5, -79.5, 39.0, 40.25, 600000.0, 0.0)),
        z("West Virginia South", 32151, None, Lcc(37.0, -81.0, dm(37.0, 29.0), dm(38.0, 53.0), 600000.0, 0.0)),
        z("Wisconsin North", 32152, us(2287), Lcc(dm(45.0, 10.0), -90.0, dm(45.0, 34.0), dm(46.0, 46.0), 600000.0, 0.0)),
        z("Wisconsin Central", 32153, us(2288), Lcc(dm(43.0, 50.0), -90.0, 44.25, 45.5, 600000.0, 0.0)),
        z("Wisconsin South", 32154, us(2289), Lcc(42.0, -90.0, dm(42.0, 44.0), dm(44.0, 4.0), 600000.0, 0.0)),
        z("Wyoming East", 32155, None, Tm(40.5, dm(-105.0, 10.0), 0.9999375, 200000.0, 0.0)),
        z("Wyoming East Central", 32156, None, Tm(40.5, dm(-107.0, 20.0), 0.9999375, 400000.0, 100000.0)),
        z("Wyoming West Central", 32157, None, Tm(40.5, -108.75, 0.9999375, 600000.0, 0.0)),
        z("Wyoming West", 32158, None, Tm(40.5, dm(-110.0, 5.0), 0.9999375, 800000.0, 100000.0)),
        z("Puerto Rico and Virgin Islands", 32161, None, Lcc(dm(17.0, 50.0), dm(-66.0, 26.0), dm(18.0, 2.0), dm(18.0, 26.0), 200000.0, 200000.0)),
    ]
}

fn projection_of(zone: &Zone) -> Projection {
    match zone.proj {
        ZoneProj::Tm(lat0, lon0, k0, fe, fn_) => Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ },
        ZoneProj::Lcc(lat0, lon0, sp1, sp2, fe, fn_) => Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ },
        // Alaska zone 1 is EPSG method 9812, Hotine Oblique Mercator
        // variant A: the false origin sits at the natural origin (where
        // the initial line crosses the aposphere's equator), which is why
        // its false northing is -5,000,000 m.
        ZoneProj::Om(latc, lonc, az, k0, fe, fn_) => Projection::HotineObliqueMercator {
            latc,
            lonc,
            azimuth: az,
            gamma: az,
            k0,
            fe,
            fn_,
            at_centre: false,
        },
    }
}

/// The zone's CRS in metres, or in its foot unit when `feet`.
pub fn crs_for(zone: &Zone, feet: bool) -> Crs {
    let (unit, epsg, suffix) = match (feet, zone.epsg_ft) {
        (true, Some((code, true))) => (Unit::us_foot(), Some(code), " (ftUS)"),
        (true, Some((code, false))) => (Unit::intl_foot(), Some(code), " (ft)"),
        _ => (Unit::metre(), Some(zone.epsg), ""),
    };
    Crs {
        name: format!("NAD83 / {}{suffix}", zone.name),
        datum: Datum::Nad83,
        ellipsoid: Ellipsoid::grs80(),
        projection: projection_of(zone),
        unit,
        epsg,
        wkt: None,
    }
}

/// A State Plane zone by either of its EPSG codes. Also accepts 2205
/// (the current Kentucky North code, same constants as 26979).
pub fn from_epsg(code: u32) -> Option<Crs> {
    let code = if code == 2205 { 26979 } else { code };
    zones().iter().find_map(|z| {
        if z.epsg == code {
            Some(crs_for(z, false))
        } else if z.epsg_ft.is_some_and(|(c, _)| c == code) {
            Some(crs_for(z, true))
        } else {
            None
        }
    })
}

/// Find the zone by name fragment (case-insensitive), e.g. "north carolina".
pub fn find_by_name(fragment: &str) -> Vec<&'static Zone> {
    let f = fragment.to_ascii_lowercase();
    zones()
        .iter()
        .filter(|z| z.name.to_ascii_lowercase().contains(&f))
        .collect()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn same_projection(a: &Projection, b: &Projection, unit_m: f64) -> bool {
    // False origins match to a centimetre in the CRS's unit (ESRI writes
    // 2000000.002616666 ft for NC's 609601.22 m); angles to 1e-7 degrees.
    let ftol = 0.02 * unit_m.max(0.3);
    match (a, b) {
        (
            Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ },
            Projection::TransverseMercator { lat0: b0, lon0: b1, k0: b2, fe: b3, fn_: b4 },
        ) => near(*lat0, *b0, 1e-7) && near(*lon0, *b1, 1e-7) && near(*k0, *b2, 1e-9) && near(*fe, *b3, ftol) && near(*fn_, *b4, ftol),
        (
            Projection::LambertConformal2Sp { lat0, lon0, sp1, sp2, fe, fn_ },
            Projection::LambertConformal2Sp { lat0: b0, lon0: b1, sp1: b2, sp2: b3, fe: b4, fn_: b5 },
        ) => {
            near(*lat0, *b0, 1e-7)
                && near(*lon0, *b1, 1e-7)
                && ((near(*sp1, *b2, 1e-7) && near(*sp2, *b3, 1e-7)) || (near(*sp1, *b3, 1e-7) && near(*sp2, *b2, 1e-7)))
                && near(*fe, *b4, ftol)
                && near(*fn_, *b5, ftol)
        }
        (
            Projection::HotineObliqueMercator { latc, lonc, azimuth, k0, fe, fn_, at_centre, .. },
            Projection::HotineObliqueMercator { latc: b0, lonc: b1, azimuth: b2, k0: b3, fe: b4, fn_: b5, at_centre: b6, .. },
        ) => {
            at_centre == b6
                && near(*latc, *b0, 1e-7)
                && near(*lonc, *b1, 1e-7)
                && near(*azimuth, *b2, 1e-6)
                && near(*k0, *b3, 1e-9)
                && near(*fe, *b4, ftol)
                && near(*fn_, *b5, ftol)
        }
        (Projection::WebMercator { .. }, Projection::WebMercator { .. }) => true,
        _ => false,
    }
}

/// The EPSG code of a system whose parameters match a State Plane zone
/// (in the matching unit), a UTM zone, or Web Mercator.
pub fn identify(crs: &Crs) -> Option<u32> {
    let um = crs.unit.to_metre;
    if let Projection::WebMercator { fe, fn_, lon0 } = crs.projection {
        return (fe == 0.0 && fn_ == 0.0 && lon0 == 0.0).then_some(3857);
    }
    if let Projection::TransverseMercator { lat0, lon0, k0, fe, fn_ } = crs.projection {
        if lat0 == 0.0 && near(k0, 0.9996, 1e-9) && near(fe, 500000.0, 0.01) && near(um, 1.0, 1e-9) {
            let z = ((lon0 + 183.0) / 6.0).round();
            if (1.0..=60.0).contains(&z) && near(lon0, -183.0 + 6.0 * z, 1e-7) {
                let north = near(fn_, 0.0, 0.01);
                let south = near(fn_, 10_000_000.0, 0.01);
                if north || south {
                    let datum = match crs.datum {
                        Datum::Nad27 => Datum::Nad27,
                        Datum::Nad83 => Datum::Nad83,
                        _ => Datum::Wgs84,
                    };
                    return Crs::utm(z as u8, north, datum).ok().and_then(|c| c.epsg);
                }
            }
        }
    }
    if crs.datum != Datum::Nad83 && crs.datum != Datum::Wgs84 {
        return None;
    }
    let feet = !near(um, 1.0, 1e-9);
    for z in zones() {
        let candidate = crs_for(z, feet);
        if feet && !near(candidate.unit.to_metre, um, 1e-9) {
            // A zone in feet of the other kind: still identify the
            // metre-form constants but do not claim a foot code.
            if same_projection(&crs.projection, &candidate.projection, um) {
                return None;
            }
            continue;
        }
        if same_projection(&crs.projection, &candidate.projection, um) {
            return candidate.epsg;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_every_zone_and_codes_are_unique() {
        let zs = zones();
        assert_eq!(zs.len(), 122);
        let mut codes: Vec<u32> = zs.iter().map(|z| z.epsg).collect();
        codes.extend(zs.iter().filter_map(|z| z.epsg_ft.map(|(c, _)| c)));
        let n = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), n, "duplicate EPSG code");
        for z in zs {
            assert!((26929..=26998).contains(&z.epsg) || (32100..=32161).contains(&z.epsg), "{}", z.name);
        }
        let nc = from_epsg(2264).unwrap();
        assert_eq!(nc.unit.name, "US survey foot");
        assert_eq!(nc.name, "NAD83 / North Carolina (ftUS)");
        let ncm = from_epsg(32119).unwrap();
        assert_eq!(ncm.unit.to_metre, 1.0);
        assert_eq!(from_epsg(2205).unwrap().epsg, Some(26979));
        assert_eq!(find_by_name("texas").len(), 5);
        assert!(from_epsg(1).is_none());
    }

    #[test]
    fn identify_matches_parameters_in_either_unit() {
        let nc = from_epsg(2264).unwrap();
        let mut anon = nc.clone();
        anon.epsg = None;
        assert_eq!(identify(&anon), Some(2264));
        let mut m = from_epsg(32119).unwrap();
        m.epsg = None;
        assert_eq!(identify(&m), Some(32119));
        let mut utm = Crs::utm(17, true, Datum::Nad83).unwrap();
        utm.epsg = None;
        assert_eq!(identify(&utm), Some(26917));
        let mut ak = from_epsg(26931).unwrap();
        ak.epsg = None;
        assert_eq!(identify(&ak), Some(26931));
        // Alabama in feet has no EPSG foot code: identified as nothing
        // rather than as the metre code.
        let mut al = from_epsg(26929).unwrap();
        al.unit = Unit::us_foot();
        al.epsg = None;
        assert_eq!(identify(&al), None);
    }
}
