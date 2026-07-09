// SPDX-License-Identifier: GPL-3.0-or-later

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
        CostEntry { diameter_in: 66, cost_per_ft: 540.0 },
        CostEntry { diameter_in: 72, cost_per_ft: 625.0 },
    ]
}

/// Lookup installed cost ($/ft) for a pipe diameter, linearly interpolating
/// between the bracketing table entries (clamped to the table's ends).
pub fn cost_per_ft(diameter_ft: f64, table: &[CostEntry]) -> f64 {
    if table.is_empty() {
        return 100.0;
    }
    let dia_in = diameter_ft * 12.0;
    // Table is ascending by diameter. Clamp below the smallest and above the
    // largest so oversized pipes are not silently under-costed at the table cap.
    if dia_in <= table[0].diameter_in as f64 {
        return table[0].cost_per_ft;
    }
    let last = table.last().unwrap();
    if dia_in >= last.diameter_in as f64 {
        return last.cost_per_ft;
    }
    // Find the [lo, hi] bracket and interpolate.
    for pair in table.windows(2) {
        let (lo, hi) = (&pair[0], &pair[1]);
        if dia_in >= lo.diameter_in as f64 && dia_in <= hi.diameter_in as f64 {
            let span = (hi.diameter_in - lo.diameter_in) as f64;
            let t = if span > 0.0 {
                (dia_in - lo.diameter_in as f64) / span
            } else {
                0.0
            };
            return lo.cost_per_ft + t * (hi.cost_per_ft - lo.cost_per_ft);
        }
    }
    last.cost_per_ft
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
    fn large_pipes_are_costed_not_floored_and_interpolated() {
        let table = default_cost_table();
        // 72 in is in the sizing catalog; it must have its own cost, not floor to 60 in.
        let c60 = cost_per_ft(60.0 / 12.0, &table);
        let c72 = cost_per_ft(72.0 / 12.0, &table);
        assert!(c72 > c60, "72in ({c72}) should cost more than 60in ({c60})");
        // A between-size diameter interpolates between its brackets (60 and 66).
        let c63 = cost_per_ft(63.0 / 12.0, &table);
        let c66 = cost_per_ft(66.0 / 12.0, &table);
        assert!(c63 > c60 && c63 < c66, "63in should interpolate between 60 and 66");
    }

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