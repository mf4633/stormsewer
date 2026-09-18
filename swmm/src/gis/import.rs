// SPDX-License-Identifier: GPL-3.0-or-later

//! Mapping GIS features onto model objects: points become nodes or rain
//! gages, lines become conduits between snapped or newly created nodes,
//! polygons become subcatchments with their area from the geometry.
//!
//! The plan is built without touching the document ([`plan`]) so the
//! dialog can preview counts and reasons for skipping; applying it is one
//! undo gesture ([`apply_plan`], or the app's own loop over
//! [`ImportPlan::items`]).
//!
//! Units: `[MAP] Units` says what a map coordinate is (FEET, METERS,
//! DEGREES, NONE) and `[OPTIONS] FLOW_UNITS` whether areas are acres
//! (US flow units) or hectares (SI) and lengths feet or metres. When the
//! model has a CRS its linear unit is used instead of `[MAP] Units`. A
//! geometry in degrees, or a model with unknown map units, yields no area
//! or length from the geometry and the column keeps its default.

use crate::doc::build::{self, LinkType, NodeType, ObjRef};
use crate::doc::{format_number, Command, InpDoc, ObjectKind};
use crate::gis::crs::{transform, Crs};
use crate::gis::vector::{FieldValue, Geometry, GeometryKind, Layer};
use crate::Result;

/// What a layer's features become.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Junction,
    Outfall,
    Storage,
    Divider,
    RainGage,
    Conduit,
    Subcatchment,
}

impl Target {
    pub const ALL: [Target; 7] = [
        Target::Junction,
        Target::Outfall,
        Target::Storage,
        Target::Divider,
        Target::RainGage,
        Target::Conduit,
        Target::Subcatchment,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Junction => "Junction",
            Self::Outfall => "Outfall",
            Self::Storage => "Storage Unit",
            Self::Divider => "Divider",
            Self::RainGage => "Rain Gage",
            Self::Conduit => "Conduit",
            Self::Subcatchment => "Subcatchment",
        }
    }

    pub fn geometry_kind(self) -> GeometryKind {
        match self {
            Self::Conduit => GeometryKind::Line,
            Self::Subcatchment => GeometryKind::Polygon,
            _ => GeometryKind::Point,
        }
    }

    /// The targets a geometry kind can become.
    pub fn for_kind(kind: GeometryKind) -> Vec<Target> {
        Self::ALL
            .into_iter()
            .filter(|t| t.geometry_kind() == kind)
            .collect()
    }

    pub fn node_type(self) -> Option<NodeType> {
        match self {
            Self::Junction => Some(NodeType::Junction),
            Self::Outfall => Some(NodeType::Outfall),
            Self::Storage => Some(NodeType::Storage),
            Self::Divider => Some(NodeType::Divider),
            _ => None,
        }
    }

    pub fn object_kind(self) -> ObjectKind {
        match self {
            Self::RainGage => ObjectKind::Gage,
            Self::Conduit => ObjectKind::Link,
            Self::Subcatchment => ObjectKind::Subcatchment,
            _ => ObjectKind::Node,
        }
    }

    pub fn default_prefix(self) -> &'static str {
        match self {
            Self::RainGage => "R",
            Self::Conduit => "C",
            Self::Subcatchment => "S",
            Self::Junction => "J",
            Self::Outfall => "O",
            Self::Storage => "ST",
            Self::Divider => "D",
        }
    }

    /// The SWMM columns a layer field may fill, as (section, column). The
    /// name and the geometry-derived columns are set by the importer and
    /// are not listed.
    pub fn columns(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Junction => &[
                ("JUNCTIONS", "Elevation"),
                ("JUNCTIONS", "MaxDepth"),
                ("JUNCTIONS", "InitDepth"),
                ("JUNCTIONS", "SurDepth"),
                ("JUNCTIONS", "Aponded"),
            ],
            Self::Outfall => &[("OUTFALLS", "Elevation"), ("OUTFALLS", "Type"), ("OUTFALLS", "Gated")],
            Self::Storage => &[
                ("STORAGE", "Elevation"),
                ("STORAGE", "MaxDepth"),
                ("STORAGE", "InitDepth"),
            ],
            Self::Divider => &[
                ("DIVIDERS", "Elevation"),
                ("DIVIDERS", "DivertLink"),
                ("DIVIDERS", "Type"),
                ("DIVIDERS", "MaxDepth"),
            ],
            Self::RainGage => &[
                ("RAINGAGES", "Format"),
                ("RAINGAGES", "Interval"),
                ("RAINGAGES", "SCF"),
                ("RAINGAGES", "Series"),
            ],
            Self::Conduit => &[
                ("CONDUITS", "Length"),
                ("CONDUITS", "Roughness"),
                ("CONDUITS", "InOffset"),
                ("CONDUITS", "OutOffset"),
                ("CONDUITS", "InitFlow"),
                ("CONDUITS", "MaxFlow"),
                ("XSECTIONS", "Shape"),
                ("XSECTIONS", "Geom1"),
                ("XSECTIONS", "Geom2"),
                ("XSECTIONS", "Geom3"),
                ("XSECTIONS", "Geom4"),
                ("XSECTIONS", "Barrels"),
            ],
            Self::Subcatchment => &[
                ("SUBCATCHMENTS", "RainGage"),
                ("SUBCATCHMENTS", "Outlet"),
                ("SUBCATCHMENTS", "Area"),
                ("SUBCATCHMENTS", "PctImperv"),
                ("SUBCATCHMENTS", "Width"),
                ("SUBCATCHMENTS", "PctSlope"),
                ("SUBCATCHMENTS", "CurbLen"),
                ("SUBAREAS", "NImperv"),
                ("SUBAREAS", "NPerv"),
                ("SUBAREAS", "SImperv"),
                ("SUBAREAS", "SPerv"),
                ("SUBAREAS", "PctZero"),
            ],
        }
    }
}

/// Where a SWMM column's value comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Leave the builder's default.
    Skip,
    /// A layer field, by index.
    Field(usize),
    /// The same text for every feature.
    Constant(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldMap {
    pub section: String,
    pub column: String,
    pub source: Source,
}

/// Everything the dialog decides.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    pub target: Target,
    /// Field holding the object name; `None` numbers them with the prefix.
    pub name_field: Option<usize>,
    pub name_prefix: String,
    pub fields: Vec<FieldMap>,
    /// Conduit ends snap to an existing node within this distance (map
    /// units).
    pub snap_tolerance: f64,
    /// Create a junction where a conduit end has no node to snap to
    /// (else the feature is skipped).
    pub create_end_nodes: bool,
    /// Subcatchment outlet = the nearest node (else `*`).
    pub outlet_nearest_node: bool,
    /// Reproject from the layer's CRS to the model's before mapping.
    pub reproject: Option<(Crs, Crs)>,
}

impl ImportOptions {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            name_field: None,
            name_prefix: target.default_prefix().to_string(),
            fields: Vec::new(),
            snap_tolerance: 1.0,
            create_end_nodes: true,
            outlet_nearest_node: true,
            reproject: None,
        }
    }

    /// Guess field mappings by name: a layer field whose name matches a
    /// SWMM column (case-insensitive, separators ignored) fills it, and a
    /// field called `name`/`id`/`label` names the object.
    pub fn auto_map(mut self, layer: &Layer) -> Self {
        let fold = |s: &str| {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>()
        };
        self.fields = self
            .target
            .columns()
            .iter()
            .map(|(section, column)| {
                let key = fold(column);
                let source = layer
                    .fields
                    .iter()
                    .position(|f| fold(&f.name) == key || (key == "elevation" && fold(&f.name) == "invert"))
                    .map_or(Source::Skip, Source::Field);
                FieldMap {
                    section: section.to_string(),
                    column: column.to_string(),
                    source,
                }
            })
            .collect();
        if self.name_field.is_none() {
            self.name_field = ["name", "id", "label", "objectid", "fid"]
                .iter()
                .find_map(|k| layer.fields.iter().position(|f| fold(&f.name) == *k));
        }
        self
    }
}

/// How map units relate to the model's length and area columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitFactors {
    /// Metres per map unit, when known.
    pub map_unit_m: Option<f64>,
    /// The model is in US units (feet, acres) rather than SI.
    pub us: bool,
}

impl UnitFactors {
    /// From the document's `[OPTIONS] FLOW_UNITS` and `[MAP] Units`, with
    /// the model CRS's unit taking precedence when there is one.
    pub fn from_doc(doc: &InpDoc, model_crs: Option<&Crs>) -> Self {
        let flow = doc.option("FLOW_UNITS").unwrap_or("CFS").to_ascii_uppercase();
        let us = matches!(flow.as_str(), "CFS" | "GPM" | "MGD");
        let map_unit_m = match model_crs {
            Some(c) if !c.is_geographic() => Some(c.unit.to_metre),
            Some(_) => None,
            None => match doc
                .key_value("MAP", "Units")
                .unwrap_or_default()
                .to_ascii_uppercase()
                .as_str()
            {
                "FEET" => Some(0.3048),
                "METERS" => Some(1.0),
                _ => None,
            },
        };
        Self { map_unit_m, us }
    }

    /// Multiply a length in map units by this for the model's length unit.
    pub fn length(&self) -> Option<f64> {
        let m = self.map_unit_m?;
        Some(if self.us { m / 0.3048 } else { m })
    }

    /// Multiply a squared-map-unit area by this for acres or hectares.
    pub fn area(&self) -> Option<f64> {
        let m = self.map_unit_m?;
        Some(if self.us { m * m / 4046.8564224 } else { m * m / 10000.0 })
    }

    pub fn area_unit(&self) -> &'static str {
        if self.us {
            "acres"
        } else {
            "hectares"
        }
    }

    pub fn length_unit(&self) -> &'static str {
        if self.us {
            "ft"
        } else {
            "m"
        }
    }
}

/// One feature's contribution to the model.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub feature: usize,
    pub name: String,
    pub command: Command,
    /// Nodes created for a conduit's ends, by name.
    pub extra_nodes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportPlan {
    pub items: Vec<PlanItem>,
    /// Feature index and why it was skipped.
    pub skipped: Vec<(usize, String)>,
    pub notes: Vec<String>,
}

impl ImportPlan {
    pub fn summary(&self, target: Target) -> String {
        let extra: usize = self.items.iter().map(|i| i.extra_nodes.len()).sum();
        let mut s = format!("{} {}(s)", self.items.len(), target.label().to_ascii_lowercase());
        if extra > 0 {
            s.push_str(&format!(" + {extra} junction(s) at conduit ends"));
        }
        if !self.skipped.is_empty() {
            s.push_str(&format!(", {} skipped", self.skipped.len()));
        }
        s
    }
}

/// Reproject every geometry of a layer.
pub fn reproject(layer: &Layer, from: &Crs, to: &Crs) -> Result<Layer> {
    let mut out = layer.clone();
    for f in &mut out.features {
        if let Some(g) = &f.geometry {
            let err: std::cell::RefCell<Option<crate::Error>> = std::cell::RefCell::new(None);
            let mapped = g.map(&|(x, y)| match transform(from, to, x, y) {
                Ok(p) => p,
                Err(e) => {
                    *err.borrow_mut() = Some(e);
                    (x, y)
                }
            });
            if let Some(e) = err.into_inner() {
                return Err(e);
            }
            f.geometry = Some(mapped);
        }
    }
    out.crs = Some(to.clone());
    out.crs_text = Some(to.label());
    Ok(out)
}

/// Rewrite the name a builder chose into the one the import wants. Every
/// row a builder adds starts with the object's name, and its geometry
/// commands name it directly.
fn rename(cmd: Command, old: &str, new: &str) -> Command {
    match cmd {
        Command::Batch(cmds) => Command::Batch(cmds.into_iter().map(|c| rename(c, old, new)).collect()),
        Command::AddRow {
            section,
            mut fields,
            comment,
        } => {
            if fields.first().is_some_and(|f| f == old) {
                fields[0] = new.to_string();
            }
            Command::AddRow {
                section,
                fields,
                comment,
            }
        }
        Command::MoveNode { name, x, y } if name == old => Command::MoveNode {
            name: new.to_string(),
            x,
            y,
        },
        Command::MoveGage { name, x, y } if name == old => Command::MoveGage {
            name: new.to_string(),
            x,
            y,
        },
        Command::SetVertices { link, points } if link == old => Command::SetVertices {
            link: new.to_string(),
            points,
        },
        Command::SetPolygon { subcatchment, points } if subcatchment == old => Command::SetPolygon {
            subcatchment: new.to_string(),
            points,
        },
        other => other,
    }
}

struct Namer<'a> {
    doc: &'a InpDoc,
    kind: ObjectKind,
    reserved: Vec<String>,
}

impl Namer<'_> {
    fn take(&mut self, wanted: Option<&str>, prefix: &str) -> String {
        // SWMM names are single tokens: whitespace becomes an underscore
        // rather than a quoted name every other tool would trip on.
        let wanted = wanted
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join("_"));
        let name = match wanted {
            Some(w) => build::unique_name_from(self.doc, self.kind, &w, &self.reserved),
            None => build::unique_name_reserving(self.doc, self.kind, prefix, &self.reserved),
        };
        self.reserved.push(name.clone());
        name
    }
}

fn field_text(layer: &Layer, feature: usize, source: &Source) -> Option<String> {
    match source {
        Source::Skip => None,
        Source::Constant(c) => Some(c.clone()),
        Source::Field(i) => {
            let v = layer.features.get(feature)?.values.get(*i)?;
            if v.is_null() {
                None
            } else {
                Some(v.to_field())
            }
        }
    }
}

fn set_fields(layer: &Layer, feature: usize, name: &str, maps: &[FieldMap], cmds: &mut Vec<Command>) {
    for m in maps {
        if let Some(value) = field_text(layer, feature, &m.source) {
            let value = if value.contains(char::is_whitespace) {
                format!("\"{value}\"")
            } else {
                value
            };
            cmds.push(Command::SetField {
                section: m.section.clone(),
                name: name.to_string(),
                field: m.column.clone(),
                value,
            });
        }
    }
}

/// Every node in the document with a position.
pub fn node_positions(doc: &InpDoc) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    for kind in NodeType::ALL {
        for name in doc.names(kind.section()) {
            if let Some((x, y)) = doc.coordinates(&name) {
                out.push((name, x, y));
            }
        }
    }
    out
}

fn nearest(nodes: &[(String, f64, f64)], p: (f64, f64)) -> Option<(usize, f64)> {
    nodes
        .iter()
        .enumerate()
        .map(|(i, (_, x, y))| (i, ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt()))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// Build the plan for importing `layer` into `doc` with `opts`. The layer
/// is reprojected first when `opts.reproject` is set.
pub fn plan(doc: &InpDoc, layer: &Layer, opts: &ImportOptions, units: UnitFactors) -> Result<ImportPlan> {
    let projected;
    let layer = match &opts.reproject {
        Some((from, to)) => {
            projected = reproject(layer, from, to)?;
            &projected
        }
        None => layer,
    };
    let mut out = ImportPlan::default();
    let mut namer = Namer {
        doc,
        kind: opts.target.object_kind(),
        reserved: Vec::new(),
    };
    let mut node_namer = Namer {
        doc,
        kind: ObjectKind::Node,
        reserved: Vec::new(),
    };
    let mut nodes = node_positions(doc);
    let want = opts.target.geometry_kind();
    let mapped_columns: Vec<&str> = opts
        .fields
        .iter()
        .filter(|m| m.source != Source::Skip)
        .map(|m| m.column.as_str())
        .collect();
    if units.map_unit_m.is_none() && matches!(opts.target, Target::Conduit | Target::Subcatchment) {
        out.notes.push(format!(
            "map units unknown: {} not computed from the geometry",
            if opts.target == Target::Conduit { "conduit lengths" } else { "subcatchment areas" }
        ));
    }
    for (i, f) in layer.features.iter().enumerate() {
        let Some(g) = &f.geometry else {
            out.skipped.push((i, "no geometry".into()));
            continue;
        };
        if g.kind() != want {
            out.skipped.push((i, format!("{} geometry, not {}", g.kind().label(), want.label())));
            continue;
        }
        if g.points().iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
            out.skipped.push((i, "coordinate is not a number".into()));
            continue;
        }
        let wanted = opts
            .name_field
            .and_then(|k| f.values.get(k))
            .filter(|v| !v.is_null())
            .map(|v| v.to_field());
        let mut cmds = Vec::new();
        let mut extra_nodes = Vec::new();
        let name = match opts.target {
            Target::Junction | Target::Outfall | Target::Storage | Target::Divider => {
                let Some((x, y)) = g.anchor() else {
                    out.skipped.push((i, "empty point".into()));
                    continue;
                };
                let kind = opts.target.node_type().expect("node target");
                let obj = build::new_node(doc, kind, x, y);
                let name = namer.take(wanted.as_deref(), &opts.name_prefix);
                cmds.push(rename(obj.command, &obj.name, &name));
                nodes.push((name.clone(), x, y));
                name
            }
            Target::RainGage => {
                let Some((x, y)) = g.anchor() else {
                    out.skipped.push((i, "empty point".into()));
                    continue;
                };
                let obj = build::new_gage(doc, x, y);
                let name = namer.take(wanted.as_deref(), &opts.name_prefix);
                cmds.push(rename(obj.command, &obj.name, &name));
                name
            }
            Target::Conduit => {
                let Some(path) = g.main_path().filter(|p| p.len() >= 2) else {
                    out.skipped.push((i, "line has fewer than two points".into()));
                    continue;
                };
                let mut ends = Vec::with_capacity(2);
                let mut skip = None;
                for (which, p) in [("start", path[0]), ("end", path[path.len() - 1])] {
                    match nearest(&nodes, p) {
                        Some((k, d)) if d <= opts.snap_tolerance => ends.push(nodes[k].0.clone()),
                        _ if opts.create_end_nodes => {
                            let obj = build::new_node(doc, NodeType::Junction, p.0, p.1);
                            let n = node_namer.take(None, "J");
                            cmds.push(rename(obj.command, &obj.name, &n));
                            nodes.push((n.clone(), p.0, p.1));
                            extra_nodes.push(n.clone());
                            ends.push(n);
                        }
                        _ => {
                            skip = Some(format!("no node within {} of the {which}", format_number(opts.snap_tolerance)));
                            break;
                        }
                    }
                }
                if let Some(reason) = skip {
                    out.skipped.push((i, reason));
                    continue;
                }
                if ends[0] == ends[1] {
                    out.skipped.push((i, format!("both ends snap to {}", ends[0])));
                    continue;
                }
                let drawn = crate::gis::vector::path_length(&path);
                let length = units.length().map_or(drawn, |k| drawn * k);
                let interior = path[1..path.len() - 1].to_vec();
                let obj = build::new_link(doc, LinkType::Conduit, &ends[0], &ends[1], &interior, length);
                let name = namer.take(wanted.as_deref(), &opts.name_prefix);
                cmds.push(rename(obj.command, &obj.name, &name));
                name
            }
            Target::Subcatchment => {
                let Some(ring) = g.outer_ring().filter(|r| r.len() >= 3) else {
                    out.skipped.push((i, "polygon has fewer than three points".into()));
                    continue;
                };
                let outlet = if opts.outlet_nearest_node && !mapped_columns.contains(&"Outlet") {
                    g.anchor()
                        .and_then(|c| nearest(&nodes, c))
                        .map(|(k, _)| nodes[k].0.clone())
                        .unwrap_or_else(|| "*".into())
                } else {
                    "*".into()
                };
                let obj = build::new_subcatchment(doc, &ring, &outlet);
                let name = namer.take(wanted.as_deref(), &opts.name_prefix);
                cmds.push(rename(obj.command, &obj.name, &name));
                if !mapped_columns.contains(&"Area") {
                    if let Some(k) = units.area() {
                        let area = g.area() * k;
                        cmds.push(Command::SetField {
                            section: "SUBCATCHMENTS".into(),
                            name: name.clone(),
                            field: "Area".into(),
                            value: format_number((area * 1000.0).round() / 1000.0),
                        });
                    }
                }
                name
            }
        };
        set_fields(layer, i, &name, &opts.fields, &mut cmds);
        out.items.push(PlanItem {
            feature: i,
            name,
            command: Command::Batch(cmds),
            extra_nodes,
        });
    }
    Ok(out)
}

/// Apply a plan as one undo step. Items whose command fails are moved to
/// `skipped` with the error; the rest stay applied.
pub fn apply_plan(doc: &mut InpDoc, plan: &ImportPlan) -> ImportPlan {
    let mut done = ImportPlan {
        notes: plan.notes.clone(),
        skipped: plan.skipped.clone(),
        ..Default::default()
    };
    doc.begin_gesture();
    for item in &plan.items {
        match doc.apply(item.command.clone()) {
            Ok(()) => done.items.push(item.clone()),
            Err(e) => done.skipped.push((item.feature, e.to_string())),
        }
    }
    doc.end_gesture();
    done
}

/// The objects a plan creates, for selecting them afterwards.
pub fn created_refs(plan: &ImportPlan, target: Target) -> Vec<ObjRef> {
    plan.items
        .iter()
        .map(|i| match target {
            Target::RainGage => ObjRef::Gage(i.name.clone()),
            Target::Conduit => ObjRef::Link(i.name.clone()),
            Target::Subcatchment => ObjRef::Subcatchment(i.name.clone()),
            _ => ObjRef::Node(i.name.clone()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Ground elevations from a DEM
// ---------------------------------------------------------------------------

/// One node's before/after for "Set Ground From DEM".
#[derive(Clone, Debug, PartialEq)]
pub struct GroundRow {
    pub name: String,
    pub section: &'static str,
    pub invert: Option<f64>,
    pub ground: Option<f64>,
    pub old_max_depth: Option<f64>,
    pub new_max_depth: Option<f64>,
    pub note: String,
}

/// Sample the DEM at each named node and compute the `MaxDepth` that puts
/// the rim at ground: `MaxDepth = ground − invert`. `dem_to_model` scales
/// DEM values into the model's elevation unit (1 when they agree);
/// `dem_xy` maps a model coordinate into the DEM's grid. Outfalls have no
/// depth column and are listed with a note only.
pub fn ground_rows(
    doc: &InpDoc,
    names: &[String],
    sample: &dyn Fn(f64, f64) -> Option<f64>,
    dem_to_model: f64,
) -> Vec<GroundRow> {
    let mut out = Vec::new();
    for name in names {
        let Some(kind) = build::node_type_of(doc, name) else { continue };
        let section = kind.section();
        let invert = doc
            .field(section, name, "Elevation")
            .and_then(|s| s.parse::<f64>().ok());
        let ground = doc
            .coordinates(name)
            .and_then(|(x, y)| sample(x, y))
            .map(|g| g * dem_to_model);
        let old = if kind == NodeType::Outfall {
            None
        } else {
            doc.field(section, name, "MaxDepth")
                .and_then(|s| s.parse::<f64>().ok())
        };
        let (new, note) = match (kind, invert, ground) {
            (NodeType::Outfall, _, Some(g)) => (None, format!("outfall: ground {} noted, no depth column", format_number(g))),
            (_, _, None) => (None, "node is outside the DEM or on no-data".into()),
            (_, None, Some(_)) => (None, "no invert elevation".into()),
            (_, Some(inv), Some(g)) if g < inv => (Some(0.0), format!("ground {} is below the invert", format_number(g))),
            (_, Some(inv), Some(g)) => (Some(((g - inv) * 1000.0).round() / 1000.0), String::new()),
        };
        out.push(GroundRow {
            name: name.clone(),
            section,
            invert,
            ground,
            old_max_depth: old,
            new_max_depth: new,
            note,
        });
    }
    out
}

/// The commands that write the new depths (rows with a change only).
pub fn ground_commands(rows: &[GroundRow]) -> Vec<Command> {
    rows.iter()
        .filter_map(|r| {
            let new = r.new_max_depth?;
            if r.old_max_depth.is_some_and(|o| (o - new).abs() < 1e-9) {
                return None;
            }
            Some(Command::SetField {
                section: r.section.to_string(),
                name: r.name.clone(),
                field: "MaxDepth".into(),
                value: format_number(new),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gis::vector::{Feature, Field, FieldKind};

    fn points_layer() -> Layer {
        let mut l = Layer::new(
            "mh",
            vec![
                Field::new("Name", FieldKind::Text),
                Field::new("Invert", FieldKind::Number),
                Field::new("MaxDepth", FieldKind::Number),
            ],
        );
        for (n, x, y, inv, d) in [("MH-1", 100.0, 100.0, 90.5, 10.0), ("MH-2", 300.0, 100.0, 88.0, 12.5), ("MH-1", 500.0, 100.0, 85.0, 9.0)] {
            l.features.push(Feature::new(
                Some(Geometry::Point(x, y)),
                vec![FieldValue::Text(n.into()), FieldValue::Number(inv), FieldValue::Number(d)],
            ));
        }
        l.features.push(Feature::new(None, vec![FieldValue::Null, FieldValue::Null, FieldValue::Null]));
        l
    }

    fn doc() -> InpDoc {
        InpDoc::parse(&build::new_model_text())
    }

    #[test]
    fn points_become_junctions_with_mapped_fields_and_unique_names() {
        let mut d = doc();
        let layer = points_layer();
        let opts = ImportOptions::new(Target::Junction).auto_map(&layer);
        assert_eq!(opts.name_field, Some(0));
        assert!(opts.fields.iter().any(|m| m.column == "Elevation" && m.source == Source::Field(1)));
        assert!(opts.fields.iter().any(|m| m.column == "MaxDepth" && m.source == Source::Field(2)));
        let p = plan(&d, &layer, &opts, UnitFactors::from_doc(&d, None)).unwrap();
        assert_eq!(p.items.len(), 3);
        assert_eq!(p.skipped, vec![(3, "no geometry".to_string())]);
        // The duplicate MH-1 counts up past the taken MH-2 to MH-3.
        let names: Vec<&str> = p.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["MH-1", "MH-2", "MH-3"]);
        let before = d.undo_depth();
        let done = apply_plan(&mut d, &p);
        assert_eq!(done.items.len(), 3);
        assert_eq!(d.undo_depth(), before + 1, "one undo step");
        assert_eq!(d.field("JUNCTIONS", "MH-1", "Elevation"), Some("90.5"));
        assert_eq!(d.field("JUNCTIONS", "MH-1", "MaxDepth"), Some("10"));
        assert_eq!(d.field("JUNCTIONS", "MH-2", "MaxDepth"), Some("12.5"));
        assert_eq!(d.coordinates("MH-2"), Some((300.0, 100.0)));
        assert!(d.undo());
        assert!(!d.contains("JUNCTIONS", "MH-1"));
    }

    #[test]
    fn lines_become_conduits_snapping_to_nodes_or_creating_them() {
        let mut d = doc();
        let layer = points_layer();
        let opts = ImportOptions::new(Target::Junction).auto_map(&layer);
        let p = plan(&d, &layer, &opts, UnitFactors::from_doc(&d, None)).unwrap();
        apply_plan(&mut d, &p);
        d.apply(Command::SetOption { section: "MAP".into(), key: "Units".into(), value: "Feet".into() }).unwrap();

        let mut lines = Layer::new("pipes", vec![Field::new("ID", FieldKind::Text), Field::new("DIAM", FieldKind::Number), Field::new("n", FieldKind::Number)]);
        lines.features.push(Feature::new(
            Some(Geometry::LineString(vec![(100.5, 100.0), (200.0, 150.0), (299.6, 100.0)])),
            vec![FieldValue::Text("P1".into()), FieldValue::Number(2.5), FieldValue::Number(0.013)],
        ));
        // Second line ends far from any node: a junction is created.
        lines.features.push(Feature::new(
            Some(Geometry::LineString(vec![(300.0, 100.0), (300.0, 400.0)])),
            vec![FieldValue::Text("P2".into()), FieldValue::Number(1.0), FieldValue::Null],
        ));
        // Third snaps both ends to the same node: skipped.
        lines.features.push(Feature::new(
            Some(Geometry::LineString(vec![(100.0, 100.0), (100.2, 100.2)])),
            vec![FieldValue::Text("P3".into()), FieldValue::Null, FieldValue::Null],
        ));
        let mut opts = ImportOptions::new(Target::Conduit).auto_map(&lines);
        opts.name_field = Some(0);
        opts.snap_tolerance = 1.0;
        for m in &mut opts.fields {
            if m.column == "Geom1" {
                m.source = Source::Field(1);
            }
            if m.column == "Roughness" {
                m.source = Source::Field(2);
            }
            if m.column == "Shape" {
                m.source = Source::Constant("CIRCULAR".into());
            }
        }
        let units = UnitFactors::from_doc(&d, None);
        assert_eq!(units.length(), Some(1.0));
        let p = plan(&d, &lines, &opts, units).unwrap();
        assert_eq!(p.items.len(), 2, "{:?}", p.skipped);
        assert_eq!(p.skipped.len(), 1);
        assert!(p.skipped[0].1.contains("both ends snap"));
        assert_eq!(p.items[1].extra_nodes.len(), 1);
        assert_eq!(p.summary(Target::Conduit), "2 conduit(s) + 1 junction(s) at conduit ends, 1 skipped");
        apply_plan(&mut d, &p);
        assert_eq!(build::link_ends(&d, "P1"), Some(("MH-1".into(), "MH-2".into())));
        assert_eq!(d.vertices("P1"), vec![(200.0, 150.0)]);
        let len: f64 = d.field("CONDUITS", "P1", "Length").unwrap().parse().unwrap();
        let expect = ((99.5f64).powi(2) + 50f64.powi(2)).sqrt() + ((99.6f64).powi(2) + 50f64.powi(2)).sqrt();
        assert!((len - expect).abs() < 0.01, "{len} vs {expect}");
        assert_eq!(d.field("CONDUITS", "P1", "Roughness"), Some("0.013"));
        assert_eq!(d.field("XSECTIONS", "P1", "Geom1"), Some("2.5"));
        assert_eq!(d.field("XSECTIONS", "P1", "Shape"), Some("CIRCULAR"));
        // P2's far end got a new junction named after the J prefix.
        let (_, to) = build::link_ends(&d, "P2").unwrap();
        assert_eq!(to, "J1");
        assert_eq!(d.coordinates("J1"), Some((300.0, 400.0)));
        assert_eq!(d.field("CONDUITS", "P2", "Roughness"), Some("0.01"), "null keeps the default");
        // With creation off, P2 is skipped instead.
        let mut opts2 = opts.clone();
        opts2.create_end_nodes = false;
        let d2 = doc();
        let p2 = plan(&d2, &lines, &opts2, units).unwrap();
        assert!(p2.items.is_empty());
        assert!(p2.skipped.iter().all(|(_, r)| r.contains("no node within")), "{:?}", p2.skipped);
    }

    #[test]
    fn polygons_become_subcatchments_with_area_in_acres_and_nearest_outlet() {
        let mut d = doc();
        d.apply(Command::SetOption { section: "MAP".into(), key: "Units".into(), value: "Feet".into() }).unwrap();
        let node = build::new_node(&d, NodeType::Outfall, 0.0, 0.0);
        d.apply(node.command).unwrap();
        let mut polys = Layer::new("basins", vec![Field::new("NAME", FieldKind::Text), Field::new("IMPERV", FieldKind::Number)]);
        // A 660 ft x 660 ft square = 435600 sq ft = 10 acres.
        polys.features.push(Feature::new(
            Some(Geometry::Polygon(vec![vec![(0.0, 0.0), (660.0, 0.0), (660.0, 660.0), (0.0, 660.0), (0.0, 0.0)]])),
            vec![FieldValue::Text("Basin A".into()), FieldValue::Number(45.0)],
        ));
        polys.features.push(Feature::new(Some(Geometry::LineString(vec![(0.0, 0.0), (1.0, 1.0)])), vec![FieldValue::Null, FieldValue::Null]));
        let mut opts = ImportOptions::new(Target::Subcatchment).auto_map(&polys);
        for m in &mut opts.fields {
            if m.column == "PctImperv" {
                m.source = Source::Field(1);
            }
        }
        let units = UnitFactors::from_doc(&d, None);
        assert!(units.us);
        let p = plan(&d, &polys, &opts, units).unwrap();
        assert_eq!(p.items.len(), 1);
        assert!(p.skipped[0].1.contains("lines geometry"));
        apply_plan(&mut d, &p);
        let name = "Basin_A";
        assert!(d.contains("SUBCATCHMENTS", name), "{}", d);
        let area: f64 = d.field("SUBCATCHMENTS", name, "Area").unwrap().parse().unwrap();
        assert!((area - 10.0).abs() < 0.001, "{area}");
        assert_eq!(d.field("SUBCATCHMENTS", name, "Outlet"), Some("O1"));
        assert_eq!(d.field("SUBCATCHMENTS", name, "PctImperv"), Some("45"));
        assert_eq!(d.polygon(name).len(), 4);
        // Metric model: hectares from a CRS in metres.
        let mut si = doc();
        si.apply(Command::SetOption { section: "OPTIONS".into(), key: "FLOW_UNITS".into(), value: "CMS".into() }).unwrap();
        let crs = Crs::from_epsg(32617).unwrap();
        let u = UnitFactors::from_doc(&si, Some(&crs));
        assert!(!u.us);
        assert_eq!(u.area_unit(), "hectares");
        assert!((u.area().unwrap() - 1e-4).abs() < 1e-12);
        // A CRS in US feet on a US model: lengths are 1:1, areas per acre.
        let nc = Crs::from_epsg(2264).unwrap();
        let u = UnitFactors::from_doc(&d, Some(&nc));
        assert!((u.length().unwrap() - 1.0000020).abs() < 1e-6);
        let geo = UnitFactors::from_doc(&d, Some(&Crs::wgs84()));
        assert_eq!(geo.area(), None);
    }

    #[test]
    fn reprojection_before_mapping_moves_the_points() {
        let d = doc();
        let mut l = Layer::new("g", vec![]);
        l.features.push(Feature::new(Some(Geometry::Point(-80.8431, 35.2271)), vec![]));
        let mut opts = ImportOptions::new(Target::Junction);
        opts.reproject = Some((Crs::wgs84(), Crs::from_epsg(32617).unwrap()));
        let p = plan(&d, &l, &opts, UnitFactors::from_doc(&d, None)).unwrap();
        match &p.items[0].command {
            Command::Batch(cmds) => {
                let mv = cmds.iter().find_map(|c| match c {
                    Command::Batch(inner) => inner.iter().find_map(|c| match c {
                        Command::MoveNode { x, y, .. } => Some((*x, *y)),
                        _ => None,
                    }),
                    _ => None,
                });
                let (x, y) = mv.unwrap();
                assert!((x - 514277.72).abs() < 0.05 && (y - 3898239.34).abs() < 0.05, "{x} {y}");
            }
            other => panic!("{other:?}"),
        }
        let mut bad = opts.clone();
        bad.reproject = Some((Crs::nad27(), Crs::wgs84()));
        assert!(plan(&d, &l, &bad, UnitFactors::from_doc(&d, None)).is_err());
    }

    #[test]
    fn ground_rows_write_rim_depths_as_one_step() {
        let mut d = doc();
        let j = build::new_node(&d, NodeType::Junction, 10.0, 10.0);
        d.apply(j.command).unwrap();
        d.apply(Command::SetField { section: "JUNCTIONS".into(), name: "J1".into(), field: "Elevation".into(), value: "100".into() }).unwrap();
        let o = build::new_node(&d, NodeType::Outfall, 20.0, 20.0);
        d.apply(o.command).unwrap();
        let k = build::new_node(&d, NodeType::Junction, 500.0, 500.0);
        d.apply(k.command).unwrap();
        let sample = |x: f64, _y: f64| if x < 100.0 { Some(112.25) } else { None };
        let rows = ground_rows(&d, &["J1".into(), "O1".into(), "J2".into()], &sample, 1.0);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].new_max_depth, Some(12.25));
        assert_eq!(rows[1].new_max_depth, None);
        assert!(rows[1].note.contains("outfall"));
        assert!(rows[2].note.contains("outside"));
        let cmds = ground_commands(&rows);
        assert_eq!(cmds.len(), 1);
        let before = d.undo_depth();
        d.begin_gesture();
        for c in cmds {
            d.apply(c).unwrap();
        }
        d.end_gesture();
        assert_eq!(d.undo_depth(), before + 1);
        assert_eq!(d.field("JUNCTIONS", "J1", "MaxDepth"), Some("12.25"));
        // Ground below invert clamps to zero and says so.
        let low = |_: f64, _: f64| Some(95.0);
        let rows = ground_rows(&d, &["J1".into()], &low, 1.0);
        assert_eq!(rows[0].new_max_depth, Some(0.0));
        assert!(rows[0].note.contains("below"));
    }
}
