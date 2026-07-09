// SPDX-License-Identifier: GPL-3.0-or-later

//! HTML report export (KaTeX formula panels + hydraulic tables).

use crate::hydrology::IdfSet;
use crate::io::project::Project;
use crate::network::Analysis;
use crate::params::StormAnalysisParams;
use crate::report_html::{format_analysis_html, HtmlReportMeta};
use std::fs;
use std::path::Path;

fn project_to_params(project: &Project) -> StormAnalysisParams {
    let mut params = StormAnalysisParams::municipal();
    let mut idf = IdfSet::municipal_default();
    let rp = project.design_return_period_years.round().max(1.0) as u32;
    idf.set_curve(rp, project.idf());
    idf.set_design_rp(rp);
    params.idf = idf;
    params.hydraulics = project.options();
    params
}

/// Write a self-contained HTML analysis report for the project.
pub fn export_html(project: &Project, analysis: &Analysis, path: &Path) -> Result<(), String> {
    let net = project.to_network();
    let params = project_to_params(project);
    let meta = HtmlReportMeta {
        title: format!("StormSewer — {}", project.name),
        drawing_name: project.name.clone(),
        generated_utc: String::new(),
        project_number: project.report.project_number.clone(),
        engineer: project.report.engineer.clone(),
        firm: project.report.firm.clone(),
        jurisdiction: project.report.jurisdiction.clone(),
    };
    let html = format_analysis_html(&net, analysis, &params, &meta);
    fs::write(path, html).map_err(|e| format!("cannot write {}: {e}", path.display()))
}