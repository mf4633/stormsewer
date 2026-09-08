// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal ASCII DXF import/export for StormSewer plan geometry.
//!
//! Structures export as `CIRCLE` on layer `SS_STRUCTURES`; pipes as `LINE` on
//! `SS_PIPES`. Extended data uses applications `STORMSEWER_STRUCT` and
//! `STORMSEWER_PIPE` with typed records (group 1001/1000/1040).

use crate::io::project::{Project, ProjectCatchment, ProjectNode, ProjectPipe};
use crate::network::Network;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const LAYER_STRUCT: &str = "SS_STRUCTURES";
const LAYER_PIPE: &str = "SS_PIPES";
const LAYER_CATCH: &str = "SS_CATCHMENTS";
const APP_STRUCT: &str = "STORMSEWER_STRUCT";
const APP_PIPE: &str = "STORMSEWER_PIPE";
const APP_CATCH: &str = "STORMSEWER_CATCHMENT";

fn push_pair(out: &mut String, code: i32, value: &str) {
    out.push_str(&format!("{code}\n{value}\n"));
}

fn push_f64(out: &mut String, code: i32, value: f64) {
    push_pair(out, code, &format!("{value}"));
}

fn push_xdata_string(out: &mut String, value: &str) {
    push_pair(out, 1000, value);
}

fn push_xdata_real(out: &mut String, value: f64) {
    push_f64(out, 1040, value);
}

fn push_xdata_app(out: &mut String, app: &str) {
    push_pair(out, 1001, app);
}

/// Emit key/value XDATA records: string keys (1000) followed by string (1000) or real (1040) values.
fn push_xdata_kv_string(out: &mut String, key: &str, value: &str) {
    push_xdata_string(out, key);
    push_xdata_string(out, value);
}

fn push_xdata_kv_real(out: &mut String, key: &str, value: f64) {
    push_xdata_string(out, key);
    push_xdata_real(out, value);
}

fn push_struct_xdata(out: &mut String, n: &ProjectNode) {
    push_xdata_app(out, APP_STRUCT);
    push_xdata_kv_string(out, "kind", &n.kind);
    push_xdata_kv_string(out, "id", &n.id);
    push_xdata_kv_real(out, "invert", n.invert);
    push_xdata_kv_real(out, "rim", n.rim);
    push_xdata_kv_real(out, "area", n.area_ac);
    push_xdata_kv_real(out, "C", n.c);
    push_xdata_kv_real(out, "tc_inlet", n.tc_inlet);
}

fn push_pipe_xdata(out: &mut String, p: &ProjectPipe) {
    push_xdata_app(out, APP_PIPE);
    push_xdata_kv_real(out, "diameter", p.diameter);
    push_xdata_kv_real(out, "n", p.n);
    push_xdata_kv_string(out, "from_id", &p.from);
    push_xdata_kv_string(out, "to_id", &p.to);
    if let Some(v) = p.invert_up {
        push_xdata_kv_real(out, "invert_up", v);
    }
    if let Some(v) = p.invert_dn {
        push_xdata_kv_real(out, "invert_dn", v);
    }
}

fn push_catchment_xdata(out: &mut String, c: &ProjectCatchment) {
    push_xdata_app(out, APP_CATCH);
    push_xdata_kv_real(out, "c", c.c);
    push_xdata_kv_real(out, "flow_length", c.flow_length_ft);
    push_xdata_kv_real(out, "slope", c.slope);
    if let Some(ref inlet_id) = c.inlet_node_id {
        push_xdata_kv_string(out, "inlet_id", inlet_id);
    }
}

/// Export a project network to ASCII DXF (R12-compatible subset).
pub fn export_dxf(project: &Project, path: &Path) -> Result<(), String> {
    let mut s = String::new();
    push_pair(&mut s, 0, "SECTION");
    push_pair(&mut s, 2, "HEADER");
    push_pair(&mut s, 0, "ENDSEC");

    push_pair(&mut s, 0, "SECTION");
    push_pair(&mut s, 2, "TABLES");
    for layer in [LAYER_STRUCT, LAYER_PIPE, LAYER_CATCH, "0"] {
        push_pair(&mut s, 0, "TABLE");
        push_pair(&mut s, 2, "LAYER");
        push_pair(&mut s, 0, "LAYER");
        push_pair(&mut s, 2, layer);
        push_pair(&mut s, 70, "0");
        push_pair(&mut s, 62, "7");
        push_pair(&mut s, 6, "CONTINUOUS");
        push_pair(&mut s, 0, "ENDTAB");
    }
    push_pair(&mut s, 0, "ENDSEC");

    push_pair(&mut s, 0, "SECTION");
    push_pair(&mut s, 2, "ENTITIES");

    for n in &project.nodes {
        push_pair(&mut s, 0, "CIRCLE");
        push_pair(&mut s, 8, LAYER_STRUCT);
        push_f64(&mut s, 10, n.x);
        push_f64(&mut s, 20, n.y);
        push_f64(&mut s, 30, 0.0);
        push_f64(&mut s, 40, 5.0);
        push_struct_xdata(&mut s, n);
        // TEXT tag for kind + id (fallback when XDATA is stripped)
        push_pair(&mut s, 0, "TEXT");
        push_pair(&mut s, 8, LAYER_STRUCT);
        push_f64(&mut s, 10, n.x + 6.0);
        push_f64(&mut s, 20, n.y);
        push_f64(&mut s, 30, 0.0);
        push_f64(&mut s, 40, 4.0);
        push_pair(&mut s, 1, &format!("{}:{}", n.kind, n.id));
    }

    let pos: HashMap<&str, (f64, f64)> = project
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), (n.x, n.y)))
        .collect();

    for p in &project.pipes {
        let Some(&(x1, y1)) = pos.get(p.from.as_str()) else {
            continue;
        };
        let Some(&(x2, y2)) = pos.get(p.to.as_str()) else {
            continue;
        };
        push_pair(&mut s, 0, "LINE");
        push_pair(&mut s, 8, LAYER_PIPE);
        push_f64(&mut s, 10, x1);
        push_f64(&mut s, 20, y1);
        push_f64(&mut s, 30, 0.0);
        push_f64(&mut s, 11, x2);
        push_f64(&mut s, 21, y2);
        push_f64(&mut s, 31, 0.0);
        push_pipe_xdata(&mut s, p);
        push_pair(&mut s, 0, "TEXT");
        push_pair(&mut s, 8, LAYER_PIPE);
        push_f64(&mut s, 10, (x1 + x2) / 2.0);
        push_f64(&mut s, 20, (y1 + y2) / 2.0 + 6.0);
        push_f64(&mut s, 30, 0.0);
        push_f64(&mut s, 40, 3.5);
        push_pair(&mut s, 1, &format!("{}:{:.2}ft", p.id, p.diameter));
    }

    for c in &project.catchments {
        if c.vertices.len() < 3 {
            continue;
        }
        push_pair(&mut s, 0, "LWPOLYLINE");
        push_pair(&mut s, 8, LAYER_CATCH);
        push_pair(&mut s, 70, "1");
        push_pair(&mut s, 90, &c.vertices.len().to_string());
        for (x, y) in &c.vertices {
            push_f64(&mut s, 10, *x);
            push_f64(&mut s, 20, *y);
        }
        push_catchment_xdata(&mut s, c);
    }

    push_pair(&mut s, 0, "ENDSEC");
    push_pair(&mut s, 0, "EOF");

    fs::write(path, s).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[derive(Clone, Default)]
struct DxfEntity {
    kind: String,
    layer: String,
    x: f64,
    y: f64,
    x2: f64,
    y2: f64,
    radius: f64,
    text: String,
    pending_x: f64,
    vertices: Vec<(f64, f64)>,
    /// Parsed XDATA keyed by application name (STORMSEWER_STRUCT / STORMSEWER_PIPE).
    xdata: HashMap<String, HashMap<String, XdataValue>>,
}

#[derive(Clone, Debug)]
enum XdataValue {
    String(String),
    Real(f64),
}

/// Parse alternating key/value pairs from an XDATA block (1000 keys, 1000/1040 values).
fn parse_xdata_block(values: &[XdataValue]) -> HashMap<String, XdataValue> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i + 1 < values.len() {
        if let XdataValue::String(key) = &values[i] {
            out.insert(key.clone(), values[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn xdata_string(map: &HashMap<String, XdataValue>, key: &str) -> Option<String> {
    match map.get(key)? {
        XdataValue::String(s) => Some(s.clone()),
        XdataValue::Real(v) => Some(v.to_string()),
    }
}

fn xdata_real(map: &HashMap<String, XdataValue>, key: &str) -> Option<f64> {
    match map.get(key)? {
        XdataValue::Real(v) => Some(*v),
        XdataValue::String(s) => s.parse().ok(),
    }
}

/// Import circles/lines from ASCII DXF into a project (merges geometry; restores hydraulics from XDATA).
pub fn import_dxf(path: &Path) -> Result<Project, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let pairs = parse_pairs(&text);
    let mut entities = Vec::new();
    let mut cur = DxfEntity::default();
    let mut in_entity = false;
    let mut cur_xapp: Option<String> = None;
    let mut cur_xvals: Vec<XdataValue> = Vec::new();

    for (code, val) in &pairs {
        if *code == 0 {
            if in_entity && !cur.kind.is_empty() {
                if let Some(app) = cur_xapp.take() {
                    cur.xdata.insert(app, parse_xdata_block(&cur_xvals));
                    cur_xvals.clear();
                }
                entities.push(cur.clone());
            }
            cur = DxfEntity::default();
            cur.kind = val.clone();
            in_entity = true;
            cur_xapp = None;
            cur_xvals.clear();
            continue;
        }
        if !in_entity {
            continue;
        }
        match *code {
            8 => cur.layer = val.clone(),
            10 => {
                cur.pending_x = val.parse().unwrap_or(0.0);
                if cur.kind != "LWPOLYLINE" {
                    cur.x = cur.pending_x;
                }
            }
            20 => {
                let y = val.parse().unwrap_or(0.0);
                if cur.kind == "LWPOLYLINE" {
                    cur.vertices.push((cur.pending_x, y));
                } else {
                    cur.y = y;
                }
            }
            11 => cur.x2 = val.parse().unwrap_or(0.0),
            21 => cur.y2 = val.parse().unwrap_or(0.0),
            40 => cur.radius = val.parse().unwrap_or(5.0),
            1 => cur.text = val.clone(),
            1001 => {
                if let Some(app) = cur_xapp.take() {
                    cur.xdata.insert(app, parse_xdata_block(&cur_xvals));
                    cur_xvals.clear();
                }
                cur_xapp = Some(val.clone());
            }
            1000 => cur_xvals.push(XdataValue::String(val.clone())),
            1040 => cur_xvals.push(XdataValue::Real(val.parse().unwrap_or(0.0))),
            _ => {}
        }
    }
    if in_entity && !cur.kind.is_empty() {
        if let Some(app) = cur_xapp.take() {
            cur.xdata.insert(app, parse_xdata_block(&cur_xvals));
        }
        entities.push(cur);
    }

    // Seed from a neutral blank project, NOT demo(): demo() carries a fixed
    // 100.5 ft tailwater and a demo design storm that would silently corrupt the
    // HGL of any imported drawing.
    let mut project = Project::empty();
    project.name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported")
        .into();
    project.nodes.clear();
    project.pipes.clear();
    project.catchments.clear();

    let mut node_idx = 0;

    for e in &entities {
        if e.kind == "CIRCLE" && e.layer == LAYER_STRUCT {
            node_idx += 1;
            let fallback_id = format!("N{node_idx}");
            let xd = e.xdata.get(APP_STRUCT);
            let (kind, label_id) = if let Some(map) = xd {
                let kind = xdata_string(map, "kind").unwrap_or_else(|| "inlet".into());
                let id = xdata_string(map, "id").unwrap_or_else(|| fallback_id.clone());
                (kind, id)
            } else {
                parse_struct_label(&e.text, &fallback_id)
            };
            let invert = xd.and_then(|m| xdata_real(m, "invert")).unwrap_or(100.0);
            let rim = xd
                .and_then(|m| xdata_real(m, "rim"))
                .unwrap_or(invert + 6.0);
            let area_ac = xd.and_then(|m| xdata_real(m, "area")).unwrap_or(1.0);
            let c = xd.and_then(|m| xdata_real(m, "C")).unwrap_or(0.7);
            let tc_inlet = xd.and_then(|m| xdata_real(m, "tc_inlet")).unwrap_or(10.0);
            project.nodes.push(ProjectNode {
                id: label_id,
                kind,
                x: e.x,
                y: e.y,
                invert,
                rim,
                area_ac,
                c,
                tc_inlet,
                inlet: Default::default(),
                bypass_to: None,
                diameter_ft: 4.0,
            });
        }
    }

    if project.nodes.is_empty() {
        return Err("no SS_STRUCTURES circles found in DXF".into());
    }

    let pos_to_id: HashMap<(i64, i64), String> = project
        .nodes
        .iter()
        .map(|n| {
            (
                ((n.x * 10.0).round() as i64, (n.y * 10.0).round() as i64),
                n.id.clone(),
            )
        })
        .collect();

    let mut pipe_idx = 0;
    for e in &entities {
        if e.kind != "LINE" || e.layer != LAYER_PIPE {
            continue;
        }
        pipe_idx += 1;
        let xd = e.xdata.get(APP_PIPE);
        let from = xd
            .and_then(|m| xdata_string(m, "from_id"))
            .or_else(|| nearest_id(&pos_to_id, e.x, e.y))
            .unwrap_or_else(|| "N1".into());
        let to = xd
            .and_then(|m| xdata_string(m, "to_id"))
            .or_else(|| nearest_id(&pos_to_id, e.x2, e.y2))
            .unwrap_or_else(|| "OUT".into());
        let dia = xd
            .and_then(|m| xdata_real(m, "diameter"))
            .or_else(|| parse_pipe_dia(&e.text))
            .unwrap_or(1.5);
        let n = xd.and_then(|m| xdata_real(m, "n")).unwrap_or(0.013);
        let length = ((e.x2 - e.x).powi(2) + (e.y2 - e.y).powi(2)).sqrt();
        let mut pipe =
            ProjectPipe::new(&format!("P{pipe_idx}"), &from, &to, length.max(1.0), dia, n);
        pipe.invert_up = xd.and_then(|m| xdata_real(m, "invert_up"));
        pipe.invert_dn = xd.and_then(|m| xdata_real(m, "invert_dn"));
        project.pipes.push(pipe);
    }

    let mut catch_idx = 0;
    for e in &entities {
        if e.kind != "LWPOLYLINE" || e.layer != LAYER_CATCH || e.vertices.len() < 3 {
            continue;
        }
        catch_idx += 1;
        let xd = e.xdata.get(APP_CATCH);
        let c = xd.and_then(|m| xdata_real(m, "c")).unwrap_or(0.7);
        let flow_length_ft = xd
            .and_then(|m| xdata_real(m, "flow_length"))
            .unwrap_or(100.0);
        let slope = xd.and_then(|m| xdata_real(m, "slope")).unwrap_or(0.01);
        let inlet_node_id = xd.and_then(|m| xdata_string(m, "inlet_id"));
        project.catchments.push(ProjectCatchment {
            id: format!("C{catch_idx}"),
            vertices: e.vertices.clone(),
            c,
            flow_length_ft,
            slope,
            inlet_node_id,
        });
    }

    if project.pipes.is_empty() {
        // chain nodes in x-order as a fallback
        let mut ordered: Vec<_> = project.nodes.iter().collect();
        ordered.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        for w in ordered.windows(2) {
            pipe_idx += 1;
            let a = w[0];
            let b = w[1];
            let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            project.pipes.push(ProjectPipe::new(
                &format!("P{pipe_idx}"),
                &a.id,
                &b.id,
                len.max(1.0),
                1.5,
                0.013,
            ));
        }
    }

    Ok(project)
}

fn parse_struct_label(text: &str, fallback: &str) -> (String, String) {
    if let Some((kind, id)) = text.split_once(':') {
        (kind.to_string(), id.to_string())
    } else if text.is_empty() {
        ("inlet".into(), fallback.into())
    } else {
        ("inlet".into(), text.to_string())
    }
}

fn parse_pipe_dia(text: &str) -> Option<f64> {
    text.split(':')
        .nth(1)?
        .split("ft")
        .next()?
        .trim()
        .parse()
        .ok()
}

fn nearest_id(map: &HashMap<(i64, i64), String>, x: f64, y: f64) -> Option<String> {
    let key = ((x * 10.0).round() as i64, (y * 10.0).round() as i64);
    map.get(&key).cloned().or_else(|| {
        map.iter()
            .min_by_key(|((kx, ky), _)| {
                let dx = *kx as f64 / 10.0 - x;
                let dy = *ky as f64 / 10.0 - y;
                ((dx * dx + dy * dy) * 100.0) as i64
            })
            .map(|(_, id)| id.clone())
    })
}

fn parse_pairs(text: &str) -> Vec<(i32, String)> {
    let mut lines = text.lines();
    let mut out = Vec::new();
    while let Some(code_line) = lines.next() {
        let Ok(code) = code_line.trim().parse::<i32>() else {
            continue;
        };
        let Some(val_line) = lines.next() else { break };
        out.push((code, val_line.trim().to_string()));
    }
    out
}

/// Line segment for a non-network DXF site underlay.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfUnderlaySegment {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

const UNDERLAY_SKIP_LAYERS: &[&str] = &["SS_STRUCTURES", "SS_PIPES", "SS_CATCHMENTS"];

/// A drawable primitive from a reference DXF, in the coordinates of the
/// section it was read from (model space, or a block's local frame).
#[derive(Clone, Debug, Default)]
struct UnderlayEntity {
    kind: String,
    layer: String,
    /// Paper-space flag (group 67 = 1): layouts, title blocks — never site geometry.
    paper_space: bool,
    x: f64,
    y: f64,
    x2: f64,
    y2: f64,
    radius: f64,
    /// ARC start / end angles (degrees, group 50 / 51).
    angle0: f64,
    angle1: f64,
    /// Polyline vertices (LWPOLYLINE inline, or gathered from VERTEX entities
    /// that follow an old-style POLYLINE until SEQEND).
    vertices: Vec<(f64, f64)>,
    /// Polyline closed flag (group 70 bit 1).
    closed: bool,
    /// INSERT: block name (group 2), scale (41 / 42), rotation (50, degrees).
    block: String,
    scale_x: f64,
    scale_y: f64,
    rotation_deg: f64,
    pending_x: f64,
}

/// Everything the underlay needs from a DXF: model-space entities plus the
/// block table so INSERTs can be expanded.
#[derive(Default)]
struct UnderlayDocument {
    entities: Vec<UnderlayEntity>,
    blocks: HashMap<String, (f64, f64, Vec<UnderlayEntity>)>,
}

/// Walk the DXF sections. Only BLOCKS and ENTITIES matter; anything read
/// outside them (HEADER, TABLES, OBJECTS) is discarded so a title block or a
/// symbol definition at the origin cannot leak into the site geometry.
fn parse_underlay_document(text: &str) -> UnderlayDocument {
    let pairs = parse_pairs(text);
    let mut doc = UnderlayDocument::default();

    #[derive(PartialEq)]
    enum Section {
        Other,
        Blocks,
        Entities,
    }
    let mut section = Section::Other;
    let mut expect_section_name = false;

    // Current block being defined (name, base point, entities).
    let mut cur_block: Option<(String, f64, f64, Vec<UnderlayEntity>)> = None;
    let mut in_block_header = false;
    let mut cur: Option<UnderlayEntity> = None;
    // Old-style POLYLINE whose VERTEX entities are still arriving.
    let mut open_polyline: Option<UnderlayEntity> = None;

    fn finish(
        cur: &mut Option<UnderlayEntity>,
        open_polyline: &mut Option<UnderlayEntity>,
        sink: &mut Vec<UnderlayEntity>,
    ) {
        let Some(e) = cur.take() else { return };
        match e.kind.as_str() {
            "POLYLINE" => *open_polyline = Some(e),
            "VERTEX" => {
                if let Some(p) = open_polyline.as_mut() {
                    p.vertices.push((e.x, e.y));
                }
            }
            "SEQEND" => {
                if let Some(p) = open_polyline.take() {
                    sink.push(p);
                }
            }
            _ => sink.push(e),
        }
    }

    for (code, val) in &pairs {
        // Section bookkeeping.
        if *code == 0 && val == "SECTION" {
            expect_section_name = true;
            continue;
        }
        if expect_section_name && *code == 2 {
            section = match val.as_str() {
                "BLOCKS" => Section::Blocks,
                "ENTITIES" => Section::Entities,
                _ => Section::Other,
            };
            expect_section_name = false;
            continue;
        }
        if *code == 0 && val == "ENDSEC" {
            if let Some((name, bx, by, mut ents)) = cur_block.take() {
                finish(&mut cur, &mut open_polyline, &mut ents);
                doc.blocks.insert(name, (bx, by, ents));
            }
            finish(&mut cur, &mut open_polyline, &mut doc.entities);
            section = Section::Other;
            continue;
        }
        if section == Section::Other {
            continue;
        }

        if *code == 0 {
            // Close the previous entity into the right sink.
            match cur_block.as_mut() {
                Some((_, _, _, ents)) => finish(&mut cur, &mut open_polyline, ents),
                None => finish(&mut cur, &mut open_polyline, &mut doc.entities),
            }
            match val.as_str() {
                "BLOCK" => {
                    cur_block = Some((String::new(), 0.0, 0.0, Vec::new()));
                    in_block_header = true;
                }
                "ENDBLK" => {
                    if let Some((name, bx, by, ents)) = cur_block.take() {
                        doc.blocks.insert(name, (bx, by, ents));
                    }
                    in_block_header = false;
                }
                kind => {
                    in_block_header = false;
                    cur = Some(UnderlayEntity {
                        kind: kind.to_string(),
                        scale_x: 1.0,
                        scale_y: 1.0,
                        ..Default::default()
                    });
                }
            }
            continue;
        }

        if in_block_header {
            if let Some((name, bx, by, _)) = cur_block.as_mut() {
                match *code {
                    2 => *name = val.clone(),
                    10 => *bx = val.parse().unwrap_or(0.0),
                    20 => *by = val.parse().unwrap_or(0.0),
                    _ => {}
                }
            }
            continue;
        }

        let Some(e) = cur.as_mut() else { continue };
        match *code {
            8 => e.layer = val.clone(),
            67 => e.paper_space = val.trim() == "1",
            2 if e.kind == "INSERT" => e.block = val.clone(),
            10 => {
                e.pending_x = val.parse().unwrap_or(0.0);
                if e.kind != "LWPOLYLINE" {
                    e.x = e.pending_x;
                }
            }
            20 => {
                let y = val.parse().unwrap_or(0.0);
                if e.kind == "LWPOLYLINE" {
                    e.vertices.push((e.pending_x, y));
                } else {
                    e.y = y;
                }
            }
            11 => e.x2 = val.parse().unwrap_or(0.0),
            21 => e.y2 = val.parse().unwrap_or(0.0),
            40 => e.radius = val.parse().unwrap_or(0.0),
            41 if e.kind == "INSERT" => e.scale_x = val.parse().unwrap_or(1.0),
            42 if e.kind == "INSERT" => e.scale_y = val.parse().unwrap_or(1.0),
            50 => {
                let v = val.parse().unwrap_or(0.0);
                if e.kind == "INSERT" {
                    e.rotation_deg = v;
                } else {
                    e.angle0 = v;
                }
            }
            51 => e.angle1 = val.parse().unwrap_or(0.0),
            70 if matches!(e.kind.as_str(), "LWPOLYLINE" | "POLYLINE") => {
                e.closed = val
                    .trim()
                    .parse::<i32>()
                    .map(|f| f & 1 == 1)
                    .unwrap_or(false);
            }
            _ => {}
        }
    }
    finish(&mut cur, &mut open_polyline, &mut doc.entities);
    doc
}

fn underlay_layer_ok(layer: &str) -> bool {
    !UNDERLAY_SKIP_LAYERS.contains(&layer)
}

fn push_segment(out: &mut Vec<DxfUnderlaySegment>, x1: f64, y1: f64, x2: f64, y2: f64) {
    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return;
    }
    if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
        return;
    }
    out.push(DxfUnderlaySegment { x1, y1, x2, y2 });
}

/// Affine placement of a block's local frame into its parent: translate by
/// the insertion point after scaling about the block base point and rotating.
#[derive(Clone, Copy)]
struct Placement {
    ox: f64,
    oy: f64,
    sx: f64,
    sy: f64,
    cos: f64,
    sin: f64,
    bx: f64,
    by: f64,
}

impl Placement {
    const IDENTITY: Placement = Placement {
        ox: 0.0,
        oy: 0.0,
        sx: 1.0,
        sy: 1.0,
        cos: 1.0,
        sin: 0.0,
        bx: 0.0,
        by: 0.0,
    };

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let lx = (x - self.bx) * self.sx;
        let ly = (y - self.by) * self.sy;
        (
            self.ox + lx * self.cos - ly * self.sin,
            self.oy + lx * self.sin + ly * self.cos,
        )
    }
}

/// Deepest INSERT nesting expanded (real drawings nest symbols 2-3 deep).
const MAX_INSERT_DEPTH: usize = 4;

fn emit_underlay(
    entities: &[UnderlayEntity],
    place: &Placement,
    blocks: &HashMap<String, (f64, f64, Vec<UnderlayEntity>)>,
    depth: usize,
    segments: &mut Vec<DxfUnderlaySegment>,
) {
    for e in entities {
        if e.paper_space || !underlay_layer_ok(&e.layer) {
            continue;
        }
        match e.kind.as_str() {
            "LINE" => {
                let (x1, y1) = place.apply(e.x, e.y);
                let (x2, y2) = place.apply(e.x2, e.y2);
                push_segment(segments, x1, y1, x2, y2);
            }
            "LWPOLYLINE" | "POLYLINE" => {
                let pts: Vec<(f64, f64)> =
                    e.vertices.iter().map(|&(x, y)| place.apply(x, y)).collect();
                for w in pts.windows(2) {
                    push_segment(segments, w[0].0, w[0].1, w[1].0, w[1].1);
                }
                if e.closed && pts.len() >= 3 {
                    let first = pts[0];
                    let last = *pts.last().unwrap();
                    push_segment(segments, last.0, last.1, first.0, first.1);
                }
            }
            "CIRCLE" | "ARC" => {
                let r = e.radius.max(0.0);
                if r <= 0.0 {
                    continue;
                }
                let (a_start, sweep) = if e.kind == "ARC" {
                    let a0 = e.angle0.to_radians();
                    let mut a1 = e.angle1.to_radians();
                    if a1 <= a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    (a0, a1 - a0)
                } else {
                    (0.0, std::f64::consts::TAU)
                };
                let steps = ((sweep / std::f64::consts::TAU) * 24.0).ceil().max(2.0) as usize;
                for i in 0..steps {
                    let t0 = a_start + sweep * i as f64 / steps as f64;
                    let t1 = a_start + sweep * (i + 1) as f64 / steps as f64;
                    let (x1, y1) = place.apply(e.x + r * t0.cos(), e.y + r * t0.sin());
                    let (x2, y2) = place.apply(e.x + r * t1.cos(), e.y + r * t1.sin());
                    push_segment(segments, x1, y1, x2, y2);
                }
            }
            "INSERT" if depth < MAX_INSERT_DEPTH => {
                let Some((bx, by, ents)) = blocks.get(&e.block) else {
                    continue;
                };
                // Compose: block-local → this INSERT's frame → parent placement.
                let (ox, oy) = place.apply(e.x, e.y);
                let rot = e.rotation_deg.to_radians();
                let (pc, ps) = (place.cos, place.sin);
                let (ic, is) = (rot.cos(), rot.sin());
                let child = Placement {
                    ox,
                    oy,
                    sx: e.scale_x * place.sx,
                    sy: e.scale_y * place.sy,
                    cos: pc * ic - ps * is,
                    sin: ps * ic + pc * is,
                    bx: *bx,
                    by: *by,
                };
                emit_underlay(ents, &child, blocks, depth + 1, segments);
            }
            _ => {}
        }
    }
}

/// Import LINE / LWPOLYLINE / POLYLINE / CIRCLE / ARC entities from a reference
/// DXF (site underlay). Model space only; INSERTed blocks are expanded in
/// place; entities on this app's own SS_* layers are skipped.
pub fn import_dxf_underlay(path: &Path) -> Result<Vec<DxfUnderlaySegment>, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = parse_underlay_document(&text);
    let mut segments = Vec::new();
    emit_underlay(
        &doc.entities,
        &Placement::IDENTITY,
        &doc.blocks,
        0,
        &mut segments,
    );

    if segments.is_empty() {
        return Err(format!("no drawable entities in {}", path.display()));
    }
    Ok(segments)
}

/// Convert imported DXF geometry into a runtime network (for tests).
pub fn network_from_dxf(path: &Path) -> Result<Network, String> {
    Ok(import_dxf(path)?.to_network())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn export_import_round_trip() {
        let p = Project::demo();
        let path = temp_dir().join("stormsewer_test.dxf");
        export_dxf(&p, &path).unwrap();
        let imported = import_dxf(&path).unwrap();
        assert!(!imported.nodes.is_empty());
        assert!(!imported.pipes.is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn export_import_round_trip_catchments() {
        let p = Project::demo();
        let path = temp_dir().join("stormsewer_catchment_test.dxf");
        export_dxf(&p, &path).unwrap();
        let imported = import_dxf(&path).unwrap();
        assert_eq!(imported.catchments.len(), p.catchments.len());
        let orig = &p.catchments[0];
        let imp = &imported.catchments[0];
        assert_eq!(imp.vertices.len(), orig.vertices.len());
        for (a, b) in imp.vertices.iter().zip(orig.vertices.iter()) {
            assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
        }
        assert!((imp.c - orig.c).abs() < 1e-6);
        assert!((imp.flow_length_ft - orig.flow_length_ft).abs() < 1e-6);
        assert!((imp.slope - orig.slope).abs() < 1e-6);
        assert_eq!(imp.inlet_node_id, orig.inlet_node_id);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn underlay_imports_sample_background_when_present() {
        let path = Path::new(
            r"C:\Users\michael.flynn\AppData\Local\Autodesk\C3D 2026\enu\HHApps\StormSewers\SampleBackground.dxf",
        );
        if !path.exists() {
            return;
        }
        let segs = import_dxf_underlay(path).expect("underlay");
        assert!(!segs.is_empty());
    }

    #[test]
    fn export_preserves_invert_values() {
        let p = Project::demo();
        let path = temp_dir().join("stormsewer_invert_test.dxf");
        export_dxf(&p, &path).unwrap();
        let imported = import_dxf(&path).unwrap();
        for orig in &p.nodes {
            let imp = imported
                .nodes
                .iter()
                .find(|n| n.id == orig.id)
                .expect("missing node after import");
            assert!(
                (imp.invert - orig.invert).abs() < 1e-6,
                "invert for {}: expected {}, got {}",
                orig.id,
                orig.invert,
                imp.invert
            );
            assert!((imp.rim - orig.rim).abs() < 1e-6);
            assert!((imp.area_ac - orig.area_ac).abs() < 1e-6);
            assert!((imp.c - orig.c).abs() < 1e-6);
            assert!((imp.tc_inlet - orig.tc_inlet).abs() < 1e-6);
        }
        let _ = fs::remove_file(path);
    }
}
