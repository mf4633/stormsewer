// SPDX-License-Identifier: GPL-3.0-or-later

//! Validation of the 2D overland-flow solver against analytical solutions
//! and conservation identities. Each test names its reference; the
//! numbers asserted here are the ones quoted in the manual's 2D methods
//! chapter (`docs/20b-2d-methods.md`).

use std::path::{Path, PathBuf};

use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::twod::grid::{LocatedSource, TimeSeries};
use stormsewer_swmm::twod::solver::{Scheme, Simulator};
use stormsewer_swmm::twod::{self, Boundary, Config, RainOnGrid, Results, Setup};

/// A small deterministic pseudo-random sequence (LCG) so the "random"
/// bed is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("stormsewer-twod-validation").join(format!(
        "{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn constant_series(q: f64) -> TimeSeries {
    TimeSeries {
        t: vec![0.0, 1e9],
        v: vec![q, q],
    }
}

// ---------------------------------------------------------------------------
// 1. Lake at rest: a still water surface over an uneven bed must stay
//    still. The local-inertial flux is driven by the free-surface slope,
//    which is exactly zero, so any motion is a bug (well-balancedness).
// ---------------------------------------------------------------------------
fn lake_at_rest(scheme: Scheme) {
    let (nc, nr) = (50, 50);
    let mut rng = Lcg(7);
    let mut bed = Raster::filled(nc, nr, 0.0, 0.0, 2.0, 0.0);
    for v in &mut bed.data {
        *v = 100.0 + 3.0 * rng.next(); // bed between 100 and 103
    }
    bed.data[nc * 10 + 10] = f64::NAN; // a no-data island: a wall
    let mut sim = Simulator::new(&bed, 0.035, true, 0.003, 0.7);
    sim.scheme = scheme;
    sim.fill_to(104.0);
    let before: Vec<f64> = sim.depths().to_vec();
    for _ in 0..1000 {
        sim.step(10.0);
    }
    let mut max_v: f64 = 0.0;
    for row in 0..nr {
        for col in 0..nc {
            let (u, v) = sim.velocity(col, row);
            max_v = max_v.max(u.abs()).max(v.abs());
        }
    }
    assert!(max_v <= 1e-12, "velocity {max_v} on a lake at rest");
    let drift = sim
        .depths()
        .iter()
        .zip(&before)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(drift <= 1e-12, "depth drift {drift}");
    assert!(sim.mass_error_pct().abs() < 1e-9);
}

#[test]
fn lake_at_rest_on_a_random_bed_stays_still() {
    lake_at_rest(Scheme::LocalInertial);
}

#[test]
fn lake_at_rest_is_also_exact_for_the_hll_scheme() {
    lake_at_rest(Scheme::Hll);
}

// ---------------------------------------------------------------------------
// 2. Steady uniform flow on a plane obeys Manning's equation:
//    h_n = (q n / (k √S))^{3/5}. Plane 200 m long at S = 0.01, n = 0.03,
//    q = 0.1 m²/s in from the top edge, open downstream boundary.
//    h_n = (0.1 × 0.03 / (1 × 0.1))^{0.6} = 0.03^{0.6} = 0.12203 m.
// ---------------------------------------------------------------------------
fn uniform_flow(scheme: Scheme) {
    let (nc, nr) = (200, 5);
    let dx = 1.0;
    let slope = 0.01;
    let n = 0.03;
    let q = 0.1;
    let mut bed = Raster::filled(nc, nr, 0.0, 0.0, dx, 0.0);
    for row in 0..nr {
        for col in 0..nc {
            bed.data[row * nc + col] = 10.0 - slope * (col as f64 + 0.5) * dx;
        }
    }
    let mut sim = Simulator::new(&bed, n, true, 0.001, 0.7);
    sim.scheme = scheme;
    sim.boundary = Boundary::Open;
    for row in 0..nr {
        sim.sources.push(LocatedSource {
            name: format!("in{row}"),
            col: 0,
            row,
            series: constant_series(q * dx),
        });
    }
    sim.advance_to(2000.0);
    let h_n = (q * n / (1.0 * slope.sqrt())).powf(0.6);
    assert!((h_n - 0.12203).abs() < 1e-4, "{h_n}");
    for col in [80, 100, 120, 150] {
        let h = sim.depth(col, 2);
        let err = (h - h_n).abs() / h_n;
        assert!(err < 0.01, "col {col}: depth {h:.5} vs Manning {h_n:.5} ({:.2}%)", err * 100.0);
        // And the unit discharge is the inflow: q = h u.
        let qf = match scheme {
            Scheme::LocalInertial => sim.qx_face(col, 2),
            Scheme::Hll => sim.depth(col, 2) * sim.velocity(col, 2).0,
        };
        // HLL's first-order diffusion costs about 1.5 % on q here.
        let q_tol = match scheme {
            Scheme::LocalInertial => 0.01,
            Scheme::Hll => 0.02,
        };
        assert!((qf - q).abs() / q < q_tol, "col {col}: q {qf:.5} vs {q}");
    }
    // Depth is the same across the plane (no cross-slope).
    assert!((sim.depth(100, 0) - sim.depth(100, 4)).abs() < 1e-6);
}

#[test]
fn steady_uniform_flow_matches_manning_within_one_percent() {
    uniform_flow(Scheme::LocalInertial);
}

#[test]
fn steady_uniform_flow_matches_manning_with_the_hll_scheme_too() {
    uniform_flow(Scheme::Hll);
}

// ---------------------------------------------------------------------------
// 3. Dam break on a flat, frictionless, dry bed: Ritter (1892), as given in
//    Stoker (1957) *Water Waves* §10.8. With c₀ = √(g h₀), for
//    −c₀t ≤ x' ≤ 2c₀t the depth is h = (2c₀ − x'/t)² / (9g), h₀ upstream
//    of −c₀t and dry beyond 2c₀t. Checked at t = 40 s away from the two
//    fronts (the negative wave at −c₀t and the dry tip at 2c₀t), where a
//    first-order scheme smears the solution.
// ---------------------------------------------------------------------------
#[test]
fn dam_break_matches_ritter_away_from_the_fronts() {
    let (nc, nr) = (1200, 3);
    let dx = 1.0;
    let g = 9.81;
    let h0 = 1.0;
    let bed = Raster::filled(nc, nr, 0.0, 0.0, dx, 0.0);
    // Ritter is a full-SWE solution with Froude numbers up to 2 at the
    // tip; the local-inertial scheme drops the convective term and is not
    // meant for it, so this case runs the full-dynamic HLL scheme.
    let mut sim = Simulator::new(&bed, 0.0, true, 1e-4, 0.7);
    sim.scheme = Scheme::Hll;
    let dam = 600usize;
    let depths: Vec<f64> = (0..nc * nr)
        .map(|i| if i % nc < dam { h0 } else { 0.0 })
        .collect();
    sim.set_depths(&depths);
    let t = 40.0;
    sim.advance_to(t);
    let c0 = (g * h0).sqrt();
    let mut worst = 0.0f64;
    let mut checked = 0;
    for col in 0..nc {
        let x = (col as f64 + 0.5) * dx - dam as f64 * dx;
        // Away from the fronts: 20% inside each.
        if x < -0.8 * c0 * t || x > 1.5 * c0 * t {
            continue;
        }
        let h_ref = (2.0 * c0 - x / t).powi(2) / (9.0 * g);
        let h = sim.depth(col, 1);
        let err = (h - h_ref).abs() / h0;
        worst = worst.max(err);
        checked += 1;
    }
    assert!(checked > 200);
    assert!(worst < 0.05, "worst depth error {:.2}% of h0", worst * 100.0);
    // The still-water depth and the dry bed far from the dam are untouched.
    assert!((sim.depth(10, 1) - h0).abs() < 1e-9);
    assert!(sim.depth(nc - 10, 1) < 1e-9);
    assert!(sim.mass_error_pct().abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// 4. Mass conservation: a closed basin fed by a source must hold exactly
//    what came in (less nothing — no infiltration, no boundary).
// ---------------------------------------------------------------------------
#[test]
fn closed_basin_with_a_source_conserves_mass() {
    let (nc, nr) = (60, 60);
    let mut bed = Raster::filled(nc, nr, 0.0, 0.0, 1.0, 0.0);
    for row in 0..nr {
        for col in 0..nc {
            let x = col as f64 - 29.5;
            let y = row as f64 - 29.5;
            bed.data[row * nc + col] = 0.002 * (x * x + y * y);
        }
    }
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    sim.sources.push(LocatedSource {
        name: "in".into(),
        col: 30,
        row: 30,
        series: TimeSeries {
            t: vec![0.0, 300.0, 600.0],
            v: vec![0.0, 2.0, 0.0],
        },
    });
    sim.advance_to(900.0);
    let expected = 600.0; // ∫ of the triangle: 0.5 × 600 s × 2 m³/s
    assert!((sim.balance.inflow - expected).abs() < 1e-9, "{}", sim.balance.inflow);
    let err = sim.mass_error_pct().abs();
    assert!(err < 0.1, "mass error {err}%");
    assert!((sim.stored() - expected).abs() / expected < 1e-3);
    // Nothing left through the closed edges.
    assert_eq!(sim.balance.outflow, 0.0);
}

// ---------------------------------------------------------------------------
// 5. Symmetry: a source at the centre of a flat plate spreads the same way
//    in every direction, so the depth field is mirror-symmetric in x and y
//    and under the diagonal swap.
// ---------------------------------------------------------------------------
#[test]
fn a_centre_source_on_a_flat_plate_spreads_symmetrically() {
    let n = 41;
    let bed = Raster::filled(n, n, 0.0, 0.0, 1.0, 5.0);
    let mut sim = Simulator::new(&bed, 0.03, false, 0.003, 0.7);
    sim.sources.push(LocatedSource {
        name: "in".into(),
        col: 20,
        row: 20,
        series: constant_series(0.5),
    });
    sim.advance_to(120.0);
    let mut worst = 0.0f64;
    for row in 0..n {
        for col in 0..n {
            let h = sim.depth(col, row);
            let mirror_x = sim.depth(n - 1 - col, row);
            let mirror_y = sim.depth(col, n - 1 - row);
            let diag = sim.depth(row, col);
            worst = worst
                .max((h - mirror_x).abs())
                .max((h - mirror_y).abs())
                .max((h - diag).abs());
        }
    }
    assert!(worst < 1e-9, "asymmetry {worst}");
    assert!(sim.depth(20, 20) > sim.depth(0, 0));
    assert!(sim.depth(25, 20) > 0.0, "water has spread");
}

// ---------------------------------------------------------------------------
// 6. CFL: the time step shrinks as the water deepens, as α Δx / √(g h).
// ---------------------------------------------------------------------------
#[test]
fn time_step_shrinks_when_depth_grows() {
    let bed = Raster::filled(10, 10, 0.0, 0.0, 2.0, 0.0);
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    sim.fill_to(0.25);
    let dt_shallow = sim.cfl_dt();
    assert!((dt_shallow - 0.7 * 2.0 / (9.81f64 * 0.25).sqrt()).abs() < 1e-12);
    sim.fill_to(4.0);
    let dt_deep = sim.cfl_dt();
    assert!(dt_deep < dt_shallow);
    assert!((dt_shallow / dt_deep - 4.0).abs() < 1e-9, "ratio {}", dt_shallow / dt_deep);
    sim.max_dt = 0.01;
    assert_eq!(sim.cfl_dt(), 0.01);
}

// ---------------------------------------------------------------------------
// 7. A closed basin loses water only where it should: infiltration and
//    sinks are booked, boundary outflow is zero, and the balance closes.
// ---------------------------------------------------------------------------
#[test]
fn rain_infiltration_and_fixed_head_edge_are_booked() {
    let bed = Raster::filled(30, 30, 0.0, 0.0, 1.0, 10.0);
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    sim.rain = twod::grid::RainInput::Constant(1e-5); // 36 mm/h
    sim.infiltration = twod::grid::InfiltrationInput::Constant(2e-6);
    sim.advance_to(600.0);
    let rained = 1e-5 * 600.0 * 900.0;
    assert!((sim.balance.inflow - rained).abs() < 1e-9);
    assert!((sim.balance.infiltrated - 2e-6 * 600.0 * 900.0).abs() < 1e-9);
    assert!(sim.mass_error_pct().abs() < 1e-9);
    // Now open one side to a fixed head below the bed: water drains.
    sim.boundary = Boundary::FixedHead(9.0);
    let before = sim.stored();
    sim.advance_to(1200.0);
    assert!(sim.balance.outflow > 0.0);
    assert!(sim.stored() < before);
    assert!(sim.mass_error_pct().abs() < 1e-9, "{}", sim.mass_error_pct());
}

// ---------------------------------------------------------------------------
// 8. Performance smoke: 300 × 300 cells for 60 s of flow at ~1 m depth.
// ---------------------------------------------------------------------------
#[test]
#[cfg_attr(debug_assertions, ignore = "timing test; run in release")]
fn a_300_by_300_grid_runs_a_minute_in_a_few_seconds() {
    let n = 300;
    let mut bed = Raster::filled(n, n, 0.0, 0.0, 1.0, 0.0);
    for row in 0..n {
        for col in 0..n {
            bed.data[row * n + col] = 0.001 * (col as f64) + 0.0005 * (row as f64);
        }
    }
    let mut sim = Simulator::new(&bed, 0.03, true, 0.003, 0.7);
    let depths: Vec<f64> = (0..n * n).map(|i| if i % n < 100 { 1.0 } else { 0.0 }).collect();
    sim.set_depths(&depths);
    let started = std::time::Instant::now();
    sim.advance_to(60.0);
    let elapsed = started.elapsed().as_secs_f64();
    assert!(sim.steps > 100);
    assert!(elapsed < 5.0, "{elapsed:.2}s for {} steps", sim.steps);
}

// ---------------------------------------------------------------------------
// 9. The whole run path: Setup → run → results file → readers.
// ---------------------------------------------------------------------------
#[test]
fn a_surface_run_writes_a_readable_results_file() {
    let dir = tmp_dir("run");
    let model = dir.join("plate.inp");
    let mut bed = Raster::filled(20, 15, 100.0, 200.0, 2.0, 50.0);
    for row in 0..15 {
        for col in 0..20 {
            bed.data[row * 20 + col] = 50.0 + 0.01 * col as f64;
        }
    }
    bed.data[0] = f64::NAN;
    let config = Config {
        duration_s: Some(120.0),
        output_step_s: 30.0,
        boundary: Boundary::Open,
        rain: RainOnGrid::Constant(1.0),
        ..Config::default()
    };
    let mut setup = Setup::from_grid(&model, bed, 0.03, false, config);
    setup.inputs.rain = twod::grid::RainInput::Constant(2.0 / 12.0 / 3600.0); // 2 in/h
    let mut seen = Vec::new();
    let summary = twod::run(&setup, &mut |p| {
        seen.push(p.time_s);
        true
    })
    .unwrap();
    assert_eq!(summary.results, twod::results_path(&model));
    assert_eq!(summary.frames, 5);
    assert_eq!(seen, vec![30.0, 60.0, 90.0, 120.0]);
    assert!(summary.inflow > 0.0);
    assert!(summary.mass_error_pct.abs() < 1e-6, "{}", summary.mass_error_pct);
    let r = Results::open(&summary.results).unwrap();
    assert_eq!((r.ncols, r.nrows), (20, 15));
    assert_eq!(r.n_frames, 5);
    assert!(!r.metric);
    assert!(r.is_sealed());
    let last = r.frame(4).unwrap();
    assert_eq!(last.time_s, 120.0);
    assert!(last.depth[0].is_nan(), "no-data stays NaN");
    assert!(last.depth[25] > 0.0);
    let md = r.max_depth().unwrap();
    assert!(md.get(5, 5).unwrap() >= last.depth[5 * 20 + 5] as f64 - 1e-6);
    let ar = r.arrival().unwrap();
    assert!(ar.get(5, 5).unwrap() > 0.0);
    let (d, _, _) = r.sample(&last, 111.0, 211.0).unwrap();
    assert!(d > 0.0);
    // The sidecar round-trips through the file system beside the model.
    let side = Config::sidecar_path(&model);
    assert_eq!(side, dir.join("plate.2d"));
    setup.config.write(&side).unwrap();
    assert_eq!(Config::read(&side).unwrap(), setup.config);
    assert_eq!(Config::read(Path::new("does-not-exist.2d")).unwrap(), Config::default());
}
