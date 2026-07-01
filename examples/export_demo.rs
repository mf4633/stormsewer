// One-shot demo export — run: cargo run --example export_demo
use std::path::PathBuf;
use stormsewer::io::{export_dxf, export_pdf, Project};

fn main() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let project = Project::demo();
    let net = project.to_network();
    let analysis = net.analyze(&project.idf(), &project.options()).expect("analyze");

    let ssproj = out.join("investor-demo.ssproj");
    let dxf = out.join("investor-demo.dxf");
    let pdf = out.join("investor-demo-report.pdf");

    project.save(&ssproj).expect("save ssproj");
    export_dxf(&project, &dxf).expect("export dxf");
    export_pdf(&project, &analysis, &pdf, None).expect("export pdf");

    println!("Demo deliverables written:");
    println!("  {}", ssproj.display());
    println!("  {}", dxf.display());
    println!("  {}", pdf.display());
    println!("\nPeak pipe flow P3: {:.2} cfs", analysis.pipes.last().map(|p| p.design_q).unwrap_or(0.0));
}