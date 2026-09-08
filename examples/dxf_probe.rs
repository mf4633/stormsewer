// Probe the DXF underlay + network importers on an arbitrary drawing.
// Run: cargo run --release --example dxf_probe -- <file.dxf>
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dxf_probe <file.dxf>");
    let path = std::path::Path::new(&path);
    let t0 = std::time::Instant::now();
    match stormsewer::io::dxf::import_dxf_underlay(path) {
        Ok(segs) => {
            let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for s in &segs {
                for (x, y) in [(s.x1, s.y1), (s.x2, s.y2)] {
                    minx = minx.min(x);
                    miny = miny.min(y);
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
            println!(
                "UNDERLAY segments={} bbox=({minx:.2},{miny:.2})-({maxx:.2},{maxy:.2}) in {:?}",
                segs.len(),
                t0.elapsed()
            );
            if let Some(out) = std::env::args().nth(2) {
                let mut csv = String::new();
                for s in &segs {
                    csv.push_str(&format!(
                        "{},{},{},{}
",
                        s.x1, s.y1, s.x2, s.y2
                    ));
                }
                std::fs::write(out, csv).unwrap();
            }
        }
        Err(e) => println!("UNDERLAY ERROR: {e} in {:?}", t0.elapsed()),
    }
    let t1 = std::time::Instant::now();
    match stormsewer::io::dxf::import_dxf(path) {
        Ok(p) => println!(
            "NETWORK nodes={} pipes={} in {:?}",
            p.nodes.len(),
            p.pipes.len(),
            t1.elapsed()
        ),
        Err(e) => println!("NETWORK ERROR: {e} in {:?}", t1.elapsed()),
    }
}
