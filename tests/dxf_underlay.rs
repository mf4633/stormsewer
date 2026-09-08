// SPDX-License-Identifier: GPL-3.0-or-later

//! DXF site-underlay import against the things real drawings do: block
//! definitions at the origin, INSERTs (scaled/rotated), paper-space title
//! blocks, old-style POLYLINE + VERTEX runs, open vs closed polylines, ARCs.
//!
//! Found on a 51 MB Civil 3D site plan and a QGIS export: the old reader
//! walked every section, so 465 block definitions drawn at (0,0) stretched the
//! extents from the site to the origin, the 781 tree INSERTs of the QGIS file
//! produced two segments, heavy POLYLINE pipe runs vanished, and every open
//! polyline was closed with a phantom segment.

use stormsewer::io::dxf::import_dxf_underlay;

fn dxf(pairs: &[(i32, &str)]) -> String {
    let mut s = String::new();
    for (code, val) in pairs {
        s.push_str(&format!("{code:>3}\n{val}\n"));
    }
    s
}

fn write_temp(name: &str, body: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn blocks_inserts_paperspace_polylines_and_arcs() {
    let body = dxf(&[
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$ACADVER"),
        (1, "AC1032"),
        (0, "ENDSEC"),
        // A symbol defined at the origin: one unit line along +X, plus a
        // stray LINE in the block table that must never reach the underlay
        // unless the block is inserted.
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (8, "0"),
        (2, "SYM"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (8, "SYM-LINES"),
        (10, "0"),
        (20, "0"),
        (11, "1"),
        (21, "0"),
        (0, "ENDBLK"),
        (0, "BLOCK"),
        (8, "0"),
        (2, "UNUSED"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (8, "0"),
        (10, "-9999"),
        (20, "-9999"),
        (11, "-9998"),
        (21, "-9998"),
        (0, "ENDBLK"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        // Model-space LINE.
        (0, "LINE"),
        (8, "SITE"),
        (10, "100"),
        (20, "100"),
        (11, "200"),
        (21, "100"),
        // Paper-space LINE (title block): skipped.
        (0, "LINE"),
        (8, "BORDER"),
        (67, "1"),
        (10, "5000"),
        (20, "5000"),
        (11, "5100"),
        (21, "5000"),
        // INSERT SYM at (300,300), scale 2, rotated 90°: the unit line becomes
        // (300,300)-(300,302).
        (0, "INSERT"),
        (8, "SYMBOLS"),
        (2, "SYM"),
        (10, "300"),
        (20, "300"),
        (41, "2"),
        (42, "2"),
        (50, "90"),
        // Old-style open POLYLINE with three VERTEX entities: 2 segments, no closure.
        (0, "POLYLINE"),
        (8, "STORM-PIPES"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (0, "VERTEX"),
        (8, "STORM-PIPES"),
        (10, "400"),
        (20, "400"),
        (0, "VERTEX"),
        (8, "STORM-PIPES"),
        (10, "500"),
        (20, "400"),
        (0, "VERTEX"),
        (8, "STORM-PIPES"),
        (10, "500"),
        (20, "500"),
        (0, "SEQEND"),
        (8, "STORM-PIPES"),
        // Closed LWPOLYLINE triangle: 3 segments.
        (0, "LWPOLYLINE"),
        (8, "WS-BOUNDARY"),
        (90, "3"),
        (70, "1"),
        (10, "600"),
        (20, "600"),
        (10, "700"),
        (20, "600"),
        (10, "700"),
        (20, "700"),
        // Open LWPOLYLINE with three vertices: 2 segments.
        (0, "LWPOLYLINE"),
        (8, "FLOW"),
        (90, "3"),
        (70, "0"),
        (10, "800"),
        (20, "800"),
        (10, "900"),
        (20, "800"),
        (10, "900"),
        (20, "900"),
        // Quarter ARC of radius 10 about (1000,1000) from 0° to 90°.
        (0, "ARC"),
        (8, "V-PVMT"),
        (10, "1000"),
        (20, "1000"),
        (40, "10"),
        (50, "0"),
        (51, "90"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ]);
    let path = write_temp("stormsewer_underlay_real_world.dxf", &body);
    let segs = import_dxf_underlay(&path).expect("underlay imports");
    let _ = std::fs::remove_file(&path);

    let xs = segs.iter().flat_map(|s| [s.x1, s.x2]);
    let ys = segs.iter().flat_map(|s| [s.y1, s.y2]);
    let min_x = xs.clone().fold(f64::INFINITY, f64::min);
    let min_y = ys.clone().fold(f64::INFINITY, f64::min);
    let max_x = xs.fold(f64::NEG_INFINITY, f64::max);
    let max_y = ys.fold(f64::NEG_INFINITY, f64::max);
    assert!(
        min_x >= 100.0 && min_y >= 100.0,
        "block definitions leaked into the underlay: min ({min_x},{min_y})"
    );
    assert!(
        max_x <= 1010.0 && max_y <= 1010.0,
        "paper-space entity drawn: max ({max_x},{max_y})"
    );

    // Model-space LINE present.
    assert!(segs
        .iter()
        .any(|s| near(s.x1, 100.0) && near(s.y1, 100.0) && near(s.x2, 200.0) && near(s.y2, 100.0)));
    // INSERT expanded with scale and rotation.
    assert!(
        segs.iter().any(|s| near(s.x1, 300.0)
            && near(s.y1, 300.0)
            && near(s.x2, 300.0)
            && near(s.y2, 302.0)),
        "INSERT of SYM at (300,300) ×2 rot 90 should give (300,300)-(300,302); got {:?}",
        segs.iter()
            .filter(|s| s.x1 >= 299.0 && s.x1 <= 301.0)
            .collect::<Vec<_>>()
    );
    // Heavy POLYLINE run: exactly its two segments, no closing edge back to (400,400).
    let storm: Vec<_> = segs
        .iter()
        .filter(|s| {
            s.x1 >= 400.0
                && s.x1 <= 500.0
                && s.y1 >= 400.0
                && s.y1 <= 500.0
                && s.x2 <= 500.0
                && s.y2 <= 500.0
        })
        .collect();
    assert_eq!(
        storm.len(),
        2,
        "open POLYLINE/VERTEX run → 2 segments, got {storm:?}"
    );
    // Closed triangle → 3 segments; open 3-vertex polyline → 2.
    let tri = segs
        .iter()
        .filter(|s| s.x1 >= 600.0 && s.x1 <= 700.0 && s.y1 >= 600.0 && s.y1 <= 700.0)
        .count();
    assert_eq!(tri, 3, "closed LWPOLYLINE keeps its closing edge");
    let open = segs
        .iter()
        .filter(|s| s.x1 >= 800.0 && s.x1 <= 900.0 && s.y1 >= 800.0 && s.y1 <= 900.0)
        .count();
    assert_eq!(open, 2, "open LWPOLYLINE must not be closed");
    // ARC: every point on the circle of radius 10, sweeping only the first quadrant.
    let arc: Vec<_> = segs.iter().filter(|s| s.x1 >= 990.0).collect();
    assert!(arc.len() >= 2, "arc tessellated");
    for s in &arc {
        for (x, y) in [(s.x1, s.y1), (s.x2, s.y2)] {
            let r = ((x - 1000.0).powi(2) + (y - 1000.0).powi(2)).sqrt();
            assert!(
                (r - 10.0).abs() < 1e-6,
                "arc point off the circle: ({x},{y})"
            );
            assert!(
                x >= 1000.0 - 1e-9 && y >= 1000.0 - 1e-9,
                "arc left its 0-90° sweep: ({x},{y})"
            );
        }
    }
}

#[test]
fn nested_inserts_compose_and_terminate() {
    // OUTER contains an INSERT of INNER; INNER is a line. A self-referencing
    // block must not recurse forever.
    let body = dxf(&[
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (2, "INNER"),
        (10, "0"),
        (20, "0"),
        (0, "LINE"),
        (8, "0"),
        (10, "0"),
        (20, "0"),
        (11, "1"),
        (21, "0"),
        (0, "ENDBLK"),
        (0, "BLOCK"),
        (2, "OUTER"),
        (10, "0"),
        (20, "0"),
        (0, "INSERT"),
        (8, "0"),
        (2, "INNER"),
        (10, "10"),
        (20, "0"),
        (0, "INSERT"),
        (8, "0"),
        (2, "OUTER"),
        (10, "0"),
        (20, "0"),
        (0, "ENDBLK"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "INSERT"),
        (8, "0"),
        (2, "OUTER"),
        (10, "1000"),
        (20, "2000"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ]);
    let path = write_temp("stormsewer_underlay_nested.dxf", &body);
    let segs = import_dxf_underlay(&path).expect("nested inserts import");
    let _ = std::fs::remove_file(&path);
    assert!(
        segs.iter()
            .any(|s| near(s.x1, 1010.0) && near(s.y1, 2000.0) && near(s.x2, 1011.0)),
        "INNER line placed through OUTER at (1010,2000); got {segs:?}"
    );
    assert!(
        segs.len() <= 8,
        "self-referencing block bounded, got {} segments",
        segs.len()
    );
}
