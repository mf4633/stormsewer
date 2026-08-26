// One-shot demo export — run: cargo run --example export_demo
use std::path::PathBuf;
use stormsewer::design::inlets::{network_inlet_pass, InletGeometry};
use stormsewer::design::{design_review, ReviewCriteria};
use stormsewer::io::{export_dxf, export_pdf_with, PdfOptions, Project};

fn main() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut project = Project::demo();
    project.report.project_number = "SS-DEMO-001".into();
    project.report.engineer = "Design Engineer, PE".into();
    project.report.firm = "Sample Engineering".into();
    project.report.jurisdiction = "City of Example".into();

    // to_analysis_network(), not to_network(): the app merges drawn catchments
    // into inlet hydrology before analyzing, and the shipped sample report must
    // show the same flows a user sees on screen.
    let net = project.to_analysis_network();
    let analysis = net.analyze(&project.idf(), &project.options()).expect("analyze");
    let findings = design_review(&net, &analysis, &ReviewCriteria::default());
    let fallback = project.idf_set().design_curve().intensity(project.min_tc);
    let inlet_rows = network_inlet_pass(&project, &|_| fallback, &InletGeometry::default());

    let ssproj = out.join("investor-demo.ssproj");
    let dxf = out.join("investor-demo.dxf");
    let pdf = out.join("investor-demo-report.pdf");

    let opts = PdfOptions {
        generated_on: "August 26, 2026".into(),
        ..PdfOptions::default()
    };

    project.save(&ssproj).expect("save ssproj");
    export_dxf(&project, &dxf).expect("export dxf");
    export_pdf_with(&project, &analysis, &inlet_rows, Some(&findings), &opts, &pdf)
        .expect("export pdf");

    println!("Demo deliverables written:");
    println!("  {}", ssproj.display());
    println!("  {}", dxf.display());
    println!("  {}", pdf.display());
    println!(
        "\nPeak pipe flow P3: {:.2} cfs",
        analysis.pipes.last().map(|p| p.design_q).unwrap_or(0.0)
    );
}
