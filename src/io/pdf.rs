// SPDX-License-Identifier: GPL-3.0-or-later

//! PDF report export (plan + profile schematics, summary, hydraulic tables).

use crate::design::{DesignFinding, Severity};
use crate::drawing::{draw_network, DrawConfig, Polyline, ProfileRole};
use crate::io::project::Project;
use crate::network::Analysis;
use crate::report::format_analysis;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

type PdfFonts<'a> = (&'a printpdf::IndirectFontRef, &'a printpdf::IndirectFontRef);

/// Map project coordinates into a schematic box (mm from page bottom-left).
fn plan_schematic_bounds(project: &Project) -> Option<(f64, f64, f64, f64, f64)> {
    if project.nodes.is_empty() {
        return None;
    }
    let min_x = project.nodes.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
    let max_x = project.nodes.iter().map(|n| n.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = project.nodes.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
    let max_y = project.nodes.iter().map(|n| n.y).fold(f64::NEG_INFINITY, f64::max);
    let span_x = (max_x - min_x).max(1.0);
    let span_y = (max_y - min_y).max(1.0);

    let left = 20.0_f64;
    let bottom = 175.0_f64;
    let width = 175.0_f64;
    let height = 70.0_f64;
    let pad = 0.08_f64;
    let scale = (width * (1.0 - 2.0 * pad) / span_x).min(height * (1.0 - 2.0 * pad) / span_y);
    Some((left, bottom, min_x, min_y, scale))
}

fn profile_bounds(lines: &[Polyline]) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;

    for pl in lines {
        for &(x, y) in &pl.pts {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            any = true;
        }
    }

    if !any || min_x >= max_x || min_y >= max_y {
        return None;
    }
    Some((min_x, min_y, max_x, max_y))
}

/// Map profile drawing coordinates into a schematic box (mm from page bottom-left).
fn profile_schematic_bounds(project: &Project, analysis: &Analysis) -> Option<(f64, f64, f64, f64, f64)> {
    let net = project.to_network();
    let drawing = draw_network(&net, analysis, &DrawConfig::default());
    let (min_x, min_y, max_x, max_y) = profile_bounds(&drawing.profile_lines)?;

    let span_x = (max_x - min_x).max(1.0);
    let span_y = (max_y - min_y).max(1.0);

    let left = 15.0_f64;
    let bottom = 55.0_f64;
    let width = 185.0_f64;
    let height = 180.0_f64;
    let pad = 0.08_f64;
    let scale = (width * (1.0 - 2.0 * pad) / span_x).min(height * (1.0 - 2.0 * pad) / span_y);
    Some((left, bottom, min_x, min_y, scale))
}

fn write_line(
    layer: &printpdf::PdfLayerReference,
    y: &mut f32,
    text: &str,
    size: f32,
    bold: bool,
    fonts: PdfFonts<'_>,
    left: f32,
    line_h: f32,
) {
    let (font, font_bold) = fonts;
    layer.use_text(text, size, printpdf::Mm(left), printpdf::Mm(*y), if bold { font_bold } else { font });
    *y -= line_h * (size / 9.0).max(1.0);
}

fn draw_plan_schematic(
    layer: &printpdf::PdfLayerReference,
    project: &Project,
    font_bold: &printpdf::IndirectFontRef,
) {
    use printpdf::*;

    let Some((left, bottom, min_x, min_y, scale)) = plan_schematic_bounds(project) else {
        return;
    };

    let to_mm = |x: f64, y: f64| (left + (x - min_x) * scale, bottom + (y - min_y) * scale);
    let pos: HashMap<&str, (f64, f64)> = project
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), to_mm(n.x, n.y)))
        .collect();

    layer.set_outline_color(Color::Rgb(Rgb::new(0.15, 0.35, 0.55, None)));
    layer.set_outline_thickness(0.6);

    for p in &project.pipes {
        let Some(&(x1, y1)) = pos.get(p.from.as_str()) else { continue };
        let Some(&(x2, y2)) = pos.get(p.to.as_str()) else { continue };
        let line = Line {
            points: vec![
                (Point::new(Mm(x1 as f32), Mm(y1 as f32)), false),
                (Point::new(Mm(x2 as f32), Mm(y2 as f32)), false),
            ],
            is_closed: false,
        };
        layer.add_line(line);
    }

    layer.set_outline_color(Color::Rgb(Rgb::new(0.2, 0.2, 0.2, None)));
    layer.set_outline_thickness(0.4);
    for n in &project.nodes {
        let (cx, cy) = to_mm(n.x, n.y);
        let r = 1.8_f32;
        let circle = Line {
            points: (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0 * std::f32::consts::TAU;
                    (
                        Point::new(Mm(cx as f32 + r * t.cos()), Mm(cy as f32 + r * t.sin())),
                        false,
                    )
                })
                .collect(),
            is_closed: true,
        };
        layer.add_line(circle);
    }

    layer.use_text("Plan Schematic", 8.0, Mm(20.0), Mm(252.0), font_bold);
}

fn profile_role_color(role: ProfileRole) -> printpdf::Rgb {
    use printpdf::Rgb;
    match role {
        ProfileRole::Ground => Rgb::new(0.55, 0.35, 0.17, None),
        ProfileRole::Invert => Rgb::new(0.4, 0.4, 0.4, None),
        ProfileRole::Hgl => Rgb::new(0.31, 0.63, 1.0, None),
    }
}

fn profile_stroke_width(role: ProfileRole) -> f32 {
    match role {
        ProfileRole::Ground => 0.8,
        ProfileRole::Invert => 0.6,
        ProfileRole::Hgl => 0.8,
    }
}

fn draw_profile_schematic(
    layer: &printpdf::PdfLayerReference,
    project: &Project,
    analysis: &Analysis,
    font_bold: &printpdf::IndirectFontRef,
) {
    use printpdf::*;

    let net = project.to_network();
    let drawing = draw_network(&net, analysis, &DrawConfig::default());
    let Some((left, bottom, min_x, min_y, scale)) = profile_schematic_bounds(project, analysis) else {
        layer.use_text("No profile data available", 10.0, Mm(20.0), Mm(140.0), font_bold);
        return;
    };

    let to_mm = |x: f64, y: f64| (left + (x - min_x) * scale, bottom + (y - min_y) * scale);

    for pl in &drawing.profile_lines {
        if pl.pts.len() < 2 {
            continue;
        }
        layer.set_outline_color(Color::Rgb(profile_role_color(pl.role)));
        layer.set_outline_thickness(profile_stroke_width(pl.role));
        for window in pl.pts.windows(2) {
            let (x1, y1) = to_mm(window[0].0, window[0].1);
            let (x2, y2) = to_mm(window[1].0, window[1].1);
            let line = Line {
                points: vec![
                    (Point::new(Mm(x1 as f32), Mm(y1 as f32)), false),
                    (Point::new(Mm(x2 as f32), Mm(y2 as f32)), false),
                ],
                is_closed: false,
            };
            layer.add_line(line);
        }
    }

    layer.use_text("Profile Schematic (main stem)", 9.0, Mm(15.0), Mm(252.0), font_bold);

    let legend_y = 42.0_f32;
    let entries = [
        (ProfileRole::Ground, "Ground"),
        (ProfileRole::Invert, "Invert"),
        (ProfileRole::Hgl, "HGL"),
    ];
    let mut lx = 15.0_f32;
    for (role, label) in entries {
        layer.set_outline_color(Color::Rgb(profile_role_color(role)));
        layer.set_outline_thickness(profile_stroke_width(role));
        let line = Line {
            points: vec![
                (Point::new(Mm(lx), Mm(legend_y)), false),
                (Point::new(Mm(lx + 12.0), Mm(legend_y)), false),
            ],
            is_closed: false,
        };
        layer.add_line(line);
        layer.use_text(label, 7.0, Mm(lx + 14.0), Mm(legend_y - 1.5), font_bold);
        lx += 48.0;
    }
}

fn write_summary_page(
    layer: &printpdf::PdfLayerReference,
    project: &Project,
    analysis: &Analysis,
    findings: Option<&[DesignFinding]>,
    fonts: PdfFonts<'_>,
) {
    let mut y = 168.0_f32;
    let left = 15.0_f32;
    let line_h = 4.2_f32;

    write_line(
        layer,
        &mut y,
        "StormSewer Analysis Report",
        16.0,
        true,
        fonts,
        left,
        line_h,
    );
    write_line(
        layer,
        &mut y,
        &format!("Project: {}", project.name),
        11.0,
        false,
        fonts,
        left,
        line_h,
    );
    write_line(
        layer,
        &mut y,
        &format!(
            "IDF: i = {:.1}/(t+{:.1})^{:.2} in/hr   Design storm: {:.0}-yr",
            project.idf_a, project.idf_b, project.idf_c, project.design_return_period_years
        ),
        10.0,
        false,
        fonts,
        left,
        line_h,
    );
    write_line(
        layer,
        &mut y,
        &format!("Nodes: {}   Pipes: {}", project.nodes.len(), project.pipes.len()),
        10.0,
        false,
        fonts,
        left,
        line_h,
    );
    y -= 4.0;

    let surcharged: Vec<&str> = analysis
        .pipes
        .iter()
        .filter(|p| p.surcharged)
        .map(|p| p.id.as_str())
        .collect();
    let flooding: Vec<&str> = analysis
        .nodes
        .iter()
        .filter(|n| n.surcharge_to_surface)
        .map(|n| n.id.as_str())
        .collect();

    write_line(layer, &mut y, "Summary", 10.0, true, fonts, left, line_h);
    if surcharged.is_empty() && flooding.is_empty() {
        write_line(
            layer,
            &mut y,
            "All pipes flow open-channel; no surface flooding.",
            9.0,
            false,
            fonts,
            left,
            line_h,
        );
    } else {
        if !surcharged.is_empty() {
            write_line(
                layer,
                &mut y,
                &format!("Surcharged pipes: {}", surcharged.join(", ")),
                9.0,
                false,
                fonts,
                left,
                line_h,
            );
        }
        if !flooding.is_empty() {
            write_line(
                layer,
                &mut y,
                &format!("Structures flooding (HGL > rim): {}", flooding.join(", ")),
                9.0,
                false,
                fonts,
                left,
                line_h,
            );
        }
    }

    if let Some(findings) = findings {
        y -= 4.0;
        write_line(layer, &mut y, "Design Review", 10.0, true, fonts, left, line_h);
        if findings.is_empty() {
            write_line(
                layer,
                &mut y,
                "No design-criteria issues found.",
                9.0,
                false,
                fonts,
                left,
                line_h,
            );
        } else {
            for f in findings {
                if y < 20.0 {
                    write_line(
                        layer,
                        &mut y,
                        "(additional findings truncated)",
                        8.0,
                        false,
                        fonts,
                        left,
                        line_h,
                    );
                    break;
                }
                let sev = match f.severity {
                    Severity::Error => "ERROR",
                    Severity::Warning => "WARN",
                };
                write_line(
                    layer,
                    &mut y,
                    &format!("[{sev}] {} — {}", f.id, f.message),
                    8.0,
                    f.severity == Severity::Error,
                    fonts,
                    left,
                    line_h,
                );
            }
        }
    }

}

fn write_tables_page(
    layer: &printpdf::PdfLayerReference,
    project: &Project,
    analysis: &Analysis,
    fonts: PdfFonts<'_>,
) {
    let mut y = 265.0_f32;
    let left = 10.0_f32;
    let line_h = 3.6_f32;

    write_line(
        layer,
        &mut y,
        &format!("Hydraulic Tables — {}", project.name),
        12.0,
        true,
        fonts,
        left,
        line_h,
    );
    y -= 4.0;

    for line in format_analysis(analysis).lines() {
        if y < 8.0 {
            break;
        }
        let bold = line.starts_with("===");
        let size = if line.starts_with("Pipe") || line.starts_with("Node") || line.starts_with("===") {
            7.5
        } else if line.starts_with('-') {
            6.0
        } else {
            6.5
        };
        write_line(layer, &mut y, line, size, bold, fonts, left, line_h);
    }
}

/// Write a letter-size PDF: page 1 plan + summary, page 2 profile, page 3 tables.
pub fn export_pdf(
    project: &Project,
    analysis: &Analysis,
    path: &Path,
    findings: Option<&[DesignFinding]>,
) -> Result<(), String> {
    use printpdf::*;

    let (doc, page1, layer1) =
        PdfDocument::new(&format!("StormSewer — {}", project.name), Mm(215.9), Mm(279.4), "Layer 1");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| e.to_string())?;
    let font_bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| e.to_string())?;

    let fonts = (&font, &font_bold);

    // Page 1: plan schematic + summary (+ optional design review).
    {
        let layer = doc.get_page(page1).get_layer(layer1);
        draw_plan_schematic(&layer, project, &font_bold);
        write_summary_page(&layer, project, analysis, findings, fonts);
    }

    // Page 2: profile schematic.
    {
        let (page2, layer2) = doc.add_page(Mm(215.9), Mm(279.4), "Page 2");
        let layer = doc.get_page(page2).get_layer(layer2);
        draw_profile_schematic(&layer, project, analysis, &font_bold);
    }

    // Page 3: full hydraulic tables.
    {
        let (page3, layer3) = doc.add_page(Mm(215.9), Mm(279.4), "Page 3");
        let layer = doc.get_page(page3).get_layer(layer3);
        write_tables_page(&layer, project, analysis, fonts);
    }

    let file = File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    doc.save(&mut BufWriter::new(file)).map_err(|e| e.to_string())
}