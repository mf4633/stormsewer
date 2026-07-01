// SPDX-License-Identifier: GPL-3.0-only

//! Pipe construction cost estimation (Hydraflow Cost Codes parity).

use crate::io::project::Project;

/// Cost per linear foot for a pipe diameter (inches, US$/ft).
#[derive(Clone, Debug, PartialEq)]
pub struct CostEntry {
    pub diameter_in: u32,
    pub cost_per_ft: f64,
}

/// Default municipal RCP installed cost table (approximate US$/ft).
pub fn default_cost_table() -> Vec<CostEntry> {
    vec![
        CostEntry { diameter_in: 12, cost_per_ft: 85.0 },
        CostEntry { diameter_in: 15, cost_per_ft: 95.0 },
        CostEntry { diameter_in: 18, cost_per_ft: 110.0 },
        CostEntry { diameter_in: 21, cost_per_ft: 125.0 },
        CostEntry { diameter_in: 24, cost_per_ft: 140.0 },
        CostEntry { diameter_in: 27, cost_per_ft: 160.0 },
        CostEntry { diameter_in: 30, cost_per_ft: 180.0 },
        CostEntry { diameter_in: 33, cost_per_ft: 200.0 },
        CostEntry { diameter_in: 36, cost_per_ft: 225.0 },
        CostEntry { diameter_in: 42, cost_per_ft: 275.0 },
        CostEntry { diameter_in: 48, cost_per_ft: 330.0 },
        CostEntry { diameter_in: 54, cost_per_ft: 390.0 },
        CostEntry { diameter_in: 60, cost_per_ft: 460.0 },
    ]
}

/// Lookup installed cost ($/ft) for a pipe diameter, interpolating from the table.
pub fn cost_per_ft(diameter_ft: f64, table: &[CostEntry]) -> f64 {
    let dia_in = (diameter_ft * 12.0).round() as u32;
    if table.is_empty() {
        return 100.0;
    }
    if let Some(exact) = table.iter().find(|e| e.diameter_in == dia_in) {
        return exact.cost_per_ft;
    }
    let dia_in = dia_in.max(table[0].diameter_in);
    table
        .iter()
        .filter(|e| e.diameter_in <= dia_in)
        .max_by_key(|e| e.diameter_in)
        .or_else(|| table.first())
        .map(|e| e.cost_per_ft)
        .unwrap_or(100.0)
}

/// Per-pipe cost line item.
#[derive(Clone, Debug, PartialEq)]
pub struct PipeCostLine {
    pub pipe_id: String,
    pub length_ft: f64,
    pub diameter_in: f64,
    pub unit_cost: f64,
    pub line_cost: f64,
}

/// Full network cost summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostSummary {
    pub lines: Vec<PipeCostLine>,
    pub total: f64,
}

/// Estimate installed pipe cost for all pipes in a project.
pub fn estimate_network_cost(project: &Project, table: &[CostEntry]) -> CostSummary {
    let mut lines = Vec::with_capacity(project.pipes.len());
    let mut total = 0.0;
    for pipe in &project.pipes {
        let unit = cost_per_ft(pipe.diameter, table);
        let line_cost = unit * pipe.length;
        total += line_cost;
        lines.push(PipeCostLine {
            pipe_id: pipe.id.clone(),
            length_ft: pipe.length,
            diameter_in: pipe.diameter * 12.0,
            unit_cost: unit,
            line_cost,
        });
    }
    CostSummary { lines, total }
}

/// Format a cost summary for display in reports.
pub fn format_cost_summary(summary: &CostSummary) -> String {
    let mut s = String::from("=== PIPE CONSTRUCTION COST ===\n\n");
    if summary.lines.is_empty() {
        s.push_str("No pipes in network.\n");
        return s;
    }
    s.push_str(&format!(
        "{:<6} {:>8} {:>8} {:>10} {:>12}\n",
        "Pipe", "Len(ft)", "Dia(in)", "$/ft", "Line $"
    ));
    s.push_str(&"-".repeat(50));
    s.push('\n');
    for line in &summary.lines {
        s.push_str(&format!(
            "{:<6} {:>8.0} {:>8.0} {:>10.2} {:>12.0}\n",
            line.pipe_id,
            line.length_ft,
            line.diameter_in,
            line.unit_cost,
            line.line_cost
        ));
    }
    s.push_str(&format!("\nTOTAL: ${:.0}\n", summary.total));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::project::ProjectPipe;

    #[test]
    fn cost_scales_with_length() {
        let table = default_cost_table();
        let mut project = Project::demo();
        project.pipes = vec![ProjectPipe::new("P1", "A", "B", 100.0, 1.5, 0.013)];
        let short = estimate_network_cost(&project, &table);
        project.pipes[0].length = 200.0;
        let long = estimate_network_cost(&project, &table);
        assert!((long.total - 2.0 * short.total).abs() < 1e-6);
    }
}