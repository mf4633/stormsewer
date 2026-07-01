// SPDX-License-Identifier: GPL-3.0-only

//! Unit system conversion (Hydraflow Options → Units parity).

use serde::{Deserialize, Serialize};

use crate::io::project::Project;

const FT_TO_M: f64 = 0.3048;
const M_TO_FT: f64 = 1.0 / FT_TO_M;
const AC_TO_HA: f64 = 0.404686;
const HA_TO_AC: f64 = 1.0 / AC_TO_HA;
const IN_HR_TO_MM_HR: f64 = 25.4;
const MM_HR_TO_IN_HR: f64 = 1.0 / IN_HR_TO_MM_HR;

/// Standard metric RCP diameters (mm).
pub const METRIC_PIPE_MM: &[u32] =
    &[300, 375, 450, 525, 600, 675, 750, 900, 1050, 1200, 1350, 1500, 1800];

/// Project unit system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    #[default]
    UsCustomary,
    Si,
}

impl UnitSystem {
    pub fn label(self) -> &'static str {
        match self {
            Self::UsCustomary => "U.S. Customary",
            Self::Si => "SI (metric)",
        }
    }

    /// Convert a length value to engine feet.
    pub fn length_to_engine_ft(self, v: f64) -> f64 {
        match self {
            Self::UsCustomary => v,
            Self::Si => v * M_TO_FT,
        }
    }

    /// Convert a tributary area value to engine acres.
    pub fn area_to_engine_ac(self, v: f64) -> f64 {
        match self {
            Self::UsCustomary => v,
            Self::Si => v * HA_TO_AC,
        }
    }

    /// Convert IDF coefficient `a` to engine units (in/hr).
    pub fn idf_a_to_engine(self, a: f64) -> f64 {
        match self {
            Self::UsCustomary => a,
            Self::Si => a * MM_HR_TO_IN_HR,
        }
    }
}

/// Nearest metric catalog diameter (mm) for a pipe size in meters.
pub fn nearest_metric_diameter_mm(diameter_m: f64) -> u32 {
    let mm = (diameter_m * 1000.0).round() as u32;
    METRIC_PIPE_MM
        .iter()
        .min_by_key(|&&d| (d as i64 - mm as i64).unsigned_abs())
        .copied()
        .unwrap_or(mm)
}

/// Convert all project values between unit systems.
pub fn convert_project(project: &mut Project, to: UnitSystem) {
    if project.units == to {
        return;
    }

    match (project.units, to) {
        (UnitSystem::UsCustomary, UnitSystem::Si) => project_to_si(project),
        (UnitSystem::Si, UnitSystem::UsCustomary) => project_to_us(project),
        _ => {}
    }
    project.units = to;
}

fn project_to_si(p: &mut Project) {
    p.idf_a *= IN_HR_TO_MM_HR;
    if let Some(ref mut tw) = p.tailwater {
        *tw *= FT_TO_M;
    }
    for n in &mut p.nodes {
        n.x *= FT_TO_M;
        n.y *= FT_TO_M;
        n.invert *= FT_TO_M;
        n.rim *= FT_TO_M;
        n.area_ac *= AC_TO_HA;
    }
    for pipe in &mut p.pipes {
        pipe.length *= FT_TO_M;
        let dia_m = pipe.diameter * FT_TO_M;
        let mm = nearest_metric_diameter_mm(dia_m);
        pipe.diameter = mm as f64 / 1000.0;
        pipe.rise_ft *= FT_TO_M;
        pipe.span_ft *= FT_TO_M;
    }
    for c in &mut p.catchments {
        for v in &mut c.vertices {
            v.0 *= FT_TO_M;
            v.1 *= FT_TO_M;
        }
        c.flow_length_ft *= FT_TO_M;
    }
    if let Some(ref mut bg) = p.background {
        bg.origin_x *= FT_TO_M;
        bg.origin_y *= FT_TO_M;
        bg.width *= FT_TO_M;
    }
    if let Some(ref mut dxf) = p.background_dxf {
        dxf.min_x *= FT_TO_M;
        dxf.min_y *= FT_TO_M;
        dxf.max_x *= FT_TO_M;
        dxf.max_y *= FT_TO_M;
    }
    for curve in &mut p.idf_curves {
        curve.a *= IN_HR_TO_MM_HR;
    }
}

fn project_to_us(p: &mut Project) {
    p.idf_a *= MM_HR_TO_IN_HR;
    if let Some(ref mut tw) = p.tailwater {
        *tw *= M_TO_FT;
    }
    for n in &mut p.nodes {
        n.x *= M_TO_FT;
        n.y *= M_TO_FT;
        n.invert *= M_TO_FT;
        n.rim *= M_TO_FT;
        n.area_ac *= HA_TO_AC;
    }
    for pipe in &mut p.pipes {
        pipe.length *= M_TO_FT;
        let dia_in = (pipe.diameter * M_TO_FT * 12.0).round();
        pipe.diameter = dia_in / 12.0;
        pipe.rise_ft *= M_TO_FT;
        pipe.span_ft *= M_TO_FT;
    }
    for c in &mut p.catchments {
        for v in &mut c.vertices {
            v.0 *= M_TO_FT;
            v.1 *= M_TO_FT;
        }
        c.flow_length_ft *= M_TO_FT;
    }
    if let Some(ref mut bg) = p.background {
        bg.origin_x *= M_TO_FT;
        bg.origin_y *= M_TO_FT;
        bg.width *= M_TO_FT;
    }
    if let Some(ref mut dxf) = p.background_dxf {
        dxf.min_x *= M_TO_FT;
        dxf.min_y *= M_TO_FT;
        dxf.max_x *= M_TO_FT;
        dxf.max_y *= M_TO_FT;
    }
    for curve in &mut p.idf_curves {
        curve.a *= MM_HR_TO_IN_HR;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_node_count() {
        let mut p = Project::demo();
        let n = p.nodes.len();
        convert_project(&mut p, UnitSystem::Si);
        assert_eq!(p.units, UnitSystem::Si);
        convert_project(&mut p, UnitSystem::UsCustomary);
        assert_eq!(p.nodes.len(), n);
    }
}