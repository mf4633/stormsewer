// SPDX-License-Identifier: GPL-3.0-or-later

//! TR-55 worksheet travel-time segments (NRCS Technical Release 55).

use super::tc::tr55_sheet_flow_minutes;

/// TR-55 flow-path segment type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tr55SegmentKind {
    /// Sheet flow (Manning n, slope, length).
    Sheet,
    /// Shallow concentrated flow (paved or unpaved).
    ShallowConcentrated,
    /// Open channel / pipe — Manning velocity from the segment's hydraulic radius.
    Channel,
}

impl Tr55SegmentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sheet => "Sheet flow",
            Self::ShallowConcentrated => "Shallow concentrated",
            Self::Channel => "Channel / pipe",
        }
    }
}

/// One leg of the TR-55 worksheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Tr55Segment {
    pub kind: Tr55SegmentKind,
    pub length_ft: f64,
    pub slope: f64,
    pub n: f64,
    pub paved: bool,
    /// 2-yr 24-hr rainfall (inches) — required for Sheet flow segments per TR-55 Eq. 3-3.
    pub p2_in: f64,
    /// Hydraulic radius (ft) for a Channel segment's Manning velocity (0 → 1 ft).
    pub hydraulic_radius_ft: f64,
}

impl Tr55Segment {
    /// Travel time for this segment (minutes).
    pub fn travel_time_minutes(&self) -> f64 {
        if self.length_ft <= 0.0 || self.slope <= 0.0 {
            return 0.0;
        }
        match self.kind {
            Tr55SegmentKind::Sheet => tr55_sheet_flow_minutes(self.length_ft, self.slope, self.n, self.p2_in),
            Tr55SegmentKind::ShallowConcentrated => {
                let k = if self.paved { 20.33 } else { 16.1346 };
                let v = k * self.slope.sqrt();
                if v <= 0.0 {
                    return 0.0;
                }
                self.length_ft / v / 60.0
            }
            Tr55SegmentKind::Channel => {
                // TR-55 open-channel / pipe: Manning velocity V = (1.49/n)·R^(2/3)·√S,
                // then Tt = L/(V·60). Uses the segment's hydraulic radius.
                let n = if self.n > 0.0 { self.n } else { 0.013 };
                let r = if self.hydraulic_radius_ft > 0.0 {
                    self.hydraulic_radius_ft
                } else {
                    1.0
                };
                let v = (1.49 / n) * r.powf(2.0 / 3.0) * self.slope.sqrt();
                if v <= 0.0 {
                    return 0.0;
                }
                self.length_ft / v / 60.0
            }
        }
    }
}

/// Sum segment travel times (Hydraflow TR-55 worksheet total Tc).
pub fn tr55_worksheet_tc_minutes(segments: &[Tr55Segment]) -> f64 {
    segments.iter().map(|s| s.travel_time_minutes()).sum()
}

/// Format worksheet breakdown for display.
pub fn format_tr55_worksheet(segments: &[Tr55Segment]) -> String {
    let mut s = String::from("=== TR-55 WORKSHEET ===\n\n");
    s.push_str(&format!(
        "{:<22} {:>8} {:>8} {:>10}\n",
        "Segment", "L(ft)", "S", "t(min)"
    ));
    s.push_str(&"-".repeat(52));
    s.push('\n');
    let mut total = 0.0;
    for (i, seg) in segments.iter().enumerate() {
        let t = seg.travel_time_minutes();
        total += t;
        s.push_str(&format!(
            "{:<22} {:>8.0} {:>8.4} {:>10.2}\n",
            format!("{} {}", i + 1, seg.kind.label()),
            seg.length_ft,
            seg.slope,
            t
        ));
    }
    s.push_str(&format!("\nTotal Tc = {total:.2} min\n"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worksheet_sums_segments() {
        let segs = vec![
            Tr55Segment {
                kind: Tr55SegmentKind::Sheet,
                length_ft: 100.0,
                slope: 0.02,
                n: 0.02,
                paved: false,
                p2_in: 3.0,
                hydraulic_radius_ft: 1.0,
            },
            Tr55Segment {
                kind: Tr55SegmentKind::ShallowConcentrated,
                length_ft: 200.0,
                slope: 0.01,
                n: 0.0,
                paved: true,
                p2_in: 3.0,
                hydraulic_radius_ft: 1.0,
            },
        ];
        let total = tr55_worksheet_tc_minutes(&segs);
        assert!(total > 0.0);
        assert!(total > segs[0].travel_time_minutes());
    }

    #[test]
    fn channel_segment_uses_manning_velocity() {
        // V = (1.49/0.035)·1^(2/3)·√0.005 = 3.010 ft/s; Tt = 400/(3.010·60) = 2.215 min.
        let seg = Tr55Segment {
            kind: Tr55SegmentKind::Channel,
            length_ft: 400.0,
            slope: 0.005,
            n: 0.035,
            paved: false,
            p2_in: 3.0,
            hydraulic_radius_ft: 1.0,
        };
        assert!((seg.travel_time_minutes() - 2.215).abs() < 0.02, "t={}", seg.travel_time_minutes());
    }
}