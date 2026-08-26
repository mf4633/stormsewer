// SPDX-License-Identifier: GPL-3.0-or-later

//! Python bindings for the StormSewer engine.
//!
//! Two layers, deliberately:
//!
//! * **Primitives** — `manning_capacity`, `normal_depth`, `intensity`, … —
//!   plain floats in and out, for spreadsheet-replacement scripting.
//! * **Whole networks** — `analyze_project` / `analyze_ssn` take a project and
//!   return dictionaries, so a notebook can go straight to pandas.
//!
//! Everything is pure computation: no files are written and nothing is sent
//! anywhere.

use pyo3::exceptions::{PyValueError, PyIOError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use engine::hydraulics::{self, K_MANNING_SI, K_MANNING_US};
use engine::idf::IdfCurve;
use engine::io::Project;
use engine::network::Analysis;

/// Manning conversion factor for the requested unit system.
fn k_for(si: bool) -> f64 {
    if si {
        K_MANNING_SI
    } else {
        K_MANNING_US
    }
}

// ── primitives ──────────────────────────────────────────────────────────────

/// Rainfall intensity from a three-parameter IDF curve: `i = a / (t + b)^c`.
///
/// Returns in/hr when the coefficients are US customary.
#[pyfunction]
#[pyo3(signature = (a, b, c, t_min))]
fn intensity(a: f64, b: f64, c: f64, t_min: f64) -> f64 {
    IdfCurve::new(a, b, c).intensity(t_min)
}

/// Rational method peak flow: `Q = C * i * A`.
///
/// With `c` dimensionless, `i` in in/hr and `area` in acres, `Q` is in cfs —
/// the customary near-unity conversion is implicit, as in hand practice.
#[pyfunction]
#[pyo3(signature = (c, i, area))]
fn rational_q(c: f64, i: f64, area: f64) -> f64 {
    c * i * area
}

/// Just-full Manning capacity of a circular pipe (cfs, or m3/s when `si`).
#[pyfunction]
#[pyo3(signature = (diameter, n, slope, *, si = false))]
fn manning_capacity(diameter: f64, n: f64, slope: f64, si: bool) -> PyResult<f64> {
    if diameter <= 0.0 || n <= 0.0 {
        return Err(PyValueError::new_err("diameter and n must be positive"));
    }
    Ok(hydraulics::full_flow_capacity(n, slope, diameter, k_for(si)))
}

/// Flow in a circular pipe at a given depth (cfs, or m3/s when `si`).
#[pyfunction]
#[pyo3(signature = (diameter, n, slope, depth, *, si = false))]
fn circular_flow(diameter: f64, n: f64, slope: f64, depth: f64, si: bool) -> PyResult<f64> {
    if diameter <= 0.0 || n <= 0.0 {
        return Err(PyValueError::new_err("diameter and n must be positive"));
    }
    Ok(hydraulics::circular_q(n, slope, diameter, depth, k_for(si)))
}

/// Normal depth for a target flow, or `None` if the pipe cannot pass it.
#[pyfunction]
#[pyo3(signature = (q, diameter, n, slope, *, si = false))]
fn normal_depth(q: f64, diameter: f64, n: f64, slope: f64, si: bool) -> PyResult<Option<f64>> {
    if diameter <= 0.0 || n <= 0.0 {
        return Err(PyValueError::new_err("diameter and n must be positive"));
    }
    Ok(hydraulics::normal_depth(q, n, slope, diameter, k_for(si)))
}

/// Critical depth for a flow in a circular pipe.
#[pyfunction]
#[pyo3(signature = (q, diameter, *, si = false))]
fn critical_depth(q: f64, diameter: f64, si: bool) -> PyResult<f64> {
    if diameter <= 0.0 {
        return Err(PyValueError::new_err("diameter must be positive"));
    }
    let g = if si {
        hydraulics::G_SI
    } else {
        hydraulics::G_US
    };
    Ok(hydraulics::critical_depth(q, diameter, g))
}

/// Flow area, wetted perimeter, hydraulic radius, and top width at a depth.
#[pyfunction]
#[pyo3(signature = (diameter, depth))]
fn circular_geometry(py: Python<'_>, diameter: f64, depth: f64) -> PyResult<Py<PyDict>> {
    if diameter <= 0.0 {
        return Err(PyValueError::new_err("diameter must be positive"));
    }
    let (area, perimeter, radius, top_width) = hydraulics::circular_geometry(depth, diameter);
    let d = PyDict::new(py);
    d.set_item("area", area)?;
    d.set_item("wetted_perimeter", perimeter)?;
    d.set_item("hydraulic_radius", radius)?;
    d.set_item("top_width", top_width)?;
    Ok(d.into())
}

// ── whole networks ──────────────────────────────────────────────────────────

fn analysis_to_dict(py: Python<'_>, project: &Project, a: &Analysis) -> PyResult<Py<PyDict>> {
    let pipes = PyList::empty(py);
    for p in &a.pipes {
        let d = PyDict::new(py);
        d.set_item("id", &p.id)?;
        d.set_item("from", &p.from)?;
        d.set_item("to", &p.to)?;
        d.set_item("slope", p.slope)?;
        d.set_item("total_ca", p.total_ca)?;
        d.set_item("tc", p.tc)?;
        d.set_item("travel_time", p.travel_time)?;
        d.set_item("intensity", p.intensity)?;
        d.set_item("design_q", p.design_q)?;
        d.set_item("capacity", p.capacity)?;
        d.set_item("pct_full", p.pct_full)?;
        d.set_item("velocity", p.velocity)?;
        d.set_item("normal_depth", p.normal_depth)?;
        d.set_item("critical_depth", p.critical_depth)?;
        d.set_item("hgl_up", p.hgl_up)?;
        d.set_item("hgl_dn", p.hgl_dn)?;
        d.set_item("surcharged", p.report_surcharged())?;
        pipes.append(d)?;
    }
    let nodes = PyList::empty(py);
    for n in &a.nodes {
        let d = PyDict::new(py);
        d.set_item("id", &n.id)?;
        d.set_item("tc", n.tc)?;
        d.set_item("rim", n.rim)?;
        d.set_item("hgl", n.hgl)?;
        d.set_item("freeboard", n.rim - n.hgl)?;
        d.set_item("floods", n.surcharge_to_surface)?;
        nodes.append(d)?;
    }
    let out = PyDict::new(py);
    out.set_item("name", &project.name)?;
    out.set_item("pipes", pipes)?;
    out.set_item("nodes", nodes)?;
    Ok(out.into())
}

/// Analyze a project given as `.ssproj` JSON text.
///
/// Returns `{"name": str, "pipes": [...], "nodes": [...]}`, ready for
/// `pandas.DataFrame(result["pipes"])`.
#[pyfunction]
#[pyo3(signature = (project_json))]
fn analyze_project(py: Python<'_>, project_json: &str) -> PyResult<Py<PyDict>> {
    let project: Project = serde_json::from_str(project_json)
        .map_err(|e| PyValueError::new_err(format!("invalid project JSON: {e}")))?;
    let net = project.to_analysis_network();
    let a = net
        .analyze(&project.idf(), &project.options())
        .map_err(|e| PyValueError::new_err(format!("analysis failed: {e}")))?;
    analysis_to_dict(py, &project, &a)
}

/// Analyze a project file on disk (`.ssproj`).
#[pyfunction]
#[pyo3(signature = (path))]
fn analyze_file(py: Python<'_>, path: &str) -> PyResult<Py<PyDict>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| PyIOError::new_err(format!("cannot read {path}: {e}")))?;
    analyze_project(py, &text)
}

/// Analyze a network in the plain-text `.ssn` format.
#[pyfunction]
#[pyo3(signature = (text))]
fn analyze_ssn(py: Python<'_>, text: &str) -> PyResult<Py<PyDict>> {
    let parsed = engine::parse::parse_ssn(text)
        .map_err(|e| PyValueError::new_err(format!("cannot parse .ssn: {e}")))?;
    let a = parsed
        .network
        .analyze(&parsed.idf, &parsed.options)
        .map_err(|e| PyValueError::new_err(format!("analysis failed: {e}")))?;
    // `.ssn` carries no project name.
    let mut shell = Project::empty();
    shell.name = String::new();
    analysis_to_dict(py, &shell, &a)
}

/// The built-in demo project as `.ssproj` JSON — a runnable starting point.
#[pyfunction]
fn demo_project_json() -> PyResult<String> {
    serde_json::to_string_pretty(&Project::demo())
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn stormsewer(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("K_MANNING_US", K_MANNING_US)?;
    m.add("K_MANNING_SI", K_MANNING_SI)?;
    m.add("G_US", hydraulics::G_US)?;
    m.add("G_SI", hydraulics::G_SI)?;

    m.add_function(wrap_pyfunction!(intensity, m)?)?;
    m.add_function(wrap_pyfunction!(rational_q, m)?)?;
    m.add_function(wrap_pyfunction!(manning_capacity, m)?)?;
    m.add_function(wrap_pyfunction!(circular_flow, m)?)?;
    m.add_function(wrap_pyfunction!(normal_depth, m)?)?;
    m.add_function(wrap_pyfunction!(critical_depth, m)?)?;
    m.add_function(wrap_pyfunction!(circular_geometry, m)?)?;

    m.add_function(wrap_pyfunction!(analyze_project, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_file, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_ssn, m)?)?;
    m.add_function(wrap_pyfunction!(demo_project_json, m)?)?;
    Ok(())
}
