// SPDX-License-Identifier: GPL-3.0-or-later

//! Plain-text report tables for an [`Analysis`], in the spirit of Hydraflow
//! Storm Sewers' pipe and HGL summaries.

use crate::network::Analysis;

fn f(x: f64, w: usize, p: usize) -> String {
    format!("{:>w$.p$}", x, w = w, p = p)
}

/// Format the pipe hydraulics table.
pub fn pipe_table(a: &Analysis) -> String {
    let mut s = String::new();
    s.push_str(
        "Pipe   From   To     Slope    Tc    i      Q      Cap    %Full  V      yn     HGLup    HGLdn    Status\n",
    );
    s.push_str(
        "                     ft/ft    min   in/hr  cfs    cfs    %      ft/s   ft     ft       ft\n",
    );
    s.push_str(&"-".repeat(110));
    s.push('\n');
    for p in &a.pipes {
        let yn = p.normal_depth.map(|y| f(y, 6, 2)).unwrap_or_else(|| "  full".into());
        let hup = p.hgl_up.map(|h| f(h, 8, 2)).unwrap_or_else(|| "      --".into());
        let hdn = p.hgl_dn.map(|h| f(h, 8, 2)).unwrap_or_else(|| "      --".into());
        let status = if p.capacity_unavailable() {
            p.capacity_na_label()
        } else if p.report_surcharged() {
            "SURCHARGED"
        } else {
            "ok"
        };
        s.push_str(&format!(
            "{:<6} {:<6} {:<6} {} {} {} {} {} {} {} {} {} {}  {}\n",
            p.id,
            p.from,
            p.to,
            f(p.slope, 7, 4),
            f(p.tc, 5, 1),
            f(p.intensity, 6, 2),
            f(p.design_q, 6, 2),
            f(p.capacity, 6, 2),
            f(p.pct_full * 100.0, 6, 1),
            f(p.velocity, 6, 2),
            yn,
            hup,
            hdn,
            status,
        ));
    }
    s
}

/// Format the node / HGL table.
pub fn node_table(a: &Analysis) -> String {
    let mut s = String::new();
    s.push_str("Node   Tc(min)  Rim(ft)   HGL(ft)   Freeboard  Status\n");
    s.push_str(&"-".repeat(60));
    s.push('\n');
    for n in &a.nodes {
        let fb = n.rim - n.hgl;
        let status = if n.surcharge_to_surface { "FLOODING" } else { "ok" };
        s.push_str(&format!(
            "{:<6} {} {} {} {}  {}\n",
            n.id,
            f(n.tc, 7, 1),
            f(n.rim, 8, 2),
            f(n.hgl, 8, 2),
            f(fb, 9, 2),
            status,
        ));
    }
    s
}

/// Full report: pipe table followed by node/HGL table.
pub fn format_analysis(a: &Analysis) -> String {
    let mut s = String::new();
    s.push_str("=== STORM SEWER ANALYSIS ===\n\n");
    s.push_str(&pipe_table(a));
    s.push('\n');
    s.push_str(&node_table(a));
    // Summary flags.
    let surcharged: Vec<&str> = a.pipes.iter().filter(|p| p.surcharged).map(|p| p.id.as_str()).collect();
    let flooding: Vec<&str> = a.nodes.iter().filter(|n| n.surcharge_to_surface).map(|n| n.id.as_str()).collect();
    s.push('\n');
    if surcharged.is_empty() && flooding.is_empty() {
        s.push_str("All pipes flow open-channel; no surface flooding.\n");
    } else {
        if !surcharged.is_empty() {
            s.push_str(&format!("Surcharged pipes: {}\n", surcharged.join(", ")));
        }
        if !flooding.is_empty() {
            s.push_str(&format!("Structures flooding (HGL > rim): {}\n", flooding.join(", ")));
        }
    }
    s
}
