// SPDX-License-Identifier: GPL-3.0-or-later

//! 1D-2D coupling: the SWMM network and the surface exchange water at the
//! node and bank interfaces while both advance in time.
//!
//! Two modes. **Tight**: the engine is driven step by step through the
//! bridge (`crate::bridge`), node heads are read and lateral inflows set
//! every synchronisation interval, so surcharge and re-entry are resolved
//! within the same time step. **Iterative**: `runswmm` runs the whole 1D
//! model, its node overflow hydrographs drive the surface, the surface's
//! captured flows are written back as `[INFLOWS]` time series into a
//! scratch copy, and the pair is re-run until the exchange volumes stop
//! changing. Iterative works with any engine and no bridge; tight is the
//! one to use when the surface feeds the network back.
//!
//! # What crosses the interface, and who decides
//!
//! At every synchronisation time `t` both models are at `t`. For each
//! node interface ([`node_exchange`]):
//!
//! - **Surcharge, engine-decided.** When the engine reports a flooding
//!   (overflow) rate at the node, that water has already left the network
//!   (the engine's own continuity books it as flooding loss), so it is put
//!   on the surface as it is: `rate × Δt_sync` into the node's cell. No
//!   lateral inflow is set for it.
//! - **Surcharge, formula-decided.** When the engine reports no flooding
//!   but its head is above the water surface at the cell (a rim above the
//!   DEM ground, a surcharged node with `SurDepth`), the weir/orifice
//!   formula gives the outflow; it is put on the surface and withdrawn
//!   from the node as a *negative* lateral inflow in tight mode. The engine
//!   caps a withdrawal at the node's stored volume, so this path can
//!   over-deliver to the surface by that cap; the summary's `surcharged`
//!   is what the surface received.
//! - **Capture.** An `Inlet`, or a `Manhole` with `lid_open`, takes water
//!   from its cell by the weir/orifice formula while the node head is below
//!   the surface. The volume is withdrawn from the cell *now* (bounded by
//!   what the cell holds), and exactly that volume, as a rate over the
//!   coming interval, is given to the node: a positive lateral inflow in
//!   tight mode, an `[INFLOWS]` series in iterative mode. Conservation is
//!   therefore exact on the capture path and on the engine-decided
//!   surcharge path.
//!
//! Bank interfaces ([`bank_exchange_volumes`]) use the lateral weir formula
//! between the channel surface (node heads interpolated along the link)
//! and each cell under the line; the 1D side is split between the link's
//! end nodes by chainage. Outfall interfaces act as sinks at the outfall's
//! stage (the engine's head in tight mode); what they take leaves the
//! system, as it would through the outfall.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::doc::{Command, InpDoc};
use crate::engine::Engine;
use crate::out::{node_series, NodeVariable, OutputFile};
use crate::{Error, Result};

use super::grid::TimeSeries;
use super::interfaces::{
    bank_exchange, coeff, inlet_capture, manhole_surcharge, opening, DEFAULT_COEFF,
};
use super::solver::{run_surface, Simulator};
use super::{InterfaceKind, LocatedBank, LocatedNode, Progress, RunSummary, Setup};

/// Which coupling to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Step the engine through the bridge executable at `bridge` with the
    /// engine DLL at `dll`, exchanging every `sync_s` seconds.
    Tight {
        bridge: PathBuf,
        dll: PathBuf,
        sync_s: f64,
    },
    /// Re-run `runswmm` up to `iterations` times, stopping early when the
    /// total exchanged volume changes by less than `tolerance` (fraction).
    Iterative { iterations: usize, tolerance: f64 },
}

/// What a coupled run reports beyond the surface summary.
#[derive(Clone, Debug, PartialEq)]
pub struct CoupledSummary {
    pub surface: RunSummary,
    /// The 1D run's files (the scratch copy for iterative mode).
    pub inp: PathBuf,
    pub rpt: PathBuf,
    pub out: PathBuf,
    /// Iterations actually run (1 for tight).
    pub iterations: usize,
    /// Volume that left the network onto the surface, and that came back.
    pub surcharged: f64,
    pub captured: f64,
    pub warnings: Vec<String>,
}

/// The network's state at a node, as the exchange sees it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NodeState1D {
    /// Hydraulic head, model length units.
    pub head: f64,
    /// Flooding (overflow) rate the engine reports, model flow units, ≥ 0.
    pub overflow: f64,
}

/// Exchange rates at a node, in DEM cubic units per second.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NodeExchange {
    /// Onto the surface.
    pub to_surface: f64,
    /// Of that, the part the engine already removed (its reported
    /// overflow) — the rest must be withdrawn from the node.
    pub engine_overflow: f64,
    /// Into the network (capture).
    pub to_network: f64,
}

/// Decide the exchange at a node from the two states (see the module
/// doc). `depth_2d` is the surface depth at the node's cell.
pub fn node_exchange(
    node: &LocatedNode,
    metric: bool,
    g: f64,
    dry_depth: f64,
    depth_2d: f64,
    state: NodeState1D,
    flow_to_cubic: f64,
) -> NodeExchange {
    let iface = &node.interface;
    if iface.kind == InterfaceKind::Sealed {
        return NodeExchange::default();
    }
    let c = coeff(iface);
    let (perimeter, area) = opening(iface, metric);
    let mut ex = NodeExchange::default();
    if state.overflow > 0.0 {
        ex.engine_overflow = state.overflow * flow_to_cubic;
        ex.to_surface = ex.engine_overflow;
        return ex;
    }
    let surcharge = manhole_surcharge(
        g, c, perimeter, area, state.head, node.ground, depth_2d, dry_depth,
    );
    if surcharge > 0.0 {
        ex.to_surface = surcharge;
        return ex;
    }
    let captures = match iface.kind {
        InterfaceKind::Inlet { .. } => true,
        InterfaceKind::Manhole => iface.lid_open,
        InterfaceKind::Sealed => false,
    };
    if captures && depth_2d > dry_depth {
        ex.to_network = inlet_capture(g, c, perimeter, area, depth_2d, state.head, node.ground);
    }
    ex
}

/// Move the exchange's volumes for an interval into and out of the
/// simulator. Returns the volumes actually moved `(to_surface,
/// to_network)`; a withdrawal is bounded by what the cell holds.
pub fn apply_node_exchange(sim: &mut Simulator, node: &LocatedNode, ex: &NodeExchange, dt: f64) -> (f64, f64) {
    let mut to_surface = 0.0;
    let mut to_network = 0.0;
    if ex.to_surface > 0.0 {
        to_surface = sim.add_volume(node.col, node.row, ex.to_surface * dt);
    }
    if ex.to_network > 0.0 {
        to_network = -sim.add_volume(node.col, node.row, -ex.to_network * dt);
    }
    (to_surface, to_network)
}

/// Bank exchange for an interval: returns the volume onto the surface
/// (signed, positive = channel → surface) and the split of the 1D side
/// between the link's from and to nodes (positive = leaves the node).
pub fn bank_exchange_volumes(
    sim: &mut Simulator,
    bank: &LocatedBank,
    head_from: f64,
    head_to: f64,
    dt: f64,
) -> (f64, f64, f64) {
    let c = bank.interface.weir_coeff.filter(|c| *c > 0.0).unwrap_or(DEFAULT_COEFF);
    let n = bank.cells.len();
    let mut from_node = 0.0;
    let mut to_node = 0.0;
    let mut total = 0.0;
    for (k, &(col, row, crest)) in bank.cells.iter().enumerate() {
        let f = if n > 1 { k as f64 / (n - 1) as f64 } else { 0.5 };
        let eta_1d = head_from + f * (head_to - head_from);
        let eta_2d = sim.surface(col, row);
        let q = bank_exchange(sim.g, c, sim.dx, crest, eta_1d, eta_2d);
        if q == 0.0 {
            continue;
        }
        let moved = sim.add_volume(col, row, q * dt);
        total += moved;
        from_node += moved * (1.0 - f);
        to_node += moved * f;
    }
    (total, from_node, to_node)
}

fn link_ends(doc: &InpDoc, link: &str) -> Option<(String, String)> {
    let from = doc.field("CONDUITS", link, "FromNode")?.to_string();
    let to = doc.field("CONDUITS", link, "ToNode")?.to_string();
    Some((from, to))
}

/// Run the surface and the network together. Writes `setup.results_path()`
/// and the 1D results; the progress callback returns `false` to stop.
pub fn run_coupled(
    setup: &Setup,
    engine: &Engine,
    mode: &Mode,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<CoupledSummary> {
    let doc = InpDoc::read(&setup.model)?;
    match mode {
        Mode::Tight { bridge, dll, sync_s } => tight(setup, &doc, bridge, dll, *sync_s, progress),
        Mode::Iterative { iterations, tolerance } => {
            iterative(setup, &doc, engine, *iterations, *tolerance, progress)
        }
    }
}

// ---------------------------------------------------------------------------
// Tight coupling through the bridge
// ---------------------------------------------------------------------------

fn tight(
    setup: &Setup,
    doc: &InpDoc,
    bridge: &std::path::Path,
    dll: &std::path::Path,
    sync_s: f64,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<CoupledSummary> {
    let text = doc.to_string();
    let prepared = crate::engine::prepare(&setup.model, &text, false)?;
    let paths = prepared.paths.clone();
    let mut session = crate::bridge::Session::open(bridge, dll, &paths.inp, &paths.rpt, &paths.out)?;
    let sync = if sync_s > 0.0 { sync_s } else { session.routing_step_s.max(1.0) };
    let mut sim = Simulator::from_setup(setup);
    let metric = setup.metric;
    let g = sim.g;
    let dry = sim.dry_depth;
    let to_cubic = setup.inputs.flow_to_cubic;
    let bank_ends: Vec<Option<(String, String)>> = setup
        .banks
        .iter()
        .map(|b| link_ends(doc, &b.interface.link))
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    let mut surcharged = 0.0;
    let mut captured = 0.0;
    let mut engine_t = 0.0;
    let mut finished_early = false;
    let mut lateral: HashMap<String, f64> = HashMap::new();
    let mut failure: Option<Error> = None;

    let result = run_surface(setup, &mut sim, sync, progress, &mut |sim, t, dt, node_vol| {
        if finished_early {
            return Ok(());
        }
        if t > engine_t {
            match session.step_until(t)? {
                Some(now) => engine_t = now,
                None => {
                    finished_early = true;
                    warnings.push(format!("the engine finished at {engine_t:.0} s, before the 2D run"));
                    return Ok(());
                }
            }
        }
        lateral.clear();
        for (i, node) in setup.nodes.iter().enumerate() {
            let name = &node.interface.node;
            let state = NodeState1D {
                head: session.node_head(name)?,
                overflow: session.node_overflow(name)?.max(0.0),
            };
            let depth = sim.depth(node.col, node.row);
            let ex = node_exchange(node, metric, g, dry, depth, state, to_cubic);
            let (vs, vn) = apply_node_exchange(sim, node, &ex, dt);
            node_vol[i] += vs - vn;
            surcharged += vs;
            captured += vn;
            let formula_out = if ex.engine_overflow > 0.0 { 0.0 } else { vs };
            let flow = (vn - formula_out) / dt / to_cubic;
            *lateral.entry(name.clone()).or_insert(0.0) += flow;
        }
        for (bank, ends) in setup.banks.iter().zip(&bank_ends) {
            let Some((from, to)) = ends else { continue };
            let head_from = session.node_head(from)?;
            let head_to = session.node_head(to)?;
            let (total, v_from, v_to) = bank_exchange_volumes(sim, bank, head_from, head_to, dt);
            if total > 0.0 {
                surcharged += total;
            } else {
                captured -= total;
            }
            *lateral.entry(from.clone()).or_insert(0.0) -= v_from / dt / to_cubic;
            *lateral.entry(to.clone()).or_insert(0.0) -= v_to / dt / to_cubic;
        }
        for k in 0..sim.sinks.len() {
            let node = &setup.nodes[sim.sinks[k].node];
            if let Ok(h) = session.node_head(&node.interface.node) {
                sim.sinks[k].stage = h;
            }
        }
        for (name, flow) in &lateral {
            if let Err(e) = session.set_node_lateral_inflow(name, *flow) {
                failure = Some(e);
                return Err(Error::Engine(format!("could not set the lateral inflow at {name}")));
            }
        }
        Ok(())
    });
    let surface = match result {
        Ok(s) => s,
        Err(e) => {
            session.abort();
            return Err(failure.unwrap_or(e));
        }
    };
    // Let the engine run out to its own end so the report is complete.
    while session.step()?.is_some() {}
    let (runoff, flow, _quality) = session.finish()?;
    if flow.abs() > 5.0 || runoff.abs() > 5.0 {
        warnings.push(format!(
            "engine continuity errors: runoff {runoff:+.2}%, flow routing {flow:+.2}%"
        ));
    }
    warnings.extend(surface.warnings.iter().cloned());
    Ok(CoupledSummary {
        surface,
        inp: paths.inp,
        rpt: paths.rpt,
        out: paths.out,
        iterations: 1,
        surcharged,
        captured,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Iterative coupling through runswmm
// ---------------------------------------------------------------------------

/// Prefix of the `[TIMESERIES]` this coupler writes into the scratch model.
pub const SERIES_PREFIX: &str = "SS2D_";

/// Piecewise-linear lookup that holds the end values outside the series
/// (the engine's first report is one step in, so `t = 0` reads the first
/// period).
fn at_or_edge(s: &TimeSeries, t: f64) -> f64 {
    match s.t.first() {
        None => 0.0,
        Some(&t0) if t <= t0 => s.v[0],
        _ => match s.t.last() {
            Some(&t1) if t >= t1 => *s.v.last().unwrap(),
            _ => s.linear_at(t),
        },
    }
}

fn out_series(out: &OutputFile, node: &str, var: NodeVariable) -> Option<TimeSeries> {
    let s = node_series(&out.path, &out.meta, node, var as usize).ok()?;
    Some(TimeSeries {
        t: s.times_s,
        v: s.values,
    })
}

/// Decimal hours with enough digits for a one-second sync step.
fn hours(t: f64) -> String {
    let s = format!("{:.5}", t / 3600.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Replace the coupler's own rows in a scratch document with the captured
/// series: one `[TIMESERIES]` per node with any capture and one
/// `[INFLOWS]` row naming it. Nodes that already have a `FLOW` inflow are
/// left alone (the engine would replace the user's inflow) and reported.
fn write_captures(
    scratch: &mut InpDoc,
    captures: &[(String, Vec<(f64, f64)>)],
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut cmds = Vec::new();
    // Old rows out: our series, and inflow rows that name them.
    let old_series: Vec<String> = scratch
        .names("TIMESERIES")
        .into_iter()
        .filter(|n| n.starts_with(SERIES_PREFIX))
        .collect();
    for name in &old_series {
        cmds.push(Command::DeleteObject {
            section: "TIMESERIES".into(),
            name: name.clone(),
        });
    }
    let mut inflow_lines: Vec<usize> = scratch
        .rows("INFLOWS")
        .into_iter()
        .filter(|(_, r)| r.value(2).is_some_and(|s| s.starts_with(SERIES_PREFIX)))
        .map(|(i, _)| i)
        .collect();
    inflow_lines.sort_unstable_by(|a, b| b.cmp(a));
    for line in inflow_lines {
        cmds.push(Command::DeleteLine {
            section: "INFLOWS".into(),
            line,
        });
    }
    scratch.apply(Command::Batch(cmds))?;

    let mut cmds = Vec::new();
    for (node, series) in captures {
        if series.iter().all(|(_, q)| *q <= 0.0) {
            continue;
        }
        let has_flow_inflow = scratch.inflows(node).iter().any(|r| {
            r.value(1).is_some_and(|c| c.eq_ignore_ascii_case("FLOW"))
                && !r.value(2).is_some_and(|s| s.starts_with(SERIES_PREFIX))
        });
        if has_flow_inflow {
            warnings.push(format!(
                "{node} already has a FLOW inflow; its captured surface flow was not written back"
            ));
            continue;
        }
        let series_name = format!("{SERIES_PREFIX}{node}");
        for (t, q) in series {
            cmds.push(Command::AddRow {
                section: "TIMESERIES".into(),
                fields: vec![series_name.clone(), hours(*t), format!("{q:.6}")],
                comment: None,
            });
        }
        cmds.push(Command::AddRow {
            section: "INFLOWS".into(),
            fields: vec![
                node.clone(),
                "FLOW".into(),
                series_name,
                "FLOW".into(),
                "1.0".into(),
                "1.0".into(),
            ],
            comment: None,
        });
    }
    scratch.apply(Command::Batch(cmds))?;
    Ok(())
}

fn iterative(
    setup: &Setup,
    doc: &InpDoc,
    engine: &Engine,
    iterations: usize,
    tolerance: f64,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<CoupledSummary> {
    let iterations = iterations.max(1);
    let text = doc.to_string();
    let prepared = crate::engine::prepare(&setup.model, &text, true)?;
    let paths = prepared.paths.clone();
    let mut scratch = InpDoc::read(&paths.inp)?;
    let metric = setup.metric;
    let to_cubic = setup.inputs.flow_to_cubic;
    let sync = setup.config.output_step_s.min(60.0);
    let bank_ends: Vec<Option<(String, String)>> = setup
        .banks
        .iter()
        .map(|b| link_ends(doc, &b.interface.link))
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    let mut prev_total: Option<f64> = None;
    let mut last: Option<(RunSummary, f64, f64, usize)> = None;

    for it in 1..=iterations {
        let run = engine.run_prepared(&prepared)?;
        if let Some(reason) = run.failure_reason() {
            return Err(Error::Engine(format!("1D run failed (iteration {it}): {reason}")));
        }
        let out = OutputFile::open(&run.out)?;
        let heads: Vec<Option<TimeSeries>> = setup
            .nodes
            .iter()
            .map(|n| out_series(&out, &n.interface.node, NodeVariable::Head))
            .collect();
        let floods: Vec<Option<TimeSeries>> = setup
            .nodes
            .iter()
            .map(|n| out_series(&out, &n.interface.node, NodeVariable::Flooding))
            .collect();
        if it == 1 {
            for (n, h) in setup.nodes.iter().zip(&heads) {
                if h.is_none() {
                    warnings.push(format!(
                        "{} is not in the engine's results (check [REPORT] NODES); no exchange there",
                        n.interface.node
                    ));
                }
            }
        }
        let head_of = |name: &str, t: f64| -> Option<f64> {
            let i = setup.nodes.iter().position(|n| n.interface.node.eq_ignore_ascii_case(name))?;
            heads[i].as_ref().map(|s| at_or_edge(s, t))
        };
        let mut sim = Simulator::from_setup(setup);
        let g = sim.g;
        let dry = sim.dry_depth;
        let mut captures: Vec<(String, Vec<(f64, f64)>)> = setup
            .nodes
            .iter()
            .map(|n| (n.interface.node.clone(), Vec::new()))
            .collect();
        let mut surcharged = 0.0;
        let mut captured = 0.0;
        let surface = run_surface(setup, &mut sim, sync, progress, &mut |sim, t, dt, node_vol| {
            let mut lateral: HashMap<usize, f64> = HashMap::new();
            for (i, node) in setup.nodes.iter().enumerate() {
                let (Some(h), Some(f)) = (&heads[i], &floods[i]) else { continue };
                let state = NodeState1D {
                    head: at_or_edge(h, t),
                    overflow: at_or_edge(f, t).max(0.0),
                };
                let depth = sim.depth(node.col, node.row);
                let ex = node_exchange(node, metric, g, dry, depth, state, to_cubic);
                let (vs, vn) = apply_node_exchange(sim, node, &ex, dt);
                node_vol[i] += vs - vn;
                surcharged += vs;
                captured += vn;
                *lateral.entry(i).or_insert(0.0) += vn / dt / to_cubic;
            }
            for (bank, ends) in setup.banks.iter().zip(&bank_ends) {
                let Some((from, to)) = ends else { continue };
                let (Some(hf), Some(ht)) = (head_of(from, t), head_of(to, t)) else { continue };
                let (total, v_from, v_to) = bank_exchange_volumes(sim, bank, hf, ht, dt);
                if total > 0.0 {
                    surcharged += total;
                } else {
                    captured -= total;
                }
                for (name, v) in [(from, v_from), (to, v_to)] {
                    if let Some(i) = setup.nodes.iter().position(|n| n.interface.node.eq_ignore_ascii_case(name)) {
                        *lateral.entry(i).or_insert(0.0) -= v / dt / to_cubic;
                    }
                }
            }
            for k in 0..sim.sinks.len() {
                let node = &setup.nodes[sim.sinks[k].node];
                if let Some(h) = head_of(&node.interface.node, t) {
                    sim.sinks[k].stage = h;
                }
            }
            for (i, q) in lateral {
                captures[i].1.push((t, q));
            }
            Ok(())
        })?;
        let total = surcharged + captured;
        let converged = prev_total.is_some_and(|p| (total - p).abs() <= tolerance * total.max(1e-9));
        prev_total = Some(total);
        last = Some((surface, surcharged, captured, it));
        if converged || it == iterations {
            if !converged && iterations > 1 {
                warnings.push(format!(
                    "iterative coupling did not converge in {iterations} iterations (last change {:.3} vs tolerance {:.3})",
                    (total - prev_total.unwrap_or(total)).abs(),
                    tolerance
                ));
            }
            break;
        }
        write_captures(&mut scratch, &captures, &mut warnings)?;
        scratch.write(&paths.inp)?;
    }
    let (surface, surcharged, captured, iterations) = last.expect("at least one iteration");
    warnings.extend(surface.warnings.iter().cloned());
    Ok(CoupledSummary {
        surface,
        inp: paths.inp,
        rpt: paths.rpt,
        out: paths.out,
        iterations,
        surcharged,
        captured,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_are_written_as_series_and_inflow_rows_and_replaced_on_rerun() {
        let mut doc = InpDoc::parse(
            "[JUNCTIONS]\nJ1 10 3\nJ2 10 3\n[INFLOWS]\nJ2 FLOW Q2 FLOW 1.0 1.0\n[TIMESERIES]\nQ2 0:00 1\n",
        );
        let mut warnings = Vec::new();
        let caps = vec![
            ("J1".to_string(), vec![(0.0, 0.0), (60.0, 0.5), (120.0, 0.25)]),
            ("J2".to_string(), vec![(0.0, 1.0)]),
        ];
        write_captures(&mut doc, &caps, &mut warnings).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let series = doc.timeseries("SS2D_J1");
        assert_eq!(series.len(), 3);
        assert_eq!((series[1].time.as_str(), series[1].value.as_str()), ("0.01667", "0.500000"));
        let inflow = doc.inflows("J1");
        assert_eq!(inflow.len(), 1);
        assert_eq!(
            inflow[0].fields,
            vec!["J1", "FLOW", "SS2D_J1", "FLOW", "1.0", "1.0"]
        );
        assert!(!doc.to_string().contains("SS2D_J2"));
        assert_eq!(doc.inflows("J2").len(), 1);
        // A second write replaces, not duplicates.
        let caps = vec![("J1".to_string(), vec![(0.0, 2.0)])];
        write_captures(&mut doc, &caps, &mut warnings).unwrap();
        assert_eq!(doc.timeseries("SS2D_J1").len(), 1);
        assert_eq!(doc.inflows("J1").len(), 1);
    }

    #[test]
    fn edge_hold_reads_the_first_period_at_time_zero() {
        let s = TimeSeries {
            t: vec![60.0, 120.0],
            v: vec![3.0, 5.0],
        };
        assert_eq!(at_or_edge(&s, 0.0), 3.0);
        assert_eq!(at_or_edge(&s, 90.0), 4.0);
        assert_eq!(at_or_edge(&s, 500.0), 5.0);
    }
}
