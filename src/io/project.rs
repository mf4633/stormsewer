// SPDX-License-Identifier: GPL-3.0-or-later

//! StormSewer `.ssproj` project file (JSON).

use crate::catchment::{
    catchment_tc_minutes, default_flow_length_ft, polygon_centroid, shoelace_area_sqft,
    sqft_to_acres,
};
use crate::hydrology::IdfSet;
use crate::idf::IdfCurve;
use crate::network::{AnalysisOptions, Network, Node, NodeKind, Pipe};
use crate::units::UnitSystem;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

fn default_design_return_period() -> f64 {
    10.0
}

fn default_min_slope() -> f64 {
    0.001
}

fn default_p2_rainfall_in() -> f64 {
    3.0
}

fn default_pipe_shape() -> String {
    "circular".into()
}

/// PNG site-plan underlay referenced by the project.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BackgroundImage {
    pub path: String,
    pub origin_x: f64,
    pub origin_y: f64,
    /// Width of the image in drawing units (feet).
    pub width: f64,
    pub opacity: f32,
}

fn default_dxf_opacity() -> f32 {
    0.45
}

/// DXF site-plan underlay (Hydraflow background drawing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackgroundDxf {
    pub path: String,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    #[serde(default = "default_dxf_opacity")]
    pub opacity: f32,
}

/// One return-period IDF curve imported from Hydraflow (coefficients in project units).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdfCurveEntry {
    pub rp_years: u32,
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

/// Per-inlet HEC-22 geometry overrides (zeros = use app-wide defaults).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InletOverrides {
    pub length_ft: f64,
    pub gutter_slope: f64,
    pub sag: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectNode {
    pub id: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub invert: f64,
    pub rim: f64,
    pub area_ac: f64,
    pub c: f64,
    pub tc_inlet: f64,
    #[serde(default)]
    pub inlet: InletOverrides,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectPipe {
    pub id: String,
    pub from: String,
    pub to: String,
    pub length: f64,
    /// Hydraulic diameter for circular pipes (ft or m per [`Project::units`]).
    pub diameter: f64,
    pub n: f64,
    /// `circular`, `box`, or `elliptical` (Hydraflow line type).
    #[serde(default = "default_pipe_shape")]
    pub shape: String,
    /// Rise for box/elliptical sections (ft or m); 0 for circular.
    #[serde(default)]
    pub rise_ft: f64,
    /// Span/width for box/elliptical sections (ft or m); 0 for circular.
    #[serde(default)]
    pub span_ft: f64,
}

impl ProjectPipe {
    pub fn new(id: &str, from: &str, to: &str, length: f64, diameter: f64, n: f64) -> Self {
        Self {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            length,
            diameter,
            n,
            shape: default_pipe_shape(),
            rise_ft: 0.0,
            span_ft: 0.0,
        }
    }
}

/// Drainage catchment polygon drawn on the plan view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectCatchment {
    pub id: String,
    pub vertices: Vec<(f64, f64)>,
    pub c: f64,
    pub flow_length_ft: f64,
    pub slope: f64,
    pub inlet_node_id: Option<String>,
}

/// Submittal metadata printed on reports (all optional).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReportInfo {
    #[serde(default)]
    pub project_number: String,
    #[serde(default)]
    pub engineer: String,
    #[serde(default)]
    pub firm: String,
    #[serde(default)]
    pub jurisdiction: String,
}

/// Full StormSewer project document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub idf_a: f64,
    pub idf_b: f64,
    pub idf_c: f64,
    pub tailwater: Option<f64>,
    pub min_tc: f64,
    pub junction_k: f64,
    /// Geometry-aware bend-loss coefficient (0 = off); see
    /// [`AnalysisOptions::bend_loss_coeff`](crate::network::AnalysisOptions::bend_loss_coeff).
    #[serde(default)]
    pub bend_loss_coeff: f64,
    /// Use the HEC-22 access-hole loss coefficient (Ko) instead of junction K.
    #[serde(default)]
    pub hec22_structure_loss: bool,
    /// Access-hole diameter (ft) for the HEC-22 loss (0 → default 4 ft manhole).
    #[serde(default)]
    pub access_hole_diam_ft: f64,
    #[serde(default = "default_design_return_period")]
    pub design_return_period_years: f64,
    /// 2-yr 24-hr rainfall depth (in) for TR-55 / FAA sheet-flow Tc (Eq. 3-3).
    #[serde(default = "default_p2_rainfall_in")]
    pub p2_rainfall_in: f64,
    #[serde(default = "default_min_slope")]
    pub min_slope: f64,
    pub nodes: Vec<ProjectNode>,
    pub pipes: Vec<ProjectPipe>,
    #[serde(default)]
    pub catchments: Vec<ProjectCatchment>,
    #[serde(default)]
    pub background: Option<BackgroundImage>,
    #[serde(default)]
    pub background_dxf: Option<BackgroundDxf>,
    /// Full IDF set from Hydraflow STM (when empty, derived from `idf_a/b/c`).
    #[serde(default)]
    pub idf_curves: Vec<IdfCurveEntry>,
    #[serde(default)]
    pub units: UnitSystem,
    /// Submittal metadata for reports (engineer, firm, project number, …).
    #[serde(default)]
    pub report: ReportInfo,
}

impl Default for Project {
    fn default() -> Self {
        Self::demo()
    }
}

impl Project {
    /// Blank project: one outfall at the origin, default IDF parameters, no pipes.
    pub fn empty() -> Self {
        Self {
            name: "Untitled".into(),
            idf_a: 60.0,
            idf_b: 10.0,
            idf_c: 0.8,
            tailwater: None,
            min_tc: 10.0,
            junction_k: 0.5,
            bend_loss_coeff: 0.0,
            hec22_structure_loss: false,
            access_hole_diam_ft: 4.0,
            design_return_period_years: 10.0,
            p2_rainfall_in: default_p2_rainfall_in(),
            min_slope: 0.001,
            nodes: vec![ProjectNode {
                id: "OUT".into(),
                kind: "outfall".into(),
                x: 0.0,
                y: 0.0,
                invert: 100.0,
                rim: 106.0,
                area_ac: 0.0,
                c: 0.0,
                tc_inlet: 0.0,
                inlet: InletOverrides::default(),
            }],
            pipes: Vec::new(),
            catchments: Vec::new(),
            background: None,
            background_dxf: None,
            idf_curves: Vec::new(),
            units: UnitSystem::default(),
            report: ReportInfo::default(),
        }
    }

    /// Built-in demo network for first launch / investor walkthrough.
    pub fn demo() -> Self {
        Self {
            name: "Demo Trunk Line".into(),
            idf_a: 60.0,
            idf_b: 10.0,
            idf_c: 0.8,
            tailwater: Some(100.5),
            min_tc: 10.0,
            junction_k: 0.5,
            bend_loss_coeff: 0.0,
            hec22_structure_loss: false,
            access_hole_diam_ft: 4.0,
            design_return_period_years: 10.0,
            p2_rainfall_in: default_p2_rainfall_in(),
            min_slope: 0.001,
            nodes: vec![
                ProjectNode {
                    id: "N1".into(),
                    kind: "inlet".into(),
                    x: 0.0,
                    y: 0.0,
                    invert: 104.0,
                    rim: 110.0,
                    area_ac: 1.0,
                    c: 0.70,
                    tc_inlet: 12.0,
                    inlet: InletOverrides::default(),
                },
                ProjectNode {
                    id: "N2".into(),
                    kind: "inlet".into(),
                    x: 300.0,
                    y: 0.0,
                    invert: 102.5,
                    rim: 108.5,
                    area_ac: 1.0,
                    c: 0.70,
                    tc_inlet: 10.0,
                    inlet: InletOverrides::default(),
                },
                ProjectNode {
                    id: "N3".into(),
                    kind: "junction".into(),
                    x: 550.0,
                    y: 0.0,
                    invert: 101.2,
                    rim: 107.0,
                    area_ac: 0.5,
                    c: 0.80,
                    tc_inlet: 8.0,
                    inlet: InletOverrides::default(),
                },
                ProjectNode {
                    id: "OUT".into(),
                    kind: "outfall".into(),
                    x: 730.0,
                    y: 0.0,
                    invert: 100.0,
                    rim: 106.0,
                    area_ac: 0.0,
                    c: 0.0,
                    tc_inlet: 0.0,
                    inlet: InletOverrides::default(),
                },
            ],
            pipes: vec![
                ProjectPipe::new("P1", "N1", "N2", 300.0, 1.25, 0.013),
                ProjectPipe::new("P2", "N2", "N3", 250.0, 1.50, 0.013),
                ProjectPipe::new("P3", "N3", "OUT", 180.0, 1.75, 0.013),
            ],
            catchments: vec![ProjectCatchment {
                id: "C1".into(),
                vertices: vec![(-40.0, 80.0), (40.0, 80.0), (0.0, 140.0)],
                c: 0.70,
                flow_length_ft: 100.0,
                slope: 0.01,
                inlet_node_id: Some("N1".into()),
            }],
            background: None,
            background_dxf: None,
            idf_curves: Vec::new(),
            units: UnitSystem::default(),
            report: ReportInfo::default(),
        }
    }

    /// Length in engine units (feet).
    fn len_to_engine_ft(&self, v: f64) -> f64 {
        self.units.length_to_engine_ft(v)
    }

    /// Area in engine units (acres).
    pub fn area_to_engine_ac(&self, v: f64) -> f64 {
        self.units.area_to_engine_ac(v)
    }

    /// Hydraulic diameter in engine units (feet).
    fn dia_to_engine_ft(&self, v: f64) -> f64 {
        self.units.length_to_engine_ft(v)
    }

    /// Plan area of each catchment in acres (shoelace formula).
    pub fn catchment_areas(&self) -> Vec<f64> {
        self.catchments
            .iter()
            .map(|c| sqft_to_acres(shoelace_area_sqft(&c.vertices)))
            .collect()
    }

    /// Total drainage area from all catchment polygons (acres).
    pub fn total_catchment_area_ac(&self) -> f64 {
        self.catchment_areas().iter().sum()
    }

    /// Area in acres for a single catchment by id.
    pub fn catchment_area_ac(&self, id: &str) -> Option<f64> {
        self.catchments
            .iter()
            .find(|c| c.id == id)
            .map(|c| sqft_to_acres(shoelace_area_sqft(&c.vertices)))
    }

    /// Recompute flow-path length from centroid to the linked inlet (or nearest inlet).
    pub fn refresh_catchment_flow_lengths(&mut self) {
        for catchment in &mut self.catchments {
            let centroid = polygon_centroid(&catchment.vertices);
            let target = catchment
                .inlet_node_id
                .as_ref()
                .and_then(|id| self.nodes.iter().find(|n| &n.id == id))
                .map(|n| (n.x, n.y))
                .or_else(|| {
                    self.nodes
                        .iter()
                        .filter(|n| n.kind == "inlet")
                        .min_by(|a, b| {
                            let da = default_flow_length_ft(centroid, (a.x, a.y));
                            let db = default_flow_length_ft(centroid, (b.x, b.y));
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|n| (n.x, n.y))
                });
            if let Some(t) = target {
                catchment.flow_length_ft = default_flow_length_ft(centroid, t);
            }
        }
    }

    pub fn idf(&self) -> IdfCurve {
        let design_rp = self.design_return_period_years.round().max(1.0) as u32;
        if let Some(entry) = self
            .idf_curves
            .iter()
            .find(|c| c.rp_years == design_rp)
        {
            return IdfCurve::new(
                self.units.idf_a_to_engine(entry.a),
                entry.b,
                entry.c,
            );
        }
        IdfCurve::new(self.units.idf_a_to_engine(self.idf_a), self.idf_b, self.idf_c)
    }

    /// Multi-return-period IDF set (imported STM curves or scaled from `idf_a/b/c`).
    pub fn idf_set(&self) -> IdfSet {
        let design_rp = self.design_return_period_years.round().max(1.0) as u32;
        if !self.idf_curves.is_empty() {
            let mut idf = IdfSet::new(design_rp);
            for entry in &self.idf_curves {
                idf.set_curve(
                    entry.rp_years,
                    IdfCurve::new(
                        self.units.idf_a_to_engine(entry.a),
                        entry.b,
                        entry.c,
                    ),
                );
            }
            idf.set_design_rp(design_rp);
            return idf;
        }

        let mut idf = IdfSet::municipal_default();
        for &rp in &[2u32, 5, 10, 25, 50, 100] {
            let factor = match rp {
                2 => 0.75,
                5 => 0.88,
                10 => 1.0,
                25 => 1.15,
                50 => 1.28,
                100 => 1.40,
                _ => 1.0,
            };
            idf.set_curve(
                rp,
                IdfCurve::new(self.idf_a * factor, self.idf_b, self.idf_c),
            );
        }
        idf.set_curve(design_rp, self.idf());
        idf.set_design_rp(design_rp);
        idf
    }

    /// Validate project topology and return human-readable error messages (empty if OK).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.nodes.is_empty() {
            errors.push("project has no nodes".into());
        }

        if !self.nodes.iter().any(|n| n.kind == "outfall") {
            errors.push("project has no outfall".into());
        }

        let mut seen_ids = HashSet::new();
        for node in &self.nodes {
            if !seen_ids.insert(&node.id) {
                errors.push(format!("duplicate node id: {}", node.id));
            }
        }

        let node_ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        for pipe in &self.pipes {
            if !node_ids.contains(pipe.from.as_str()) {
                errors.push(format!(
                    "pipe {} references missing upstream node {}",
                    pipe.id, pipe.from
                ));
            }
            if !node_ids.contains(pipe.to.as_str()) {
                errors.push(format!(
                    "pipe {} references missing downstream node {}",
                    pipe.id, pipe.to
                ));
            }
        }

        errors
    }

    pub fn options(&self) -> AnalysisOptions {
        let tailwater = self.tailwater.map(|tw| self.len_to_engine_ft(tw));
        AnalysisOptions {
            min_tc: self.min_tc,
            tailwater,
            junction_k: self.junction_k,
            intensity_override: None,
            min_slope: self.min_slope,
            bend_loss_coeff: self.bend_loss_coeff,
            hec22_structure_loss: self.hec22_structure_loss,
            access_hole_diam_ft: if self.access_hole_diam_ft > 0.0 {
                self.len_to_engine_ft(self.access_hole_diam_ft)
            } else {
                self.len_to_engine_ft(4.0)
            },
            access_hole_bench_factor: 1.0,
        }
    }

    pub fn to_network(&self) -> Network {
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                let kind = match n.kind.as_str() {
                    "outfall" => NodeKind::Outfall,
                    "junction" => NodeKind::Junction,
                    _ => NodeKind::Inlet,
                };
                let mut node = match kind {
                    NodeKind::Outfall => Node::outfall(
                        &n.id,
                        self.len_to_engine_ft(n.invert),
                        self.len_to_engine_ft(n.rim),
                    ),
                    NodeKind::Junction => Node::junction(
                        &n.id,
                        self.len_to_engine_ft(n.invert),
                        self.len_to_engine_ft(n.rim),
                        self.area_to_engine_ac(n.area_ac),
                        n.c,
                    ),
                    NodeKind::Inlet => Node::inlet(
                        &n.id,
                        self.len_to_engine_ft(n.invert),
                        self.len_to_engine_ft(n.rim),
                        self.area_to_engine_ac(n.area_ac),
                        n.c,
                    ),
                };
                node = node.at(self.len_to_engine_ft(n.x), self.len_to_engine_ft(n.y));
                if n.tc_inlet > 0.0 {
                    node = node.with_tc_inlet(n.tc_inlet);
                }
                node
            })
            .collect();
        let pipes = self
            .pipes
            .iter()
            .map(|p| {
                let length = self.len_to_engine_ft(p.length);
                match p.shape.as_str() {
                    "box" if p.rise_ft > 0.0 && p.span_ft > 0.0 => Pipe::rectangular(
                        &p.id,
                        &p.from,
                        &p.to,
                        length,
                        self.len_to_engine_ft(p.rise_ft),
                        self.len_to_engine_ft(p.span_ft),
                        p.n,
                    ),
                    "elliptical" if p.rise_ft > 0.0 && p.span_ft > 0.0 => Pipe::elliptical(
                        &p.id,
                        &p.from,
                        &p.to,
                        length,
                        self.len_to_engine_ft(p.rise_ft),
                        self.len_to_engine_ft(p.span_ft),
                        p.n,
                    ),
                    "arch" if p.rise_ft > 0.0 && p.span_ft > 0.0 => Pipe::arch(
                        &p.id,
                        &p.from,
                        &p.to,
                        length,
                        self.len_to_engine_ft(p.rise_ft),
                        self.len_to_engine_ft(p.span_ft),
                        p.n,
                    ),
                    _ => Pipe::new(&p.id, &p.from, &p.to, length, self.dia_to_engine_ft(p.diameter), p.n),
                }
            })
            .collect();
        Network { nodes, pipes }
    }

    /// Build a runtime network with catchment polygons merged into their linked inlet nodes.
    pub fn to_analysis_network(&self) -> Network {
        let mut net = self.to_network();

        for catchment in &self.catchments {
            let Some(ref inlet_id) = catchment.inlet_node_id else {
                continue;
            };
            let area = sqft_to_acres(shoelace_area_sqft(&catchment.vertices));
            if area <= 0.0 {
                continue;
            }
            let tc_cat = catchment_tc_minutes(catchment.flow_length_ft, catchment.slope);

            if let Some(node) = net.nodes.iter_mut().find(|n| n.id == *inlet_id) {
                let old_area = node.area_ac;
                let old_c = node.c;
                let new_area = old_area + area;
                if new_area > 0.0 {
                    node.c = (old_c * old_area + catchment.c * area) / new_area;
                    node.area_ac = new_area;
                }
                node.tc_inlet = node.tc_inlet.max(tc_cat);
            }
        }

        net
    }

    pub fn from_network(net: &Network, name: &str, idf: &IdfCurve, opts: &AnalysisOptions) -> Self {
        Self {
            name: name.into(),
            idf_a: idf.a,
            idf_b: idf.b,
            idf_c: idf.c,
            tailwater: opts.tailwater,
            min_tc: opts.min_tc,
            junction_k: opts.junction_k,
            bend_loss_coeff: opts.bend_loss_coeff,
            hec22_structure_loss: opts.hec22_structure_loss,
            access_hole_diam_ft: opts.access_hole_diam_ft,
            design_return_period_years: 10.0,
            p2_rainfall_in: default_p2_rainfall_in(),
            min_slope: opts.min_slope,
            nodes: net
                .nodes
                .iter()
                .map(|n| ProjectNode {
                    id: n.id.clone(),
                    kind: match n.kind {
                        NodeKind::Outfall => "outfall",
                        NodeKind::Junction => "junction",
                        NodeKind::Inlet => "inlet",
                    }
                    .into(),
                    x: n.x,
                    y: n.y,
                    invert: n.invert,
                    rim: n.rim,
                    area_ac: n.area_ac,
                    c: n.c,
                    tc_inlet: n.tc_inlet,
                    inlet: InletOverrides::default(),
                })
                .collect(),
            pipes: net
                .pipes
                .iter()
                .map(|p| ProjectPipe::new(&p.id, &p.from, &p.to, p.length, p.diameter, p.n))
                .collect(),
            catchments: Vec::new(),
            background: None,
            background_dxf: None,
            idf_curves: Vec::new(),
            units: UnitSystem::default(),
            report: ReportInfo::default(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("invalid project file: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p2_rainfall_defaults_on_legacy_json() {
        let json = r#"{"name":"x","idf_a":60,"idf_b":10,"idf_c":0.8,"tailwater":null,"min_tc":10,"junction_k":0.5,"design_return_period_years":10,"min_slope":0.001,"nodes":[],"pipes":[]}"#;
        let p: Project = serde_json::from_str(json).unwrap();
        assert!((p.p2_rainfall_in - 3.0).abs() < 1e-9);
    }

    #[test]
    fn demo_round_trips_to_network() {
        let p = Project::demo();
        let net = p.to_network();
        assert_eq!(net.nodes.len(), 4);
        assert_eq!(net.pipes.len(), 3);
        let a = net.analyze(&p.idf(), &p.options()).unwrap();
        assert_eq!(a.pipes.len(), 3);
    }

    #[test]
    fn empty_project_has_outfall_only() {
        let p = Project::empty();
        assert_eq!(p.nodes.len(), 1);
        assert_eq!(p.nodes[0].id, "OUT");
        assert!(p.pipes.is_empty());
        assert!(p.validate().is_empty());
    }

    #[test]
    fn validate_catches_topology_errors() {
        let mut p = Project::empty();
        p.nodes[0].kind = "inlet".into();
        let errs = p.validate();
        assert!(errs.iter().any(|e| e.contains("outfall")));

        p.nodes[0].kind = "outfall".into();
        p.nodes.push(ProjectNode {
            id: "OUT".into(),
            kind: "outfall".into(),
            x: 10.0,
            y: 0.0,
            invert: 100.0,
            rim: 106.0,
            area_ac: 0.0,
            c: 0.0,
            tc_inlet: 0.0,
            inlet: InletOverrides::default(),
        });
        let errs = p.validate();
        assert!(errs.iter().any(|e| e.contains("duplicate node id")));

        let mut p2 = Project::demo();
        p2.pipes.push(ProjectPipe::new("PX", "MISSING", "OUT", 10.0, 1.0, 0.013));
        let errs = p2.validate();
        assert!(errs.iter().any(|e| e.contains("MISSING")));
    }

    #[test]
    fn to_analysis_network_merges_catchments_into_inlets() {
        let mut p = Project::demo();
        p.nodes.iter_mut().find(|n| n.id == "N1").unwrap().area_ac = 1.0;
        p.nodes.iter_mut().find(|n| n.id == "N1").unwrap().c = 0.80;
        p.nodes.iter_mut().find(|n| n.id == "N1").unwrap().tc_inlet = 5.0;

        p.catchments = vec![ProjectCatchment {
            id: "C1".into(),
            vertices: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            c: 0.50,
            flow_length_ft: 200.0,
            slope: 0.02,
            inlet_node_id: Some("N1".into()),
        }];

        let cat_area = sqft_to_acres(shoelace_area_sqft(&p.catchments[0].vertices));
        let tc_cat = catchment_tc_minutes(200.0, 0.02);

        let net = p.to_analysis_network();
        let n1 = net.nodes.iter().find(|n| n.id == "N1").unwrap();
        let expected_area = 1.0 + cat_area;
        let expected_c = (0.80 * 1.0 + 0.50 * cat_area) / expected_area;

        assert!((n1.area_ac - expected_area).abs() < 1e-9);
        assert!((n1.c - expected_c).abs() < 1e-9);
        assert!((n1.tc_inlet - 5.0f64.max(tc_cat)).abs() < 1e-9);

        let plain = p.to_network();
        let plain_n1 = plain.nodes.iter().find(|n| n.id == "N1").unwrap();
        assert!((plain_n1.area_ac - 1.0).abs() < 1e-9);
    }
}