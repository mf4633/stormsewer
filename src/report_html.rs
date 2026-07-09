// SPDX-License-Identifier: GPL-3.0-or-later

//! KaTeX-enabled HTML analysis reports (Hydraflow-style transparency).

use crate::design::{check_inlet_geom, InletGeometry};
use crate::network::{Analysis, Network, NodeKind};
use crate::params::StormAnalysisParams;

/// Metadata for the HTML report header.
#[derive(Clone, Debug)]
pub struct HtmlReportMeta {
    pub title: String,
    pub drawing_name: String,
    pub generated_utc: String,
    /// Submittal metadata (blank fields are omitted from the report header).
    pub project_number: String,
    pub engineer: String,
    pub firm: String,
    pub jurisdiction: String,
}

impl Default for HtmlReportMeta {
    fn default() -> Self {
        Self {
            title: "StormSewer Analysis Report".into(),
            drawing_name: "drawing".into(),
            generated_utc: String::new(),
            project_number: String::new(),
            engineer: String::new(),
            firm: String::new(),
            jurisdiction: String::new(),
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn f(x: f64, p: usize) -> String {
    format!("{x:.prec$}", prec = p)
}

/// A formula step. `equation_html` / `result_html` are trusted, self-generated
/// HTML (Unicode math with `<sub>`/`<sup>`), so they are inserted verbatim; only
/// the caller-supplied `title` is escaped.
fn formula_step(title: &str, equation_html: &str, result_html: &str) -> String {
    format!(
        r#"<div class="hc-formula-step">
<div class="hc-formula-title">{title}</div>
<div class="hc-formula-equation"><span class="hc-formula-label">Equation</span><span class="hc-formula-math">{eq}</span></div>
<div class="hc-formula-result"><span class="hc-formula-label">Result</span><span class="hc-formula-math">{res}</span></div>
</div>"#,
        title = esc(title),
        eq = equation_html,
        res = result_html,
    )
}

fn append_css(out: &mut String) {
    out.push_str(
        r#"<style>
body{font-family:Segoe UI,Arial,sans-serif;margin:24px;color:#1a1a1a;}
h1{font-size:1.4rem;} h2{font-size:1.15rem;margin-top:28px;} h3{font-size:1rem;margin-top:16px;}
table{border-collapse:collapse;width:100%;margin:16px 0;}
th,td{border:1px solid #ccc;padding:6px 8px;text-align:left;font-size:0.9rem;}
th{background:#f0f4f8;} tr.surcharged{background:#ffe6e6;} tr.flooding{background:#fff0f0;} tr.capacity-na{background:#fff8e6;}
.disclaimer{margin-top:24px;padding:12px;background:#fff8e6;border:1px solid #e6c200;font-size:0.85rem;}
.hc-formula-panel{margin:12px 0;}
.hc-formula-step{border:1px solid #e0e6ed;border-radius:6px;padding:10px 12px;margin:8px 0;background:#fafbfc;}
.hc-formula-title{font-weight:600;font-size:0.95rem;margin-bottom:6px;}
.hc-formula-equation,.hc-formula-result{margin:6px 0;padding:6px 8px;border-radius:4px;}
.hc-formula-equation{background:#f4f6f8;border-left:3px solid #7a8a9a;}
.hc-formula-result{background:#e8f4ec;border-left:3px solid #2e7d4f;}
.hc-formula-label{display:block;font-size:0.72rem;font-weight:600;letter-spacing:0.04em;text-transform:uppercase;color:#5a6570;margin-bottom:4px;}
.hc-formula-result .hc-formula-label{color:#2e7d4f;}
.hc-formula-math{font-family:'Cambria Math','Times New Roman',Georgia,serif;font-size:1rem;}
.hc-formula-math sub{font-size:0.72em;} .hc-formula-math sup{font-size:0.72em;}
.meta{color:#555;font-size:0.9rem;}
.pass{color:#0a7a2f;font-weight:600;} .failtxt{color:#b00020;font-weight:600;}
@media print{
  body{margin:0.5in;font-size:11pt;}
  h2{page-break-after:avoid;}
  table,.hc-formula-step,.disclaimer{page-break-inside:avoid;}
  thead{display:table-header-group;}
}
</style>"#,
    );
}

fn pipe_table_html(a: &Analysis) -> String {
    let mut s = String::from(
        r#"<table><thead><tr>
<th>Pipe</th><th>From</th><th>To</th><th>Slope</th><th>Tc</th><th>i</th>
<th>Q</th><th>Cap</th><th>%Full</th><th>V</th><th>yn</th><th>HGL up</th><th>HGL dn</th><th>Status</th>
</tr></thead><tbody>"#,
    );
    for p in &a.pipes {
        let row_class = if p.capacity_unavailable() {
            r#" class="capacity-na""#
        } else if p.report_surcharged() {
            r#" class="surcharged""#
        } else {
            ""
        };
        let yn = p
            .normal_depth
            .map(|y| f(y, 2))
            .unwrap_or_else(|| "full".into());
        let hup = p.hgl_up.map(|h| f(h, 2)).unwrap_or_else(|| "--".into());
        let hdn = p.hgl_dn.map(|h| f(h, 2)).unwrap_or_else(|| "--".into());
        let status = if p.capacity_unavailable() {
            format!(
                r#"<span class="failtxt">{}</span>"#,
                esc(p.capacity_na_label())
            )
        } else if p.report_surcharged() {
            r#"<span class="failtxt">SURCHARGED</span>"#.to_string()
        } else {
            r#"<span class="pass">ok</span>"#.to_string()
        };
        s.push_str(&format!(
            r#"<tr{row_class}><td>{id}</td><td>{from}</td><td>{to}</td>
<td>{slope}</td><td>{tc}</td><td>{i}</td><td>{q}</td><td>{cap}</td>
<td>{pf}</td><td>{v}</td><td>{yn}</td><td>{hup}</td><td>{hdn}</td><td>{status}</td></tr>"#,
            id = esc(&p.id),
            from = esc(&p.from),
            to = esc(&p.to),
            slope = f(p.slope, 4),
            tc = f(p.tc, 1),
            i = f(p.intensity, 2),
            q = f(p.design_q, 2),
            cap = f(p.capacity, 2),
            pf = f(p.pct_full * 100.0, 1),
            v = f(p.velocity, 2),
            yn = esc(&yn),
            hup = esc(&hup),
            hdn = esc(&hdn),
            status = status,
        ));
    }
    s.push_str("</tbody></table>");
    s
}

fn node_table_html(a: &Analysis) -> String {
    let mut s = String::from(
        r#"<table><thead><tr>
<th>Node</th><th>Tc (min)</th><th>Rim (ft)</th><th>HGL (ft)</th><th>Freeboard (ft)</th><th>Status</th>
</tr></thead><tbody>"#,
    );
    for n in &a.nodes {
        let row_class = if n.surcharge_to_surface {
            r#" class="flooding""#
        } else {
            ""
        };
        let fb = n.rim - n.hgl;
        let status = if n.surcharge_to_surface {
            r#"<span class="failtxt">FLOODING</span>"#
        } else {
            r#"<span class="pass">ok</span>"#
        };
        s.push_str(&format!(
            r#"<tr{row_class}><td>{id}</td><td>{tc}</td><td>{rim}</td><td>{hgl}</td><td>{fb}</td><td>{status}</td></tr>"#,
            id = esc(&n.id),
            tc = f(n.tc, 1),
            rim = f(n.rim, 2),
            hgl = f(n.hgl, 2),
            fb = f(fb, 2),
            status = status,
        ));
    }
    s.push_str("</tbody></table>");
    s
}

fn formula_panel(net: &Network, a: &Analysis, params: &StormAnalysisParams) -> String {
    let mut s = String::from(r#"<div class="hc-formula-panel">"#);
    let idf = params.idf.design_curve();
    s.push_str(&formula_step(
        "Rational peak flow",
        "Q = C &middot; i &middot; A",
        "Q<sub>design</sub> = C &middot; i &middot; A &nbsp;&nbsp;(acres &rarr; cfs via 1.008)",
    ));
    s.push_str(&formula_step(
        "IDF intensity",
        "i = a / (t + b)<sup>c</sup>",
        &format!(
            "i = {a} / (t + {b})<sup>{c}</sup> &nbsp;&nbsp;(RP {rp}-yr)",
            a = f(idf.a, 1),
            b = f(idf.b, 1),
            c = f(idf.c, 2),
            rp = params.idf.design_rp,
        ),
    ));
    s.push_str(&formula_step(
        "Manning (US)",
        "Q = (1.486 / n) &middot; A &middot; R<sup>2/3</sup> &middot; S<sup>1/2</sup>",
        "V = Q / A",
    ));
    s.push_str(&formula_step(
        "Junction loss",
        "h<sub>m</sub> = K &middot; V<sup>2</sup> / 2g",
        &format!("K = {:.2}", params.hydraulics.junction_k),
    ));

    if let Some(p) = a.pipes.first() {
        if let Some(nd) = net.nodes.iter().find(|n| n.id == p.from) {
            s.push_str(&formula_step(
                &format!("Example: pipe {}", p.id),
                "Q = C &middot; i &middot; A",
                &format!(
                    "Q<sub>{pid}</sub> = {q:.2} cfs, &nbsp; i = {i:.2} in/hr, &nbsp; t<sub>c</sub> = {tc:.1} min",
                    pid = esc(&p.id),
                    q = p.design_q,
                    i = p.intensity,
                    tc = p.tc,
                ),
            ));
            let _ = nd;
        }
    }
    s.push_str("</div>");
    s
}

fn inlet_table_html(net: &Network, a: &Analysis, geom: &InletGeometry) -> String {
    let mut rows = String::new();
    let mut any = false;
    for nd in &net.nodes {
        if nd.kind != NodeKind::Inlet {
            continue;
        }
        // Approach flow is the LOCAL gutter runoff at this inlet (C·A·i), not the
        // outgoing pipe's accumulated design flow — an inlet only intercepts the
        // surface flow tributary to it, not runoff already underground.
        let intensity = a
            .pipes
            .iter()
            .filter(|p| p.from == nd.id)
            .map(|p| p.intensity)
            .fold(0.0f64, f64::max);
        let q = nd.ca() * intensity;
        if q <= 0.0 {
            continue;
        }
        any = true;
        let chk = check_inlet_geom(q, geom);
        let status = if chk.ok {
            r#"<span class="pass">ok</span>"#
        } else {
            r#"<span class="failtxt">BYPASS</span>"#
        };
        rows.push_str(&format!(
            r#"<tr><td>{id}</td><td>{kind}</td><td>{q:.2}</td><td>{cap:.2}</td><td>{status}</td></tr>"#,
            id = esc(&nd.id),
            kind = esc(geom.kind.label()),
            q = chk.design_q_cfs,
            cap = chk.capacity_cfs,
            status = status,
        ));
    }
    if !any {
        return String::new();
    }
    format!(
        r#"<h2>Inlet capacity (HEC-22)</h2>
<p class="meta">Grate L={gl:.1} ft × W={gw:.1} ft, curb L={cl:.1} ft, Sx={sx:.3}, SL={s:.4} ft/ft, n={n:.3}</p>
<table><thead><tr><th>Node</th><th>Type</th><th>Q (cfs)</th><th>Intercepted (cfs)</th><th>Status</th></tr></thead><tbody>{rows}</tbody></table>"#,
        gl = geom.grate_length_ft,
        gw = geom.grate_width_ft,
        cl = geom.curb_opening_length_ft,
        sx = geom.cross_slope,
        s = geom.gutter_slope,
        n = geom.gutter_n,
        rows = rows,
    )
}

/// Build a self-contained HTML document with KaTeX formula panels and result tables.
pub fn format_analysis_html(
    net: &Network,
    a: &Analysis,
    params: &StormAnalysisParams,
    meta: &HtmlReportMeta,
) -> String {
    let geom = params.inlet_geometry();
    let mut out = String::new();
    out.push_str("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"/>");
    out.push_str(r#"<meta name="viewport" content="width=device-width, initial-scale=1"/>"#);
    out.push_str(&format!("<title>{}</title>", esc(&meta.title)));
    // Fully self-contained — no external stylesheets or scripts, so the report
    // renders identically offline and when archived alongside a signed drawing.
    append_css(&mut out);
    out.push_str("</head><body>");
    out.push_str(&format!("<h1>{}</h1>", esc(&meta.title)));
    out.push_str(&format!(
        r#"<p class="meta">Drawing: <strong>{}</strong> &nbsp;|&nbsp; Generated: {} &nbsp;|&nbsp; {}"#,
        esc(&meta.drawing_name),
        esc(&meta.generated_utc),
        esc(&params.summary()),
    ));
    out.push_str("</p>");

    // Submittal metadata — only the fields the engineer filled in.
    let mut submittal: Vec<String> = Vec::new();
    for (label, val) in [
        ("Project No.", &meta.project_number),
        ("Engineer", &meta.engineer),
        ("Firm", &meta.firm),
        ("Jurisdiction", &meta.jurisdiction),
    ] {
        if !val.trim().is_empty() {
            submittal.push(format!("{label}: <strong>{}</strong>", esc(val)));
        }
    }
    if !submittal.is_empty() {
        out.push_str(&format!(
            r#"<p class="meta">{}</p>"#,
            submittal.join(" &nbsp;|&nbsp; ")
        ));
    }

    out.push_str("<h2>Calculation trace</h2>");
    out.push_str(&formula_panel(net, a, params));

    out.push_str("<h2>Pipe hydraulics</h2>");
    out.push_str(&pipe_table_html(a));

    out.push_str("<h2>Nodes / HGL</h2>");
    out.push_str(&node_table_html(a));

    let inlet = inlet_table_html(net, a, &geom);
    if !inlet.is_empty() {
        out.push_str(&inlet);
    }

    let surcharged: Vec<&str> = a
        .pipes
        .iter()
        .filter(|p| p.report_surcharged())
        .map(|p| p.id.as_str())
        .collect();
    let flooding: Vec<&str> = a
        .nodes
        .iter()
        .filter(|n| n.surcharge_to_surface)
        .map(|n| n.id.as_str())
        .collect();
    out.push_str("<h2>Summary</h2><ul>");
    if surcharged.is_empty() && flooding.is_empty() {
        out.push_str("<li>All pipes flow open-channel; no surface flooding.</li>");
    } else {
        if !surcharged.is_empty() {
            out.push_str(&format!(
                "<li>Surcharged pipes: <strong>{}</strong></li>",
                esc(&surcharged.join(", "))
            ));
        }
        if !flooding.is_empty() {
            out.push_str(&format!(
                "<li>Structures flooding (HGL &gt; rim): <strong>{}</strong></li>",
                esc(&flooding.join(", "))
            ));
        }
    }
    out.push_str("</ul>");

    out.push_str(
        r#"<div class="disclaimer">Formula-transparent report generated by StormSewer. Verify inputs and agency criteria before construction.</div>"#,
    );
    out.push_str("</body></html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idf::IdfCurve;
    use crate::network::{Network, Node, Pipe};
    use crate::params::StormAnalysisParams;

    fn sample() -> (Network, Analysis) {
        let net = Network {
            nodes: vec![
                Node::inlet("N1", 104.0, 110.0, 1.0, 0.70).with_tc_inlet(12.0),
                Node::outfall("OUT", 100.0, 106.0),
            ],
            pipes: vec![Pipe::new("P1", "N1", "OUT", 200.0, 1.25, 0.013)],
        };
        let idf = IdfCurve::new(60.0, 10.0, 0.8);
        let a = net.analyze(&idf, &Default::default()).unwrap();
        (net, a)
    }

    #[test]
    fn html_report_is_self_contained_with_tables() {
        let (net, a) = sample();
        let html = format_analysis_html(
            &net,
            &a,
            &StormAnalysisParams::default(),
            &HtmlReportMeta {
                title: "Test".into(),
                drawing_name: "test.dwg".into(),
                generated_utc: "2026-06-22".into(),
                engineer: "Jane Roe, PE".into(),
                firm: "Acme Engineering".into(),
                project_number: "2026-042".into(),
                jurisdiction: String::new(),
            },
        );
        // No external resources — the report must render offline / when archived.
        assert!(!html.contains("http://") && !html.contains("https://"), "report must be self-contained");
        assert!(!html.to_lowercase().contains("hydrocomplete") && !html.contains("OpenCAD"), "no stray branding");
        assert!(html.contains("hc-formula-panel") && html.contains("hc-formula-math"));
        assert!(html.contains("<table>") && html.contains("P1"));
        assert!(html.contains("@media print"), "print stylesheet present");
        // Submittal metadata is rendered; blank fields (jurisdiction) are omitted.
        assert!(html.contains("Jane Roe, PE") && html.contains("Acme Engineering"));
        assert!(html.contains("2026-042"));
        assert!(!html.contains("Jurisdiction:"), "blank fields omitted");
    }
}