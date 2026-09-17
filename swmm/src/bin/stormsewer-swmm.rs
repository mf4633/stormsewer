// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line front end to the SWMM integration.
//!
//! This exists so the engine registry, the result reader, and the ALR bridge
//! can be exercised end to end before any of it is wired into the app, and so
//! a model can be run and checked from a script.
//!
//! Unlike `runswmm.exe`, which exits 0 whatever happens, this exits non-zero
//! when a run fails — which is the point of parsing the report.

use std::path::PathBuf;
use std::process::ExitCode;

use stormsewer_swmm::alr::{Alr, AlrOptions};
use stormsewer_swmm::doc::{InpDoc, Severity};
use stormsewer_swmm::engine::Registry;
use stormsewer_swmm::out::{format_datetime, LinkVariable, NodeVariable, OutputFile};
use stormsewer_swmm::{rpt, Result};

const USAGE: &str = "\
stormsewer-swmm — run EPA SWMM engines and read their results

USAGE:
    stormsewer-swmm engines
    stormsewer-swmm run <model.inp> [--engine <id>] [--alr]
    stormsewer-swmm info <model.out>
    stormsewer-swmm series <model.out> (--node <name> | --link <name>) [--var <variable>]
    stormsewer-swmm report <model.rpt>
    stormsewer-swmm alr <model.out> [--nodes a,b] [--top N] [--all]
    stormsewer-swmm inp <model.inp>

`inp` parses a model losslessly, proves the round trip, lists its sections,
and prints referential findings; it exits non-zero on an error-level finding.

Node variables: depth head volume lateral-inflow total-inflow flooding
Link variables: flow depth velocity volume capacity

Engines are found in the usual install locations and on PATH. Point at one
explicitly with STORMSEWER_SWMM_ENGINE_DIR. Configure ALR with
STORMSEWER_ALR_SCRIPT (and STORMSEWER_ALR_PYTHON if not `python`).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match dispatch(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Returns whether the command succeeded, separately from whether it ran.
fn dispatch(args: &[String]) -> Result<bool> {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let positional = |n: usize| args.get(n).cloned();

    match args[0].as_str() {
        "engines" => {
            let registry = Registry::discover();
            if registry.is_empty() {
                println!(
                    "No SWMM engines found.\n\
                     Install EPA SWMM, or set STORMSEWER_SWMM_ENGINE_DIR to the folder \
                     holding runswmm."
                );
                return Ok(false);
            }
            for engine in registry.engines() {
                println!("{}", engine.label());
                println!("  id      {}", engine.id);
                println!("  path    {}", engine.exe.display());
                println!("  sha256  {}", engine.sha256);
                if engine.arch.requires_subprocess_from_x64() {
                    println!("  note    32-bit: must run out-of-process from a 64-bit host");
                }
            }
            Ok(true)
        }

        "run" => {
            let inp = PathBuf::from(
                positional(1).ok_or_else(|| err("run needs a path to a .inp model"))?,
            );
            let registry = Registry::discover();
            let engine = match value("--engine") {
                Some(id) => registry
                    .by_id(&id)
                    .ok_or_else(|| err(&format!("no registered engine with id {id}")))?,
                None => registry
                    .default_engine()
                    .ok_or_else(|| err("no SWMM engine found — see `stormsewer-swmm engines`"))?,
            };

            println!("Running {} with {}", inp.display(), engine.label());
            let run = engine.run(&inp)?;
            println!("  finished in {:.2}s", run.elapsed.as_secs_f64());
            println!("  engine    {} ({})", run.engine_version, &run.engine_sha256[..12]);
            println!("  report    {}", run.rpt.display());

            for warning in &run.report.warnings {
                println!("  {warning}");
            }
            for error in &run.report.errors {
                println!("  {error}");
            }
            if let Some((section, pct)) = run.report.worst_continuity() {
                println!("  worst continuity error: {pct:+.3}% ({section})");
            }

            if let Some(reason) = run.failure_reason() {
                println!("FAILED: {reason}");
                return Ok(false);
            }

            let results = OutputFile::open(&run.out)?;
            println!(
                "  results   {} periods, {} nodes, {} links, {}",
                results.meta.n_periods,
                results.meta.n_nodes,
                results.meta.n_links,
                results.meta.flow_units.label()
            );

            if flag("--alr") {
                let alr = Alr::from_env()
                    .ok_or_else(|| err("set STORMSEWER_ALR_SCRIPT to run ALR"))?;
                let report = alr.analyze(&run.out, &AlrOptions::default())?;
                println!("  {}", report.summary());
                for check in report.failures() {
                    println!("    failed: {} {}", check.name, check.detail.clone().unwrap_or_default());
                }
                return Ok(report.all_pass);
            }
            Ok(true)
        }

        "info" => {
            let path = PathBuf::from(
                positional(1).ok_or_else(|| err("info needs a path to a .out file"))?,
            );
            let f = OutputFile::open(&path)?;
            let m = &f.meta;
            println!("{}", path.display());
            println!("  version        {}", m.version);
            println!("  flow units     {}", m.flow_units.label());
            println!("  start          {}", format_datetime(m.start_days));
            println!(
                "  reporting      {} periods every {}s ({:.2} h total)",
                m.n_periods,
                m.report_step_s,
                m.duration_seconds() / 3600.0
            );
            println!(
                "  objects        {} subcatchments, {} nodes, {} links, {} pollutants",
                m.n_subcatch, m.n_nodes, m.n_links, m.n_pollutants
            );
            if m.error_code != 0 {
                println!("  error code     {}", m.error_code);
            }
            print_names("nodes", &m.node_ids);
            print_names("links", &m.link_ids);
            Ok(m.error_code == 0)
        }

        "series" => {
            let path = PathBuf::from(
                positional(1).ok_or_else(|| err("series needs a path to a .out file"))?,
            );
            let f = OutputFile::open(&path)?;
            let var = value("--var").unwrap_or_else(|| "depth".to_string());

            let series = if let Some(node) = value("--node") {
                f.node(&node, node_variable(&var)?)?
            } else if let Some(link) = value("--link") {
                f.link(&link, link_variable(&var)?)?
            } else {
                return Err(err("series needs --node <name> or --link <name>"));
            };

            println!("time_s,value");
            for (t, v) in series.times_s.iter().zip(&series.values) {
                println!("{t},{v}");
            }
            if let Some((peak, at)) = series.peak() {
                eprintln!("peak {peak} at {at}s ({:.2} h)", at / 3600.0);
            }
            Ok(true)
        }

        "report" => {
            let path = PathBuf::from(
                positional(1).ok_or_else(|| err("report needs a path to a .rpt file"))?,
            );
            let summary = rpt::read(&path)?;
            println!(
                "engine {}",
                summary.engine_version.clone().unwrap_or_else(|| "unknown".into())
            );
            if let Some(begun) = &summary.analysis_begun {
                println!("begun  {begun}");
            }
            for (section, pct) in &summary.continuity {
                println!("continuity {pct:+.3}%  {section}");
            }
            for warning in &summary.warnings {
                println!("{warning}");
            }
            for error in &summary.errors {
                println!("{error}");
            }
            println!(
                "{} errors, {} warnings",
                summary.errors.len(),
                summary.warnings.len()
            );
            Ok(summary.is_clean())
        }

        "alr" => {
            let path = PathBuf::from(
                positional(1).ok_or_else(|| err("alr needs a path to a .out file"))?,
            );
            let alr =
                Alr::from_env().ok_or_else(|| err("set STORMSEWER_ALR_SCRIPT to run ALR"))?;
            let options = AlrOptions {
                nodes: value("--nodes")
                    .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
                    .unwrap_or_default(),
                top: value("--top").and_then(|t| t.parse().ok()),
                all: flag("--all"),
            };
            let report = alr.analyze(&path, &options)?;
            println!("{}", report.summary());
            for check in &report.checks {
                let mark = if check.passed { "pass" } else { "FAIL" };
                println!("  {mark}  {} {}", check.name, check.detail.clone().unwrap_or_default());
            }
            Ok(report.all_pass)
        }

        "inp" => {
            let path = PathBuf::from(
                positional(1).ok_or_else(|| err("inp needs a path to a .inp file"))?,
            );
            let text = std::fs::read(&path)?;
            let text = String::from_utf8(text)
                .map_err(|e| err(&format!("{}: not UTF-8: {e}", path.display())))?;
            let doc = InpDoc::parse(&text);
            let lossless = doc.to_string() == text;
            println!(
                "{}: {} sections, round trip {}",
                path.display(),
                doc.sections().len(),
                if lossless { "byte-for-byte" } else { "DIFFERS" }
            );
            for section in doc.sections() {
                println!("  {:<18} {:>5} rows", section.header, section.rows().count());
            }
            let findings = doc.validate();
            let mut errors = 0;
            for f in &findings {
                let mark = match f.severity {
                    Severity::Error => {
                        errors += 1;
                        "error"
                    }
                    Severity::Warning => "warn "
                };
                println!("  {mark}  [{}] {}: {}", f.section, f.name, f.message);
            }
            if findings.is_empty() {
                println!("  no findings");
            }
            Ok(lossless && errors == 0)
        }

        other => Err(err(&format!(
            "unknown command {other:?} — run with --help"
        ))),
    }
}

fn print_names(label: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let shown: Vec<&str> = names.iter().take(12).map(|s| s.as_str()).collect();
    let more = names.len().saturating_sub(shown.len());
    let suffix = if more > 0 { format!(", … {more} more") } else { String::new() };
    println!("  {label:<14} {}{suffix}", shown.join(", "));
}

fn node_variable(name: &str) -> Result<NodeVariable> {
    Ok(match name {
        "depth" => NodeVariable::Depth,
        "head" => NodeVariable::Head,
        "volume" => NodeVariable::Volume,
        "lateral-inflow" => NodeVariable::LateralInflow,
        "total-inflow" | "inflow" => NodeVariable::TotalInflow,
        "flooding" => NodeVariable::Flooding,
        other => return Err(err(&format!("unknown node variable {other:?}"))),
    })
}

fn link_variable(name: &str) -> Result<LinkVariable> {
    Ok(match name {
        "flow" => LinkVariable::Flow,
        "depth" => LinkVariable::Depth,
        "velocity" => LinkVariable::Velocity,
        "volume" => LinkVariable::Volume,
        "capacity" => LinkVariable::Capacity,
        other => return Err(err(&format!("unknown link variable {other:?}"))),
    })
}

fn err(message: &str) -> stormsewer_swmm::Error {
    stormsewer_swmm::Error::NotFound(message.to_string())
}
