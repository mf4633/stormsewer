// SPDX-License-Identifier: GPL-3.0-or-later

//! End to end against a real engine, when one is registered: a synthetic
//! observed series is produced from a run of the Detention Pond model with
//! a perturbed parameter set, and the engine evaluator scores the
//! unperturbed model worse than the perturbed one. Skipped, with a note,
//! when no engine is discovered.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use stormsewer_swmm::calib::observed::{ObsTime, ObservedSeries, Variable};
use stormsewer_swmm::calib::params::{apply_set, Group, Parameter, Transform};
use stormsewer_swmm::calib::{self, EngineEvaluator, Evaluator, Objective};
use stormsewer_swmm::doc::InpDoc;
use stormsewer_swmm::engine::{self, Registry};
use stormsewer_swmm::out::OutputFile;
use stormsewer_swmm::scenario::{self, Scenario, ScenarioResult, ScenarioSet};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epa-samples/Detention_Pond_Model.inp")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("stormsewer-calib-engine-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn engine_evaluator_recovers_a_perturbed_width_when_an_engine_is_present() {
    let registry = Registry::discover();
    let Some(engine) = registry.default_engine().cloned() else {
        eprintln!("skipped: no SWMM engine discovered (install EPA SWMM or set STORMSEWER_SWMM_ENGINE_DIR)");
        return;
    };
    let model = scratch("Detention_Pond_Model.inp");
    std::fs::copy(fixture(), &model).unwrap();
    let base = InpDoc::read(&model).unwrap();
    let width = Parameter::new("Width", "SUBCATCHMENTS", "Width", Group::All, Transform::Multiply, 0.5, 2.0);
    let imperv = Parameter::new("%Imperv", "SUBCATCHMENTS", "PctImperv", Group::All, Transform::Multiply, 0.6, 1.4);
    let params = vec![width, imperv];
    // "Truth": widths × 1.6, imperviousness × 0.8.
    let truth = [1.6, 0.8];
    let mut perturbed = InpDoc::parse(&base.to_string());
    perturbed.apply(apply_set(&base, &params, &truth)).unwrap();
    let truth_path = scratch("truth.inp");
    std::fs::write(&truth_path, perturbed.to_string()).unwrap();
    let run = engine.run(&truth_path).unwrap();
    assert!(run.succeeded(), "{:?}", run.failure_reason());
    let out = OutputFile::open(&run.out).unwrap();
    let sim = Variable::SystemInflow.read(&out, "O2").unwrap();
    // Observations every 15 minutes, as absolute stamps.
    let observed = ObservedSeries {
        name: "synthetic O2".into(),
        id: "O2".into(),
        variable: Variable::SystemInflow,
        times: sim
            .times_s
            .iter()
            .step_by(3)
            .map(|t| ObsTime::Absolute(out.meta.start_days + t / 86_400.0))
            .collect(),
        values: sim.values.iter().step_by(3).copied().collect(),
        source: "perturbed run".into(),
    };
    let evaluator = EngineEvaluator::new(
        engine.clone(),
        model.clone(),
        base.to_string(),
        params.clone(),
        vec![observed],
        Objective::Nse,
    );
    let at_truth = evaluator.evaluate(&truth);
    assert!(at_truth.error.is_none(), "{:?}", at_truth.error);
    assert!(at_truth.objective < 1e-6, "1 − NSE at the truth: {}", at_truth.objective);
    assert_eq!(at_truth.fits.len(), 1);
    assert!(at_truth.fits[0].metrics.nse > 0.999);
    let at_base = evaluator.evaluate(&[1.0, 1.0]);
    assert!(at_base.objective > at_truth.objective + 1e-3, "base {} vs truth {}", at_base.objective, at_truth.objective);
    eprintln!(
        "engine {}: 1−NSE base = {:.4}, truth = {:.6} ({} ms/run)",
        engine.version, at_base.objective, at_truth.objective, at_base.elapsed_ms
    );
    // Scratch folders are removed after each evaluation.
    let root = engine::scratch_root();
    let leftovers = std::fs::read_dir(&root)
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    std::fs::read_dir(e.path()).map(|f| {
                        f.flatten().any(|g| {
                            let n = g.file_name().to_string_lossy().into_owned();
                            n.starts_with("Detention_Pond_Model.calib-") && n.ends_with(".inp")
                        })
                    }).unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    assert_eq!(leftovers, 0, "every evaluation removes its scratch folder");
    // A short DDS with two workers improves on the base.
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let result = calib::run_dds(
        Arc::new(evaluator),
        Objective::Nse,
        params,
        vec![1.0, 1.0],
        8,
        3,
        2,
        (engine.version.clone(), engine.sha256.clone()),
        stop,
        tx,
    );
    assert_eq!(result.evaluations, 8);
    assert!(result.best_objective <= at_base.objective, "{result:?}");
    assert_eq!(result.history[0].1, at_base.objective);
    assert!(rx.try_iter().count() >= 8);
    eprintln!("DDS 8 evals: best 1−NSE {:.4} at {:?}", result.best_objective, result.best_x);
    let md = calib::report_markdown(&result, "Detention_Pond_Model.inp");
    assert!(md.contains(&engine.sha256));
    let _ = std::fs::remove_file(&truth_path);
}

#[test]
fn scenarios_run_through_the_engine_in_their_own_scratch_folders() {
    let registry = Registry::discover();
    let Some(engine) = registry.default_engine().cloned() else {
        eprintln!("skipped: no SWMM engine discovered");
        return;
    };
    let model = scratch("Pond_scenarios.inp");
    std::fs::copy(fixture(), &model).unwrap();
    let base = InpDoc::read(&model).unwrap();
    let mut set = ScenarioSet::default();
    set.add(Scenario::from_commands(
        "double roughness",
        &[stormsewer_swmm::doc::Command::SetField {
            section: "CONDUITS".into(),
            name: "C1".into(),
            field: "Roughness".into(),
            value: "0.1".into(),
        }],
    ));
    let mut results = Vec::new();
    for (label, text) in [
        ("base".to_string(), base.to_string()),
        (set.scenarios[0].name.clone(), scenario::materialize(&base, &set.scenarios[0]).unwrap()),
    ] {
        let synthetic = scenario::scenario_model_path(&model, &label);
        let prepared = engine::prepare(&synthetic, &text, true).unwrap();
        assert!(prepared.is_scratch());
        let run = engine.run_prepared(&prepared).unwrap();
        assert!(run.succeeded(), "{label}: {:?}", run.failure_reason());
        let mut r = ScenarioResult::from_report(&label, &run.report, run.succeeded());
        r.elapsed_ms = run.elapsed.as_millis();
        results.push(r);
    }
    assert!(results[0].peak_outfall_flow.is_some());
    assert!(results[1].peak_outfall_flow.is_some());
    let csv = scenario::results_csv(&results);
    assert_eq!(csv.lines().count(), 3);
    eprintln!("{csv}");
}
