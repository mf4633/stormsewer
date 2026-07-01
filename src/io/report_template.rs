// SPDX-License-Identifier: GPL-3.0-only

//! Custom report templates (Hydraflow MyReport parity).

use serde::{Deserialize, Serialize};

use crate::io::project::Project;
use crate::network::Analysis;

/// Report column variable (subset of Hydraflow Custom Report Variables).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportVariable {
    PipeId,
    FromNode,
    ToNode,
    Length,
    DiameterIn,
    Shape,
    DesignQ,
    Capacity,
    PctFull,
    Velocity,
    Slope,
    Tc,
    HglUp,
    HglDn,
    InvertUp,
    InvertDn,
    RimUp,
    RimDn,
    CoverUp,
    CoverDn,
    AreaAc,
    RunoffC,
    LineCost,
}

impl ReportVariable {
    pub fn header(self) -> &'static str {
        match self {
            Self::PipeId => "Pipe",
            Self::FromNode => "From",
            Self::ToNode => "To",
            Self::Length => "Length(ft)",
            Self::DiameterIn => "Dia(in)",
            Self::Shape => "Shape",
            Self::DesignQ => "Q(cfs)",
            Self::Capacity => "Cap(cfs)",
            Self::PctFull => "%Full",
            Self::Velocity => "V(ft/s)",
            Self::Slope => "Slope",
            Self::Tc => "Tc(min)",
            Self::HglUp => "HGL Up",
            Self::HglDn => "HGL Dn",
            Self::InvertUp => "Invert Up",
            Self::InvertDn => "Invert Dn",
            Self::RimUp => "Rim Up",
            Self::RimDn => "Rim Dn",
            Self::CoverUp => "Cover Up",
            Self::CoverDn => "Cover Dn",
            Self::AreaAc => "Area(ac)",
            Self::RunoffC => "C",
            Self::LineCost => "Cost($)",
        }
    }

    pub const ALL: [ReportVariable; 23] = [
        ReportVariable::PipeId,
        ReportVariable::FromNode,
        ReportVariable::ToNode,
        ReportVariable::Length,
        ReportVariable::DiameterIn,
        ReportVariable::Shape,
        ReportVariable::DesignQ,
        ReportVariable::Capacity,
        ReportVariable::PctFull,
        ReportVariable::Velocity,
        ReportVariable::Slope,
        ReportVariable::Tc,
        ReportVariable::HglUp,
        ReportVariable::HglDn,
        ReportVariable::InvertUp,
        ReportVariable::InvertDn,
        ReportVariable::RimUp,
        ReportVariable::RimDn,
        ReportVariable::CoverUp,
        ReportVariable::CoverDn,
        ReportVariable::AreaAc,
        ReportVariable::RunoffC,
        ReportVariable::LineCost,
    ];
}

/// Saved custom report layout (`.srpt` JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportTemplate {
    pub name: String,
    pub columns: Vec<ReportVariable>,
}

impl Default for ReportTemplate {
    fn default() -> Self {
        Self::municipal_summary()
    }
}

impl ReportTemplate {
    pub fn municipal_summary() -> Self {
        Self {
            name: "Municipal Summary".into(),
            columns: vec![
                ReportVariable::PipeId,
                ReportVariable::FromNode,
                ReportVariable::ToNode,
                ReportVariable::Length,
                ReportVariable::DiameterIn,
                ReportVariable::DesignQ,
                ReportVariable::Velocity,
                ReportVariable::PctFull,
                ReportVariable::HglUp,
                ReportVariable::HglDn,
            ],
        }
    }

    pub fn hydraflow_style() -> Self {
        Self {
            name: "Hydraflow Pipe Table".into(),
            columns: vec![
                ReportVariable::PipeId,
                ReportVariable::Length,
                ReportVariable::Slope,
                ReportVariable::DiameterIn,
                ReportVariable::Shape,
                ReportVariable::InvertUp,
                ReportVariable::InvertDn,
                ReportVariable::RimUp,
                ReportVariable::CoverUp,
                ReportVariable::DesignQ,
                ReportVariable::Capacity,
                ReportVariable::Tc,
                ReportVariable::HglUp,
                ReportVariable::HglDn,
                ReportVariable::AreaAc,
                ReportVariable::RunoffC,
            ],
        }
    }

    pub fn cost_report() -> Self {
        Self {
            name: "Cost Report".into(),
            columns: vec![
                ReportVariable::PipeId,
                ReportVariable::Length,
                ReportVariable::DiameterIn,
                ReportVariable::LineCost,
            ],
        }
    }
}

struct RowContext<'a> {
    project: &'a Project,
    analysis: &'a Analysis,
    pipe_idx: usize,
    cost_per_ft: f64,
}

fn resolve_var(ctx: &RowContext<'_>, var: ReportVariable) -> String {
    let pipe = &ctx.project.pipes[ctx.pipe_idx];
    let pr = ctx.analysis.pipes.iter().find(|p| p.id == pipe.id);
    let from = ctx.project.nodes.iter().find(|n| n.id == pipe.from);
    let to = ctx.project.nodes.iter().find(|n| n.id == pipe.to);

    match var {
        ReportVariable::PipeId => pipe.id.clone(),
        ReportVariable::FromNode => pipe.from.clone(),
        ReportVariable::ToNode => pipe.to.clone(),
        ReportVariable::Length => format!("{:.1}", pipe.length),
        ReportVariable::DiameterIn => format!("{:.0}", pipe.diameter * 12.0),
        ReportVariable::Shape => pipe.shape.clone(),
        ReportVariable::DesignQ => pr.map(|p| format!("{:.2}", p.design_q)).unwrap_or_else(|| "--".into()),
        ReportVariable::Capacity => pr
            .map(|p| format!("{:.2}", p.capacity))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::PctFull => pr
            .map(|p| format!("{:.1}", p.pct_full * 100.0))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::Velocity => pr
            .map(|p| format!("{:.2}", p.velocity))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::Slope => pr
            .map(|p| format!("{:.4}", p.slope))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::Tc => pr
            .map(|p| format!("{:.1}", p.tc))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::HglUp => pr
            .and_then(|p| p.hgl_up)
            .map(|h| format!("{:.2}", h))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::HglDn => pr
            .and_then(|p| p.hgl_dn)
            .map(|h| format!("{:.2}", h))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::InvertUp => from
            .map(|n| format!("{:.2}", n.invert))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::InvertDn => to
            .map(|n| format!("{:.2}", n.invert))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::RimUp => from
            .map(|n| format!("{:.2}", n.rim))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::RimDn => to
            .map(|n| format!("{:.2}", n.rim))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::CoverUp => from
            .map(|n| format!("{:.2}", (n.rim - n.invert - pipe.diameter).max(0.0)))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::CoverDn => to
            .map(|n| format!("{:.2}", (n.rim - n.invert - pipe.diameter).max(0.0)))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::AreaAc => from
            .map(|n| format!("{:.2}", n.area_ac))
            .unwrap_or_else(|| "0".into()),
        ReportVariable::RunoffC => from
            .map(|n| format!("{:.2}", n.c))
            .unwrap_or_else(|| "--".into()),
        ReportVariable::LineCost => format!("{:.0}", ctx.cost_per_ft * pipe.length),
    }
}

/// Render template as CSV text.
pub fn render_csv(
    project: &Project,
    analysis: &Analysis,
    template: &ReportTemplate,
) -> String {
    use crate::design::cost::{cost_per_ft, default_cost_table};
    let table = default_cost_table();
    let mut out = template.name.clone();
    out.push('\n');
    for col in &template.columns {
        out.push_str(col.header());
        out.push(',');
    }
    out.pop();
    out.push('\n');
    for (i, _pipe) in project.pipes.iter().enumerate() {
        let cpf = cost_per_ft(project.pipes[i].diameter, &table);
        let ctx = RowContext {
            project,
            analysis,
            pipe_idx: i,
            cost_per_ft: cpf,
        };
        for (j, col) in template.columns.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            let cell = resolve_var(&ctx, *col);
            if cell.contains(',') {
                out.push('"');
                out.push_str(&cell);
                out.push('"');
            } else {
                out.push_str(&cell);
            }
        }
        out.push('\n');
    }
    out
}

/// Render template as simple HTML table.
pub fn render_html_table(
    project: &Project,
    analysis: &Analysis,
    template: &ReportTemplate,
) -> String {
    use crate::design::cost::{cost_per_ft, default_cost_table};
    let table = default_cost_table();
    let mut out = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{}</title>
<style>body{{font-family:Segoe UI,Arial,sans-serif;margin:24px;}}
table{{border-collapse:collapse;width:100%;}} th,td{{border:1px solid #ccc;padding:6px 8px;font-size:0.9rem;}}
th{{background:#f0f4f8;}}</style></head><body>
<h1>{}</h1><p>Project: {}</p><table><thead><tr>",
        template.name, template.name, project.name
    );
    for col in &template.columns {
        out.push_str("<th>");
        out.push_str(col.header());
        out.push_str("</th>");
    }
    out.push_str("</tr></thead><tbody>");
    for (i, _) in project.pipes.iter().enumerate() {
        let cpf = cost_per_ft(project.pipes[i].diameter, &table);
        let ctx = RowContext {
            project,
            analysis,
            pipe_idx: i,
            cost_per_ft: cpf,
        };
        out.push_str("<tr>");
        for col in &template.columns {
            out.push_str("<td>");
            out.push_str(&resolve_var(&ctx, *col));
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></body></html>");
    out
}

/// Load a `.srpt` template file.
pub fn load_template(path: &std::path::Path) -> Result<ReportTemplate, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid template: {e}"))
}

/// Save a `.srpt` template file.
pub fn save_template(template: &ReportTemplate, path: &std::path::Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(template).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_has_header_and_rows() {
        let project = Project::demo();
        let net = project.to_network();
        let a = net
            .analyze(&project.idf(), &project.options())
            .expect("analyze");
        let csv = render_csv(&project, &a, &ReportTemplate::default());
        assert!(csv.contains("Pipe"));
        assert!(csv.contains("P1"));
    }
}