// SPDX-License-Identifier: GPL-3.0-or-later

//! Hydraflow Storm Sewers `.stm` project import (text format, 2008+).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::io::project::{
    BackgroundDxf, IdfCurveEntry, InletOverrides, Project, ProjectNode, ProjectPipe,
};

const COORD_TOL: f64 = 0.5;

/// Hydraflow STM return-period index → storm return period (years).
const STM_RP_YEARS: [(u32, u32); 6] = [(1, 2), (3, 5), (4, 10), (5, 25), (6, 50), (7, 100)];

#[derive(Clone, Debug, Default)]
struct StmLine {
    line_no: u32,
    line_id: String,
    downstream: u32,
    x_up: f64,
    y_up: f64,
    x_dn: f64,
    y_dn: f64,
    area_ac: f64,
    c: f64,
    tc_inlet: f64,
    length: f64,
    invert_up: f64,
    invert_dn: f64,
    rim_up: f64,
    rim_dn: f64,
    rise: f64,
    span: f64,
    n: f64,
    line_type: String,
    junction_type: u32,
    inlet_id: String,
    inlet_length: f64,
    gutter_slope: f64,
    inlet_sag: u32,
}

#[derive(Clone, Debug, Default)]
struct StmHeader {
    name: String,
    si_units: bool,
    min_tc: f64,
    default_n: f64,
    min_slope: f64,
    return_period_index: u32,
    grate_design_depth: f64,
}

#[derive(Clone, Debug, Default)]
struct StmTail {
    idf_a: Vec<f64>,
    idf_b: Vec<f64>,
    idf_c: Vec<f64>,
    background_enabled: bool,
    background_path: String,
    background_display: String,
    background_bounds: Option<(f64, f64, f64, f64)>,
}

/// Import a Hydraflow `.stm` project into a StormSewer [`Project`].
pub fn import_stm(path: &Path) -> Result<Project, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !text.contains("Hydraflow Storm Sewers") {
        return Err("not a Hydraflow Storm Sewers STM file".into());
    }

    let header = parse_stm_header(&text, path);
    let lines = parse_stm_lines(&text, header.default_n)?;
    let tail = parse_stm_tail(&text);

    stm_lines_to_project(path, &lines, &header, &tail)
}

fn parse_stm_header(text: &str, path: &Path) -> StmHeader {
    let mut header = StmHeader {
        name: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("STM Import")
            .to_string(),
        min_tc: 10.0,
        default_n: 0.013,
        min_slope: 0.001,
        ..Default::default()
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.contains("Project Name = ") {
            if let Some(v) = parse_string_field(line, "Project Name = ") {
                if !v.is_empty() && v != "," {
                    header.name = v;
                }
            }
            if let Some(idx) = line.rfind(',') {
                let candidate = line[idx + 1..].trim().trim_matches('"');
                if !candidate.is_empty() {
                    header.name = candidate.to_string();
                }
            }
        } else if let Some(v) = parse_bool_field(line, "SI Units? ") {
            header.si_units = v;
        } else if let Some(v) = parse_number_field(line, "Minimum Tc used to calc Intensity") {
            header.min_tc = v;
        } else if let Some(v) = parse_number_field(line, "Default Pipe n-value = ") {
            header.default_n = v;
        } else if let Some(v) = parse_number_field(line, "Min Slope = ") {
            header.min_slope = v / 100.0;
        } else if let Some(v) = parse_number_field(line, "Return Period Index = ") {
            header.return_period_index = v as u32;
        } else if let Some(v) = parse_number_field(line, "Grate Design Depth = ") {
            header.grate_design_depth = v;
        }
    }
    header
}

fn parse_stm_lines(text: &str, default_n: f64) -> Result<Vec<StmLine>, String> {
    let data_start = text
        .find("LINE DATA")
        .ok_or("STM file missing LINE DATA section")?;
    let data_section = &text[data_start..];
    let mut lines: Vec<StmLine> = Vec::new();

    for block in data_section.split("---------------------------------------") {
        let mut current = StmLine {
            n: default_n,
            ..Default::default()
        };
        for raw in block.lines() {
            let line = raw.trim();
            if line.is_empty() || line.contains("LINE DATA") {
                continue;
            }
            if line.contains("Line No. = ") && !line.contains("Downstream Line No.") {
                if let Some(n) = parse_number_field(line, "Line No. = ") {
                    current.line_no = n as u32;
                } else if let Some(n) = parse_trailing_number(line) {
                    current.line_no = n as u32;
                }
            } else if let Some(v) = parse_string_field(line, "Line ID = ") {
                current.line_id = v;
            } else if let Some(v) = parse_number_field(line, "Downstream Line No. = ") {
                current.downstream = v as u32;
            } else if line.contains("X,Y Coord Dn = ") {
                if let Some((x, y)) = parse_xy(line, "X,Y Coord Dn = ") {
                    current.x_dn = x;
                    current.y_dn = y;
                }
            } else if line.contains("X,Y Coord Up = ") {
                if let Some((x, y)) = parse_xy(line, "X,Y Coord Up = ") {
                    current.x_up = x;
                    current.y_up = y;
                }
            } else if let Some(v) = parse_number_field(line, "Drainage Area = ") {
                current.area_ac = v;
            } else if let Some(v) = parse_number_field(line, "Runoff Coeff. = ") {
                current.c = v;
            } else if let Some(v) = parse_number_field(line, "Inlet Time = ") {
                current.tc_inlet = v;
            } else if let Some(v) = parse_number_field(line, "Line Length = ") {
                current.length = v;
            } else if let Some(v) = parse_number_field(line, "Invert Elev Up = ") {
                current.invert_up = v;
            } else if let Some(v) = parse_number_field(line, "Invert Elev Dn = ") {
                current.invert_dn = v;
            } else if let Some(v) = parse_number_field(line, "Ground / Rim Elev Up = ") {
                current.rim_up = v;
            } else if let Some(v) = parse_number_field(line, "Ground / Rim Elev Dn = ") {
                current.rim_dn = v;
            } else if let Some(v) = parse_number_field(line, "Rise = ") {
                current.rise = v;
            } else if let Some(v) = parse_number_field(line, "Span = ") {
                current.span = v;
            } else if let Some(v) = parse_number_field(line, "N-Value = ") {
                current.n = v;
            } else if let Some(v) = parse_string_field(line, "Line Type = ") {
                current.line_type = v;
            } else if let Some(v) = parse_number_field(line, "Junction Type = ") {
                current.junction_type = v as u32;
            } else if let Some(v) = parse_string_field(line, "Inlet ID = ") {
                current.inlet_id = v;
            } else if let Some(v) = parse_number_field(line, "Inlet Length = ") {
                current.inlet_length = v;
            } else if let Some(v) = parse_number_field(line, "Gutter Slope = ") {
                current.gutter_slope = v;
            } else if let Some(v) = parse_number_field(line, "Inlet Sag = ") {
                current.inlet_sag = v as u32;
            }
        }
        if current.line_no > 0 {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        return Err("STM file contains no line data".into());
    }
    Ok(lines)
}

fn parse_stm_tail(text: &str) -> StmTail {
    let mut tail = StmTail::default();
    let Some(pos) = text.find("IDF Curves") else {
        return tail;
    };

    let section: Vec<&str> = text[pos..].lines().map(str::trim).collect();
    let mut idf_rows = 0usize;
    let mut after_background = false;

    for line in section.iter().skip(2) {
        if line.is_empty() {
            continue;
        }
        if line.contains("#TRUE#") && line.to_ascii_lowercase().contains(".dxf") {
            tail.background_enabled = true;
            let quotes = parse_quoted_strings(line);
            if quotes.len() >= 2 {
                tail.background_path = quotes[0].clone();
                tail.background_display = quotes[1].clone();
            } else if let Some(p) = quotes.first() {
                tail.background_path = p.clone();
            }
            after_background = true;
            continue;
        }
        if after_background {
            let vals = parse_csv_floats(line);
            if vals.len() == 4 {
                tail.background_bounds = Some((vals[0], vals[1], vals[2], vals[3]));
                break;
            }
            continue;
        }
        if line.starts_with('"') {
            continue;
        }
        let vals = parse_csv_floats(line);
        if vals.len() >= 6 && idf_rows < 3 {
            match idf_rows {
                0 => tail.idf_a = vals,
                1 => tail.idf_b = vals,
                2 => tail.idf_c = vals,
                _ => {}
            }
            idf_rows += 1;
        }
    }
    tail
}

fn parse_csv_floats(line: &str) -> Vec<f64> {
    line.split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .collect()
}

fn parse_quoted_strings(line: &str) -> Vec<String> {
    line.split('"')
        .filter(|s| !s.is_empty() && !s.contains('#'))
        .map(|s| s.trim_matches(',').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn stm_rp_years(index: u32) -> Option<u32> {
    STM_RP_YEARS
        .iter()
        .find(|(i, _)| *i == index)
        .map(|(_, y)| *y)
}

fn build_idf_curves(tail: &StmTail) -> Vec<IdfCurveEntry> {
    let mut curves = Vec::new();
    for &(idx, rp) in &STM_RP_YEARS {
        let ai = idx as usize;
        let a = tail.idf_a.get(ai).copied().unwrap_or(0.0);
        let b = tail.idf_b.get(ai).copied().unwrap_or(0.0);
        let c = tail.idf_c.get(ai).copied().unwrap_or(0.0);
        if a > 0.0 && b > 0.0 && c > 0.0 {
            curves.push(IdfCurveEntry {
                rp_years: rp,
                a,
                b,
                c,
            });
        }
    }
    curves
}

fn resolve_stm_background_dxf(stm_path: &Path, tail: &StmTail) -> Option<PathBuf> {
    if !tail.background_enabled {
        return None;
    }
    let stm_dir = stm_path.parent()?;
    let mut candidates = Vec::new();
    if !tail.background_path.is_empty() {
        candidates.push(PathBuf::from(&tail.background_path));
    }
    if !tail.background_display.is_empty() {
        candidates.push(stm_dir.join(&tail.background_display));
    }
    if !tail.background_path.is_empty() {
        if let Some(name) = Path::new(&tail.background_path).file_name() {
            candidates.push(stm_dir.join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn parse_trailing_number(line: &str) -> Option<f64> {
    let after_comma = line.rsplit(',').next()?;
    after_comma.trim().trim_matches('"').parse().ok()
}

fn parse_string_field(line: &str, key: &str) -> Option<String> {
    let quoted = format!("\"{key}");
    let rest = if let Some(pos) = line.find(&quoted) {
        &line[pos + quoted.len()..]
    } else if let Some(pos) = line.find(key) {
        &line[pos + key.len()..]
    } else {
        return None;
    };
    // Hydraflow quotes the key too (`"Inlet ID = ","CB-1"`), so `rest` can begin
    // with the key's closing quote; drop it, then the separating comma, before
    // reading the quoted value.
    let rest = rest.trim();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let rest = rest.trim_start().trim_start_matches(',').trim();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"').unwrap_or(inner.len());
        Some(inner[..end].to_string())
    } else {
        Some(rest.trim_matches('"').to_string())
    }
}

fn parse_number_field(line: &str, key: &str) -> Option<f64> {
    let quoted = format!("\"{key}");
    let rest = if let Some(pos) = line.find(&quoted) {
        &line[pos + quoted.len()..]
    } else if let Some(pos) = line.find(key) {
        &line[pos + key.len()..]
    } else {
        return None;
    };
    let mut rest = rest.trim().trim_start_matches(',').trim();
    // Hydraflow often writes `"Field = ",value` with a closing quote before the comma.
    if rest.starts_with('"') {
        let inner = rest.trim_start_matches('"');
        let end = inner.find('"').unwrap_or(inner.len());
        rest = inner[..end].trim();
    }
    rest.parse().ok().or_else(|| parse_trailing_number(line))
}

fn parse_bool_field(line: &str, key: &str) -> Option<bool> {
    let q = format!("\"{key}");
    if !line.starts_with(&q) {
        return None;
    }
    let rest = &line[q.len()..].to_ascii_uppercase();
    Some(rest.contains("#TRUE#") || rest.contains("TRUE"))
}

fn parse_xy(line: &str, key: &str) -> Option<(f64, f64)> {
    let pos = line.find(key)?;
    let rest = &line[pos + key.len()..];
    let nums: Vec<f64> = rest
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if nums.len() >= 2 {
        Some((nums[0], nums[1]))
    } else {
        None
    }
}

fn coord_key(x: f64, y: f64) -> (i64, i64) {
    ((x / COORD_TOL).round() as i64, (y / COORD_TOL).round() as i64)
}

fn junction_kind(stm: &StmLine, end: &str) -> String {
    if end == "dn" && stm.downstream == 0 {
        return "outfall".into();
    }
    if end == "up" {
        if stm.area_ac > 0.0 || !stm.inlet_id.is_empty() {
            return "inlet".into();
        }
        return match stm.junction_type {
            2 | 3 | 4 | 5 | 6 | 7 => "inlet".into(),
            _ => "junction".into(),
        };
    }
    "junction".into()
}

fn line_shape(line_type: &str, rise: f64, span: f64, si: bool) -> (String, f64, f64, f64) {
    // Hydraflow stores conduit Rise/Span in inches (US) or millimetres (SI).
    // Convert to the project's linear unit: feet (US) or metres (SI).
    let (div, min_dim) = if si { (1000.0, 0.15) } else { (12.0, 0.5) };
    let t = line_type.to_ascii_lowercase();
    if t.starts_with("box") {
        let dia = (rise.max(span) / div).max(min_dim);
        ("box".into(), rise / div, span / div, dia)
    } else if t.starts_with("ell") {
        let dia = (rise.max(span) / div).max(min_dim);
        ("elliptical".into(), rise / div, span / div, dia)
    } else {
        let raw = if rise > 0.0 { rise } else { span };
        // US files sometimes store the circular diameter already in feet (small
        // value); the >3 heuristic keeps that path. SI is always mm.
        let dia = if si {
            (raw / div).max(min_dim)
        } else if raw > 3.0 {
            raw / div
        } else {
            raw.max(min_dim)
        };
        ("circular".into(), 0.0, 0.0, dia)
    }
}

fn inlet_overrides(stm: &StmLine) -> InletOverrides {
    InletOverrides {
        length_ft: stm.inlet_length,
        gutter_slope: stm.gutter_slope,
        sag: stm.inlet_sag != 0,
    }
}

fn stm_lines_to_project(
    stm_path: &Path,
    lines: &[StmLine],
    header: &StmHeader,
    tail: &StmTail,
) -> Result<Project, String> {
    let mut node_id_at: HashMap<(i64, i64), String> = HashMap::new();
    let mut node_inlet_at: HashMap<(i64, i64), InletOverrides> = HashMap::new();
    let mut nodes: Vec<ProjectNode> = Vec::new();
    let mut pipes: Vec<ProjectPipe> = Vec::new();
    let mut next_id = 1u32;

    let mut ensure_node = |x: f64,
                           y: f64,
                           invert: f64,
                           rim: f64,
                           kind: &str,
                           area: f64,
                           c: f64,
                           tc: f64,
                           label: &str,
                           inlet: InletOverrides| -> String {
        let key = coord_key(x, y);
        if let Some(id) = node_id_at.get(&key) {
            if kind == "inlet" && inlet.length_ft > 0.0 {
                node_inlet_at.insert(key, inlet);
            }
            return id.clone();
        }
        let id = if !label.is_empty() && kind == "inlet" {
            sanitize_id(label)
        } else {
            format!("N{next_id}")
        };
        next_id += 1;
        node_id_at.insert(key, id.clone());
        if kind == "inlet" {
            node_inlet_at.insert(key, inlet.clone());
        }
        nodes.push(ProjectNode {
            id: id.clone(),
            kind: kind.into(),
            x,
            y,
            invert,
            rim,
            area_ac: area,
            c,
            tc_inlet: tc,
            inlet,
        });
        id
    };

    for stm in lines {
        let up_kind = junction_kind(stm, "up");
        let dn_kind = junction_kind(stm, "dn");
        let up_label = if up_kind == "inlet" { &stm.inlet_id } else { "" };
        let up_inlet = if up_kind == "inlet" {
            inlet_overrides(stm)
        } else {
            InletOverrides::default()
        };
        let up_id = ensure_node(
            stm.x_up,
            stm.y_up,
            stm.invert_up,
            stm.rim_up,
            &up_kind,
            if up_kind == "inlet" { stm.area_ac } else { 0.0 },
            if up_kind == "inlet" { stm.c } else { 0.0 },
            if up_kind == "inlet" { stm.tc_inlet } else { 0.0 },
            up_label,
            up_inlet,
        );
        let dn_id = ensure_node(
            stm.x_dn,
            stm.y_dn,
            stm.invert_dn,
            stm.rim_dn,
            &dn_kind,
            0.0,
            0.0,
            0.0,
            "",
            InletOverrides::default(),
        );

        let (shape, rise_ft, span_ft, diameter) =
            line_shape(&stm.line_type, stm.rise, stm.span, header.si_units);
        let mut pipe = ProjectPipe::new(
            &format!("P{}", stm.line_no),
            &up_id,
            &dn_id,
            stm.length.max(1.0),
            diameter,
            stm.n,
        );
        pipe.shape = shape;
        pipe.rise_ft = rise_ft;
        pipe.span_ft = span_ft;
        pipes.push(pipe);
    }

    let idf_curves = build_idf_curves(tail);
    let design_rp = stm_rp_years(header.return_period_index).unwrap_or(10) as f64;
    let (idf_a, idf_b, idf_c) = if let Some(entry) = idf_curves
        .iter()
        .find(|c| c.rp_years == design_rp as u32)
    {
        (entry.a, entry.b, entry.c)
    } else if let Some(entry) = idf_curves.first() {
        (entry.a, entry.b, entry.c)
    } else {
        (60.0, 10.0, 0.8)
    };

    let background_dxf = resolve_stm_background_dxf(stm_path, tail).map(|resolved| {
        let bounds = tail.background_bounds.unwrap_or((0.0, 0.0, 1000.0, 1000.0));
        BackgroundDxf {
            path: resolved.display().to_string(),
            min_x: bounds.0,
            min_y: bounds.1,
            max_x: bounds.2,
            max_y: bounds.3,
            opacity: 0.45,
        }
    });

    Ok(Project {
        name: header.name.clone(),
        idf_a,
        idf_b,
        idf_c,
        tailwater: None,
        min_tc: header.min_tc,
        junction_k: 0.5,
        bend_loss_coeff: 0.0,
        hec22_structure_loss: false,
        access_hole_diam_ft: 4.0,
        design_return_period_years: design_rp,
        p2_rainfall_in: 3.0,
        min_slope: header.min_slope.max(0.0001),
        nodes,
        pipes,
        catchments: Vec::new(),
        background: None,
        background_dxf,
        idf_curves,
        units: if header.si_units {
            crate::units::UnitSystem::Si
        } else {
            crate::units::UnitSystem::UsCustomary
        },
    })
}

fn sanitize_id(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "N1".into();
    }
    t.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_minimal_stm_block() {
        let dir = std::env::temp_dir();
        let path = dir.join("stormsewer_test_minimal.stm");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, r#""Hydraflow Storm Sewers 2008""#).unwrap();
            writeln!(f, r#""Project Name = ","Test STM""#).unwrap();
            writeln!(f, r#""SI Units? ",#FALSE#"#).unwrap();
            writeln!(f, r#""Minimum Tc used to calc Intensity",10"#).unwrap();
            writeln!(f, r#""LINE DATA =============================""#).unwrap();
            writeln!(f, r#""Line No. = ",1"#).unwrap();
            writeln!(f, r#""Downstream Line No. = ",0"#).unwrap();
            writeln!(f, r#""X,Y Coord Dn = ",0,0"#).unwrap();
            writeln!(f, r#""X,Y Coord Up = ",100,0"#).unwrap();
            writeln!(f, r#""Drainage Area = ",1"#).unwrap();
            writeln!(f, r#""Runoff Coeff. = ",0.8"#).unwrap();
            writeln!(f, r#""Inlet Time = ",12"#).unwrap();
            writeln!(f, r#""Line Length = ",100"#).unwrap();
            writeln!(f, r#""Invert Elev Up = ",105"#).unwrap();
            writeln!(f, r#""Invert Elev Dn = ",100"#).unwrap();
            writeln!(f, r#""Ground / Rim Elev Up = ",110"#).unwrap();
            writeln!(f, r#""Ground / Rim Elev Dn = ",106"#).unwrap();
            writeln!(f, r#""Rise = ",1.5"#).unwrap();
            writeln!(f, r#""Span = ",1.5"#).unwrap();
            writeln!(f, r#""N-Value = ",0.013"#).unwrap();
            writeln!(f, r#""Line Type = ","Cir""#).unwrap();
            writeln!(f, r#""Junction Type = ",1"#).unwrap();
            writeln!(f, r#""Inlet ID = ","IN-1""#).unwrap();
            writeln!(f, r#""Inlet Length = ",6"#).unwrap();
            writeln!(f, r#""Gutter Slope = ",0.005"#).unwrap();
            writeln!(f, r#""Inlet Sag = ",0"#).unwrap();
            writeln!(f, r#""---------------------------------------""#).unwrap();
        }
        let p = import_stm(&path).expect("import minimal stm");
        assert_eq!(p.pipes.len(), 1);
        assert!(p.nodes.iter().any(|n| n.kind == "outfall"));
        let inlet = p.nodes.iter().find(|n| n.kind == "inlet").unwrap();
        assert!((inlet.inlet.length_ft - 6.0).abs() < 1e-6);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parses_autodesk_sample_when_present() {
        let path = Path::new(
            r"C:\Users\michael.flynn\AppData\Local\Autodesk\C3D 2026\enu\HHApps\StormSewers\SampleProject.stm",
        );
        if !path.exists() {
            return;
        }
        let text = fs::read_to_string(path).unwrap();
        let header = parse_stm_header(&text, path);
        let p = import_stm(path).expect("sample stm");
        assert_eq!(p.pipes.len(), 7);
        assert_eq!(p.name, "Windward Phase IV");
        assert!(!p.idf_curves.is_empty(), "expected STM IDF curves");
        let expected_rp = stm_rp_years(header.return_period_index).unwrap_or(10) as f64;
        assert!(
            (p.design_return_period_years - expected_rp).abs() < 1e-6,
            "design RP {} should match index {} mapping",
            p.design_return_period_years,
            header.return_period_index
        );
        assert!(p.background_dxf.is_some(), "expected background DXF ref");
        let inlet = p
            .nodes
            .iter()
            .find(|n| n.kind == "inlet" && n.inlet.length_ft > 0.0)
            .expect("inlet with STM geometry");
        assert!((inlet.inlet.length_ft - 6.0).abs() < 1e-6);
        assert!(inlet.inlet.sag);
    }

    #[test]
    fn stm_rp_index_maps_to_years() {
        assert_eq!(stm_rp_years(5), Some(25));
        assert_eq!(stm_rp_years(4), Some(10));
    }

    #[test]
    fn parses_inlet_length_field() {
        let line = r#""Inlet Length = ",6"#;
        assert_eq!(parse_number_field(line, "Inlet Length = "), Some(6.0));
    }

    #[test]
    fn parses_quoted_string_field_value() {
        // Both key and value are quoted in real STM files — the value must survive.
        assert_eq!(
            parse_string_field(r#""Inlet ID = ","CB-1""#, "Inlet ID = "),
            Some("CB-1".into())
        );
        assert_eq!(
            parse_string_field(r#""Line ID = ","Trunk Upper""#, "Line ID = "),
            Some("Trunk Upper".into())
        );
        // Bare (unquoted) value still works.
        assert_eq!(
            parse_string_field(r#""Inlet ID = ",CB-2"#, "Inlet ID = "),
            Some("CB-2".into())
        );
    }
}