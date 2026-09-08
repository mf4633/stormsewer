// SPDX-License-Identifier: GPL-3.0-or-later

//! LandXML 1.2 pipe-network import (Civil 3D / InfraModel compatible subset).

use crate::network::NodeKind;
use quick_xml::events::Event;
use quick_xml::Reader;
/// Linear units used in the source document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinearUnit {
    #[default]
    Foot,
    Meter,
}

/// Diameter units for circular pipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiameterUnit {
    #[default]
    Inch,
    Foot,
    Millimeter,
    Meter,
}

/// Parsed LandXML document (one or more pipe networks).
#[derive(Clone, Debug, Default)]
pub struct LandXmlDocument {
    pub linear_unit: LinearUnit,
    pub diameter_unit: DiameterUnit,
    pub area_unit: AreaUnit,
    pub networks: Vec<LandXmlNetwork>,
}

/// A single pipe network from LandXML.
#[derive(Clone, Debug, Default)]
pub struct LandXmlNetwork {
    pub name: String,
    pub structures: Vec<LandXmlStruct>,
    pub pipes: Vec<LandXmlPipe>,
}

/// Structure (manhole / inlet / outfall) from LandXML.
#[derive(Clone, Debug)]
pub struct LandXmlStruct {
    pub name: String,
    pub kind: NodeKind,
    /// Easting / X (ft).
    pub x: f64,
    /// Northing / Y (ft).
    pub y: f64,
    /// Structure (outlet) invert, ft.
    pub invert: f64,
    pub rim: f64,
    /// `elevSump` attribute (Civil 3D), ft.
    pub sump: Option<f64>,
    pub area_ac: f64,
    pub c: f64,
    /// Inlet time from the outgoing pipe's `<PipeFlow timeInlet>` (min; 0 = unset).
    pub tc_inlet: f64,
    /// `role` attribute (rare) and `desc` (Civil 3D part description).
    pub role: String,
    pub desc: String,
    /// Structure geometry element name: CircStruct / RectStruct / InletStruct / OutletStruct.
    pub geometry: String,
    /// Structure inside diameter or largest plan dimension, ft (0 = unknown).
    pub diameter_ft: f64,
    /// Per-pipe `<Invert elev flowDir refPipe>` records: (pipe name, is outflow, elev ft).
    pub pipe_inverts: Vec<(String, bool, f64)>,
}

/// Pipe link from LandXML.
#[derive(Clone, Debug)]
pub struct LandXmlPipe {
    pub name: String,
    pub from: String,
    pub to: String,
    /// Internal diameter (ft), or equal-area diameter for box/elliptical.
    pub diameter_ft: f64,
    pub n: f64,
    /// `length` attribute (ft) when the producer wrote one.
    pub length_ft: Option<f64>,
    /// `circular`, `box`, or `elliptical`.
    pub shape: String,
    pub rise_ft: f64,
    pub span_ft: f64,
    /// `material` attribute / pipe `desc` (used to pick Manning's n).
    pub material: String,
    /// `<PipeFlow>` catchment data attached to the pipe (acres, C, minutes).
    pub area_ac: f64,
    pub c: f64,
    pub tc_inlet: f64,
}

/// Area units used in the source document (for `<PipeFlow areaCatchment>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AreaUnit {
    #[default]
    SquareFoot,
    Acre,
    SquareMeter,
    Hectare,
}

impl LandXmlDocument {
    /// First network, or an error when the file contains none.
    pub fn primary_network(&self) -> Result<&LandXmlNetwork, String> {
        self.networks
            .first()
            .ok_or_else(|| "LandXML: no <PipeNetwork> found".into())
    }
}

/// Parse a LandXML document string into structures and pipes.
pub fn parse_landxml(xml: &str) -> Result<LandXmlDocument, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut doc = LandXmlDocument::default();
    let mut buf = Vec::new();

    let mut in_imperial = false;
    let mut in_metric = false;

    let mut cur_network: Option<LandXmlNetwork> = None;
    let mut in_structs = false;
    let mut in_pipes = false;

    let mut cur_struct: Option<LandXmlStruct> = None;
    let mut cur_pipe: Option<LandXmlPipe> = None;
    let mut text_buf = String::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            // Civil 3D writes most pipe-network data as ATTRIBUTES, on elements
            // that may be self-closing (`<Invert .../>`) or open-and-close with
            // no text (`<Invert ...></Invert>`), so Start and Empty are handled
            // by the same code path.
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let is_start = matches!(event, Ok(Event::Start(_)));
                let raw = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let name = local_name(&raw);
                if is_start {
                    text_buf.clear();
                }
                let num = |key: &str| attr_value(e, key).and_then(|s| s.trim().parse::<f64>().ok());

                match name.as_str() {
                    "Imperial" => {
                        in_imperial = is_start;
                        if let Some(u) = attr_value(e, "linearUnit") {
                            doc.linear_unit = parse_linear_unit(&u);
                        }
                        if let Some(u) = attr_value(e, "diameterUnit") {
                            doc.diameter_unit = parse_diameter_unit(&u);
                        }
                        if let Some(u) = attr_value(e, "areaUnit") {
                            doc.area_unit = parse_area_unit(&u);
                        }
                    }
                    "Metric" => {
                        in_metric = is_start;
                        doc.linear_unit = LinearUnit::Meter;
                        doc.diameter_unit = attr_value(e, "diameterUnit")
                            .map(|u| parse_diameter_unit(&u))
                            .unwrap_or(DiameterUnit::Millimeter);
                        doc.area_unit = attr_value(e, "areaUnit")
                            .map(|u| parse_area_unit(&u))
                            .unwrap_or(AreaUnit::SquareMeter);
                    }
                    "PipeNetwork" => {
                        let net_name = attr_value(e, "name").unwrap_or_else(|| "Network".into());
                        cur_network = Some(LandXmlNetwork {
                            name: net_name,
                            ..Default::default()
                        });
                    }
                    "Structs" if cur_network.is_some() => in_structs = true,
                    "Pipes" if cur_network.is_some() => in_pipes = true,
                    "Struct" if in_structs => {
                        let sname = attr_value(e, "name")
                            .or_else(|| attr_value(e, "id"))
                            .unwrap_or_else(|| {
                                format!(
                                    "S{}",
                                    cur_network
                                        .as_ref()
                                        .map(|n| n.structures.len())
                                        .unwrap_or(0)
                                        + 1
                                )
                            });
                        let role = attr_value(e, "role").unwrap_or_default();
                        let desc = attr_value(e, "desc").unwrap_or_default();
                        let mut s = LandXmlStruct {
                            name: sname,
                            role,
                            desc,
                            kind: NodeKind::Junction,
                            x: 0.0,
                            y: 0.0,
                            invert: 0.0,
                            rim: 0.0,
                            sump: None,
                            area_ac: 0.0,
                            c: 0.7,
                            tc_inlet: 0.0,
                            pipe_inverts: Vec::new(),
                            geometry: String::new(),
                            diameter_ft: 0.0,
                        };
                        if let Some(v) = num("elevRim") {
                            s.rim = to_linear_ft(v, doc.linear_unit);
                        }
                        if let Some(v) = num("elevSump") {
                            s.sump = Some(to_linear_ft(v, doc.linear_unit));
                        }
                        cur_struct = Some(s);
                    }
                    // Structure geometry (LandXML 1.2): the element name tells us
                    // what the structure IS, which Civil 3D never states elsewhere.
                    "CircStruct" | "RectStruct" | "InletStruct" | "OutletStruct" | "EggStruct"
                        if cur_struct.is_some() =>
                    {
                        if let Some(s) = cur_struct.as_mut() {
                            s.geometry = name.clone();
                            if name == "CircStruct" {
                                if let Some(d) = num("diameter") {
                                    s.diameter_ft = to_linear_ft(d, doc.linear_unit);
                                }
                            } else if name == "RectStruct" {
                                if let (Some(l), Some(w)) = (num("length"), num("width")) {
                                    s.diameter_ft = to_linear_ft(l.max(w), doc.linear_unit);
                                }
                            }
                        }
                    }
                    "Invert" if cur_struct.is_some() => {
                        if let (Some(s), Some(el)) = (cur_struct.as_mut(), num("elev")) {
                            let out = attr_value(e, "flowDir")
                                .map(|d| d.eq_ignore_ascii_case("out"))
                                .unwrap_or(false);
                            let pipe = attr_value(e, "refPipe").unwrap_or_default();
                            s.pipe_inverts
                                .push((pipe, out, to_linear_ft(el, doc.linear_unit)));
                        }
                    }
                    "Pipe" if in_pipes => {
                        let pname = attr_value(e, "name")
                            .or_else(|| attr_value(e, "id"))
                            .unwrap_or_else(|| {
                                format!(
                                    "P{}",
                                    cur_network.as_ref().map(|n| n.pipes.len()).unwrap_or(0) + 1
                                )
                            });
                        // Civil 3D (LandXML 1.2) links pipes via refStart / refEnd
                        // ATTRIBUTES on <Pipe>. Child StartStruct/EndStruct elements
                        // (handled in the End branch) are only emitted by some other
                        // producers; without reading the attributes, real Civil 3D
                        // files import with every pipe dangling and get discarded.
                        cur_pipe = Some(LandXmlPipe {
                            name: pname,
                            from: attr_value(e, "refStart").unwrap_or_default(),
                            to: attr_value(e, "refEnd").unwrap_or_default(),
                            diameter_ft: 1.0,
                            n: 0.013,
                            length_ft: num("length")
                                .filter(|v| *v > 0.0)
                                .map(|v| to_linear_ft(v, doc.linear_unit)),
                            shape: "circular".into(),
                            rise_ft: 0.0,
                            span_ft: 0.0,
                            material: attr_value(e, "desc").unwrap_or_default(),
                            area_ac: 0.0,
                            c: 0.0,
                            tc_inlet: 0.0,
                        });
                    }
                    "CircPipe" if cur_pipe.is_some() => {
                        if let Some(p) = cur_pipe.as_mut() {
                            if let Some(d) = num("diameter") {
                                p.diameter_ft = to_diameter_ft(d, doc.diameter_unit);
                            }
                            if let Some(m) = attr_value(e, "material") {
                                p.material = m;
                            }
                        }
                    }
                    "RectPipe" | "ElliPipe" | "EggPipe" if cur_pipe.is_some() => {
                        if let Some(p) = cur_pipe.as_mut() {
                            // RectPipe: width × height; ElliPipe/EggPipe: span × rise.
                            let span = num("width").or_else(|| num("span"));
                            let rise = num("height").or_else(|| num("rise"));
                            if let (Some(sp), Some(ri)) = (span, rise) {
                                p.span_ft = to_diameter_ft(sp, doc.diameter_unit);
                                p.rise_ft = to_diameter_ft(ri, doc.diameter_unit);
                                p.shape = if name == "RectPipe" {
                                    "box"
                                } else {
                                    "elliptical"
                                }
                                .into();
                                p.diameter_ft = (p.span_ft * p.rise_ft).sqrt();
                            }
                            if let Some(m) = attr_value(e, "material") {
                                p.material = m;
                            }
                        }
                    }
                    "PipeFlow" if cur_pipe.is_some() => {
                        if let Some(p) = cur_pipe.as_mut() {
                            if let Some(a) = num("areaCatchment") {
                                p.area_ac = to_acres(a, doc.area_unit);
                            }
                            if let Some(c) = num("runoffCoeff") {
                                p.c = c;
                            }
                            if let Some(t) = num("timeInlet") {
                                p.tc_inlet = t;
                            }
                        }
                    }
                    "Center" if cur_struct.is_some() => {
                        if let (Some(n), Some(ea)) = (num("north"), num("east")) {
                            if let Some(s) = cur_struct.as_mut() {
                                s.y = to_linear_ft(n, doc.linear_unit);
                                s.x = to_linear_ft(ea, doc.linear_unit);
                                if let Some(el) = num("elev") {
                                    s.rim = to_linear_ft(el, doc.linear_unit);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                text_buf.push_str(&t.unescape().map_err(|e| e.to_string())?);
            }
            Ok(Event::End(e)) => {
                let raw = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let name = local_name(&raw);
                let text = text_buf.trim().to_string();

                if !text.is_empty() {
                    match name.as_str() {
                        "linearUnit" if in_imperial => doc.linear_unit = parse_linear_unit(&text),
                        "diameterUnit" if in_imperial => {
                            doc.diameter_unit = parse_diameter_unit(&text)
                        }
                        "linearUnit" if in_metric => doc.linear_unit = LinearUnit::Meter,
                        "diameterUnit" if in_metric => {
                            doc.diameter_unit = parse_diameter_unit(&text)
                        }
                        "Center" => {
                            if let (Some(s), Some((a, b, c))) =
                                (cur_struct.as_mut(), parse_coords(&text))
                            {
                                // LandXML point content is "northing easting [elev]".
                                // First → y (north), second → x (east), matching the
                                // attribute path above; the old code had them swapped.
                                s.y = to_linear_ft(a, doc.linear_unit);
                                s.x = to_linear_ft(b, doc.linear_unit);
                                if c.abs() > 1e-6 && s.rim == 0.0 {
                                    s.rim = to_linear_ft(c, doc.linear_unit);
                                }
                            }
                        }
                        "Invert" | "InvertElev" => {
                            if let (Some(s), Ok(v)) = (cur_struct.as_mut(), text.parse::<f64>()) {
                                s.invert = to_linear_ft(v, doc.linear_unit);
                            }
                        }
                        "ElevRim" | "Rim" | "RimElev" => {
                            if let (Some(s), Ok(v)) = (cur_struct.as_mut(), text.parse::<f64>()) {
                                s.rim = to_linear_ft(v, doc.linear_unit);
                            }
                        }
                        "StartStruct" | "RefStart" | "BegStruct" => {
                            if let Some(p) = cur_pipe.as_mut() {
                                p.from = text;
                            }
                        }
                        "EndStruct" | "RefEnd" | "EndStructRef" => {
                            if let Some(p) = cur_pipe.as_mut() {
                                p.to = text;
                            }
                        }
                        "CircPipe" => {
                            if let (Some(p), Ok(d)) = (cur_pipe.as_mut(), text.parse::<f64>()) {
                                p.diameter_ft = to_diameter_ft(d, doc.diameter_unit);
                            }
                        }
                        _ => {}
                    }
                }

                match name.as_str() {
                    "Imperial" => in_imperial = false,
                    "Metric" => in_metric = false,
                    "Struct" => {
                        if let Some(s) = cur_struct.take() {
                            if let Some(net) = cur_network.as_mut() {
                                net.structures.push(s);
                            }
                        }
                    }
                    "Pipe" => {
                        if let Some(p) = cur_pipe.take() {
                            if !p.from.is_empty() && !p.to.is_empty() {
                                if let Some(net) = cur_network.as_mut() {
                                    net.pipes.push(p);
                                }
                            }
                        }
                    }
                    "Structs" => in_structs = false,
                    "Pipes" => in_pipes = false,
                    "PipeNetwork" => {
                        if let Some(net) = cur_network.take() {
                            if !net.structures.is_empty() {
                                doc.networks.push(net);
                            }
                        }
                    }
                    _ => {}
                }
                text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(format!(
                    "LandXML parse error at {}: {e}",
                    reader.error_position()
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    if doc.networks.is_empty() {
        return Err("LandXML: no pipe network with structures found".into());
    }

    for net in &mut doc.networks {
        resolve_structures(net);
        drop_dangling_pipes(net);
    }

    Ok(doc)
}

/// Settle each structure's invert, rim, kind, and drainage data from what the
/// producer actually wrote: Civil 3D gives per-pipe `<Invert>` records and
/// `elevSump`/`elevRim` attributes rather than a single invert, never states a
/// role, and hangs catchment data on the pipe's `<PipeFlow>`.
fn resolve_structures(net: &mut LandXmlNetwork) {
    let outgoing_flow: std::collections::HashMap<&str, (f64, f64, f64)> = net
        .pipes
        .iter()
        .map(|p| (p.from.as_str(), (p.area_ac, p.c, p.tc_inlet)))
        .collect();

    for s in &mut net.structures {
        // Invert: explicit element text wins, then the sump, then the lowest
        // connected pipe invert (the outlet, normally).
        if s.invert == 0.0 {
            let lowest = s
                .pipe_inverts
                .iter()
                .map(|(_, _, el)| *el)
                .fold(f64::INFINITY, f64::min);
            s.invert = match s.sump {
                Some(v) => v,
                None if lowest.is_finite() => lowest,
                None => 0.0,
            };
        }
        if s.invert == 0.0 && s.rim != 0.0 {
            s.invert = s.rim - 5.0;
        }
        let n_out = s.pipe_inverts.iter().filter(|(_, out, _)| *out).count();
        let n_in = s.pipe_inverts.iter().filter(|(_, out, _)| !*out).count();
        let (area, c, tc) = outgoing_flow
            .get(s.name.as_str())
            .copied()
            .unwrap_or((0.0, 0.0, 0.0));

        s.kind = infer_kind(&s.name, &s.role, &s.desc, &s.geometry, n_in, n_out);
        if s.rim <= s.invert {
            // No rim written. An outfall has none (Civil 3D's null structure
            // says elevRim="0"); a structure gets a nominal 5 ft depth.
            s.rim = if s.kind == NodeKind::Outfall {
                s.invert
            } else {
                s.invert + 5.0
            };
        }
        if s.kind != NodeKind::Outfall && area > 0.0 {
            s.kind = NodeKind::Inlet;
        }
        if s.kind == NodeKind::Inlet || s.kind == NodeKind::Junction {
            if area > 0.0 {
                s.area_ac = area;
            }
            if c > 0.0 {
                s.c = c;
            }
            if tc > 0.0 {
                s.tc_inlet = tc;
            }
        }
    }
}

fn drop_dangling_pipes(net: &mut LandXmlNetwork) {
    let names: std::collections::HashSet<_> =
        net.structures.iter().map(|s| s.name.as_str()).collect();
    net.pipes
        .retain(|p| names.contains(p.from.as_str()) && names.contains(p.to.as_str()));
}

fn local_name(tag: &str) -> String {
    tag.rsplit(':').next().unwrap_or(tag).to_string()
}

fn attr_value(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key.as_bytes())
        .and_then(|a| String::from_utf8(a.value.into_owned()).ok())
}

fn parse_coords(text: &str) -> Option<(f64, f64, f64)> {
    let nums: Vec<f64> = text
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    match nums.len() {
        0 => None,
        1 => Some((nums[0], 0.0, 0.0)),
        2 => Some((nums[0], nums[1], 0.0)),
        _ => Some((nums[0], nums[1], nums[2])),
    }
}

fn parse_linear_unit(s: &str) -> LinearUnit {
    let l = s.to_ascii_lowercase();
    if l.contains("meter") || l == "m" {
        LinearUnit::Meter
    } else {
        LinearUnit::Foot
    }
}

fn parse_diameter_unit(s: &str) -> DiameterUnit {
    let l = s.to_ascii_lowercase();
    if l.contains("milli") {
        DiameterUnit::Millimeter
    } else if l.contains("meter") || l == "m" {
        DiameterUnit::Meter
    } else if l.contains("foot") || l == "ft" {
        DiameterUnit::Foot
    } else {
        DiameterUnit::Inch
    }
}

fn to_linear_ft(v: f64, unit: LinearUnit) -> f64 {
    match unit {
        LinearUnit::Foot => v,
        LinearUnit::Meter => v * 3.280_839_895,
    }
}

fn to_diameter_ft(v: f64, unit: DiameterUnit) -> f64 {
    match unit {
        DiameterUnit::Inch => v / 12.0,
        DiameterUnit::Foot => v,
        DiameterUnit::Millimeter => v / 304.8,
        DiameterUnit::Meter => v * 3.280_839_895,
    }
}

fn parse_area_unit(s: &str) -> AreaUnit {
    let l = s.to_ascii_lowercase();
    if l.contains("acre") {
        AreaUnit::Acre
    } else if l.contains("hectare") {
        AreaUnit::Hectare
    } else if l.contains("meter") || l.contains("metre") {
        AreaUnit::SquareMeter
    } else {
        AreaUnit::SquareFoot
    }
}

fn to_acres(v: f64, unit: AreaUnit) -> f64 {
    match unit {
        AreaUnit::SquareFoot => v / 43_560.0,
        AreaUnit::Acre => v,
        AreaUnit::SquareMeter => v / 4_046.856_422_4,
        AreaUnit::Hectare => v * 2.471_053_814_67,
    }
}

/// Manning's n from the LandXML material / description string.
fn n_from_material(material: &str) -> f64 {
    let m = material.to_ascii_lowercase();
    if m.contains("hdpe")
        || m.contains("pvc")
        || m.contains("polyethylene")
        || m.contains("plastic")
    {
        0.012
    } else if m.contains("cmp") || m.contains("corrugated") {
        0.024
    } else if m.contains("ductile") || m.contains("steel") {
        0.012
    } else {
        0.013 // concrete and unknown
    }
}

/// Decide what a LandXML structure is. Civil 3D never writes a `role`; the
/// signal is the structure geometry element, the part description, the name,
/// and — most reliably — the flow directions of the pipes tied to it: a
/// structure that only receives is the end of the run (Civil 3D's "null
/// structure" outfall), one that only sends is a headwall/terminal inlet.
fn infer_kind(
    name: &str,
    role: &str,
    desc: &str,
    geometry: &str,
    n_in: usize,
    n_out: usize,
) -> NodeKind {
    let n = name.to_ascii_lowercase();
    let r = role.to_ascii_lowercase();
    let d = desc.to_ascii_lowercase();
    let g = geometry.to_ascii_lowercase();
    if r.contains("outfall")
        || n.contains("outfall")
        || n.starts_with("of")
        || n.contains("nullstruct")
        || d.contains("null structure")
        || g == "outletstruct"
        || (n_in > 0 && n_out == 0)
    {
        NodeKind::Outfall
    } else if r.contains("inlet")
        || g == "inletstruct"
        || n.contains("inlet")
        || n.starts_with("in")
        || n.starts_with("ai-")
        || n.starts_with("ci-")
        || n.starts_with("cb")
        || n.starts_with("di-")
        || n.starts_with("yi")
        || d.contains("inlet")
        || d.contains("catch basin")
        || d.contains("grate")
        || d.contains("frame")
        || d.contains("curb")
    {
        NodeKind::Inlet
    } else {
        NodeKind::Junction
    }
}

/// Import a LandXML file into a StormSewer [`Project`](crate::io::project::Project).
pub fn import_landxml(path: &std::path::Path) -> Result<crate::io::project::Project, String> {
    use crate::idf::IdfCurve;
    use crate::io::project::Project;
    use crate::network::AnalysisOptions;
    use std::fs;

    let xml =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = parse_landxml(&xml)?;
    let lx_net = doc.primary_network()?;
    let name = if lx_net.name.is_empty() {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("LandXML Import")
            .to_string()
    } else {
        lx_net.name.clone()
    };
    let network = network_from_landxml(lx_net)?;
    let idf = IdfCurve::new(60.0, 10.0, 0.8);
    let opts = AnalysisOptions::default();
    Ok(Project::from_network(&network, &name, &idf, &opts))
}

/// Export a StormSewer project to LandXML 1.2 (Civil 3D compatible subset).
pub fn export_landxml(
    project: &crate::io::project::Project,
    path: &std::path::Path,
) -> Result<(), String> {
    use std::fmt::Write as _;
    use std::fs;

    let mut xml = String::new();
    writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#).map_err(|e| e.to_string())?;
    writeln!(
        xml,
        r#"<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2" date="2026-06-26">"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(xml, "  <Units>").map_err(|e| e.to_string())?;
    writeln!(
        xml,
        r#"    <Imperial areaUnit="squareFoot" linearUnit="foot" diameterUnit="inch"/>"#
    )
    .map_err(|e| e.to_string())?;
    writeln!(xml, "  </Units>").map_err(|e| e.to_string())?;
    writeln!(xml, "  <PipeNetworks>").map_err(|e| e.to_string())?;
    writeln!(
        xml,
        r#"    <PipeNetwork name="{}">"#,
        escape_xml(&project.name)
    )
    .map_err(|e| e.to_string())?;
    writeln!(xml, "      <Structs>").map_err(|e| e.to_string())?;
    for node in &project.nodes {
        let role = match node.kind.as_str() {
            "inlet" => "inlet",
            "outfall" => "outfall",
            _ => "junction",
        };
        writeln!(
            xml,
            r#"        <Struct name="{}" role="{}" elevRim="{:.3}" elevSump="{:.3}">"#,
            escape_xml(&node.id),
            role,
            node.rim,
            node.invert
        )
        .map_err(|e| e.to_string())?;
        // LandXML point order is northing easting elevation → y x rim.
        writeln!(
            xml,
            "          <Center>{:.3} {:.3} {:.3}</Center>",
            node.y, node.x, node.rim
        )
        .map_err(|e| e.to_string())?;
        // One <Invert> per connected pipe, as Civil 3D writes it, carrying the
        // pipe's own end invert (a drop through the structure survives).
        for pipe in &project.pipes {
            if pipe.from == node.id {
                writeln!(
                    xml,
                    r#"          <Invert elev="{:.3}" flowDir="out" refPipe="{}"/>"#,
                    pipe.invert_up.unwrap_or(node.invert),
                    escape_xml(&pipe.id)
                )
                .map_err(|e| e.to_string())?;
            }
            if pipe.to == node.id {
                writeln!(
                    xml,
                    r#"          <Invert elev="{:.3}" flowDir="in" refPipe="{}"/>"#,
                    pipe.invert_dn.unwrap_or(node.invert),
                    escape_xml(&pipe.id)
                )
                .map_err(|e| e.to_string())?;
            }
        }
        writeln!(xml, "          <ElevRim>{:.3}</ElevRim>", node.rim).map_err(|e| e.to_string())?;
        writeln!(xml, "        </Struct>").map_err(|e| e.to_string())?;
    }
    writeln!(xml, "      </Structs>").map_err(|e| e.to_string())?;
    writeln!(xml, "      <Pipes>").map_err(|e| e.to_string())?;
    for pipe in &project.pipes {
        let dia_in = pipe.diameter * 12.0;
        // Connectivity via refStart / refEnd attributes — the form Civil 3D and
        // other LandXML consumers expect (our importer reads both these and the
        // legacy child StartStruct/EndStruct elements).
        writeln!(
            xml,
            r#"        <Pipe name="{}" refStart="{}" refEnd="{}" length="{:.3}">"#,
            escape_xml(&pipe.id),
            escape_xml(&pipe.from),
            escape_xml(&pipe.to),
            pipe.length
        )
        .map_err(|e| e.to_string())?;
        writeln!(xml, r#"          <CircPipe diameter="{:.1}"/>"#, dia_in)
            .map_err(|e| e.to_string())?;
        writeln!(xml, "        </Pipe>").map_err(|e| e.to_string())?;
    }
    writeln!(xml, "      </Pipes>").map_err(|e| e.to_string())?;
    writeln!(xml, "    </PipeNetwork>").map_err(|e| e.to_string())?;
    writeln!(xml, "  </PipeNetworks>").map_err(|e| e.to_string())?;
    writeln!(xml, "</LandXML>").map_err(|e| e.to_string())?;
    fs::write(path, xml).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A StormSewer node/pipe id from a Civil 3D name: drop the " (Network)"
/// suffix Civil 3D appends, "Pipe - (5)" → "P5", then keep `[A-Za-z0-9_-]`.
fn id_from_name(name: &str, network: &str) -> String {
    let mut base = name.trim().to_string();
    if !network.is_empty() {
        let suffix = format!(" ({network})");
        if let Some(stripped) = base.strip_suffix(&suffix) {
            base = stripped.trim().to_string();
        }
    }
    let lower = base.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("pipe") {
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return format!("P{digits}");
        }
    }
    if let Some(rest) = lower.strip_prefix("structure") {
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return format!("S{digits}");
        }
    }
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() {
        "N".into()
    } else {
        cleaned
    }
}

/// Convert a parsed LandXML network into a HydroComplete [`Network`](crate::network::Network).
pub fn network_from_landxml(net: &LandXmlNetwork) -> Result<crate::network::Network, String> {
    use crate::network::{Network, Node, Pipe};
    use std::collections::{HashMap, HashSet};

    if net.structures.is_empty() {
        return Err("LandXML network has no structures".into());
    }

    let mut used: HashSet<String> = HashSet::new();
    let mut unique = |wanted: String| -> String {
        let mut id = wanted.clone();
        let mut k = 2;
        while !used.insert(id.clone()) {
            id = format!("{wanted}_{k}");
            k += 1;
        }
        id
    };

    let mut id_of: HashMap<String, String> = HashMap::new();
    let mut nodes = Vec::with_capacity(net.structures.len());
    for s in &net.structures {
        let id = unique(id_from_name(&s.name, &net.name));
        id_of.insert(s.name.clone(), id.clone());
        let mut node = match s.kind {
            NodeKind::Inlet => Node::inlet(&id, s.invert, s.rim, s.area_ac, s.c),
            NodeKind::Junction => Node::junction(&id, s.invert, s.rim, s.area_ac, s.c),
            NodeKind::Outfall => Node::outfall(&id, s.invert, s.rim),
        }
        .at(s.x, s.y);
        if s.tc_inlet > 0.0 {
            node = node.with_tc_inlet(s.tc_inlet);
        }
        nodes.push(node);
    }

    let by_name: HashMap<&str, &LandXmlStruct> = net
        .structures
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();
    let pipe_invert_at = |struct_name: &str, pipe_name: &str, out: bool| -> Option<f64> {
        by_name.get(struct_name).and_then(|s| {
            s.pipe_inverts
                .iter()
                .find(|(p, o, _)| p == pipe_name && *o == out)
                .map(|(_, _, el)| *el)
        })
    };

    let mut pipe_ids: HashSet<String> = HashSet::new();
    let mut pipes = Vec::new();
    for (k, p) in net.pipes.iter().enumerate() {
        let Some(from_id) = id_of.get(&p.from) else {
            continue;
        };
        let Some(to_id) = id_of.get(&p.to) else {
            continue;
        };
        let length = p.length_ft.unwrap_or_else(|| {
            match (by_name.get(p.from.as_str()), by_name.get(p.to.as_str())) {
                (Some(a), Some(b)) => ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt(),
                _ => 100.0,
            }
        });
        let mut id = id_from_name(&p.name, &net.name);
        if id == "N" {
            id = format!("P{}", k + 1);
        }
        let mut cand = id.clone();
        let mut j = 2;
        while !pipe_ids.insert(cand.clone()) {
            cand = format!("{id}_{j}");
            j += 1;
        }
        let n = if p.material.is_empty() {
            p.n
        } else {
            n_from_material(&p.material)
        };
        let pipe = match p.shape.as_str() {
            "box" if p.rise_ft > 0.0 && p.span_ft > 0.0 => {
                Pipe::rectangular(&cand, from_id, to_id, length, p.rise_ft, p.span_ft, n)
            }
            "elliptical" if p.rise_ft > 0.0 && p.span_ft > 0.0 => {
                Pipe::elliptical(&cand, from_id, to_id, length, p.rise_ft, p.span_ft, n)
            }
            _ => Pipe::new(&cand, from_id, to_id, length, p.diameter_ft, n),
        };
        // Pin the pipe's own end inverts where Civil 3D wrote them and they
        // differ from the structure invert (a drop through the structure).
        let node_inv = |name: &str| by_name.get(name).map(|s| s.invert);
        let inv_up = pipe_invert_at(&p.from, &p.name, true)
            .filter(|v| node_inv(&p.from).is_some_and(|ni| (ni - v).abs() > 1e-6));
        let inv_dn = pipe_invert_at(&p.to, &p.name, false)
            .filter(|v| node_inv(&p.to).is_some_and(|ni| (ni - v).abs() > 1e-6));
        pipes.push(pipe.with_inverts(inv_up, inv_dn));
    }

    if pipes.is_empty() {
        return Err("LandXML network has no connected pipes".into());
    }

    Ok(Network { nodes, pipes })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2" date="2026-06-09">
  <Units>
    <Imperial areaUnit="squareFoot" linearUnit="foot" diameterUnit="inch"/>
  </Units>
  <PipeNetworks>
    <PipeNetwork name="Main">
      <Structs>
        <Struct name="IN1" role="inlet">
          <Center>0.0 0.0 110.0</Center>
          <Invert>104.0</Invert>
          <ElevRim>110.0</ElevRim>
        </Struct>
        <Struct name="OUT1" role="outfall">
          <Center>300.0 0.0 106.0</Center>
          <Invert>100.0</Invert>
          <ElevRim>106.0</ElevRim>
        </Struct>
      </Structs>
      <Pipes>
        <Pipe name="P1">
          <CircPipe diameter="18"/>
          <StartStruct>IN1</StartStruct>
          <EndStruct>OUT1</EndStruct>
        </Pipe>
      </Pipes>
    </PipeNetwork>
  </PipeNetworks>
</LandXML>"#;

    #[test]
    fn parses_sample_network() {
        let doc = parse_landxml(SAMPLE).expect("parse");
        let net = doc.primary_network().unwrap();
        assert_eq!(net.structures.len(), 2);
        assert_eq!(net.pipes.len(), 1);
        assert!((net.pipes[0].diameter_ft - 1.5).abs() < 1e-6);
    }

    #[test]
    fn builds_engine_network() {
        let doc = parse_landxml(SAMPLE).unwrap();
        let net = doc.primary_network().unwrap();
        let engine = network_from_landxml(net).unwrap();
        assert_eq!(engine.nodes.len(), 2);
        assert_eq!(engine.pipes.len(), 1);
        assert!((engine.pipes[0].length - 300.0).abs() < 1e-3);
    }
}
