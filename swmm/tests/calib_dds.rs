// SPDX-License-Identifier: GPL-3.0-or-later

//! DDS on a synthetic problem, parameter transforms on a document, the
//! sensitivity screens, the runners over a fake evaluator, the setup
//! sidecar, and the report.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use stormsewer_swmm::calib::dds::{minimize, Bounds, Dds, Rng};
use stormsewer_swmm::calib::params::{apply_set, suggestions, Group, Parameter, Transform};
use stormsewer_swmm::calib::sensitivity::{morris, morris_trajectories, oat_points, tornado};
use stormsewer_swmm::calib::{
    self, CalibResult, CalibSetup, FnEvaluator, Objective, Progress, SeriesFit,
};
use stormsewer_swmm::doc::InpDoc;

fn pond() -> InpDoc {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/epa-samples/Detention_Pond_Model.inp");
    InpDoc::read(&p).unwrap()
}

fn quadratic(x: &[f64]) -> f64 {
    (x[0] - 1.5).powi(2) + (x[1] + 2.5).powi(2)
}

#[test]
fn dds_converges_on_a_two_parameter_quadratic_within_one_percent_in_100_evals() {
    let bounds = vec![Bounds::new(-10.0, 10.0), Bounds::new(-10.0, 10.0)];
    let x0 = vec![8.0, -7.0];
    let f0 = quadratic(&x0);
    let dds = minimize(bounds, x0, 100, 1, quadratic);
    let (bx, bf) = dds.best();
    eprintln!("DDS quadratic: f0 = {f0}, best = {bf:.6} at {bx:?} after {} evals", dds.evaluations());
    assert_eq!(dds.evaluations(), 100);
    assert!(dds.done());
    assert!(bf <= 0.01 * f0, "best {bf} is not within 1 % of the start value {f0}");
    // Within 1 % of the range of each parameter (0.2 of 20).
    assert!((bx[0] - 1.5).abs() <= 0.2 && (bx[1] + 2.5).abs() <= 0.2, "{bx:?}");
    // The convergence curve never goes up, and has one point per evaluation.
    let h = dds.history();
    assert_eq!(h.len(), 100);
    assert!(h.windows(2).all(|w| w[1].1 <= w[0].1));
    assert_eq!(h[0].1, f0, "the start point is evaluated first");
    // Same seed, same answer; another seed, another path.
    let again = minimize(
        vec![Bounds::new(-10.0, 10.0), Bounds::new(-10.0, 10.0)],
        vec![8.0, -7.0],
        100,
        1,
        quadratic,
    );
    assert_eq!(again.best().0, bx);
    let other = minimize(
        vec![Bounds::new(-10.0, 10.0), Bounds::new(-10.0, 10.0)],
        vec![8.0, -7.0],
        100,
        2,
        quadratic,
    );
    assert_ne!(other.history(), dds.history());
    assert!(other.best().1 <= 0.05 * f0, "seed 2: {}", other.best().1);
}

#[test]
fn dds_respects_bounds_and_budget_with_batched_candidates() {
    let bounds = vec![Bounds::new(0.0, 1.0), Bounds::new(5.0, 6.0), Bounds::new(-1.0, -0.5)];
    let mut dds = Dds::new(bounds.clone(), vec![0.5, 5.5, -0.75], 37, 9);
    let mut n = 0;
    while !dds.done() {
        let cands = dds.propose(4);
        assert!(!cands.is_empty());
        for x in cands {
            for (xi, b) in x.iter().zip(&bounds) {
                assert!(*xi >= b.lower && *xi <= b.upper, "{x:?} out of {bounds:?}");
            }
            n += 1;
            dds.report(&x, x.iter().map(|v| v * v).sum());
        }
    }
    assert_eq!(n, 37, "exactly the budget, even with batches of 4");
    assert_eq!(dds.evaluations(), 37);
    // A failed evaluation (non-finite) never becomes the best.
    let mut d = Dds::new(vec![Bounds::new(0.0, 1.0)], vec![0.5], 3, 1);
    let x = d.propose(1);
    d.report(&x[0], f64::INFINITY);
    assert_eq!(d.best().1, f64::INFINITY);
    let x = d.propose(1);
    d.report(&x[0], 1.0);
    assert_eq!(d.best().1, 1.0);
    let x = d.propose(1);
    d.report(&x[0], f64::NAN);
    assert_eq!(d.best().1, 1.0);
    // The RNG is deterministic and roughly normal.
    let mut r = Rng::new(42);
    let mut r2 = Rng::new(42);
    assert_eq!(r.next_u64(), r2.next_u64());
    let n = 20_000;
    let mean: f64 = (0..n).map(|_| r.normal()).sum::<f64>() / n as f64;
    assert!(mean.abs() < 0.05, "{mean}");
    let u: Vec<f64> = (0..1000).map(|_| r.uniform()).collect();
    assert!(u.iter().all(|v| (0.0..1.0).contains(v)));
    assert!(r.below(5) < 5);
    assert_eq!(r.below(0), 0);
}

#[test]
fn parameter_transforms_edit_the_document_as_one_batch() {
    let doc = pond();
    let width = Parameter::new("Width", "SUBCATCHMENTS", "Width", Group::All, Transform::Multiply, 0.5, 2.0);
    let n_perv = Parameter::new("N-Perv S1", "SUBAREAS", "NPerv", Group::Object("S1".into()), Transform::Offset, -0.1, 0.1);
    let rough = Parameter::new(
        "Roughness C1 C2",
        "CONDUITS",
        "Roughness",
        Group::Selection(vec!["C1".into(), "C2".into()]),
        Transform::Set,
        0.01,
        0.05,
    );
    assert_eq!(width.members(&doc).len(), 8);
    assert_eq!(width.identity(&doc), 1.0);
    assert_eq!(n_perv.identity(&doc), 0.0);
    assert!((rough.identity(&doc) - 0.033).abs() < 1e-12, "Set: the mean current value of C1 (0.05) and C2 (0.016)");
    let params = vec![width.clone(), n_perv.clone(), rough.clone()];
    let mut edited = InpDoc::parse(&doc.to_string());
    edited.apply(apply_set(&doc, &params, &[2.0, 0.05, 0.013])).unwrap();
    assert_eq!(edited.undo_depth(), 1, "one undo step");
    assert_eq!(edited.field("SUBCATCHMENTS", "S1", "Width"), Some("3174"));
    assert_eq!(edited.field("SUBCATCHMENTS", "S7", "Width"), Some("1814"));
    assert_eq!(edited.field("SUBAREAS", "S1", "NPerv"), Some("0.29"));
    assert_eq!(edited.field("SUBAREAS", "S2", "NPerv"), Some("0.24"), "only S1 offset");
    assert_eq!(edited.field("CONDUITS", "C1", "Roughness"), Some("0.013"));
    assert_eq!(edited.field("CONDUITS", "C3", "Roughness"), Some("0.016"), "not in the selection");
    assert!(edited.undo());
    assert_eq!(edited.to_string(), doc.to_string());
    // Out-of-bounds factors are clamped; percentages stay within 0..100.
    let imperv = Parameter::new("%Imperv", "SUBCATCHMENTS", "PctImperv", Group::All, Transform::Multiply, 0.5, 5.0);
    let mut e2 = InpDoc::parse(&doc.to_string());
    e2.apply(apply_set(&doc, std::slice::from_ref(&imperv), &[50.0])).unwrap();
    assert_eq!(e2.field("SUBCATCHMENTS", "S6", "PctImperv"), Some("100"));
    assert_eq!(e2.field("SUBCATCHMENTS", "S7", "PctImperv"), Some("0"));
    // A tag group edits only tagged objects (the pond tags S1..S7 by land use?).
    let mut tagged = InpDoc::parse(&doc.to_string());
    tagged
        .apply(stormsewer_swmm::doc::Command::AddRow {
            section: "TAGS".into(),
            fields: vec!["Subcatch".into(), "S3".into(), "steep".into()],
            comment: None,
        })
        .unwrap();
    let by_tag = Parameter::new("steep width", "SUBCATCHMENTS", "Width", Group::Tag("steep".into()), Transform::Multiply, 0.5, 2.0);
    assert_eq!(by_tag.members(&tagged), vec!["S3".to_string()]);
    assert_eq!(by_tag.tag_kind(), Some("Subcatch"));
    assert_eq!(rough.tag_kind(), Some("Link"));
    assert_eq!(Parameter::new("x", "AQUIFERS", "4", Group::All, Transform::Multiply, 0.5, 2.0).tag_kind(), None);
    // Suggestions follow the infiltration method.
    let s = suggestions(&doc);
    let names: Vec<&str> = s.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Horton max rate") && names.contains(&"Conduit roughness") && names.contains(&"Aquifer conductivity"));
    assert!(!names.contains(&"Ksat"));
    let mut ga = InpDoc::parse(&doc.to_string());
    ga.apply(stormsewer_swmm::doc::Command::SetOption { section: "OPTIONS".into(), key: "INFILTRATION".into(), value: "GREEN_AMPT".into() }).unwrap();
    let names: Vec<String> = suggestions(&ga).iter().map(|p| p.name.clone()).collect();
    assert!(names.iter().any(|n| n == "Ksat") && !names.iter().any(|n| n == "Horton decay"));
    // Every suggestion serialises.
    let json = serde_json::to_string(&s).unwrap();
    let back: Vec<Parameter> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    assert_eq!(calib::params::format_value(0.0025), "0.0025");
    assert_eq!(calib::params::format_value(1587.0), "1587");
    assert_eq!(calib::params::format_value(-0.0), "0");
}

#[test]
fn oat_tornado_ranks_the_steeper_parameter_first() {
    // f = 10·x² + y²: x swings ten times harder.
    let f = |x: &[f64]| 10.0 * x[0] * x[0] + x[1] * x[1];
    let bounds = vec![Bounds::new(0.0, 4.0), Bounds::new(0.0, 4.0)];
    let x0 = vec![2.0, 2.0];
    let points = oat_points(&bounds, &x0, 10.0);
    assert_eq!(points.len(), 4);
    assert_eq!(points[0].2, vec![1.8, 2.0]);
    assert_eq!(points[1].2, vec![2.2, 2.0]);
    let values: Vec<f64> = points.iter().map(|(_, _, x)| f(x)).collect();
    let rows = tornado(&["x".into(), "y".into()], &points, &values);
    assert_eq!(rows[0].name, "x");
    assert!((rows[0].swing - (10.0 * 2.2f64.powi(2) - 10.0 * 1.8f64.powi(2))).abs() < 1e-9);
    assert!((rows[1].swing - (2.2f64.powi(2) - 1.8f64.powi(2))).abs() < 1e-9);
    // A zero current value swings by a fraction of the range instead.
    let pts = oat_points(&[Bounds::new(-1.0, 1.0)], &[0.0], 10.0);
    assert_eq!(pts[0].2, vec![-0.2]);
    assert_eq!(pts[1].2, vec![0.2]);
    // Clamped to the bounds.
    let pts = oat_points(&[Bounds::new(0.0, 1.0)], &[1.0], 50.0);
    assert_eq!(pts[1].2, vec![1.0]);
    let csv = calib::tornado_csv(&rows);
    assert!(csv.starts_with("Parameter,Low x,High x,f(low),f(high),Swing\nx,"));
}

#[test]
fn morris_elementary_effects_find_the_linear_and_the_interacting_parameter() {
    // f = 3·u + 0·v + 2·u·w: u matters, v does not, w only through u.
    let f = |x: &[f64]| 3.0 * x[0] + 2.0 * x[0] * x[2];
    let bounds = vec![Bounds::new(0.0, 1.0); 3];
    let traj = morris_trajectories(&bounds, 6, 4, 3);
    assert_eq!(traj.len(), 6);
    for (pts, steps) in &traj {
        assert_eq!(pts.len(), 4, "k + 1 points");
        assert_eq!(steps.len(), 3);
        for p in pts {
            assert!(p.iter().all(|v| (0.0..=1.0).contains(v)), "{p:?}");
        }
        // Each step changes exactly one variable.
        for (i, (j, d)) in steps.iter().enumerate() {
            let a = &pts[i];
            let b = &pts[i + 1];
            for k in 0..3 {
                if k == *j {
                    assert!((b[k] - a[k] - d).abs() < 1e-9);
                } else {
                    assert_eq!(a[k], b[k]);
                }
            }
        }
    }
    let values: Vec<Vec<f64>> = traj.iter().map(|(pts, _)| pts.iter().map(|p| f(p)).collect()).collect();
    let rows = morris(&["u".into(), "v".into(), "w".into()], &traj, &values);
    assert_eq!(rows[0].name, "u");
    let v = rows.iter().find(|r| r.name == "v").unwrap();
    assert!(v.mu_star.abs() < 1e-9 && v.sigma.abs() < 1e-9);
    let u = rows.iter().find(|r| r.name == "u").unwrap();
    assert!(u.mu_star >= 3.0 - 1e-9 && u.mu_star <= 5.0 + 1e-9, "{u:?}");
    assert!(u.sigma > 0.0, "u's effect depends on w");
    assert_eq!(u.n_effects, 6);
}

fn param(name: &str, lo: f64, hi: f64) -> Parameter {
    Parameter::new(name, "SUBCATCHMENTS", "Width", Group::All, Transform::Multiply, lo, hi)
}

#[test]
fn runners_drive_a_fake_evaluator_with_progress_and_stop() {
    let evaluator = Arc::new(FnEvaluator(quadratic));
    let params = vec![param("a", -10.0, 10.0), param("b", -10.0, 10.0)];
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let result = calib::run_dds(
        evaluator.clone(),
        Objective::Rmse,
        params.clone(),
        vec![8.0, -7.0],
        60,
        7,
        3,
        ("5.2.4".into(), "abc".into()),
        stop.clone(),
        tx,
    );
    assert_eq!(result.evaluations, 60);
    assert!(!result.stopped_early);
    assert!(result.best_objective < quadratic(&[8.0, -7.0]));
    assert_eq!(result.history.len(), 60);
    assert!(result.improvement().unwrap() > 0.0);
    let msgs: Vec<Progress> = rx.try_iter().collect();
    let evaluated = msgs.iter().filter(|m| matches!(m, Progress::Evaluated { .. })).count();
    assert_eq!(evaluated, 60);
    assert!(matches!(msgs.last(), Some(Progress::Finished(r)) if r.evaluations == 60));
    if let Some(Progress::Evaluated { done, total, best_x, .. }) = msgs.first() {
        assert_eq!((*done, *total), (1, 60));
        assert_eq!(best_x, &vec![8.0, -7.0]);
    } else {
        panic!("first message is an evaluation");
    }
    // Stop before starting: only what the first round produced.
    let (tx, _rx) = mpsc::channel();
    stop.store(true, Ordering::Relaxed);
    let r = calib::run_dds(evaluator.clone(), Objective::Rmse, params.clone(), vec![8.0, -7.0], 60, 7, 2, (String::new(), String::new()), stop.clone(), tx);
    assert!(r.stopped_early);
    assert_eq!(r.evaluations, 0);
    stop.store(false, Ordering::Relaxed);
    // OAT and Morris runners.
    let (tx, rx) = mpsc::channel();
    let rows = calib::run_oat(evaluator.clone(), &params, &[2.0, 2.0], 10.0, 2, stop.clone(), tx);
    assert_eq!(rows.len(), 2);
    assert!(matches!(rx.try_iter().last(), Some(Progress::Tornado(r)) if r.len() == 2));
    let (tx, rx) = mpsc::channel();
    let rows = calib::run_morris(evaluator, &params, 3, 4, 1, 2, stop, tx);
    assert_eq!(rows.len(), 2);
    assert!(matches!(rx.try_iter().last(), Some(Progress::Morris(_))));
    // Report.
    let md = calib::report_markdown(&result, "Pond.inp");
    assert!(md.starts_with("# Calibration report"));
    assert!(md.contains("EPA SWMM 5.2.4") && md.contains("abc"));
    assert!(md.contains("| a |") && md.contains("Tolson"));
    assert!(md.contains("## Convergence"));
    let html = calib::report_html(&result, "Pond.inp");
    assert!(html.starts_with("<!doctype html>") && html.contains("<table>") && html.contains("</html>"));
    let json = serde_json::to_string(&result).unwrap();
    let back: CalibResult = serde_json::from_str(&json).unwrap();
    // serde_json's default float parser is not bit-exact; compare loosely.
    assert_eq!(back.best_x, result.best_x);
    assert_eq!(back.evaluations, result.evaluations);
    assert_eq!(back.history.len(), result.history.len());
    assert!(back.history.iter().zip(&result.history).all(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() < 1e-9));
    assert_eq!(back.parameters, result.parameters);
    let fits = vec![SeriesFit {
        name: "O2".into(),
        metrics: Default::default(),
        paired: calib::Paired { times: vec![0.0, 60.0], obs: vec![1.0, 2.0], sim: vec![1.5, 2.5] },
    }];
    let csv = calib::fits_csv(&fits);
    assert_eq!(csv.lines().count(), 3);
    assert!(csv.contains("O2,60.0,2.0000,2.5000"));
}

#[test]
fn calibration_setup_persists_beside_the_model() {
    let dir = std::env::temp_dir().join("stormsewer-calib-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("Pond.inp");
    let side = CalibSetup::sidecar_path(&model);
    assert_eq!(side.file_name().unwrap(), "Pond.calib.json");
    let _ = std::fs::remove_file(&side);
    let loaded = CalibSetup::load_for(&model).unwrap();
    assert_eq!(loaded, CalibSetup::default());
    assert_eq!(loaded.max_evals, 100);
    let mut setup = loaded;
    setup.parameters = suggestions(&pond());
    setup.objective = Objective::Kge;
    setup.max_evals = 42;
    setup.observed.push(stormsewer_swmm::calib::ObservedSeries {
        name: "gauge".into(),
        id: "O2".into(),
        variable: calib::Variable::SystemInflow,
        times: vec![calib::observed::ObsTime::Relative(300.0)],
        values: vec![1.0],
        source: "test".into(),
    });
    setup.save_for(&model).unwrap();
    let back = CalibSetup::load_for(&model).unwrap();
    assert_eq!(back, setup);
    assert_eq!(back.bounds().len(), setup.parameters.len());
    assert_eq!(back.parameter_names()[0], "%Imperv");
    let ident = back.identity(&pond());
    assert!(ident.iter().all(|v| *v == 1.0));
    std::fs::write(&side, "nope").unwrap();
    assert!(CalibSetup::load(&side).is_err());
    let _ = std::fs::remove_file(&side);
}
