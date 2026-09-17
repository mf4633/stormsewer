// SPDX-License-Identifier: GPL-3.0-or-later

//! Differences between two runs' peaks: the same model on two engines, or
//! two edits of one model on one engine. Pure: takes the peak lists the
//! `.out` reader already produces and tabulates them.

use crate::out::{LinkPeak, NodePeak};
use crate::rpt::csv_line;

/// Which node peak is compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeMetric {
    MaxDepth,
    MaxInflow,
    MaxFlooding,
}

impl NodeMetric {
    pub const ALL: [NodeMetric; 3] = [
        NodeMetric::MaxDepth,
        NodeMetric::MaxInflow,
        NodeMetric::MaxFlooding,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::MaxDepth => "Max depth",
            Self::MaxInflow => "Max total inflow",
            Self::MaxFlooding => "Max flooding",
        }
    }

    fn of(self, p: &NodePeak) -> f64 {
        match self {
            Self::MaxDepth => p.max_depth,
            Self::MaxInflow => p.max_total_inflow,
            Self::MaxFlooding => p.max_flooding,
        }
    }
}

/// Which link peak is compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkMetric {
    MaxFlow,
    MaxVelocity,
    MaxCapacity,
}

impl LinkMetric {
    pub const ALL: [LinkMetric; 3] = [
        LinkMetric::MaxFlow,
        LinkMetric::MaxVelocity,
        LinkMetric::MaxCapacity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::MaxFlow => "Max flow",
            Self::MaxVelocity => "Max velocity",
            Self::MaxCapacity => "Max capacity (fraction full)",
        }
    }

    fn of(self, p: &LinkPeak) -> f64 {
        match self {
            Self::MaxFlow => p.max_flow,
            Self::MaxVelocity => p.max_velocity,
            Self::MaxCapacity => p.max_capacity,
        }
    }
}

/// One object's value in run A and run B.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRow {
    pub id: String,
    pub a: f64,
    pub b: f64,
}

impl DiffRow {
    /// `B − A`.
    pub fn delta(&self) -> f64 {
        self.b - self.a
    }

    /// `100·(B − A)/|A|`, or `None` when A is zero (the ratio is undefined,
    /// not infinite; the absolute difference still says what changed).
    pub fn percent(&self) -> Option<f64> {
        (self.a.abs() > 1e-12).then(|| 100.0 * self.delta() / self.a.abs())
    }
}

/// The tabulated difference, with the objects only one run reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    pub rows: Vec<DiffRow>,
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
}

impl Diff {
    /// Largest |Δ| first.
    pub fn sort_by_abs_delta(&mut self) {
        self.rows
            .sort_by(|x, y| y.delta().abs().total_cmp(&x.delta().abs()).then(x.id.cmp(&y.id)));
    }

    /// Largest |%| first; rows without a percentage last.
    pub fn sort_by_percent(&mut self) {
        self.rows.sort_by(|x, y| match (x.percent(), y.percent()) {
            (Some(a), Some(b)) => b.abs().total_cmp(&a.abs()).then(x.id.cmp(&y.id)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => x.id.cmp(&y.id),
        });
    }

    pub fn is_identical(&self, tol: f64) -> bool {
        self.only_a.is_empty()
            && self.only_b.is_empty()
            && self.rows.iter().all(|r| r.delta().abs() <= tol)
    }

    pub fn max_abs_delta(&self) -> f64 {
        self.rows
            .iter()
            .map(|r| r.delta().abs())
            .fold(0.0, f64::max)
    }

    /// `Object,A,B,Delta,Percent` with the objects only one run has listed
    /// after the rows.
    pub fn to_csv(&self, metric: &str) -> String {
        let mut out = csv_line(&[
            "Object".to_string(),
            format!("{metric} A"),
            format!("{metric} B"),
            "Delta".to_string(),
            "Percent".to_string(),
        ]);
        for r in &self.rows {
            out.push_str(&csv_line(&[
                r.id.clone(),
                format!("{:.4}", r.a),
                format!("{:.4}", r.b),
                format!("{:.4}", r.delta()),
                r.percent().map(|p| format!("{p:.2}")).unwrap_or_default(),
            ]));
        }
        for id in &self.only_a {
            out.push_str(&csv_line(&[id.as_str(), "", "", "", "only in A"]));
        }
        for id in &self.only_b {
            out.push_str(&csv_line(&[id.as_str(), "", "", "", "only in B"]));
        }
        out
    }
}

fn diff_by<T>(a: &[T], b: &[T], id: impl Fn(&T) -> &str, value: impl Fn(&T) -> f64) -> Diff {
    let mut diff = Diff::default();
    for pa in a {
        match b.iter().find(|pb| id(pb).eq_ignore_ascii_case(id(pa))) {
            Some(pb) => diff.rows.push(DiffRow {
                id: id(pa).to_string(),
                a: value(pa),
                b: value(pb),
            }),
            None => diff.only_a.push(id(pa).to_string()),
        }
    }
    for pb in b {
        if !a.iter().any(|pa| id(pa).eq_ignore_ascii_case(id(pb))) {
            diff.only_b.push(id(pb).to_string());
        }
    }
    diff
}

pub fn diff_nodes(a: &[NodePeak], b: &[NodePeak], metric: NodeMetric) -> Diff {
    diff_by(a, b, |p| p.id.as_str(), |p| metric.of(p))
}

pub fn diff_links(a: &[LinkPeak], b: &[LinkPeak], metric: LinkMetric) -> Diff {
    diff_by(a, b, |p| p.id.as_str(), |p| metric.of(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::out::{link_peaks, node_peaks, OutputFile};
    use std::path::PathBuf;

    fn pond() -> OutputFile {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/results/Detention_Pond_Model.out");
        OutputFile::open(path).unwrap()
    }

    #[test]
    fn a_run_against_itself_is_all_zero() {
        let out = pond();
        let nodes = node_peaks(&out.path, &out.meta).unwrap();
        let links = link_peaks(&out.path, &out.meta).unwrap();
        for m in NodeMetric::ALL {
            let d = diff_nodes(&nodes, &nodes, m);
            assert_eq!(d.rows.len(), 14);
            assert!(d.is_identical(0.0), "{}", m.label());
            assert_eq!(d.max_abs_delta(), 0.0);
        }
        for m in LinkMetric::ALL {
            let d = diff_links(&links, &links, m);
            assert_eq!(d.rows.len(), 14);
            assert!(d.is_identical(0.0), "{}", m.label());
        }
    }

    #[test]
    fn a_perturbed_copy_tabulates_sorts_and_exports() {
        let out = pond();
        let nodes = node_peaks(&out.path, &out.meta).unwrap();
        let mut other = nodes.clone();
        // J1 up 10 %, SU1 doubled, O2 dropped, a new node added.
        for p in &mut other {
            if p.id == "J1" {
                p.max_depth *= 1.10;
            }
            if p.id == "SU1" {
                p.max_depth *= 2.0;
            }
        }
        other.retain(|p| p.id != "O2");
        other.push(NodePeak {
            id: "NEW".into(),
            max_depth: 1.0,
            depth_at_s: 0.0,
            max_total_inflow: 0.0,
            max_flooding: 0.0,
        });
        let mut d = diff_nodes(&nodes, &other, NodeMetric::MaxDepth);
        assert_eq!(d.rows.len(), 13);
        assert_eq!(d.only_a, vec!["O2"]);
        assert_eq!(d.only_b, vec!["NEW"]);
        assert!(!d.is_identical(1e-9));
        let su1 = d.rows.iter().find(|r| r.id == "SU1").unwrap().clone();
        assert!((su1.b - 2.0 * su1.a).abs() < 1e-9);
        assert!((su1.percent().unwrap() - 100.0).abs() < 1e-9);
        let j1 = d.rows.iter().find(|r| r.id == "J1").unwrap().clone();
        assert!((j1.percent().unwrap() - 10.0).abs() < 1e-6);
        d.sort_by_abs_delta();
        assert_eq!(d.rows[0].id, "SU1", "the pond's depth change is the largest");
        assert!((d.max_abs_delta() - su1.delta().abs()).abs() < 1e-12);
        d.sort_by_percent();
        assert_eq!(d.rows[0].id, "SU1");
        assert_eq!(d.rows[1].id, "J1");
        // Unchanged rows have 0 %; a zero-in-A row has none and sorts last.
        let mut z = Diff {
            rows: vec![
                DiffRow { id: "x".into(), a: 0.0, b: 1.0 },
                DiffRow { id: "y".into(), a: 2.0, b: 2.5 },
            ],
            ..Default::default()
        };
        z.sort_by_percent();
        assert_eq!(z.rows[0].id, "y");
        assert_eq!(z.rows[1].percent(), None);
        let csv = d.to_csv("Max depth");
        assert!(csv.starts_with("Object,Max depth A,Max depth B,Delta,Percent\n"));
        assert!(csv.contains("SU1,"));
        assert!(csv.contains("O2,,,,only in A\n"));
        assert!(csv.contains("NEW,,,,only in B\n"));
    }
}
