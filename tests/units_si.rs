// SPDX-License-Identifier: GPL-3.0-or-later

//! SI-unit-path validation. The engine always computes in US customary; the
//! Project layer converts stored SI values (m, mm, ha, mm/hr) back to engine
//! feet/acres/in-hr before analysis. This test pins that the physics is
//! unit-consistent: toggling U.S. ↔ SI must not change the answer.
//!
//! It would catch the two classic unit bugs — a Rational flow off by the metric
//! 1/360 factor (design flows would diverge) or a Manning capacity off by the
//! 1.486 US<->SI coefficient (capacities would shift ~49%).

use stormsewer::io::{export_pdf_with, PdfOptions, Project};
use stormsewer::network::AnalysisOptions;
use stormsewer::units::{convert_project, UnitSystem};

// A fixed design intensity removes time-of-concentration coupling (downstream Tc
// depends on pipe travel time → velocity → diameter, and metric diameters snap
// to the catalog). With intensity fixed, design flow is exactly i·ΣCA, so any
// difference across unit systems is a genuine conversion bug, not physics.
const I: f64 = 5.0; // in/hr (engine units) applied identically to both systems

fn flows(p: &Project) -> Vec<(String, f64, f64)> {
    let net = p.to_network();
    let opts = AnalysisOptions {
        intensity_override: Some(I),
        ..p.options()
    };
    let a = net.analyze(&p.idf(), &opts).expect("analyze");
    a.pipes
        .iter()
        .map(|r| (r.id.clone(), r.design_q, r.capacity))
        .collect()
}

#[test]
fn si_toggle_preserves_flows_and_capacity() {
    let mut p = Project::demo();
    assert_eq!(p.units, UnitSystem::UsCustomary);
    let us = flows(&p);
    assert!(!us.is_empty());

    convert_project(&mut p, UnitSystem::Si);
    assert_eq!(p.units, UnitSystem::Si);
    let si = flows(&p);

    assert_eq!(us.len(), si.len(), "pipe count preserved");
    for ((id, q_us, cap_us), (_, q_si, cap_si)) in us.iter().zip(si.iter()) {
        // Design flow (i·ΣCA) must be identical — area converts exactly (ac↔ha),
        // catching e.g. a metric Rational 1/360 factor bug.
        assert!(
            (q_us - q_si).abs() < 1e-6,
            "{id}: design Q differs across units — US {q_us} vs SI {q_si}"
        );
        // Capacity may shift a little as diameters snap to the metric catalog,
        // but nowhere near the 1.486 Manning factor a units bug would introduce.
        let rel = (cap_us - cap_si).abs() / cap_us.max(1e-9);
        assert!(
            rel < 0.15,
            "{id}: capacity differs too much — US {cap_us} vs SI {cap_si} (rel {rel:.3})"
        );
    }

    // Round-trip back to US restores the original values exactly.
    convert_project(&mut p, UnitSystem::UsCustomary);
    let back = flows(&p);
    for ((id, q0, c0), (_, q1, c1)) in us.iter().zip(back.iter()) {
        assert!((q0 - q1).abs() < 1e-6, "{id}: round-trip Q {q0} vs {q1}");
        assert!((c0 - c1).abs() < 1e-6, "{id}: round-trip capacity {c0} vs {c1}");
    }
}

/// printpdf writes show-text operands as uppercase hex strings.
fn hex_upper(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02X}")).collect()
}

/// The SI toggle converts inputs only; results stay U.S. customary. The PDF
/// must label them that way: the stored mm/hr IDF coefficient as mm/hr, pipe
/// sizes from the project in mm, and never a metric flow unit on engine cfs.
#[test]
fn si_pdf_labels_match_the_numbers() {
    let mut p = Project::demo();
    convert_project(&mut p, UnitSystem::Si);
    let a = p
        .to_analysis_network()
        .analyze(&p.idf(), &p.options())
        .expect("analyze");
    let path = std::env::temp_dir().join("stormsewer-si-labels.pdf");
    export_pdf_with(&p, &a, &[], None, &PdfOptions::default(), &path).expect("pdf");
    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    let _ = std::fs::remove_file(&path);

    assert!(text.contains(&hex_upper(" mm/hr ")), "IDF line not labeled mm/hr");
    assert!(!text.contains(&hex_upper("in/hr      Design")), "mm/hr coefficient labeled in/hr");
    assert!(text.contains(&hex_upper("U.S. customary results")), "units note missing");
    assert!(!text.contains(&hex_upper("m³/s")) && !text.contains(&hex_upper("m3/s")));
    for pipe in &p.pipes {
        if pipe.shape == "circular" {
            let mm = format!("{:.0}", pipe.diameter * 1000.0);
            assert!(text.contains(&hex_upper(&mm)), "pipe {} size {mm} mm missing", pipe.id);
        }
    }
}
