// SPDX-License-Identifier: GPL-3.0-or-later
//! Regression: LandXML as produced by Autodesk Civil 3D links pipes via
//! `refStart`/`refEnd` ATTRIBUTES and uses "northing easting" point order.
//! Before the fix, such files imported with every pipe dangling (dropped) and
//! coordinates transposed.

use stormsewer::io::import_landxml;

const CIVIL3D_LANDXML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2">
  <Units>
    <Imperial areaUnit="squareFoot" linearUnit="foot" diameterUnit="inch"/>
  </Units>
  <PipeNetworks>
    <PipeNetwork name="Storm Network">
      <Structs>
        <Struct name="MH-1" role="junction"><Center>500.0 1000.0 106.0</Center><Invert>100.0</Invert></Struct>
        <Struct name="MH-2" role="junction"><Center>700.0 1000.0 105.0</Center><Invert>99.0</Invert></Struct>
        <Struct name="OUT-1" role="outfall"><Center>900.0 1000.0 104.0</Center><Invert>98.0</Invert></Struct>
      </Structs>
      <Pipes>
        <Pipe name="P-1" refStart="MH-1" refEnd="MH-2"><CircPipe diameter="18.0"/></Pipe>
        <Pipe name="P-2" refStart="MH-2" refEnd="OUT-1"><CircPipe diameter="24.0"/></Pipe>
      </Pipes>
    </PipeNetwork>
  </PipeNetworks>
</LandXML>"#;

#[test]
fn civil3d_landxml_imports_connected_pipes_with_correct_coordinates() {
    let path = std::env::temp_dir().join("stormsewer_civil3d_test.xml");
    std::fs::write(&path, CIVIL3D_LANDXML).unwrap();
    let project = import_landxml(&path).expect("import should succeed");
    let _ = std::fs::remove_file(&path);

    // Both pipes must survive: refStart/refEnd resolved, so nothing dangled.
    assert_eq!(
        project.pipes.len(),
        2,
        "Civil 3D refStart/refEnd pipes must connect (got {})",
        project.pipes.len()
    );
    assert_eq!(project.nodes.len(), 3);

    // The importer renumbers structures (MH-1..OUT-1 -> N1..N3), so assert on the
    // outfall, whose <Center> was "900.0 1000.0 104.0". "northing easting elev"
    // order means first number -> Y (north), second -> X (east).
    let outfall = project
        .nodes
        .iter()
        .find(|n| n.kind == "outfall")
        .expect("outfall imported");
    assert!((outfall.y - 900.0).abs() < 1e-6, "north should map to y, got {}", outfall.y);
    assert!((outfall.x - 1000.0).abs() < 1e-6, "east should map to x, got {}", outfall.x);

    // Topology is a single run: two pipes chained to the outfall.
    let out_id = &outfall.id;
    assert!(
        project.pipes.iter().any(|p| &p.to == out_id),
        "a pipe should discharge to the outfall"
    );
}
