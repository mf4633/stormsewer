// SPDX-License-Identifier: GPL-3.0-or-later

//! Calibration: observed series, objective functions, sensitivity, and the
//! DDS optimiser over batch engine runs.
//!
//! * [`observed`] — importing `datetime,value` text and attaching it to a
//!   model object and variable.
//! * [`objectives`] — NSE, KGE, RMSE, PBIAS, RSR, peak/volume/time-to-peak
//!   errors, with Moriasi (2007) ratings.
//! * [`params`] — what the optimiser may change: object group × column ×
//!   transform × bounds.
//! * [`dds`] — Tolson & Shoemaker's (2007) dynamically dimensioned search.
//! * [`sensitivity`] — one-at-a-time tornado and Morris (1991) elementary
//!   effects.
//!
//! This file holds the pieces that tie them together: the setup stored in
//! `<stem>.calib.json`, the [`Evaluator`] that turns a parameter vector
//! into an objective (for the engine: write a scratch `.inp`, run, read
//! the `.out`, pair with the observations), the runners that drive the
//! evaluator from a worker thread with progress and a stop flag, and the
//! report.

pub mod dds;
pub mod objectives;
pub mod observed;
pub mod params;
pub mod sensitivity;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::doc::InpDoc;
use crate::engine::{self, Engine};
use crate::out::OutputFile;
use crate::rpt::csv_line;
use crate::{Error, Result};

pub use dds::{Bounds, Dds};
pub use objectives::{rating, Metrics, Objective, Paired, Rating};
pub use observed::{ObservedSeries, Variable};
pub use params::{Group, Parameter, Transform};
pub use sensitivity::{MorrisRow, TornadoRow};

/// Sidecar format version.
pub const FORMAT_VERSION: u32 = 1;

/// Everything the calibration window holds, stored beside the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibSetup {
    pub version: u32,
    #[serde(default)]
    pub observed: Vec<ObservedSeries>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub objective: Objective,
    #[serde(default = "default_max_evals")]
    pub max_evals: usize,
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(default = "default_threads")]
    pub threads: usize,
    /// One-at-a-time swing, percent of the current value.
    #[serde(default = "default_oat_pct")]
    pub oat_pct: f64,
    /// Morris trajectories and grid levels.
    #[serde(default = "default_morris_r")]
    pub morris_r: usize,
    #[serde(default = "default_morris_p")]
    pub morris_p: usize,
}

fn default_max_evals() -> usize {
    100
}
fn default_seed() -> u64 {
    1
}
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 8))
        .unwrap_or(2)
}
fn default_oat_pct() -> f64 {
    10.0
}
fn default_morris_r() -> usize {
    4
}
fn default_morris_p() -> usize {
    4
}

impl Default for CalibSetup {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            observed: Vec::new(),
            parameters: Vec::new(),
            objective: Objective::Nse,
            max_evals: default_max_evals(),
            seed: default_seed(),
            threads: default_threads(),
            oat_pct: default_oat_pct(),
            morris_r: default_morris_r(),
            morris_p: default_morris_p(),
        }
    }
}

impl CalibSetup {
    /// `<folder>/<stem>.calib.json` for `<folder>/<stem>.inp`.
    pub fn sidecar_path(model: &Path) -> PathBuf {
        let stem = model
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        model
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}.calib.json"))
    }

    /// A missing sidecar is the default setup.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Format(format!("{}: {e}", path.display())))
    }

    pub fn load_for(model: &Path) -> Result<Self> {
        Self::load(&Self::sidecar_path(model))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Format(format!("serialise calibration: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn save_for(&self, model: &Path) -> Result<()> {
        self.save(&Self::sidecar_path(model))
    }

    pub fn bounds(&self) -> Vec<Bounds> {
        self.parameters
            .iter()
            .map(|p| Bounds::new(p.lower, p.upper))
            .collect()
    }

    /// The factors that reproduce `doc` unchanged.
    pub fn identity(&self, doc: &InpDoc) -> Vec<f64> {
        self.parameters.iter().map(|p| p.identity(doc)).collect()
    }

    pub fn parameter_names(&self) -> Vec<String> {
        self.parameters.iter().map(|p| p.name.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// One observed series against one run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesFit {
    pub name: String,
    pub metrics: Metrics,
    pub paired: Paired,
}

/// What evaluating one parameter vector gave.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluation {
    pub x: Vec<f64>,
    /// The minimised objective; `+∞` when the run failed.
    pub objective: f64,
    pub fits: Vec<SeriesFit>,
    pub error: Option<String>,
    pub elapsed_ms: u128,
}

impl Evaluation {
    pub fn failed(x: &[f64], error: impl Into<String>) -> Self {
        Self {
            x: x.to_vec(),
            objective: f64::INFINITY,
            fits: Vec::new(),
            error: Some(error.into()),
            elapsed_ms: 0,
        }
    }
}

/// Anything that turns a parameter vector into an objective.
pub trait Evaluator: Send + Sync {
    fn evaluate(&self, x: &[f64]) -> Evaluation;
}

/// An evaluator over a plain function, for tests and synthetic problems.
pub struct FnEvaluator<F: Fn(&[f64]) -> f64 + Send + Sync>(pub F);

impl<F: Fn(&[f64]) -> f64 + Send + Sync> Evaluator for FnEvaluator<F> {
    fn evaluate(&self, x: &[f64]) -> Evaluation {
        Evaluation {
            x: x.to_vec(),
            objective: (self.0)(x),
            fits: Vec::new(),
            error: None,
            elapsed_ms: 0,
        }
    }
}

/// Pair every observed series with the run's results and aggregate the
/// objective as the mean over series of [`Objective::minimized`].
pub fn fit_results(
    out: &OutputFile,
    observed: &[ObservedSeries],
    objective: Objective,
) -> Result<(f64, Vec<SeriesFit>)> {
    let mut fits = Vec::new();
    let mut total = 0.0;
    for obs in observed {
        let sim = obs.variable.read(out, &obs.id)?;
        let times = obs.seconds_from(out.meta.start_days);
        let paired = Paired::new(&times, &obs.values, &sim);
        if paired.is_empty() {
            return Err(Error::Format(format!(
                "{}: no observations fall inside the simulated period",
                obs.label()
            )));
        }
        let metrics = Metrics::of(&paired);
        let v = objective.minimized(&metrics);
        total += if v.is_finite() { v } else { f64::INFINITY };
        fits.push(SeriesFit {
            name: obs.label(),
            metrics,
            paired,
        });
    }
    if fits.is_empty() {
        return Err(Error::Format("no observed series to fit".into()));
    }
    Ok((total / fits.len() as f64, fits))
}

/// The real thing: a scratch `.inp` per evaluation, an engine run, the
/// `.out` read back. Each evaluation gets its own scratch folder (keyed by
/// a synthetic model path `<stem>.calib-<n>.inp` beside the model, so
/// relative `[FILES]` still resolve), which is removed afterwards unless
/// `keep_files` is set.
pub struct EngineEvaluator {
    pub engine: Engine,
    pub model_path: PathBuf,
    pub base_text: String,
    pub params: Vec<Parameter>,
    pub observed: Vec<ObservedSeries>,
    pub objective: Objective,
    pub keep_files: bool,
}

/// Evaluation counter shared by every evaluator in the process, so two
/// evaluators over the same model never share a scratch folder.
static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);

impl EngineEvaluator {
    pub fn new(
        engine: Engine,
        model_path: PathBuf,
        base_text: String,
        params: Vec<Parameter>,
        observed: Vec<ObservedSeries>,
        objective: Objective,
    ) -> Self {
        Self {
            engine,
            model_path,
            base_text,
            params,
            observed,
            objective,
            keep_files: false,
        }
    }

    /// The `.inp` text at parameter vector `x`.
    pub fn text_at(&self, x: &[f64]) -> Result<String> {
        let mut doc = InpDoc::parse(&self.base_text);
        doc.apply(params::apply_set(&doc, &self.params, x))?;
        Ok(doc.to_string())
    }

    /// Run `text` under a unique scratch name and score it.
    pub fn evaluate_text(&self, x: &[f64], text: &str) -> Evaluation {
        let n = EVALUATIONS.fetch_add(1, Ordering::Relaxed);
        let stem = self
            .model_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        let synthetic = self
            .model_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}.calib-{}-{n}.inp", std::process::id()));
        let prepared = match engine::prepare(&synthetic, text, true) {
            Ok(p) => p,
            Err(e) => return Evaluation::failed(x, format!("could not lay out the model: {e}")),
        };
        let result = (|| -> Result<Evaluation> {
            let run = self.engine.run_with(&prepared.paths)?;
            if !run.succeeded() {
                return Err(Error::Engine(
                    run.failure_reason()
                        .unwrap_or_else(|| "the run failed".into()),
                ));
            }
            let out = OutputFile::open(&run.out)?;
            let (objective, fits) = fit_results(&out, &self.observed, self.objective)?;
            Ok(Evaluation {
                x: x.to_vec(),
                objective,
                fits,
                error: None,
                elapsed_ms: run.elapsed.as_millis(),
            })
        })();
        if !self.keep_files {
            if let Some(dir) = prepared.paths.inp.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        result.unwrap_or_else(|e| Evaluation::failed(x, e.to_string()))
    }
}

impl Evaluator for EngineEvaluator {
    fn evaluate(&self, x: &[f64]) -> Evaluation {
        match self.text_at(x) {
            Ok(text) => self.evaluate_text(x, &text),
            Err(e) => Evaluation::failed(x, e.to_string()),
        }
    }
}

/// Evaluate `points` with up to `threads` at once, in order. Stops early
/// (returning what is done) when `stop` is raised between batches.
pub fn evaluate_all(
    evaluator: &dyn Evaluator,
    points: &[Vec<f64>],
    threads: usize,
    stop: &AtomicBool,
    mut each: impl FnMut(&Evaluation),
) -> Vec<Evaluation> {
    let threads = threads.max(1);
    let mut out = Vec::with_capacity(points.len());
    for chunk in points.chunks(threads) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let evals: Vec<Evaluation> = std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|x| s.spawn(move || evaluator.evaluate(x)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Evaluation::failed(&[], "evaluation panicked")))
                .collect()
        });
        for e in evals {
            each(&e);
            out.push(e);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Runners
// ---------------------------------------------------------------------------

/// A message from a worker to the progress window.
#[derive(Clone, Debug)]
pub enum Progress {
    Evaluated {
        done: usize,
        total: usize,
        objective: f64,
        best_objective: f64,
        best_x: Vec<f64>,
        error: Option<String>,
    },
    Finished(Box<CalibResult>),
    Tornado(Vec<TornadoRow>),
    Morris(Vec<MorrisRow>),
    Failed(String),
}

/// The outcome of a DDS run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibResult {
    pub objective: Objective,
    pub parameters: Vec<Parameter>,
    pub x0: Vec<f64>,
    pub best_x: Vec<f64>,
    pub best_objective: f64,
    pub evaluations: usize,
    pub max_evals: usize,
    pub seed: u64,
    /// `(evaluation, best objective so far)`.
    pub history: Vec<(usize, f64)>,
    /// The fits at the best point.
    pub fits: Vec<SeriesFit>,
    /// The fits at the start point, for the before/after table.
    pub initial_fits: Vec<SeriesFit>,
    pub engine_version: String,
    pub engine_sha256: String,
    pub stopped_early: bool,
    pub failed_evaluations: usize,
}

impl CalibResult {
    pub fn improvement(&self) -> Option<f64> {
        let f0 = self.history.first().map(|h| h.1)?;
        (f0.is_finite() && self.best_objective.is_finite()).then_some(f0 - self.best_objective)
    }
}

/// Run DDS on a worker: `threads` candidates per round are evaluated at
/// once (one = the paper's serial algorithm). Sends a [`Progress`] per
/// evaluation and `Finished` at the end; `stop` ends the run after the
/// current round, keeping the best so far.
#[allow(clippy::too_many_arguments)]
pub fn run_dds(
    evaluator: Arc<dyn Evaluator>,
    objective: Objective,
    params: Vec<Parameter>,
    x0: Vec<f64>,
    max_evals: usize,
    seed: u64,
    threads: usize,
    engine: (String, String),
    stop: Arc<AtomicBool>,
    tx: Sender<Progress>,
) -> CalibResult {
    let bounds: Vec<Bounds> = params.iter().map(|p| Bounds::new(p.lower, p.upper)).collect();
    let mut dds = Dds::new(bounds, x0.clone(), max_evals, seed);
    let mut best_eval: Option<Evaluation> = None;
    let mut initial: Option<Evaluation> = None;
    let mut failed = 0usize;
    let mut stopped = false;
    while !dds.done() {
        if stop.load(Ordering::Relaxed) {
            stopped = true;
            break;
        }
        let cands = dds.propose(threads.max(1));
        let evals = evaluate_all(evaluator.as_ref(), &cands, threads, &stop, |_| {});
        for e in evals {
            dds.report(&e.x, e.objective);
            if e.error.is_some() {
                failed += 1;
            }
            if initial.is_none() {
                initial = Some(e.clone());
            }
            let (bx, bf) = dds.best();
            if e.objective.is_finite() && e.objective <= bf && bx == e.x.as_slice() {
                best_eval = Some(e.clone());
            }
            let _ = tx.send(Progress::Evaluated {
                done: dds.evaluations(),
                total: max_evals,
                objective: e.objective,
                best_objective: bf,
                best_x: bx.to_vec(),
                error: e.error.clone(),
            });
        }
    }
    let (bx, bf) = dds.best();
    let result = CalibResult {
        objective,
        parameters: params,
        x0,
        best_x: bx.to_vec(),
        best_objective: bf,
        evaluations: dds.evaluations(),
        max_evals,
        seed,
        history: dds.history().to_vec(),
        fits: best_eval.map(|e| e.fits).unwrap_or_default(),
        initial_fits: initial.map(|e| e.fits).unwrap_or_default(),
        engine_version: engine.0,
        engine_sha256: engine.1,
        stopped_early: stopped,
        failed_evaluations: failed,
    };
    let _ = tx.send(Progress::Finished(Box::new(result.clone())));
    result
}

/// One-at-a-time sensitivity on a worker; sends `Tornado` at the end.
pub fn run_oat(
    evaluator: Arc<dyn Evaluator>,
    params: &[Parameter],
    x0: &[f64],
    pct: f64,
    threads: usize,
    stop: Arc<AtomicBool>,
    tx: Sender<Progress>,
) -> Vec<TornadoRow> {
    let bounds: Vec<Bounds> = params.iter().map(|p| Bounds::new(p.lower, p.upper)).collect();
    let points = sensitivity::oat_points(&bounds, x0, pct);
    let xs: Vec<Vec<f64>> = points.iter().map(|(_, _, x)| x.clone()).collect();
    let total = xs.len();
    let mut done = 0usize;
    let evals = evaluate_all(evaluator.as_ref(), &xs, threads, &stop, |e| {
        done += 1;
        let _ = tx.send(Progress::Evaluated {
            done,
            total,
            objective: e.objective,
            best_objective: f64::NAN,
            best_x: Vec::new(),
            error: e.error.clone(),
        });
    });
    let values: Vec<f64> = evals.iter().map(|e| e.objective).collect();
    let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    let rows = sensitivity::tornado(&names, &points[..values.len()], &values);
    let _ = tx.send(Progress::Tornado(rows.clone()));
    rows
}

/// Morris elementary effects on a worker; sends `Morris` at the end.
#[allow(clippy::too_many_arguments)]
pub fn run_morris(
    evaluator: Arc<dyn Evaluator>,
    params: &[Parameter],
    r: usize,
    p: usize,
    seed: u64,
    threads: usize,
    stop: Arc<AtomicBool>,
    tx: Sender<Progress>,
) -> Vec<MorrisRow> {
    let bounds: Vec<Bounds> = params.iter().map(|p| Bounds::new(p.lower, p.upper)).collect();
    let trajectories = sensitivity::morris_trajectories(&bounds, r.max(1), p, seed);
    let mut values: Vec<Vec<f64>> = Vec::new();
    let total: usize = trajectories.iter().map(|(pts, _)| pts.len()).sum();
    let mut done = 0usize;
    for (pts, _) in &trajectories {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let evals = evaluate_all(evaluator.as_ref(), pts, threads, &stop, |e| {
            done += 1;
            let _ = tx.send(Progress::Evaluated {
                done,
                total,
                objective: e.objective,
                best_objective: f64::NAN,
                best_x: Vec::new(),
                error: e.error.clone(),
            });
        });
        values.push(evals.iter().map(|e| e.objective).collect());
    }
    let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    let rows = sensitivity::morris(&names, &trajectories[..values.len()], &values);
    let _ = tx.send(Progress::Morris(rows.clone()));
    rows
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

fn fmt(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.4}")
    } else {
        "—".into()
    }
}

fn rating_text(o: Objective, v: f64) -> String {
    rating(o, v).map(|r| r.label().to_string()).unwrap_or_default()
}

/// The rows of the objective table for one fit: label, value, rating.
pub fn metric_rows(m: &Metrics) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    for o in Objective::ALL {
        let v = o.value(m);
        rows.push((o.label().to_string(), fmt(v), rating_text(o, v)));
    }
    rows.push(("KGE r / α / β".into(), format!("{} / {} / {}", fmt(m.kge_r), fmt(m.kge_alpha), fmt(m.kge_beta)), String::new()));
    rows.push(("Peak obs / sim".into(), format!("{} / {}", fmt(m.peak_obs), fmt(m.peak_sim)), String::new()));
    rows.push(("Points".into(), m.n.to_string(), String::new()));
    rows
}

/// A tiny two-format table writer.
enum Fmt {
    Markdown,
    Html,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn table(f: &Fmt, headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut s = String::new();
    match f {
        Fmt::Markdown => {
            s.push_str(&format!("| {} |\n", headers.join(" | ")));
            s.push_str(&format!("|{}\n", headers.iter().map(|_| "---|").collect::<String>()));
            for r in rows {
                s.push_str(&format!("| {} |\n", r.join(" | ")));
            }
            s.push('\n');
        }
        Fmt::Html => {
            s.push_str("<table><thead><tr>");
            for h in headers {
                s.push_str(&format!("<th>{}</th>", esc(h)));
            }
            s.push_str("</tr></thead><tbody>");
            for r in rows {
                s.push_str("<tr>");
                for c in r {
                    s.push_str(&format!("<td>{}</td>", esc(c)));
                }
                s.push_str("</tr>");
            }
            s.push_str("</tbody></table>\n");
        }
    }
    s
}

fn heading(f: &Fmt, level: usize, text: &str) -> String {
    match f {
        Fmt::Markdown => format!("{} {text}\n\n", "#".repeat(level)),
        Fmt::Html => format!("<h{level}>{}</h{level}>\n", esc(text)),
    }
}

fn para(f: &Fmt, text: &str) -> String {
    match f {
        Fmt::Markdown => format!("{text}\n\n"),
        Fmt::Html => format!("<p>{}</p>\n", esc(text)),
    }
}

fn render(f: Fmt, r: &CalibResult, model: &str) -> String {
    let mut s = String::new();
    if matches!(f, Fmt::Html) {
        s.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>Calibration report</title><style>body{font-family:sans-serif;max-width:60em;margin:2em auto}table{border-collapse:collapse;margin:0 0 1em}td,th{border:1px solid #999;padding:2px 6px;text-align:right}th:first-child,td:first-child{text-align:left}</style></head><body>\n");
    }
    s.push_str(&heading(&f, 1, "Calibration report"));
    s.push_str(&para(&f, &format!("Model: {model}")));
    s.push_str(&para(
        &f,
        &format!(
            "Engine: EPA SWMM {} (SHA-256 {})",
            r.engine_version, r.engine_sha256
        ),
    ));
    s.push_str(&para(
        &f,
        &format!(
            "Optimiser: DDS (Tolson & Shoemaker 2007), r = 0.2, seed {}, {} of {} evaluations{}{}. Objective: {} → best {}.",
            r.seed,
            r.evaluations,
            r.max_evals,
            if r.stopped_early { " (stopped early)" } else { "" },
            if r.failed_evaluations > 0 { format!(", {} failed", r.failed_evaluations) } else { String::new() },
            r.objective.label(),
            fmt(r.best_objective)
        ),
    ));
    s.push_str(&heading(&f, 2, "Parameters"));
    let rows: Vec<Vec<String>> = r
        .parameters
        .iter()
        .enumerate()
        .map(|(i, p)| {
            vec![
                p.name.clone(),
                format!("[{}] {}", p.section, p.column),
                p.group.label(),
                p.transform.label().into(),
                fmt(p.lower),
                fmt(p.upper),
                r.x0.get(i).copied().map(fmt).unwrap_or_default(),
                r.best_x.get(i).copied().map(fmt).unwrap_or_default(),
            ]
        })
        .collect();
    s.push_str(&table(
        &f,
        &["Parameter", "Field", "Objects", "Transform", "Lower", "Upper", "Start", "Best"],
        &rows,
    ));
    s.push_str(&heading(&f, 2, "Objectives at the best point"));
    s.push_str(&para(&f, "Ratings after Moriasi et al. (2007), Table 4 (NSE, RSR and PBIAS; none is defined for the others)."));
    for (i, fit) in r.fits.iter().enumerate() {
        s.push_str(&heading(&f, 3, &fit.name));
        let initial = r.initial_fits.get(i).map(|f| f.metrics);
        let rows: Vec<Vec<String>> = metric_rows(&fit.metrics)
            .into_iter()
            .zip(initial.map(|m| metric_rows(&m)).unwrap_or_else(|| metric_rows(&fit.metrics)).into_iter().map(|r| r.1).chain(std::iter::repeat(String::new())))
            .map(|((label, v, rating), start)| vec![label, start, v, rating])
            .collect();
        s.push_str(&table(&f, &["Statistic", "Start", "Best", "Rating"], &rows));
    }
    s.push_str(&heading(&f, 2, "Convergence"));
    let rows: Vec<Vec<String>> = r
        .history
        .iter()
        .map(|(n, v)| vec![n.to_string(), fmt(*v)])
        .collect();
    s.push_str(&table(&f, &["Evaluation", "Best objective"], &rows));
    s.push_str(&heading(&f, 2, "Observed and simulated"));
    for fit in &r.fits {
        s.push_str(&heading(&f, 3, &fit.name));
        let rows: Vec<Vec<String>> = fit
            .paired
            .times
            .iter()
            .zip(fit.paired.obs.iter().zip(&fit.paired.sim))
            .map(|(t, (o, sim))| vec![format!("{:.1}", t), fmt(*o), fmt(*sim)])
            .collect();
        s.push_str(&table(&f, &["Time (s)", "Observed", "Simulated"], &rows));
    }
    if matches!(f, Fmt::Html) {
        s.push_str("</body></html>\n");
    }
    s
}

/// The calibration report as Markdown.
pub fn report_markdown(r: &CalibResult, model: &str) -> String {
    render(Fmt::Markdown, r, model)
}

/// The calibration report as a self-contained HTML page.
pub fn report_html(r: &CalibResult, model: &str) -> String {
    render(Fmt::Html, r, model)
}

/// `Parameter,Low x,High x,f(low),f(high),Swing` for a tornado table.
pub fn tornado_csv(rows: &[TornadoRow]) -> String {
    let mut out = csv_line(&["Parameter", "Low x", "High x", "f(low)", "f(high)", "Swing"]);
    for r in rows {
        out.push_str(&csv_line(&[
            r.name.clone(),
            fmt(r.x_low),
            fmt(r.x_high),
            fmt(r.f_low),
            fmt(r.f_high),
            fmt(r.swing),
        ]));
    }
    out
}

/// The paired series of every fit as `Series,Time s,Observed,Simulated`.
pub fn fits_csv(fits: &[SeriesFit]) -> String {
    let mut out = csv_line(&["Series", "Time s", "Observed", "Simulated"]);
    for f in fits {
        for ((t, o), s) in f.paired.times.iter().zip(&f.paired.obs).zip(&f.paired.sim) {
            out.push_str(&csv_line(&[
                f.name.clone(),
                format!("{t:.1}"),
                fmt(*o),
                fmt(*s),
            ]));
        }
    }
    out
}
