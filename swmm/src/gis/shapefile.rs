// SPDX-License-Identifier: GPL-3.0-or-later

//! ESRI shapefiles, read and written from the public *ESRI Shapefile
//! Technical Description* (July 1998).
//!
//! A shapefile is three files: `.shp` (geometry), `.shx` (record offsets)
//! and `.dbf` (attributes), with `.prj` (coordinate system as WKT) and
//! `.cpg` (text encoding) beside them. The `.shp` is self-describing —
//! every record carries its own length — so the index is optional on read
//! and always written.
//!
//! Read: Null, Point, PolyLine, Polygon, MultiPoint and their Z and M
//! variants (Z and M are dropped; only X/Y reach the model). MultiPatch
//! (31) is refused. Polygon rings are grouped into polygons by the spec's
//! rule: clockwise rings are outer rings, counter-clockwise rings are holes
//! of the outer ring that contains them.
//!
//! Write: Point, PolyLine and Polygon (2D) with outer rings clockwise.

use std::path::{Path, PathBuf};

use crate::gis::dbf;
use crate::gis::prj;
use crate::gis::vector::{bounds_of, close, ring_area, Feature, Field, FieldValue, Geometry, Layer};
use crate::{Error, Result};

fn i32be(b: &[u8], i: usize) -> Result<i32> {
    b.get(i..i + 4)
        .map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::Format("shapefile ends inside a record header".into()))
}

fn i32le(b: &[u8], i: usize) -> Result<i32> {
    b.get(i..i + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::Format("shapefile ends inside a record".into()))
}

fn f64le(b: &[u8], i: usize) -> Result<f64> {
    b.get(i..i + 8)
        .map(|s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or_else(|| Error::Format("shapefile ends inside a coordinate".into()))
}

/// The shape type code of a `.shp` header.
pub fn shape_type(bytes: &[u8]) -> Result<i32> {
    if bytes.len() < 100 {
        return Err(Error::Format("shapefile is shorter than its 100-byte header".into()));
    }
    if i32be(bytes, 0)? != 9994 {
        return Err(Error::Format("not a shapefile: file code is not 9994".into()));
    }
    i32le(bytes, 32)
}

pub fn shape_type_name(code: i32) -> &'static str {
    match code {
        0 => "Null",
        1 => "Point",
        3 => "PolyLine",
        5 => "Polygon",
        8 => "MultiPoint",
        11 => "PointZ",
        13 => "PolyLineZ",
        15 => "PolygonZ",
        18 => "MultiPointZ",
        21 => "PointM",
        23 => "PolyLineM",
        25 => "PolygonM",
        28 => "MultiPointM",
        31 => "MultiPatch",
        _ => "unknown",
    }
}

fn read_points(b: &[u8], at: usize, n: usize) -> Result<Vec<(f64, f64)>> {
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        pts.push((f64le(b, at + 16 * i)?, f64le(b, at + 16 * i + 8)?));
    }
    Ok(pts)
}

/// Parse one record's content (after the 8-byte record header).
fn parse_shape(b: &[u8]) -> Result<Option<Geometry>> {
    let code = i32le(b, 0)?;
    match code {
        0 => Ok(None),
        1 | 11 | 21 => Ok(Some(Geometry::Point(f64le(b, 4)?, f64le(b, 12)?))),
        8 | 18 | 28 => {
            let n = i32le(b, 36)?.max(0) as usize;
            let pts = read_points(b, 40, n)?;
            Ok(Some(Geometry::MultiPoint(pts)))
        }
        3 | 13 | 23 | 5 | 15 | 25 => {
            let nparts = i32le(b, 36)?.max(0) as usize;
            let npoints = i32le(b, 40)?.max(0) as usize;
            let mut parts = Vec::with_capacity(nparts);
            for i in 0..nparts {
                parts.push(i32le(b, 44 + 4 * i)?.max(0) as usize);
            }
            let pts = read_points(b, 44 + 4 * nparts, npoints)?;
            let mut pieces: Vec<Vec<(f64, f64)>> = Vec::with_capacity(nparts);
            for (i, &start) in parts.iter().enumerate() {
                let end = parts.get(i + 1).copied().unwrap_or(npoints).min(npoints);
                if start < end {
                    pieces.push(pts[start..end].to_vec());
                }
            }
            if matches!(code, 3 | 13 | 23) {
                Ok(Some(if pieces.len() == 1 {
                    Geometry::LineString(pieces.remove(0))
                } else {
                    Geometry::MultiLineString(pieces)
                }))
            } else {
                Ok(Some(group_rings(pieces)))
            }
        }
        31 => Err(Error::Format("MultiPatch shapefiles are not supported".into())),
        other => Err(Error::Format(format!("unknown shape type {other}"))),
    }
}

/// Ray-casting point-in-ring test.
pub fn point_in_ring(p: (f64, f64), ring: &[(f64, f64)]) -> bool {
    let n = ring.len();
    let mut inside = false;
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Group rings into polygons: clockwise rings (negative shoelace area)
/// are outer rings; each counter-clockwise ring becomes a hole of the
/// outer ring containing its first vertex, or a polygon of its own when
/// none does (a writer that ignored orientation).
pub fn group_rings(rings: Vec<Vec<(f64, f64)>>) -> Geometry {
    let mut outers: Vec<Vec<Vec<(f64, f64)>>> = Vec::new();
    let mut holes: Vec<Vec<(f64, f64)>> = Vec::new();
    for r in rings {
        if ring_area(&r) <= 0.0 {
            outers.push(vec![r]);
        } else {
            holes.push(r);
        }
    }
    for h in holes {
        let Some(p) = h.first().copied() else { continue };
        match outers.iter_mut().find(|o| point_in_ring(p, &o[0])) {
            Some(o) => o.push(h),
            None => outers.push(vec![h]),
        }
    }
    if outers.len() == 1 {
        Geometry::Polygon(outers.remove(0))
    } else {
        Geometry::MultiPolygon(outers)
    }
}

/// Parse a `.shp` image into geometries (None for null shapes), in record
/// order.
pub fn parse_shp(bytes: &[u8]) -> Result<(i32, Vec<Option<Geometry>>)> {
    let code = shape_type(bytes)?;
    if code == 31 {
        return Err(Error::Format("MultiPatch shapefiles are not supported".into()));
    }
    let file_len = (i32be(bytes, 24)? as usize) * 2;
    let end = file_len.min(bytes.len());
    let mut shapes = Vec::new();
    let mut pos = 100;
    while pos + 8 <= end {
        let content_len = (i32be(bytes, pos + 4)? as usize) * 2;
        let start = pos + 8;
        let stop = start + content_len;
        if stop > bytes.len() || content_len < 4 {
            return Err(Error::Format(format!(
                "shapefile record {} runs past the end of the file",
                shapes.len() + 1
            )));
        }
        shapes.push(parse_shape(&bytes[start..stop])?);
        pos = stop;
    }
    Ok((code, shapes))
}

/// The companion path with another extension, matching the case of the
/// original's extension (`.SHP` → `.DBF`).
pub fn sibling(path: &Path, ext: &str) -> PathBuf {
    let upper = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.chars().all(|c| !c.is_ascii_lowercase()) && e.chars().any(|c| c.is_ascii_uppercase()));
    let e = if upper { ext.to_ascii_uppercase() } else { ext.to_string() };
    path.with_extension(e)
}

/// Read a shapefile set as a layer: the `.shp` geometries, the `.dbf`
/// attributes (when present) and the `.prj` coordinate system (when
/// present). Fewer attribute records than shapes leaves the tail null.
pub fn read(path: &Path) -> Result<Layer> {
    let bytes = std::fs::read(path)?;
    let (_, shapes) = parse_shp(&bytes)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("layer")
        .to_string();
    let dbf_path = sibling(path, "dbf");
    let (fields, records) = if dbf_path.exists() {
        let t = dbf::read(&dbf_path)?;
        (t.fields(), t.records)
    } else {
        (Vec::new(), Vec::new())
    };
    let mut layer = Layer::new(&name, fields);
    let nf = layer.fields.len();
    let mut records = records.into_iter();
    for g in shapes {
        let mut values = records.next().unwrap_or_default();
        values.resize(nf, FieldValue::Null);
        layer.features.push(Feature::new(g, values));
    }
    let prj_path = sibling(path, "prj");
    if let Ok(text) = std::fs::read_to_string(&prj_path) {
        let text = text.trim().to_string();
        if !text.is_empty() {
            layer.crs = prj::parse_wkt(&text).ok();
            layer.crs_text = Some(text);
        }
    }
    Ok(layer)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

fn put_i32be(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_i32le(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn oriented(ring: Vec<(f64, f64)>, clockwise: bool) -> Vec<(f64, f64)> {
    let ring = close(ring);
    let cw = ring_area(&ring) < 0.0;
    if cw == clockwise {
        ring
    } else {
        ring.into_iter().rev().collect()
    }
}

/// The record content for one geometry (shape type + body). Multi-part
/// geometries become one multi-part record; a geometry of another kind
/// than `code` is written as a Null shape.
fn encode_shape(g: Option<&Geometry>, code: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let parts: Vec<Vec<(f64, f64)>> = match (g, code) {
        (Some(Geometry::Point(x, y)), 1) => {
            put_i32le(&mut out, 1);
            put_f64(&mut out, *x);
            put_f64(&mut out, *y);
            return out;
        }
        (Some(Geometry::MultiPoint(p)), 1) if !p.is_empty() => {
            put_i32le(&mut out, 1);
            put_f64(&mut out, p[0].0);
            put_f64(&mut out, p[0].1);
            return out;
        }
        (Some(Geometry::LineString(p)), 3) => vec![p.clone()],
        (Some(Geometry::MultiLineString(parts)), 3) => parts.clone(),
        (Some(Geometry::Polygon(rings)), 5) => rings
            .iter()
            .enumerate()
            .map(|(i, r)| oriented(r.clone(), i == 0))
            .collect(),
        (Some(Geometry::MultiPolygon(polys)), 5) => polys
            .iter()
            .flat_map(|rings| {
                rings
                    .iter()
                    .enumerate()
                    .map(|(i, r)| oriented(r.clone(), i == 0))
            })
            .collect(),
        _ => {
            put_i32le(&mut out, 0);
            return out;
        }
    };
    let parts: Vec<Vec<(f64, f64)>> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        put_i32le(&mut out, 0);
        return out;
    }
    let all: Vec<(f64, f64)> = parts.concat();
    let (x0, y0, x1, y1) = bounds_of(&all).unwrap_or((0.0, 0.0, 0.0, 0.0));
    put_i32le(&mut out, code);
    for v in [x0, y0, x1, y1] {
        put_f64(&mut out, v);
    }
    put_i32le(&mut out, parts.len() as i32);
    put_i32le(&mut out, all.len() as i32);
    let mut start = 0;
    for p in &parts {
        put_i32le(&mut out, start as i32);
        start += p.len();
    }
    for (x, y) in &all {
        put_f64(&mut out, *x);
        put_f64(&mut out, *y);
    }
    out
}

fn header(code: i32, file_len_bytes: usize, bbox: (f64, f64, f64, f64)) -> Vec<u8> {
    let mut h = Vec::with_capacity(100);
    put_i32be(&mut h, 9994);
    for _ in 0..5 {
        put_i32be(&mut h, 0);
    }
    put_i32be(&mut h, (file_len_bytes / 2) as i32);
    put_i32le(&mut h, 1000);
    put_i32le(&mut h, code);
    for v in [bbox.0, bbox.1, bbox.2, bbox.3, 0.0, 0.0, 0.0, 0.0] {
        put_f64(&mut h, v);
    }
    h
}

/// The shape type code for a layer: 1, 3 or 5 by its geometry kind.
pub fn code_for(layer: &Layer) -> i32 {
    match layer.geometry_kind() {
        Some(crate::gis::vector::GeometryKind::Point) | None => 1,
        Some(crate::gis::vector::GeometryKind::Line) => 3,
        Some(crate::gis::vector::GeometryKind::Polygon) => 5,
    }
}

/// Encode the `.shp` and `.shx` images.
pub fn encode(layer: &Layer) -> (Vec<u8>, Vec<u8>) {
    let code = code_for(layer);
    let mut records = Vec::new();
    let mut index = Vec::new();
    let mut offset = 100usize;
    for (i, f) in layer.features.iter().enumerate() {
        let content = encode_shape(f.geometry.as_ref(), code);
        put_i32be(&mut index, (offset / 2) as i32);
        put_i32be(&mut index, (content.len() / 2) as i32);
        put_i32be(&mut records, (i + 1) as i32);
        put_i32be(&mut records, (content.len() / 2) as i32);
        records.extend_from_slice(&content);
        offset += 8 + content.len();
    }
    let bbox = layer.bounds().unwrap_or((0.0, 0.0, 0.0, 0.0));
    let mut shp = header(code, 100 + records.len(), bbox);
    shp.extend_from_slice(&records);
    let mut shx = header(code, 100 + index.len(), bbox);
    shx.extend_from_slice(&index);
    (shp, shx)
}

/// Write `<path>.shp`, `.shx`, `.dbf` (+ `.cpg`) and, when `prj_wkt` is
/// given, `.prj`. A layer with no fields gets one `FID` column so the
/// `.dbf` is never empty (readers require at least one field).
pub fn write(path: &Path, layer: &Layer, prj_wkt: Option<&str>) -> Result<()> {
    let shp_path = path.with_extension("shp");
    let (shp, shx) = encode(layer);
    std::fs::write(&shp_path, shp)?;
    std::fs::write(sibling(&shp_path, "shx"), shx)?;
    let (fields, records): (Vec<Field>, Vec<Vec<FieldValue>>) = if layer.fields.is_empty() {
        (
            vec![Field::new("FID", crate::gis::vector::FieldKind::Integer)],
            (0..layer.features.len())
                .map(|i| vec![FieldValue::Number(i as f64)])
                .collect(),
        )
    } else {
        (
            layer.fields.clone(),
            layer.features.iter().map(|f| f.values.clone()).collect(),
        )
    };
    dbf::write(&sibling(&shp_path, "dbf"), &fields, &records)?;
    if let Some(wkt) = prj_wkt {
        std::fs::write(sibling(&shp_path, "prj"), wkt)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gis::vector::FieldKind;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("stormsewer-gis-tests").join(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn points_round_trip_with_attributes_and_prj() {
        let dir = temp("shp-points");
        let mut layer = Layer::new(
            "nodes",
            vec![
                Field::new("NAME", FieldKind::Text),
                Field::new("INVERT", FieldKind::Number),
            ],
        );
        layer.features.push(Feature::new(
            Some(Geometry::Point(1000.5, 2000.25)),
            vec![FieldValue::Text("J1".into()), FieldValue::Number(100.5)],
        ));
        layer.features.push(Feature::new(
            None,
            vec![FieldValue::Text("J2".into()), FieldValue::Null],
        ));
        let p = dir.join("nodes.shp");
        write(&p, &layer, Some("PROJCS[\"x\",GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",0.0],PARAMETER[\"Central_Meridian\",-81.0],PARAMETER[\"Scale_Factor\",0.9996],PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]")).unwrap();
        assert!(dir.join("nodes.shx").exists());
        assert!(dir.join("nodes.dbf").exists());
        assert!(dir.join("nodes.cpg").exists());
        let back = read(&p).unwrap();
        assert_eq!(back.name, "nodes");
        assert_eq!(back.features.len(), 2);
        assert_eq!(back.features[0].geometry, Some(Geometry::Point(1000.5, 2000.25)));
        assert_eq!(back.features[1].geometry, None);
        assert_eq!(back.value(0, "NAME"), Some(&FieldValue::Text("J1".into())));
        assert_eq!(back.value(0, "INVERT"), Some(&FieldValue::Number(100.5)));
        assert_eq!(back.value(1, "INVERT"), Some(&FieldValue::Null));
        assert!(back.crs.is_some(), "{:?}", back.crs_text);
        assert_eq!(back.crs.as_ref().unwrap().epsg, Some(32617));
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(shape_type(&bytes).unwrap(), 1);
        // The file length in the header is the whole file, in words.
        assert_eq!(i32be(&bytes, 24).unwrap() as usize * 2, bytes.len());
    }

    #[test]
    fn polygons_with_holes_and_multiparts_group_by_orientation() {
        let dir = temp("shp-polys");
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let hole = vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0), (2.0, 2.0)];
        let island = vec![(20.0, 0.0), (30.0, 0.0), (30.0, 5.0), (20.0, 5.0), (20.0, 0.0)];
        let mut layer = Layer::new("subs", vec![]);
        layer.features.push(Feature::new(
            Some(Geometry::MultiPolygon(vec![vec![outer.clone(), hole.clone()], vec![island.clone()]])),
            vec![],
        ));
        let p = dir.join("subs.shp");
        write(&p, &layer, None).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.fields.len(), 1, "FID column added");
        let g = back.features[0].geometry.as_ref().unwrap();
        match g {
            Geometry::MultiPolygon(polys) => {
                assert_eq!(polys.len(), 2);
                assert_eq!(polys[0].len(), 2, "outer + hole");
                assert!(ring_area(&polys[0][0]) < 0.0, "outer written clockwise");
                assert!(ring_area(&polys[0][1]) > 0.0, "hole counter-clockwise");
                assert_eq!(polys[1].len(), 1);
            }
            other => panic!("{other:?}"),
        }
        assert!((g.area() - (100.0 - 4.0 + 50.0)).abs() < 1e-9);
        // A single polygon reads back as Polygon, not MultiPolygon.
        let mut one = Layer::new("one", vec![]);
        one.features
            .push(Feature::new(Some(Geometry::Polygon(vec![outer.clone()])), vec![]));
        let p1 = dir.join("one.shp");
        write(&p1, &one, None).unwrap();
        let back = read(&p1).unwrap();
        assert!(matches!(back.features[0].geometry, Some(Geometry::Polygon(_))));
        assert_eq!(back.features[0].geometry.as_ref().unwrap().outer_ring().unwrap().len(), 4);
    }

    #[test]
    fn polylines_multipart_and_z_variants_read() {
        let dir = temp("shp-lines");
        let mut layer = Layer::new("links", vec![Field::new("N", FieldKind::Text)]);
        layer.features.push(Feature::new(
            Some(Geometry::LineString(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)])),
            vec![FieldValue::Text("C1".into())],
        ));
        layer.features.push(Feature::new(
            Some(Geometry::MultiLineString(vec![
                vec![(0.0, 0.0), (1.0, 1.0)],
                vec![(5.0, 5.0), (9.0, 8.0)],
            ])),
            vec![FieldValue::Text("C2".into())],
        ));
        let p = dir.join("links.shp");
        write(&p, &layer, None).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.features[0].geometry.as_ref().unwrap().length(), 15.0);
        assert!(matches!(back.features[1].geometry, Some(Geometry::MultiLineString(_))));
        assert_eq!(back.features[1].geometry.as_ref().unwrap().main_path().unwrap()[0], (5.0, 5.0));

        // Patch the file into PolyLineZ (13) with Z and M arrays appended
        // to the first record: the reader ignores them.
        let mut bytes = std::fs::read(&p).unwrap();
        bytes[32..36].copy_from_slice(&13i32.to_le_bytes());
        // First record: header at 100, content at 108. Its content length
        // stays as written; a Z reader only needs the X/Y block, which is
        // identical in the Z variant up to the Z range.
        bytes[108..112].copy_from_slice(&13i32.to_le_bytes());
        let (code, shapes) = parse_shp(&bytes).unwrap();
        assert_eq!(code, 13);
        assert_eq!(shapes[0].as_ref().unwrap().length(), 15.0);
        // PointZ record: x y z m.
        let mut content = Vec::new();
        put_i32le(&mut content, 11);
        for v in [3.0, 4.0, 99.0, 0.0] {
            put_f64(&mut content, v);
        }
        assert_eq!(parse_shape(&content).unwrap(), Some(Geometry::Point(3.0, 4.0)));
        let mut mp = Vec::new();
        put_i32le(&mut mp, 31);
        assert!(parse_shape(&mp).is_err(), "MultiPatch is refused");
    }

    #[test]
    fn a_non_shapefile_is_refused_with_a_reason() {
        let e = parse_shp(&[0u8; 120]).unwrap_err();
        assert!(e.to_string().contains("9994"), "{e}");
        assert!(parse_shp(&[0u8; 10]).is_err());
        assert_eq!(sibling(Path::new("A.SHP"), "dbf"), PathBuf::from("A.DBF"));
        assert_eq!(sibling(Path::new("a.shp"), "dbf"), PathBuf::from("a.dbf"));
        assert!(point_in_ring((5.0, 5.0), &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]));
        assert!(!point_in_ring((15.0, 5.0), &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]));
    }
}
