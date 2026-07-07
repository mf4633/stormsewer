// SPDX-License-Identifier: GPL-3.0-or-later

//! Network topology and data-quality diagnostics.

use std::collections::{HashMap, HashSet};

use crate::io::project::Project;

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagSeverity {
    Error,
    Warning,
    Info,
}

/// A single network diagnostic message.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: DiagSeverity,
    pub id: String,
    pub message: String,
}

/// Run validation errors plus extended topology/data checks.
pub fn run_diagnostics(project: &Project) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = project
        .validate()
        .into_iter()
        .map(|msg| Diagnostic {
            severity: DiagSeverity::Error,
            id: "validation".into(),
            message: msg,
        })
        .collect();

    let node_ids: HashSet<&str> = project.nodes.iter().map(|n| n.id.as_str()).collect();

    // Orphan nodes (no connected pipes).
    for node in &project.nodes {
        let connected = project.pipes.iter().any(|p| p.from == node.id || p.to == node.id);
        if !connected && node.kind != "outfall" {
            out.push(Diagnostic {
                severity: DiagSeverity::Warning,
                id: node.id.clone(),
                message: format!("{} ({}) has no connected pipes", node.id, node.kind),
            });
        }
    }

    // Duplicate pipe connections.
    let mut seen_pairs = HashSet::new();
    for pipe in &project.pipes {
        let key = (pipe.from.as_str(), pipe.to.as_str());
        if !seen_pairs.insert(key) {
            out.push(Diagnostic {
                severity: DiagSeverity::Warning,
                id: pipe.id.clone(),
                message: format!("duplicate pipe connection {} → {}", pipe.from, pipe.to),
            });
        }
    }

    // Inlets without tributary area.
    for node in &project.nodes {
        if node.kind == "inlet" && node.area_ac <= 0.0 {
            let has_catchment = project.catchments.iter().any(|c| {
                c.inlet_node_id.as_deref() == Some(node.id.as_str())
            });
            if !has_catchment {
                out.push(Diagnostic {
                    severity: DiagSeverity::Warning,
                    id: node.id.clone(),
                    message: format!("{} has no tributary area or linked catchment", node.id),
                });
            }
        }
    }

    // Catchments without inlet link.
    for catchment in &project.catchments {
        if catchment.inlet_node_id.is_none() {
            out.push(Diagnostic {
                severity: DiagSeverity::Warning,
                id: catchment.id.clone(),
                message: format!("{} is not linked to an inlet", catchment.id),
            });
        } else if let Some(ref inlet_id) = catchment.inlet_node_id {
            if !node_ids.contains(inlet_id.as_str()) {
                out.push(Diagnostic {
                    severity: DiagSeverity::Error,
                    id: catchment.id.clone(),
                    message: format!(
                        "{} links to missing inlet {}",
                        catchment.id, inlet_id
                    ),
                });
            }
        }
        if catchment.vertices.len() < 3 {
            out.push(Diagnostic {
                severity: DiagSeverity::Error,
                id: catchment.id.clone(),
                message: format!("{} has fewer than 3 vertices", catchment.id),
            });
        }
    }

    // Multiple outfalls.
    let outfall_count = project.nodes.iter().filter(|n| n.kind == "outfall").count();
    if outfall_count > 1 {
        out.push(Diagnostic {
            severity: DiagSeverity::Info,
            id: "network".into(),
            message: format!("network has {outfall_count} outfalls (branched system)"),
        });
    }

    // Unreachable nodes upstream of outfall (simple BFS from outfalls downstream→upstream).
    let mut downstream_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for pipe in &project.pipes {
        downstream_of.entry(pipe.to.as_str()).or_default().push(pipe.from.as_str());
    }
    let mut reachable = HashSet::new();
    let mut stack: Vec<&str> = project
        .nodes
        .iter()
        .filter(|n| n.kind == "outfall")
        .map(|n| n.id.as_str())
        .collect();
    while let Some(id) = stack.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(upstream) = downstream_of.get(id) {
            for u in upstream {
                stack.push(u);
            }
        }
    }
    for node in &project.nodes {
        if !reachable.contains(node.id.as_str()) {
            out.push(Diagnostic {
                severity: DiagSeverity::Warning,
                id: node.id.clone(),
                message: format!("{} is not connected to an outfall", node.id),
            });
        }
    }

    out
}

/// Format diagnostics for the report panel.
pub fn format_diagnostics(diags: &[Diagnostic]) -> String {
    if diags.is_empty() {
        return "Network diagnostics: no issues found.".into();
    }
    let mut s = String::from("=== NETWORK DIAGNOSTICS ===\n\n");
    for d in diags {
        let tag = match d.severity {
            DiagSeverity::Error => "ERROR",
            DiagSeverity::Warning => "WARN",
            DiagSeverity::Info => "INFO",
        };
        s.push_str(&format!("[{tag}] {}: {}\n", d.id, d.message));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_has_no_errors() {
        let diags = run_diagnostics(&Project::demo());
        assert!(!diags.iter().any(|d| d.severity == DiagSeverity::Error));
    }
}