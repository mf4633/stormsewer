// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end 2D runs through the editor, as a user would do them: open an
//! EPA sample model, point 2D Setup at a DEM, Save the sidecar, run from
//! the 2D menu, watch the worker thread finish, look at the overlay and
//! export the maximum depth. The coupled runs need an installed EPA SWMM
//! engine (and, for tight coupling, the 32-bit bridge from
//! `scripts/build-bridge.ps1`); without one they print a note and return.
//! These are the steps of the tutorial in `docs/20-2d-overland.md`.

use std::path::Path;

use stormsewer_swmm::engine::Engine;
use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::twod::Config;

use super::tests::{temp_dir, Harness};
use super::*;

/// Where the ground is at a node: its rim when it has a MaxDepth, else
/// (SWMM's rule for MaxDepth 0) the crown of its highest connecting
/// conduit, both with `LINK_OFFSETS DEPTH` as the EPA samples use.
fn ground_at(ed: &SwmmEditor, kind: stormsewer_swmm::doc::build::NodeType, name: &str) -> Option<f64> {
    let rim = node_rim(ed, kind, name)?;
    let sec = kind.section();
    let max_depth: f64 = ed.doc.field(sec, name, "MaxDepth").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    if max_depth > 0.0 {
        return Some(rim);
    }
    let num = |s: Option<&str>| s.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let mut crown: f64 = 0.0;
    for link in ed.doc.names("CONDUITS") {
        let geom1 = num(ed.doc.field("XSECTIONS", &link, "Geom1"));
        if ed.doc.field("CONDUITS", &link, "FromNode").is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            crown = crown.max(num(ed.doc.field("CONDUITS", &link, "InOffset")) + geom1);
        }
        if ed.doc.field("CONDUITS", &link, "ToNode").is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            crown = crown.max(num(ed.doc.field("CONDUITS", &link, "OutOffset")) + geom1);
        }
    }
    Some(rim + crown)
}

/// A DEM through the ground at every node (inverse-distance weighted
/// between them). `cell` in model units, a cell of margin all round.
fn write_ground_dem(ed: &SwmmEditor, path: &Path, cell: f64) {
    write_dem_through(ed, path, cell, ground_at);
}

/// A DEM through `z(node)` at every node.
fn write_dem_through(
    ed: &SwmmEditor,
    path: &Path,
    cell: f64,
    z: fn(&SwmmEditor, stormsewer_swmm::doc::build::NodeType, &str) -> Option<f64>,
) {
    let pts: Vec<(f64, f64, f64)> = ed
        .nodes
        .iter()
        .filter_map(|n| z(ed, n.kind, &n.name).map(|z| (n.x, n.y, z)))
        .collect();
    assert!(pts.len() >= 3, "the model has nodes with elevations");
    let (x0, y0, x1, y1) = ed.bounds.unwrap();
    let ncols = ((x1 - x0) / cell).ceil() as usize + 3;
    let nrows = ((y1 - y0) / cell).ceil() as usize + 3;
    let (ox, oy) = (x0 - cell, y0 - cell);
    let mut r = Raster::filled(ncols, nrows, ox, oy, cell, 0.0);
    for row in 0..nrows {
        for col in 0..ncols {
            // Row 0 is the top (north) row of an ESRI grid.
            let x = ox + (col as f64 + 0.5) * cell;
            let y = oy + (nrows - row) as f64 * cell - 0.5 * cell;
            let (mut num, mut den) = (0.0, 0.0);
            let mut exact = None;
            for &(px, py, z) in &pts {
                let d2 = (px - x).powi(2) + (py - y).powi(2);
                if d2 < 1e-6 {
                    exact = Some(z);
                    break;
                }
                num += z / d2;
                den += 1.0 / d2;
            }
            r.set(col, row, exact.unwrap_or(num / den));
        }
    }
    r.write_asc(path).unwrap();
}

/// Open the `2D` menu and click an item in it.
fn menu_item(h: &mut Harness, item: &str) {
    h.frame(vec![], 1.0);
    let menu = h.text_pos("2D").expect("the 2D menu is on the bar");
    h.click_at(menu);
    h.frame(vec![], 0.05);
    let p = h.text_pos(item).unwrap_or_else(|| panic!("{item} is in the 2D menu"));
    h.click_at(p);
}

/// The lowest painted copy of a label (below the menu bar's namesakes).
fn click_lowest(h: &mut Harness, text: &str) {
    h.frame(vec![], 0.05);
    let p = h
        .text_positions(text)
        .into_iter()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| panic!("{text} drawn: {:?}", h.all_texts()));
    h.click_at(p);
}

/// Tutorial steps 1–3: the model, the DEM, rain on the grid, a short
/// duration, optionally inlets at the junctions, saved through the Setup
/// window's Save button.
fn set_up_wet_run(dir: &Path, fixture: &str, name: &str, minutes: f64, inlets: bool) -> Harness {
    let mut h = Harness::open_fixture(dir, fixture, name);
    let dem = dir.join("ground.asc");
    write_ground_dem(h.ed(), &dem, 20.0);
    menu_item(&mut h, "2D Setup…");
    assert!(h.td().setup_open);
    {
        let f = &mut h.app.state.swmm_doc.twod.fields;
        f.dem = dem.to_string_lossy().into_owned();
        f.rain_kind = 2;
        f.rain_const = 3.0;
        f.boundary_kind = 1;
        f.duration_on = true;
        f.duration_h = minutes / 60.0;
        f.output_step_s = 300.0;
    }
    if inlets {
        // 2D → Interfaces…: select everything, make the junctions inlets
        // that take surface water in, seal the outfalls.
        menu_item(&mut h, "Interfaces…");
        h.app.state.swmm_doc.select_all();
        h.app.state.swmm_doc.twod.bulk_kind = 1;
        click_lowest(&mut h, "Apply to selection");
        assert!(h.app.state.status.contains("set to Inlet"), "{}", h.app.state.status);
        click_lowest(&mut h, "Seal all outfalls");
        let d = &h.td().draft;
        assert!(d.nodes.iter().any(|n| matches!(n.kind, twod::InterfaceKind::Inlet { .. })), "{:?}", d.nodes);
        assert!(d.nodes.iter().any(|n| n.kind == twod::InterfaceKind::Sealed), "{:?}", d.nodes);
        h.app.state.swmm_doc.twod.interfaces_open = false;
    }
    h.frame(vec![], 0.05);
    h.frame(vec![], 1.0);
    let save = h.text_pos("Save").expect("Save button drawn");
    h.click_at(save);
    assert!(h.td().sidecar_status.starts_with("Saved"), "{}", h.td().sidecar_status);
    let sidecar = dir.join(name).with_extension("2d");
    let back = Config::read(&sidecar).unwrap();
    assert_eq!(back.dem.as_deref(), Some(dem.as_path()));
    assert_eq!(back.rain, twod::RainOnGrid::Constant(3.0));
    assert_eq!(back.duration_s, Some(minutes * 60.0));
    h
}

/// Tutorial steps 5–6: the overlay shows frames, the maximum depth
/// exports as a grid that reads back with water in it.
fn check_results(h: &mut Harness, dir: &Path) -> f64 {
    let (ncols, nrows, n_frames) = {
        let r = h.td().results.as_ref().unwrap_or_else(|| panic!("{:?}", h.td().results_error));
        (r.ncols, r.nrows, r.n_frames)
    };
    assert!(n_frames >= 2, "{n_frames} frames");
    assert!(h.td().overlay.on);
    h.frame(vec![], 0.05);
    assert!(h.td().overlay.texture.is_some(), "{:?}", h.td().overlay.error);
    // The last frame is wet somewhere.
    h.app.state.swmm_doc.twod.overlay.frame = n_frames - 1;
    h.frame(vec![], 0.05);
    h.frame(vec![], 0.05);
    let wet = h
        .td()
        .overlay
        .frame_data
        .as_ref()
        .map(|f| f.depth.iter().filter(|d| **d > 0.01).count())
        .unwrap_or(0);
    assert!(wet > 0, "the last frame has wet cells");
    let out = dir.join("max_depth.asc");
    let msg = export_grid(h.td(), ExportKind::MaxDepth, &out).unwrap();
    assert!(msg.contains("max_depth.asc"), "{msg}");
    let back = Raster::read_asc(&out).unwrap();
    assert_eq!((back.ncols, back.nrows), (ncols, nrows));
    let peak = back.data.iter().copied().filter(|v| back.nodata != Some(*v)).fold(0.0, f64::max);
    assert!(peak > 0.0, "the max-depth grid has water");
    let hazard = dir.join("hazard.asc");
    export_grid(h.td(), ExportKind::Hazard, &hazard).unwrap();
    assert!(Raster::read_asc(&hazard).is_ok());
    peak
}

#[test]
fn rain_on_grid_run_from_the_menu_fills_the_overlay_and_exports() {
    let dir = temp_dir("e2e-2d");
    let mut h = set_up_wet_run(&dir, "Site_Drainage_Model.inp", "site.inp", 20.0, false);
    // Step 4: 2D → Run 2D Only; the worker thread reports and finishes.
    menu_item(&mut h, "Run 2D Only");
    assert!(h.td().run.is_some(), "{:?}", h.td().run_view);
    assert!(h.td().run_open);
    h.frame(vec![], 0.05);
    assert!(h.text_pos("Run 2D").is_some(), "the run window is drawn");
    h.wait_run(180);
    let v = h.td().run_view.clone();
    assert!(v.error.is_none() && !v.stopped, "{v:?}");
    let s = v.outcome.as_ref().expect("finished").surface().clone();
    eprintln!(
        "2D only: {} steps, {} frames, {:.2} s, mass error {:+.4}%, peak {:.3} ft, wet {:.0} ft², in {:.1} out {:.1} stored {:.1} ft³",
        s.steps, s.frames, s.elapsed_s, s.mass_error_pct, s.peak_depth, s.wet_area_max, s.inflow, s.outflow, s.stored
    );
    assert_eq!(s.frames, 5, "0 … 20 minutes every 5");
    assert!(s.inflow > 0.0 && s.peak_depth > 0.0, "{s:?}");
    assert!(s.mass_error_pct.abs() < 1.0, "{s:?}");
    // The window shows the summary.
    h.frame(vec![], 0.05);
    assert!(h.text_pos("Run finished and wrote results").is_some());
    assert!(h.app.state.status.starts_with("2D run finished"), "{}", h.app.state.status);
    let peak = check_results(&mut h, &dir);
    assert!((peak - s.peak_depth).abs() < 1e-3 * s.peak_depth.max(1.0), "{peak} vs {}", s.peak_depth);
    // Load 2D Results re-reads the same file from the menu.
    menu_item(&mut h, "Load 2D Results");
    assert!(h.app.state.status.starts_with("Loaded"), "{}", h.app.state.status);
    assert_eq!(h.td().results.as_ref().unwrap().n_frames, 5);
}

#[test]
fn stop_2d_ends_a_long_run_early() {
    let dir = temp_dir("e2e-stop");
    let mut h = set_up_wet_run(&dir, "Site_Drainage_Model.inp", "site.inp", 600.0, false);
    menu_item(&mut h, "Run 2D Only");
    assert!(h.td().run.is_some(), "{:?}", h.td().run_view);
    // Let it report once, then Stop 2D from the menu.
    let started = std::time::Instant::now();
    while h.td().run_view.last.is_none() && h.td().run.is_some() {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.frame(vec![], 0.05);
        assert!(started.elapsed().as_secs() < 60, "no progress");
    }
    menu_item(&mut h, "Stop 2D");
    assert!(h.td().run_view.stopped || h.td().run.is_none());
    h.wait_run(60);
    let v = h.td().run_view.clone();
    let t = v.last.as_ref().map(|p| p.time_s).unwrap_or(0.0);
    assert!(t < 600.0 * 60.0, "stopped before the end: {v:?}");
    assert!(v.error.is_some() || v.outcome.is_some(), "{v:?}");
}

/// The installed engine the coupled tests use: for tight coupling, one
/// with a `swmm5.dll` beside it that the 32-bit bridge can load.
fn engine_for(h: &mut Harness, tight: bool) -> Option<Engine> {
    h.app.state.swmm.ensure_discovered();
    let engines = h.app.state.swmm.registry.engines().to_vec();
    if tight {
        engines
            .into_iter()
            .find(|e| matches!(e.arch, stormsewer_swmm::pe::Arch::X86) && bridge::find_dll(&e.exe).is_some())
    } else {
        engines.into_iter().next()
    }
}

/// Run Coupled (1D-2D)… from the menu, choose the mode in the window,
/// press Run, and wait.
fn coupled_run(h: &mut Harness, engine: &Engine, tight: bool) -> CoupledSummary {
    menu_item(h, "Run Coupled (1D-2D)…");
    {
        let d = h.app.state.swmm_doc.twod.coupled.as_mut().expect("the dialog opened");
        d.engine_id = Some(engine.id.clone());
        refresh_bridge(d, &h.app.state.swmm.registry);
        if tight {
            assert!(d.bridge.is_some() && d.dll.is_some(), "bridge {:?} dll {:?}", d.bridge, d.dll);
        }
        d.tight = tight;
        d.sync_s = 30.0;
        d.iterations = 3;
        d.tolerance = 0.02;
    }
    h.frame(vec![], 0.05);
    h.frame(vec![], 1.0);
    click_lowest(h, "Run");
    assert!(h.td().coupled.is_none(), "the dialog closed: {:?}", h.td().coupled);
    assert!(h.td().run.as_ref().is_some_and(|j| j.coupled), "{:?}", h.td().run_view);
    h.wait_run(300);
    let v = h.td().run_view.clone();
    assert!(v.error.is_none() && !v.stopped, "{v:?}");
    match v.outcome {
        Some(RunOutcome::Coupled(c)) => c,
        other => panic!("{other:?}"),
    }
}

fn report(label: &str, c: &CoupledSummary) {
    let s = &c.surface;
    eprintln!(
        "{label}: {} iteration(s), {} steps, {} frames, {:.2} s, surface mass error {:+.4}%, peak {:.3} ft, \
         surcharged {:.1} ft³, captured {:.1} ft³, rain in {:.1} out {:.1} stored {:.1} ft³, warnings {:?}",
        c.iterations,
        s.steps,
        s.frames,
        s.elapsed_s,
        s.mass_error_pct,
        s.peak_depth,
        c.surcharged,
        c.captured,
        s.inflow,
        s.outflow,
        s.stored,
        c.warnings
    );
}

#[test]
fn coupled_iterative_run_exchanges_water_at_the_manholes() {
    let dir = temp_dir("e2e-iterative");
    let mut h = set_up_wet_run(&dir, "Site_Drainage_Model.inp", "site.inp", 30.0, true);
    let Some(engine) = engine_for(&mut h, false) else {
        eprintln!("SKIPPED: no EPA SWMM engine installed; the iterative coupled run needs runswmm");
        return;
    };
    let c = coupled_run(&mut h, &engine, false);
    report("iterative", &c);
    assert!((1..=3).contains(&c.iterations));
    assert!(c.captured > 0.0, "rain on the grid drains into the manholes");
    assert!(c.inp.exists() && c.rpt.exists() && c.out.exists(), "{c:?}");
    assert_ne!(c.inp, dir.join("site.inp"), "the model's own .inp is not rewritten");
    assert!(c.surface.mass_error_pct.abs() < 1.0, "{c:?}");
    // The captured inflow is on the node exchange series.
    check_results(&mut h, &dir);
    assert!(!h.td().exchange.is_empty(), "exchange series loaded");
    // The window shows the coupled rows.
    h.frame(vec![], 0.05);
    assert!(h.text_pos("Surcharged / captured").is_some());
}

#[test]
fn coupled_tight_run_steps_the_engine_through_the_bridge() {
    let dir = temp_dir("e2e-tight");
    let mut h = set_up_wet_run(&dir, "Site_Drainage_Model.inp", "site.inp", 30.0, true);
    if bridge::find_bridge().is_none() {
        eprintln!("SKIPPED: no {} (powershell -File scripts/build-bridge.ps1)", bridge::BRIDGE_EXE);
        return;
    }
    let Some(engine) = engine_for(&mut h, true) else {
        eprintln!("SKIPPED: no 32-bit EPA SWMM engine with swmm5.dll for the bridge");
        return;
    };
    let c = coupled_run(&mut h, &engine, true);
    report("tight", &c);
    assert_eq!(c.iterations, 1);
    assert!(c.captured > 0.0, "rain on the grid drains into the manholes");
    assert!(c.rpt.exists() && c.out.exists(), "{c:?}");
    assert!(c.surface.mass_error_pct.abs() < 1.0, "{c:?}");
    check_results(&mut h, &dir);
}

/// ENGINE BUG REPRODUCTION (ignored until the engine stream fixes it; run
/// with `--ignored`). With the ground at each node's `Elevation +
/// MaxDepth` — the invert, for the MaxDepth-0 junctions of the Site
/// Drainage sample — any flow in the network puts the 1D head above the
/// ground, the manhole formula surcharges, and tight coupling withdraws
/// that volume through a negative lateral inflow the node does not hold.
/// Observed: surcharged 13,709 ft³, engine flow-routing continuity
/// -666.90%. The withdrawal should be bounded by what the node can give
/// (or the surcharge taken from the engine's own overflow only).
#[test]
#[ignore = "engine bug: tight-coupling surcharge withdrawal is unbounded (see doc comment)"]
fn tight_surcharge_keeps_engine_continuity() {
    let dir = temp_dir("e2e-tight-invert");
    let mut h = Harness::open_fixture(&dir, "Site_Drainage_Model.inp", "site.inp");
    let dem = dir.join("ground.asc");
    write_dem_through(h.ed(), &dem, 20.0, node_rim);
    {
        let f = &mut h.app.state.swmm_doc.twod.fields;
        f.dem = dem.to_string_lossy().into_owned();
        f.rain_kind = 2;
        f.rain_const = 3.0;
        f.boundary_kind = 1;
        f.duration_on = true;
        f.duration_h = 0.5;
        f.output_step_s = 300.0;
    }
    assert!(save_sidecar(&mut h.app.state.swmm_doc).starts_with("Saved"));
    if bridge::find_bridge().is_none() {
        eprintln!("SKIPPED: no bridge");
        return;
    }
    let Some(engine) = engine_for(&mut h, true) else {
        eprintln!("SKIPPED: no 32-bit engine");
        return;
    };
    let c = coupled_run(&mut h, &engine, true);
    report("tight, ground at invert", &c);
    assert!(
        !c.warnings.iter().any(|w| w.contains("continuity")),
        "the network lost water the coupling took: {:?}",
        c.warnings
    );
}
