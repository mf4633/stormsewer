// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal ASCII DXF import/export for StormSewer plan geometry.
//!
//! Structures export as `CIRCLE` on layer `SS_STRUCTURES`; pipes as `LINE` on
//! `SS_PIPES`. Extended data uses applications `STORMSEWER_STRUCT` and
//! `STORMSEWER_PIPE` with typed records (group 1001/1000/1040).

use crate::network::Network;
use crate::io::project::{Project, ProjectCatchment, ProjectNode, ProjectPipe};
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
        let Some(&(x1, y1)) = pos.get(p.from.as_str()) else { continue };
        let Some(&(x2, y2)) = pos.get(p.to.as_str()) else { continue };
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
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
            let invert = xd
                .and_then(|m| xdata_real(m, "invert"))
                .unwrap_or(100.0);
            let rim = xd.and_then(|m| xdata_real(m, "rim")).unwrap_or(invert + 6.0);
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
            });
        }
    }

    if project.nodes.is_empty() {
        return Err("no SS_STRUCTURES circles found in DXF".into());
    }

    let pos_to_id: HashMap<(i64, i64), String> = project
        .nodes
        .iter()
        .map(|n| (((n.x * 10.0).round() as i64, (n.y * 10.0).round() as i64), n.id.clone()))
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
        project.pipes.push(ProjectPipe::new(
            &format!("P{pipe_idx}"),
            &from,
            &to,
            length.max(1.0),
            dia,
            n,
        ));
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
    text.split(':').nth(1)?.split("ft").next()?.trim().parse().ok()
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
        let Ok(code) = code_line.trim().parse::<i32>() else { continue };
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

fn parse_dxf_entities(text: &str) -> Vec<DxfEntity> {
    let pairs = parse_pairs(text);
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
    entities
}

fn underlay_layer_ok(layer: &str) -> bool {
    !UNDERLAY_SKIP_LAYERS.contains(&layer)
}

fn push_segment(out: &mut Vec<DxfUnderlaySegment>, x1: f64, y1: f64, x2: f64, y2: f64) {
    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return;
    }
    out.push(DxfUnderlaySegment { x1, y1, x2, y2 });
}

/// Import LINE / LWPOLYLINE / CIRCLE entities from a reference DXF (site underlay).
pub fn import_dxf_underlay(path: &Path) -> Result<Vec<DxfUnderlaySegment>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let entities = parse_dxf_entities(&text);
    let mut segments = Vec::new();

    for e in entities {
        if !underlay_layer_ok(&e.layer) {
            continue;
        }
        match e.kind.as_str() {
            "LINE" => push_segment(&mut segments, e.x, e.y, e.x2, e.y2),
            "LWPOLYLINE" | "POLYLINE" => {
                for w in e.vertices.windows(2) {
                    push_segment(&mut segments, w[0].0, w[0].1, w[1].0, w[1].1);
                }
                if e.vertices.len() >= 3 {
                    let first = e.vertices[0];
                    let last = *e.vertices.last().unwrap();
                    push_segment(&mut segments, last.0, last.1, first.0, first.1);
                }
            }
            "CIRCLE" => {
                let r = e.radius.max(0.1);
                let steps = 24;
                for i in 0..steps {
                    let a0 = std::f64::consts::TAU * i as f64 / steps as f64;
                    let a1 = std::f64::consts::TAU * (i + 1) as f64 / steps as f64;
                    push_segment(
                        &mut segments,
                        e.x + r * a0.cos(),
                        e.y + r * a0.sin(),
                        e.x + r * a1.cos(),
                        e.y + r * a1.sin(),
                    );
                }
            }
            _ => {}
        }
    }

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