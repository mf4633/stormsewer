// SPDX-License-Identifier: GPL-3.0-or-later

//! `stormsewer-cli` — run a storm-sewer analysis from a network file.
//!
//! Usage:  stormsewer-cli [--size] [--review] <network-file>
//!
//! The file may be a `.ssn` text network (see `stormsewer::parse`), a
//! StormSewer `.ssproj` project, a Hydraflow / Civil 3D `.stm`, a LandXML
//! `.xml`, or a `.dxf` exported by StormSewer. Project-type files analyze
//! exactly as the desktop app does (IDF set, tailwater, structure K, and
//! per-pipe inverts from the file) and also print the HEC-22 inlet schedule.
//!
//! Flags:
//!   --size    Also run pipe sizing and print the sizing table.
//!   --review  Also run design review and print findings.

use std::process::exit;
use stormsewer::design::inlets::{
    format_inlet_rows, network_inlet_pass_for_project, InletGeometry,
};
use stormsewer::design::{
    design_review, format_design_review, format_sizing_table, size_network, DesignCriteria,
    ReviewCriteria,
};
use stormsewer::io::Project;
use stormsewer::parse::parse_ssn;
use stormsewer::report::format_analysis;

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let size = if let Some(i) = args.iter().position(|a| a == "--size") {
        args.remove(i);
        true
    } else {
        false
    };
    let review = if let Some(i) = args.iter().position(|a| a == "--review") {
        args.remove(i);
        true
    } else {
        false
    };

    let path = match args.into_iter().next() {
        Some(p) => p,
        None => die("usage: stormsewer-cli [--size] [--review] <network-file>"),
    };

    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    // Project-type inputs go through the same path as the desktop app.
    let (network, idf, options, inlet_text) = match ext.as_str() {
        "ssn" | "" => {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
            let parsed = parse_ssn(&text).unwrap_or_else(|e| die(&e));
            (parsed.network, parsed.idf, parsed.options, None)
        }
        _ => {
            let project = load_project(&path).unwrap_or_else(|e| die(&e));
            let errors = project.validate();
            if !errors.is_empty() {
                die(&format!(
                    "project is not analyzable:\n  {}",
                    errors.join("\n  ")
                ));
            }
            let network = project.to_analysis_network();
            let idf = *project.idf_set().design_curve();
            let options = project.options();
            let rows = network_inlet_pass_for_project(&project, &InletGeometry::default());
            (network, idf, options, Some(format_inlet_rows(&rows)))
        }
    };

    match network.analyze(&idf, &options) {
        Ok(a) => {
            print!("{}", format_analysis(&a));
            if let Some(t) = inlet_text {
                print!("{t}");
            }

            if size {
                let criteria = DesignCriteria::default();
                let recs = size_network(&network, &a, &criteria);
                print!("\n{}", format_sizing_table(&recs));
            }

            if review {
                let criteria = ReviewCriteria::default();
                let findings = design_review(&network, &a, &criteria);
                print!("\n{}", format_design_review(&findings));
            }
        }
        Err(e) => die(&e.to_string()),
    }
}

/// Open a project-type file by extension.
fn load_project(path: &str) -> Result<Project, String> {
    let p = std::path::Path::new(path);
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("ssproj") => Project::load(p),
        Some("stm") => stormsewer::io::import_stm(p),
        Some("xml") => stormsewer::io::import_landxml(p),
        Some("dxf") => stormsewer::io::import_dxf(p),
        other => Err(format!(
            "unsupported file type {:?}: expected .ssn, .ssproj, .stm, .xml, or .dxf",
            other.unwrap_or("")
        )),
    }
}
