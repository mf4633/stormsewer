// SPDX-License-Identifier: GPL-3.0-or-later

//! Design-storm hyetographs for a SWMM rain gage: the NRCS (SCS) 24-hour
//! Type I, IA, II and III distributions, the NRCS NOAA Atlas 14 regional
//! distributions for the Ohio Valley and neighbouring states, the
//! alternating-block method from an IDF curve or a NOAA Atlas 14
//! depth-duration row, the Chicago method, and a uniform storm — and the
//! one-step command that writes the result as a `[TIMESERIES]` plus a
//! `[RAINGAGES]` row.
//!
//! # Sources (all public domain, U.S. Government works)
//!
//! * **Type I, IA, II, III** cumulative mass curves are the NRCS TR-20
//!   tabular rainfall distributions (Type IA and II: 1982 tables; Type I and
//!   III: 1992 tables, as shipped with TR-20 2/92), at 0.1-hour points
//!   (Type IA: 0.5-hour points). Their derivation and the coarser hourly
//!   Type II ratios are in USDA-NRCS, *National Engineering Handbook Part
//!   630, Chapter 4 — Storm Rainfall Depth and Distribution* (210-630-H,
//!   Amend. 88, Aug 2019), §630.0403 and figure 4-36 (column 6: Type II
//!   hourly ratios 0.011, 0.022, …, 0.235 at 11 h, 0.663 at 12 h, 0.772,
//!   0.820, …, 1.000); the 0.1-hour Type IA and Type II tables are also
//!   reproduced in the WSDOT *Highway Runoff Manual* M 31-16.04, Appendix
//!   4C, tables 4C-3 and 4C-4 (April 2014). The tables here were checked
//!   against both.
//! * **NOAA Atlas 14 regional distributions (regions A–D)** follow NEH 630
//!   Ch. 4 §630.0408, figure 4-72 ("Mean Ratios for Four Rainfall
//!   Distribution Regions, NOAA Atlas 14, Ohio Valley and Neighboring
//!   States"): the ratio of the 5-minute … 12-hour depth to the 24-hour
//!   depth, and the chapter's nesting rule — every shorter duration's depth
//!   is centred inside the longer one (§630.0403 A(9), §630.0407). The
//!   curve is built by that rule with log-linear interpolation between the
//!   tabulated durations; the handbook additionally smooths in WinTR-20.
//! * **Type II nested ratios** (figure 4-31 of the same chapter: 5 min
//!   0.114, 10 min 0.201, 15 min 0.270, 30 min 0.380, 1 h 0.454, 2 h 0.538,
//!   3 h 0.595, 6 h 0.707, 12 h 0.841) are kept as a check on the table.
//! * **Alternating block** and **Chicago** (Keifer & Chu 1957) are
//!   textbook methods (Chow, Maidment & Mays, *Applied Hydrology*, 1988,
//!   §14.4); the alternating-block placement reuses
//!   [`crate::design::alternating_block`] for an IDF curve.
//! * **NOAA Atlas 14** point precipitation frequency estimates are read
//!   from the PFDS "csv" download (rows `5-min:,d1,d2,…` under a header
//!   `by duration for ARI (years):,1,2,5,…`), which is public.
//!
//! Depths are in whatever unit the model's rainfall uses (inches for CFS /
//! GPM / MGD models, millimetres otherwise); nothing here converts.

use crate::doc::build::{self, ObjRef};
use crate::doc::{Command, InpDoc, ObjectKind};

// ---------------------------------------------------------------------------
// NRCS tables
// ---------------------------------------------------------------------------

/// TYPE_I: 241 points, 0.1 h apart.
const TYPE_I: [f64; 241] = [
    0.0000, 0.0017, 0.0035, 0.0052, 0.0070, 0.0087, 0.0105, 0.0122, 0.0139, 0.0157, 0.0174, 0.0192,
    0.0210, 0.0227, 0.0245, 0.0262, 0.0280, 0.0297, 0.0315, 0.0332, 0.0350, 0.0368, 0.0386, 0.0404,
    0.0423, 0.0442, 0.0461, 0.0480, 0.0500, 0.0520, 0.0540, 0.0561, 0.0582, 0.0603, 0.0625, 0.0647,
    0.0669, 0.0691, 0.0714, 0.0737, 0.0760, 0.0784, 0.0807, 0.0831, 0.0855, 0.0878, 0.0902, 0.0926,
    0.0951, 0.0975, 0.1000, 0.1024, 0.1049, 0.1073, 0.1098, 0.1123, 0.1148, 0.1174, 0.1199, 0.1225,
    0.1250, 0.1276, 0.1303, 0.1332, 0.1361, 0.1391, 0.1423, 0.1456, 0.1489, 0.1524, 0.1560, 0.1597,
    0.1633, 0.1671, 0.1708, 0.1746, 0.1784, 0.1823, 0.1861, 0.1901, 0.1940, 0.1982, 0.2027, 0.2077,
    0.2132, 0.2190, 0.2252, 0.2318, 0.2388, 0.2462, 0.2540, 0.2623, 0.2714, 0.2812, 0.2917, 0.3030,
    0.3194, 0.3454, 0.3878, 0.4632, 0.5150, 0.5322, 0.5476, 0.5612, 0.5730, 0.5830, 0.5919, 0.6003,
    0.6083, 0.6159, 0.6230, 0.6298, 0.6365, 0.6430, 0.6493, 0.6555, 0.6615, 0.6674, 0.6731, 0.6786,
    0.6840, 0.6892, 0.6944, 0.6995, 0.7044, 0.7092, 0.7140, 0.7186, 0.7232, 0.7276, 0.7320, 0.7362,
    0.7404, 0.7444, 0.7484, 0.7523, 0.7560, 0.7596, 0.7632, 0.7667, 0.7700, 0.7733, 0.7766, 0.7798,
    0.7830, 0.7862, 0.7894, 0.7926, 0.7958, 0.7989, 0.8020, 0.8051, 0.8082, 0.8112, 0.8142, 0.8173,
    0.8202, 0.8232, 0.8262, 0.8291, 0.8320, 0.8349, 0.8378, 0.8406, 0.8434, 0.8462, 0.8490, 0.8518,
    0.8546, 0.8573, 0.8600, 0.8627, 0.8654, 0.8680, 0.8706, 0.8733, 0.8758, 0.8784, 0.8810, 0.8835,
    0.8860, 0.8885, 0.8910, 0.8934, 0.8958, 0.8982, 0.9006, 0.9030, 0.9054, 0.9077, 0.9100, 0.9123,
    0.9146, 0.9168, 0.9190, 0.9212, 0.9234, 0.9256, 0.9278, 0.9299, 0.9320, 0.9341, 0.9362, 0.9382,
    0.9402, 0.9423, 0.9442, 0.9462, 0.9482, 0.9501, 0.9520, 0.9539, 0.9558, 0.9576, 0.9594, 0.9613,
    0.9630, 0.9648, 0.9666, 0.9683, 0.9700, 0.9717, 0.9734, 0.9750, 0.9766, 0.9783, 0.9798, 0.9814,
    0.9830, 0.9845, 0.9860, 0.9875, 0.9890, 0.9904, 0.9918, 0.9933, 0.9946, 0.9960, 0.9974, 0.9987,
    1.0000,
];

/// TYPE_IA: 49 points, 0.5 h apart.
const TYPE_IA: [f64; 49] = [
    0.0000, 0.0100, 0.0220, 0.0360, 0.0510, 0.0670, 0.0830, 0.0990, 0.1160, 0.1350, 0.1560, 0.1790,
    0.2040, 0.2330, 0.2680, 0.3100, 0.4250, 0.4800, 0.5200, 0.5500, 0.5770, 0.6010, 0.6230, 0.6440,
    0.6640, 0.6830, 0.7010, 0.7190, 0.7360, 0.7530, 0.7690, 0.7850, 0.8000, 0.8150, 0.8300, 0.8440,
    0.8580, 0.8710, 0.8840, 0.8960, 0.9080, 0.9200, 0.9320, 0.9440, 0.9560, 0.9670, 0.9780, 0.9890,
    1.0000,
];

/// TYPE_II: 241 points, 0.1 h apart.
const TYPE_II: [f64; 241] = [
    0.0000, 0.0010, 0.0020, 0.0030, 0.0041, 0.0051, 0.0062, 0.0072, 0.0083, 0.0094, 0.0105, 0.0116,
    0.0127, 0.0138, 0.0150, 0.0161, 0.0173, 0.0184, 0.0196, 0.0208, 0.0220, 0.0232, 0.0244, 0.0257,
    0.0269, 0.0281, 0.0294, 0.0306, 0.0319, 0.0332, 0.0345, 0.0358, 0.0371, 0.0384, 0.0398, 0.0411,
    0.0425, 0.0439, 0.0452, 0.0466, 0.0480, 0.0494, 0.0508, 0.0523, 0.0538, 0.0553, 0.0568, 0.0583,
    0.0598, 0.0614, 0.0630, 0.0646, 0.0662, 0.0679, 0.0696, 0.0712, 0.0730, 0.0747, 0.0764, 0.0782,
    0.0800, 0.0818, 0.0836, 0.0855, 0.0874, 0.0892, 0.0912, 0.0931, 0.0950, 0.0970, 0.0990, 0.1010,
    0.1030, 0.1051, 0.1072, 0.1093, 0.1114, 0.1135, 0.1156, 0.1178, 0.1200, 0.1222, 0.1246, 0.1270,
    0.1296, 0.1322, 0.1350, 0.1379, 0.1408, 0.1438, 0.1470, 0.1502, 0.1534, 0.1566, 0.1598, 0.1630,
    0.1663, 0.1697, 0.1733, 0.1771, 0.1810, 0.1851, 0.1895, 0.1941, 0.1989, 0.2040, 0.2094, 0.2152,
    0.2214, 0.2280, 0.2350, 0.2427, 0.2513, 0.2609, 0.2715, 0.2830, 0.3068, 0.3544, 0.4308, 0.5679,
    0.6630, 0.6820, 0.6986, 0.7130, 0.7252, 0.7350, 0.7434, 0.7514, 0.7588, 0.7656, 0.7720, 0.7780,
    0.7836, 0.7890, 0.7942, 0.7990, 0.8036, 0.8080, 0.8122, 0.8162, 0.8200, 0.8237, 0.8273, 0.8308,
    0.8342, 0.8376, 0.8409, 0.8442, 0.8474, 0.8505, 0.8535, 0.8565, 0.8594, 0.8622, 0.8649, 0.8676,
    0.8702, 0.8728, 0.8753, 0.8777, 0.8800, 0.8823, 0.8845, 0.8868, 0.8890, 0.8912, 0.8934, 0.8955,
    0.8976, 0.8997, 0.9018, 0.9038, 0.9058, 0.9078, 0.9097, 0.9117, 0.9136, 0.9155, 0.9173, 0.9192,
    0.9210, 0.9228, 0.9245, 0.9263, 0.9280, 0.9297, 0.9313, 0.9330, 0.9346, 0.9362, 0.9377, 0.9393,
    0.9408, 0.9423, 0.9438, 0.9452, 0.9466, 0.9480, 0.9493, 0.9507, 0.9520, 0.9533, 0.9546, 0.9559,
    0.9572, 0.9584, 0.9597, 0.9610, 0.9622, 0.9635, 0.9647, 0.9660, 0.9672, 0.9685, 0.9697, 0.9709,
    0.9722, 0.9734, 0.9746, 0.9758, 0.9770, 0.9782, 0.9794, 0.9806, 0.9818, 0.9829, 0.9841, 0.9853,
    0.9864, 0.9876, 0.9887, 0.9899, 0.9910, 0.9922, 0.9933, 0.9944, 0.9956, 0.9967, 0.9978, 0.9989,
    1.0000,
];

/// TYPE_III: 241 points, 0.1 h apart.
const TYPE_III: [f64; 241] = [
    0.0000, 0.0010, 0.0020, 0.0030, 0.0040, 0.0050, 0.0060, 0.0070, 0.0080, 0.0090, 0.0100, 0.0110,
    0.0120, 0.0130, 0.0140, 0.0150, 0.0160, 0.0170, 0.0180, 0.0190, 0.0200, 0.0210, 0.0220, 0.0231,
    0.0241, 0.0252, 0.0263, 0.0274, 0.0285, 0.0296, 0.0308, 0.0319, 0.0331, 0.0343, 0.0355, 0.0367,
    0.0379, 0.0392, 0.0404, 0.0417, 0.0430, 0.0443, 0.0456, 0.0470, 0.0483, 0.0497, 0.0511, 0.0525,
    0.0539, 0.0553, 0.0567, 0.0582, 0.0597, 0.0612, 0.0627, 0.0642, 0.0657, 0.0673, 0.0688, 0.0704,
    0.0720, 0.0736, 0.0753, 0.0770, 0.0788, 0.0806, 0.0825, 0.0844, 0.0864, 0.0884, 0.0905, 0.0926,
    0.0948, 0.0970, 0.0993, 0.1016, 0.1040, 0.1064, 0.1089, 0.1114, 0.1140, 0.1167, 0.1194, 0.1223,
    0.1253, 0.1284, 0.1317, 0.1350, 0.1385, 0.1421, 0.1458, 0.1496, 0.1535, 0.1575, 0.1617, 0.1659,
    0.1703, 0.1748, 0.1794, 0.1842, 0.1890, 0.1940, 0.1993, 0.2048, 0.2105, 0.2165, 0.2227, 0.2292,
    0.2359, 0.2428, 0.2500, 0.2578, 0.2664, 0.2760, 0.2866, 0.2980, 0.3143, 0.3394, 0.3733, 0.4166,
    0.5000, 0.5840, 0.6267, 0.6606, 0.6857, 0.7020, 0.7134, 0.7240, 0.7336, 0.7422, 0.7500, 0.7572,
    0.7641, 0.7708, 0.7773, 0.7835, 0.7895, 0.7952, 0.8007, 0.8060, 0.8110, 0.8158, 0.8206, 0.8252,
    0.8297, 0.8341, 0.8383, 0.8425, 0.8465, 0.8504, 0.8543, 0.8579, 0.8615, 0.8650, 0.8683, 0.8716,
    0.8747, 0.8777, 0.8806, 0.8833, 0.8860, 0.8886, 0.8911, 0.8936, 0.8960, 0.8984, 0.9007, 0.9030,
    0.9052, 0.9074, 0.9095, 0.9116, 0.9136, 0.9156, 0.9175, 0.9194, 0.9212, 0.9230, 0.9247, 0.9264,
    0.9280, 0.9296, 0.9312, 0.9327, 0.9343, 0.9358, 0.9373, 0.9388, 0.9403, 0.9418, 0.9433, 0.9447,
    0.9461, 0.9475, 0.9489, 0.9503, 0.9517, 0.9530, 0.9544, 0.9557, 0.9570, 0.9583, 0.9596, 0.9609,
    0.9621, 0.9634, 0.9646, 0.9658, 0.9670, 0.9682, 0.9694, 0.9706, 0.9718, 0.9729, 0.9741, 0.9752,
    0.9764, 0.9775, 0.9786, 0.9797, 0.9808, 0.9818, 0.9829, 0.9839, 0.9850, 0.9860, 0.9870, 0.9880,
    0.9890, 0.9900, 0.9909, 0.9919, 0.9928, 0.9938, 0.9947, 0.9956, 0.9965, 0.9974, 0.9983, 0.9991,
    1.0000,
];

/// NEH 630 Ch. 4 figure 4-31: Type II ratios of the nested shorter-duration
/// depth to the 24-hour depth (minutes, ratio).
pub const TYPE_II_RATIOS: [(f64, f64); 10] = [
    (5.0, 0.114),
    (10.0, 0.201),
    (15.0, 0.270),
    (30.0, 0.380),
    (60.0, 0.454),
    (120.0, 0.538),
    (180.0, 0.595),
    (360.0, 0.707),
    (720.0, 0.841),
    (1440.0, 1.0),
];

/// NEH 630 Ch. 4 figure 4-72: mean ratios to the 24-hour depth for the
/// four NOAA Atlas 14 (Volume 2, Ohio Valley and neighbouring states)
/// rainfall-distribution regions, at 5, 10, 15, 30, 60, 120, 180, 360 and
/// 720 minutes. Region A is the most intense, D the least.
const REGION_DURATIONS: [f64; 9] = [5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 180.0, 360.0, 720.0];
const REGION_A: [f64; 9] = [
    0.143, 0.219, 0.272, 0.386, 0.502, 0.594, 0.635, 0.749, 0.864,
];
const REGION_B: [f64; 9] = [
    0.121, 0.189, 0.237, 0.344, 0.453, 0.543, 0.585, 0.705, 0.840,
];
const REGION_C: [f64; 9] = [
    0.105, 0.166, 0.210, 0.308, 0.409, 0.500, 0.545, 0.672, 0.823,
];
const REGION_D: [f64; 9] = [
    0.094, 0.149, 0.188, 0.276, 0.366, 0.454, 0.501, 0.636, 0.805,
];

/// The four NRCS 24-hour distributions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScsType {
    I,
    IA,
    II,
    III,
}

impl ScsType {
    pub const ALL: [ScsType; 4] = [ScsType::I, ScsType::IA, ScsType::II, ScsType::III];

    pub fn label(self) -> &'static str {
        match self {
            Self::I => "Type I",
            Self::IA => "Type IA",
            Self::II => "Type II",
            Self::III => "Type III",
        }
    }

    /// The cumulative table and its point spacing in hours.
    pub fn table(self) -> (&'static [f64], f64) {
        match self {
            Self::I => (&TYPE_I, 0.1),
            Self::IA => (&TYPE_IA, 0.5),
            Self::II => (&TYPE_II, 0.1),
            Self::III => (&TYPE_III, 0.1),
        }
    }

    /// The cumulative fraction of the 24-hour depth fallen by `hours`,
    /// linearly interpolated between the table's points.
    pub fn fraction(self, hours: f64) -> f64 {
        let (t, dt) = self.table();
        interp_uniform(t, dt, hours)
    }
}

/// The NOAA Atlas 14 regional distributions of NEH 630 Ch. 4 §630.0408.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoaaRegion {
    A,
    B,
    C,
    D,
}

impl NoaaRegion {
    pub const ALL: [NoaaRegion; 4] = [NoaaRegion::A, NoaaRegion::B, NoaaRegion::C, NoaaRegion::D];

    pub fn label(self) -> &'static str {
        match self {
            Self::A => "Region A (most intense)",
            Self::B => "Region B",
            Self::C => "Region C",
            Self::D => "Region D (least intense)",
        }
    }

    /// `(minutes, ratio to the 24-hour depth)`, the 24-hour point included.
    pub fn ratios(self) -> Vec<(f64, f64)> {
        let r = match self {
            Self::A => &REGION_A,
            Self::B => &REGION_B,
            Self::C => &REGION_C,
            Self::D => &REGION_D,
        };
        let mut out: Vec<(f64, f64)> = REGION_DURATIONS
            .iter()
            .copied()
            .zip(r.iter().copied())
            .collect();
        out.push((1440.0, 1.0));
        out
    }
}

/// Linear interpolation in a table of values `dt` apart starting at 0.
fn interp_uniform(t: &[f64], dt: f64, x: f64) -> f64 {
    if t.is_empty() {
        return 0.0;
    }
    let last = (t.len() - 1) as f64 * dt;
    if x <= 0.0 {
        return t[0];
    }
    if x >= last {
        return t[t.len() - 1];
    }
    let f = x / dt;
    let i = f.floor() as usize;
    let a = t[i];
    let b = t[(i + 1).min(t.len() - 1)];
    a + (b - a) * (f - i as f64)
}

/// Log-linear interpolation of `(x, y)` pairs sorted by `x`, linear through
/// the origin below the first point, clamped above the last.
pub fn interp_loglog(pts: &[(f64, f64)], x: f64) -> f64 {
    let Some(first) = pts.first() else {
        return 0.0;
    };
    if x <= 0.0 {
        return 0.0;
    }
    if x <= first.0 {
        return first.1 * x / first.0;
    }
    let last = pts[pts.len() - 1];
    if x >= last.0 {
        return last.1;
    }
    for w in pts.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x >= x0 && x <= x1 {
            if y0 <= 0.0 || y1 <= 0.0 || x0 <= 0.0 {
                return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
            }
            let f = (x.ln() - x0.ln()) / (x1.ln() - x0.ln());
            return (y0.ln() + (y1.ln() - y0.ln()) * f).exp();
        }
    }
    last.1
}

/// The cumulative fraction at `minutes` of a 24-hour storm built by the
/// NRCS nesting rule from `ratios` (`(minutes, ratio to 24-hour depth)`):
/// the depth of every duration `d` sits centred on hour 12, so the curve
/// is `0.5 ± r(d)/2` at `12 h ± d/2`.
pub fn nested_fraction(ratios: &[(f64, f64)], minutes: f64) -> f64 {
    let t = minutes.clamp(0.0, 1440.0);
    let d = 2.0 * (t - 720.0).abs();
    let r = interp_loglog(ratios, d).min(1.0);
    if t < 720.0 {
        0.5 - r / 2.0
    } else {
        0.5 + r / 2.0
    }
}

// ---------------------------------------------------------------------------
// NOAA Atlas 14 PFDS csv
// ---------------------------------------------------------------------------

/// A parsed NOAA Atlas 14 point-precipitation-frequency table: the return
/// periods (ARI, years) and, per duration, the depth for each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pfds {
    pub aris: Vec<f64>,
    /// `(minutes, depths by ARI)`.
    pub rows: Vec<(f64, Vec<f64>)>,
    /// The unit line, if the file named one ("inches", "mm").
    pub units: Option<String>,
}

impl Pfds {
    /// The depth-duration column for one return period, or the nearest.
    pub fn column(&self, ari: f64) -> Option<(f64, Vec<(f64, f64)>)> {
        let (i, &picked) = self
            .aris
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - ari).abs().total_cmp(&(b.1 - ari).abs()))?;
        let col: Vec<(f64, f64)> = self
            .rows
            .iter()
            .filter_map(|(m, d)| d.get(i).map(|v| (*m, *v)))
            .collect();
        Some((picked, col))
    }
}

/// `5-min`, `60-min`, `2-hr`, `24-hr`, `2-day`, `10-day` … as minutes.
pub fn parse_duration_label(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches(':').trim().to_ascii_lowercase();
    let (num, unit) = s.split_once('-').or_else(|| {
        let i = s.find(|c: char| c.is_ascii_alphabetic())?;
        Some((&s[..i], &s[i..]))
    })?;
    let n: f64 = num.trim().parse().ok()?;
    let unit = unit.trim();
    let factor = if unit.starts_with("min") {
        1.0
    } else if unit.starts_with('h') {
        60.0
    } else if unit.starts_with("day") || unit == "d" {
        1440.0
    } else {
        return None;
    };
    Some(n * factor)
}

/// Read the PFDS csv text (the whole download, or just the estimates
/// block). Rows are `<duration>:,v1,v2,…`; the ARI header is the line
/// containing `ARI`. A block with no ARI header but numeric rows is read
/// as one column of ARI 0.
pub fn parse_pfds(text: &str) -> Result<Pfds, String> {
    let mut out = Pfds::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("precipitation") && lower.contains("estimates") && lower.contains('(') {
            if let (Some(a), Some(b)) = (line.find('('), line.find(')')) {
                if b > a {
                    out.units = Some(line[a + 1..b].to_string());
                }
            }
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if lower.contains("ari") {
            out.aris = cells[1..].iter().filter_map(|c| c.parse().ok()).collect();
            continue;
        }
        let Some(minutes) = parse_duration_label(cells[0]) else {
            continue;
        };
        let depths: Vec<f64> = cells[1..].iter().filter_map(|c| c.parse().ok()).collect();
        if depths.is_empty() {
            continue;
        }
        out.rows.push((minutes, depths));
    }
    if out.rows.is_empty() {
        return Err(
            "no `<duration>:,depth,…` rows found (paste the NOAA Atlas 14 PFDS csv)".into(),
        );
    }
    if out.aris.is_empty() {
        out.aris = vec![0.0];
    }
    out.rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Storm construction
// ---------------------------------------------------------------------------

/// How the series values are expressed, matching `[RAINGAGES] Format`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Average intensity over the interval (depth per hour).
    Intensity,
    /// Depth fallen in the interval.
    Volume,
}

impl Format {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Intensity => "INTENSITY",
            Self::Volume => "VOLUME",
        }
    }
}

/// A design-storm method with its inputs.
#[derive(Clone, Debug, PartialEq)]
pub enum Method {
    /// NRCS 24-hour distribution scaled to `depth`.
    Scs { kind: ScsType, depth: f64 },
    /// NRCS NOAA Atlas 14 regional 24-hour distribution scaled to `depth`.
    NoaaRegion { region: NoaaRegion, depth: f64 },
    /// Alternating block from an IDF curve `i = a / (t + b)^c` (t in
    /// minutes, i in depth per hour).
    AlternatingBlockIdf { a: f64, b: f64, c: f64 },
    /// Alternating block from a depth-duration table `(minutes, depth)`.
    AlternatingBlockDepths { depths: Vec<(f64, f64)> },
    /// Chicago (Keifer–Chu) storm from the same IDF form, the peak at
    /// fraction `r` of the duration.
    Chicago { a: f64, b: f64, c: f64, r: f64 },
    /// `depth` spread evenly over the duration.
    Uniform { depth: f64 },
}

/// What to build.
#[derive(Clone, Debug, PartialEq)]
pub struct StormSpec {
    pub method: Method,
    /// Ignored by the 24-hour NRCS methods.
    pub duration_min: u32,
    pub step_min: u32,
    pub format: Format,
}

/// A built hyetograph: one value per interval, plus the cumulative curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Storm {
    pub step_min: u32,
    pub duration_min: u32,
    pub format: Format,
    /// Depth fallen in each interval.
    pub increments: Vec<f64>,
}

impl Storm {
    pub fn total_depth(&self) -> f64 {
        self.increments.iter().sum()
    }

    pub fn peak_intensity(&self) -> f64 {
        let hours = self.step_min as f64 / 60.0;
        self.increments.iter().copied().fold(0.0, f64::max) / hours
    }

    /// The series values in the storm's format.
    pub fn values(&self) -> Vec<f64> {
        let hours = self.step_min as f64 / 60.0;
        self.increments
            .iter()
            .map(|d| match self.format {
                Format::Intensity => d / hours,
                Format::Volume => *d,
            })
            .collect()
    }

    /// `(minutes, cumulative depth)` at every interval end, starting at 0.
    pub fn cumulative(&self) -> Vec<(f64, f64)> {
        let mut out = vec![(0.0, 0.0)];
        let mut sum = 0.0;
        for (k, d) in self.increments.iter().enumerate() {
            sum += d;
            out.push((((k + 1) * self.step_min as usize) as f64, sum));
        }
        out
    }

    /// The `[TIMESERIES]` rows: `name h:mm value` per interval start, then
    /// a closing zero at the end so the rain stops.
    pub fn rows(&self, name: &str) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = self
            .values()
            .iter()
            .enumerate()
            .map(|(k, v)| {
                vec![
                    name.to_string(),
                    hhmm(k as u32 * self.step_min),
                    format_value(*v),
                ]
            })
            .collect();
        rows.push(vec![
            name.to_string(),
            hhmm(self.increments.len() as u32 * self.step_min),
            "0".into(),
        ]);
        rows
    }
}

fn hhmm(minutes: u32) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn format_value(v: f64) -> String {
    if v.abs() < 1e-12 {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Depth increments per block placed by the alternating-block rule: the
/// largest block at the centre, the rest alternately after and before it.
/// Mirrors [`crate::design::alternating_block`]'s placement so both agree.
pub fn alternate(mut blocks: Vec<f64>) -> Vec<f64> {
    let n = blocks.len();
    if n == 0 {
        return blocks;
    }
    blocks.sort_by(|a, b| b.total_cmp(a));
    let centre = n / 2;
    let mut order = vec![centre];
    let (mut l, mut r) = (centre, centre);
    while order.len() < n {
        if r + 1 < n {
            r += 1;
            order.push(r);
        }
        if order.len() < n && l > 0 {
            l -= 1;
            order.push(l);
        }
    }
    let mut placed = vec![0.0; n];
    for (pos, d) in order.into_iter().zip(blocks) {
        placed[pos] = d;
    }
    placed
}

/// Chicago cumulative depth at `t` minutes for an IDF `a, b, c`, duration
/// `td` minutes, peak at `r · td`.
fn chicago_cumulative(a: f64, b: f64, c: f64, r: f64, td: f64, t: f64) -> f64 {
    // `a` is in depth per hour with `t` in minutes, so depth = i · t / 60.
    let k = a / 60.0;
    let total = k * td / (td + b).powf(c);
    let tp = r * td;
    let before = |tb: f64| k * tb / (tb / r + b).powf(c);
    let after = |ta: f64| k * ta / (ta / (1.0 - r) + b).powf(c);
    if t <= 0.0 {
        0.0
    } else if t >= td {
        total
    } else if t < tp {
        r * total - before(tp - t)
    } else {
        r * total + after(t - tp)
    }
}

/// Build the hyetograph. Errors name the bad input.
pub fn build(spec: &StormSpec) -> Result<Storm, String> {
    let step = spec.step_min;
    if step == 0 {
        return Err("time step must be at least 1 minute".into());
    }
    let is_24h = matches!(spec.method, Method::Scs { .. } | Method::NoaaRegion { .. });
    let duration = if is_24h { 1440 } else { spec.duration_min };
    if duration == 0 {
        return Err("duration must be at least one time step".into());
    }
    let n = duration.div_ceil(step) as usize;
    let step_f = step as f64;
    let increments: Vec<f64> = match &spec.method {
        Method::Scs { kind, depth } => {
            check_depth(*depth)?;
            (0..n)
                .map(|k| {
                    let t0 = k as f64 * step_f / 60.0;
                    let t1 = ((k + 1) as f64 * step_f / 60.0).min(24.0);
                    depth * (kind.fraction(t1) - kind.fraction(t0))
                })
                .collect()
        }
        Method::NoaaRegion { region, depth } => {
            check_depth(*depth)?;
            let ratios = region.ratios();
            (0..n)
                .map(|k| {
                    let t0 = k as f64 * step_f;
                    let t1 = ((k + 1) as f64 * step_f).min(1440.0);
                    depth * (nested_fraction(&ratios, t1) - nested_fraction(&ratios, t0))
                })
                .collect()
        }
        Method::AlternatingBlockIdf { a, b, c } => {
            check_idf(*a, *b, *c)?;
            let idf = stormsewer::idf::IdfCurve::new(*a, *b, *c);
            let blocks = crate::design::alternating_block(&idf, duration, step);
            blocks.iter().map(|(_, i)| i * step_f / 60.0).collect()
        }
        Method::AlternatingBlockDepths { depths } => {
            if depths.len() < 2 {
                return Err("a depth-duration table needs at least two durations".into());
            }
            let mut table = depths.clone();
            table.sort_by(|a, b| a.0.total_cmp(&b.0));
            let cum = |t: f64| interp_loglog(&table, t);
            let blocks: Vec<f64> = (1..=n)
                .map(|k| (cum(k as f64 * step_f) - cum((k - 1) as f64 * step_f)).max(0.0))
                .collect();
            alternate(blocks)
        }
        Method::Chicago { a, b, c, r } => {
            check_idf(*a, *b, *c)?;
            if !(*r > 0.0 && *r < 1.0) {
                return Err("the Chicago peak position r must be between 0 and 1".into());
            }
            let td = duration as f64;
            (0..n)
                .map(|k| {
                    let t0 = k as f64 * step_f;
                    let t1 = ((k + 1) as f64 * step_f).min(td);
                    chicago_cumulative(*a, *b, *c, *r, td, t1)
                        - chicago_cumulative(*a, *b, *c, *r, td, t0)
                })
                .collect()
        }
        Method::Uniform { depth } => {
            check_depth(*depth)?;
            let per = depth / duration as f64;
            (0..n)
                .map(|k| {
                    let t0 = k as f64 * step_f;
                    let t1 = ((k + 1) as f64 * step_f).min(duration as f64);
                    per * (t1 - t0)
                })
                .collect()
        }
    };
    Ok(Storm {
        step_min: step,
        duration_min: duration,
        format: spec.format,
        increments,
    })
}

fn check_depth(d: f64) -> Result<(), String> {
    if d.is_finite() && d > 0.0 {
        Ok(())
    } else {
        Err("total depth must be a positive number".into())
    }
}

fn check_idf(a: f64, b: f64, c: f64) -> Result<(), String> {
    if !(a.is_finite() && a > 0.0) {
        return Err("IDF coefficient a must be positive".into());
    }
    if !(b.is_finite() && b >= 0.0) {
        return Err("IDF coefficient b must be zero or more".into());
    }
    if !(c.is_finite() && c > 0.0 && c < 1.0) {
        return Err("IDF exponent c must be between 0 and 1".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Writing the model
// ---------------------------------------------------------------------------

/// What the batch created.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub gage: String,
    pub series: String,
    pub command: Command,
}

/// The one-step batch that adds the storm as a `[TIMESERIES]`, a
/// `[RAINGAGES]` row reading it (with a `[SYMBOLS]` position when given),
/// and points each subcatchment in `assign` at the gage. `series_base` and
/// `gage_base` are made unique against the document.
pub fn command(
    doc: &InpDoc,
    storm: &Storm,
    series_base: &str,
    gage_base: &str,
    symbol: Option<(f64, f64)>,
    assign: &[String],
) -> Written {
    let series = build::unique_name_from(doc, ObjectKind::Timeseries, series_base.trim(), &[]);
    let gage = build::unique_name_from(doc, ObjectKind::Gage, gage_base.trim(), &[]);
    let mut cmds: Vec<Command> = storm
        .rows(&series)
        .into_iter()
        .map(|fields| Command::AddRow {
            section: "TIMESERIES".into(),
            fields,
            comment: None,
        })
        .collect();
    cmds.push(Command::AddRow {
        section: "RAINGAGES".into(),
        fields: vec![
            gage.clone(),
            storm.format.keyword().into(),
            hhmm(storm.step_min),
            "1.0".into(),
            "TIMESERIES".into(),
            series.clone(),
        ],
        comment: None,
    });
    if let Some((x, y)) = symbol {
        cmds.push(Command::MoveGage {
            name: gage.clone(),
            x,
            y,
        });
    }
    for sub in assign {
        if doc.contains("SUBCATCHMENTS", sub) {
            cmds.push(Command::SetField {
                section: "SUBCATCHMENTS".into(),
                name: sub.clone(),
                field: "RainGage".into(),
                value: gage.clone(),
            });
        }
    }
    Written {
        gage,
        series,
        command: Command::Batch(cmds),
    }
}

/// The map object the written gage is, for selecting it.
pub fn gage_ref(written: &Written) -> ObjRef {
    ObjRef::Gage(written.gage.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(method: Method, duration: u32, step: u32, format: Format) -> StormSpec {
        StormSpec {
            method,
            duration_min: duration,
            step_min: step,
            format,
        }
    }

    #[test]
    fn tables_are_monotone_and_end_at_one() {
        for kind in ScsType::ALL {
            let (t, dt) = kind.table();
            assert_eq!(t[0], 0.0);
            assert!((t[t.len() - 1] - 1.0).abs() < 1e-9);
            assert!(t.windows(2).all(|w| w[1] >= w[0]), "{kind:?}");
            assert!(((t.len() - 1) as f64 * dt - 24.0).abs() < 1e-9, "{kind:?}");
        }
        // Spot checks against NEH 630 Ch. 4 figure 4-36 (Type II hourly) and
        // the half-hour Type IA / Type I / Type III points.
        assert!((ScsType::II.fraction(11.0) - 0.235).abs() < 1e-9);
        assert!((ScsType::II.fraction(12.0) - 0.663).abs() < 1e-9);
        assert!((ScsType::II.fraction(13.0) - 0.772).abs() < 1e-9);
        assert!((ScsType::II.fraction(16.0) - 0.880).abs() < 1e-9);
        assert!((ScsType::II.fraction(20.0) - 0.952).abs() < 1e-9);
        assert!((ScsType::IA.fraction(8.0) - 0.425).abs() < 1e-9);
        assert!((ScsType::I.fraction(10.0) - 0.515).abs() < 1e-9);
        assert!((ScsType::III.fraction(12.0) - 0.500).abs() < 1e-9);
        // The one-hour Type II ratio of figure 4-31 (0.454) is the nested
        // 11.5–12.5 h depth, within the table's rounding.
        let one_hour = ScsType::II.fraction(12.5) - ScsType::II.fraction(11.5);
        assert!((one_hour - 0.454).abs() < 0.003, "{one_hour}");
    }

    #[test]
    fn type_ii_six_inch_storm_matches_the_table_at_every_tabulated_point() {
        let storm = build(&spec(
            Method::Scs {
                kind: ScsType::II,
                depth: 6.0,
            },
            1440,
            30,
            Format::Volume,
        ))
        .unwrap();
        assert_eq!(storm.increments.len(), 48);
        assert!((storm.total_depth() - 6.0).abs() < 1e-9);
        let (table, dt) = ScsType::II.table();
        for (minutes, cum) in storm.cumulative() {
            let i = (minutes / 60.0 / dt).round() as usize;
            assert!((cum - 6.0 * table[i]).abs() < 1e-9, "{minutes} min: {cum}");
        }
        // Intensity format holds the same storm.
        let storm_i = build(&spec(
            Method::Scs {
                kind: ScsType::II,
                depth: 6.0,
            },
            1440,
            30,
            Format::Intensity,
        ))
        .unwrap();
        let v = storm_i.values();
        assert!((v[23] - storm.increments[23] * 2.0).abs() < 1e-9);
        // The peak half-hour of a Type II storm is at 11.5–12.0 h.
        let peak = storm
            .increments
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, 23);
        // Every table point is hit by a 6-minute step, too.
        let fine = build(&spec(
            Method::Scs {
                kind: ScsType::II,
                depth: 6.0,
            },
            1440,
            6,
            Format::Volume,
        ))
        .unwrap();
        for (k, cum) in fine.cumulative().iter().skip(1).enumerate() {
            assert!((cum.1 - 6.0 * table[k + 1]).abs() < 1e-9);
        }
    }

    #[test]
    fn alternating_block_total_equals_the_input_depth() {
        let depths = vec![
            (5.0, 0.50),
            (15.0, 1.00),
            (60.0, 2.00),
            (120.0, 2.50),
            (360.0, 3.30),
            (1440.0, 4.40),
        ];
        let storm = build(&spec(
            Method::AlternatingBlockDepths {
                depths: depths.clone(),
            },
            1440,
            15,
            Format::Volume,
        ))
        .unwrap();
        assert!(
            (storm.total_depth() - 4.40).abs() < 1e-9,
            "{}",
            storm.total_depth()
        );
        // Centre-peaked, blocks fall away on both sides.
        let inc = &storm.increments;
        let peak = inc
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, inc.len() / 2);
        assert!(inc[peak] >= inc[peak + 1] && inc[peak + 1] >= inc[peak + 2]);
        assert!(inc[peak] >= inc[peak - 1] && inc[peak - 1] >= inc[peak - 2]);
        // A shorter storm holds the depth of that duration.
        let two_hour = build(&spec(
            Method::AlternatingBlockDepths { depths },
            120,
            5,
            Format::Intensity,
        ))
        .unwrap();
        assert!((two_hour.total_depth() - 2.50).abs() < 1e-9);
        assert_eq!(two_hour.values().len(), 24);
        // IDF path: the total is the curve's depth at the duration and the
        // placement agrees with the design module's.
        let (a, b, c) = (60.0, 8.0, 0.75);
        let storm = build(&spec(
            Method::AlternatingBlockIdf { a, b, c },
            120,
            5,
            Format::Intensity,
        ))
        .unwrap();
        let expect = a * 120.0 / (120.0_f64 + b).powf(c) / 60.0;
        assert!((storm.total_depth() - expect).abs() < 1e-9);
        let cum = |t: f64| a * t / (t + b).powf(c) / 60.0;
        let blocks: Vec<f64> = (1..=24)
            .map(|k| cum(k as f64 * 5.0) - cum((k - 1) as f64 * 5.0))
            .collect();
        let mine = alternate(blocks);
        for (x, y) in mine.iter().zip(&storm.increments) {
            assert!((x - y).abs() < 1e-9);
        }
    }

    #[test]
    fn chicago_uniform_and_regional_storms_conserve_depth() {
        let (a, b, c) = (60.0, 8.0, 0.75);
        let storm = build(&spec(
            Method::Chicago { a, b, c, r: 0.4 },
            180,
            5,
            Format::Intensity,
        ))
        .unwrap();
        let expect = a * 180.0 / (180.0_f64 + b).powf(c) / 60.0;
        assert!((storm.total_depth() - expect).abs() < 1e-9);
        assert!(storm.increments.iter().all(|d| *d >= -1e-12));
        let peak = storm
            .increments
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .unwrap()
            .0;
        // The peak block straddles 0.4 × 180 = 72 min (blocks 13 and 14).
        assert!((13..=14).contains(&peak), "{peak}");
        // Any 5-minute block's depth cannot exceed the 5-minute IDF depth.
        let d5 = a * 5.0 / (5.0_f64 + b).powf(c) / 60.0;
        assert!(storm.increments.iter().all(|d| *d <= d5 + 1e-9));

        let uniform = build(&spec(
            Method::Uniform { depth: 2.4 },
            60,
            10,
            Format::Volume,
        ))
        .unwrap();
        assert_eq!(uniform.increments.len(), 6);
        assert!(uniform.increments.iter().all(|d| (d - 0.4).abs() < 1e-12));

        for region in NoaaRegion::ALL {
            let storm = build(&spec(
                Method::NoaaRegion { region, depth: 5.0 },
                1440,
                15,
                Format::Volume,
            ))
            .unwrap();
            assert!((storm.total_depth() - 5.0).abs() < 1e-9);
            assert!(storm.increments.iter().all(|d| *d >= -1e-12));
            // The nested one-hour depth is the tabulated ratio.
            let ratios = region.ratios();
            let one_hour = nested_fraction(&ratios, 750.0) - nested_fraction(&ratios, 690.0);
            let expect = ratios.iter().find(|(m, _)| *m == 60.0).unwrap().1;
            assert!((one_hour - expect).abs() < 1e-9);
        }
        // The Type II ratios nested the same way land near the table.
        let f = |m: f64| nested_fraction(&TYPE_II_RATIOS, m);
        assert!((f(750.0) - f(690.0) - 0.454).abs() < 1e-9);
        assert!((f(720.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn pfds_csv_row_parses_and_picks_a_return_period() {
        let text = "Point precipitation frequency estimates (inches)\nNOAA Atlas 14 Volume 2 Version 3\n\nPRECIPITATION FREQUENCY ESTIMATES\nby duration for ARI (years):, 1,2,5,10,25,50,100,200,500,1000\n5-min:,0.384,0.456,0.529,0.588,0.664,0.720,0.774,0.826,0.892,0.940\n10-min:,0.614,0.729,0.846,0.940,1.06,1.15,1.24,1.32,1.43,1.50\n60-min:,1.23,1.48,1.79,2.03,2.35,2.60,2.85,3.10,3.43,3.68\n2-hr:,1.47,1.78,2.17,2.48,2.90,3.23,3.57,3.91,4.38,4.73\n24-hr:,2.85,3.44,4.28,4.98,6.02,6.87,7.78,8.76,10.2,11.3\n2-day:,3.30,3.98,4.94,5.74,6.91,7.86,8.89,9.99,11.6,12.9\n";
        let p = parse_pfds(text).unwrap();
        assert_eq!(p.units.as_deref(), Some("inches"));
        assert_eq!(p.aris.len(), 10);
        assert_eq!(p.rows.len(), 6);
        let (ari, col) = p.column(10.0).unwrap();
        assert_eq!(ari, 10.0);
        assert_eq!(col[0], (5.0, 0.588));
        assert_eq!(col[4], (1440.0, 4.98));
        assert_eq!(col[5], (2880.0, 5.74));
        assert_eq!(parse_duration_label("3-hr:"), Some(180.0));
        assert_eq!(parse_duration_label("45-day"), Some(64800.0));
        assert_eq!(parse_duration_label("nonsense"), None);
        let storm = build(&spec(
            Method::AlternatingBlockDepths { depths: col },
            1440,
            60,
            Format::Volume,
        ))
        .unwrap();
        assert!((storm.total_depth() - 4.98).abs() < 1e-9);
        assert!(parse_pfds("nothing here").is_err());
    }

    #[test]
    fn command_writes_series_gage_symbol_and_assignment_as_one_step() {
        let mut doc = InpDoc::parse(
            "[JUNCTIONS]\nJ1 0 0\n[RAINGAGES]\nRG1 INTENSITY 0:15 1.0 TIMESERIES TS1\n[SUBCATCHMENTS]\nS1 RG1 J1 5 25 500 0.5 0\nS2 RG1 J1 5 25 500 0.5 0\n[TIMESERIES]\nTS1 0:00 1\n",
        );
        let storm = build(&spec(
            Method::Uniform { depth: 1.0 },
            30,
            15,
            Format::Intensity,
        ))
        .unwrap();
        let w = command(
            &doc,
            &storm,
            "TS1",
            "RG1",
            Some((10.0, 20.0)),
            &["S2".into(), "NOPE".into()],
        );
        assert_eq!(w.series, "TS2");
        assert_eq!(w.gage, "RG2");
        doc.apply(w.command).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        let pts = doc.timeseries("TS2");
        assert_eq!(pts.len(), 3, "{pts:?}");
        assert_eq!(pts[0].time, "0:00");
        assert_eq!(pts[0].value, "2");
        assert_eq!(pts[2].time, "0:30");
        assert_eq!(pts[2].value, "0");
        assert_eq!(doc.field("RAINGAGES", "RG2", "Format"), Some("INTENSITY"));
        assert_eq!(doc.field("RAINGAGES", "RG2", "Interval"), Some("0:15"));
        assert_eq!(doc.field("RAINGAGES", "RG2", "Series"), Some("TS2"));
        assert_eq!(doc.symbol("RG2"), Some((10.0, 20.0)));
        assert_eq!(doc.field("SUBCATCHMENTS", "S2", "RainGage"), Some("RG2"));
        assert_eq!(doc.field("SUBCATCHMENTS", "S1", "RainGage"), Some("RG1"));
        assert!(doc
            .validate()
            .iter()
            .all(|f| f.severity != crate::doc::Severity::Error));
        assert!(doc.undo());
        assert!(!doc.contains("RAINGAGES", "RG2"));
    }

    /// Opt-in: the pond fixture still runs after a storm is added. Set
    /// STORMSEWER_SWMM_ENGINE_DIR (as the engine tests do).
    #[test]
    fn fixture_runs_with_a_generated_storm_when_an_engine_is_available() {
        if std::env::var("STORMSEWER_SWMM_ENGINE_DIR").is_err() {
            eprintln!("skipped: STORMSEWER_SWMM_ENGINE_DIR not set");
            return;
        }
        let registry = crate::engine::Registry::discover();
        let Some(engine) = registry.default_engine() else {
            eprintln!("skipped: no engine discovered");
            return;
        };
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/epa-samples/Detention_Pond_Model.inp");
        let mut doc = InpDoc::read(&path).unwrap();
        let storm = build(&spec(
            Method::Scs {
                kind: ScsType::II,
                depth: 3.0,
            },
            1440,
            5,
            Format::Intensity,
        ))
        .unwrap();
        let subs = doc.names("SUBCATCHMENTS");
        let w = command(&doc, &storm, "SCS_II_3in", "RG_storm", None, &subs);
        doc.apply(w.command).unwrap();
        // The run must cover the 24-hour storm.
        doc.apply(Command::SetOption {
            section: "OPTIONS".into(),
            key: "END_DATE".into(),
            value: "01/02/2007".into(),
        })
        .unwrap();
        let dir = std::env::temp_dir()
            .join("stormsewer-swmm-tests")
            .join("storm");
        std::fs::create_dir_all(&dir).unwrap();
        let inp = dir.join("pond_with_storm.inp");
        doc.write(&inp).unwrap();
        let run = engine.run(&inp).unwrap();
        assert!(run.succeeded(), "{:?}", run.failure_reason());
        let results = crate::out::OutputFile::open(&run.out).unwrap();
        assert!(results.meta.n_periods > 0);
    }
}
