// SPDX-License-Identifier: GPL-3.0-or-later

//! GeoJSON (RFC 7946) in and out: `FeatureCollection`, `Feature` and bare
//! geometries; `Point`, `MultiPoint`, `LineString`, `MultiLineString`,
//! `Polygon`, `MultiPolygon` (a `GeometryCollection` is flattened to its
//! first member). Extra positions (Z) are dropped.
//!
//! RFC 7946 fixes the coordinate system to WGS 84 longitude/latitude and
//! removed the 2008 `crs` member, but files with a `crs` name such as
//! `urn:ogc:def:crs:EPSG::2264` are common; when one is present it names
//! the layer's CRS. Without one, coordinates inside ±180/±90 are taken as
//! WGS 84 and anything else is left with no CRS for the dialog to ask.

use serde_json::{json, Map, Value};

use crate::gis::crs::Crs;
use crate::gis::vector::{Feature, Field, FieldKind, FieldValue, Geometry, Layer};
use crate::{Error, Result};

fn position(v: &Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

fn positions(v: &Value) -> Vec<(f64, f64)> {
    v.as_array()
        .map(|a| a.iter().filter_map(position).collect())
        .unwrap_or_default()
}

fn rings(v: &Value) -> Vec<Vec<(f64, f64)>> {
    v.as_array()
        .map(|a| a.iter().map(positions).filter(|r| !r.is_empty()).collect())
        .unwrap_or_default()
}

/// Parse a GeoJSON geometry object.
pub fn geometry(v: &Value) -> Result<Option<Geometry>> {
    if v.is_null() {
        return Ok(None);
    }
    let ty = v["type"].as_str().unwrap_or("");
    let c = &v["coordinates"];
    Ok(match ty {
        "Point" => position(c).map(|(x, y)| Geometry::Point(x, y)),
        "MultiPoint" => Some(Geometry::MultiPoint(positions(c))),
        "LineString" => Some(Geometry::LineString(positions(c))),
        "MultiLineString" => Some(Geometry::MultiLineString(rings(c))),
        "Polygon" => Some(Geometry::Polygon(rings(c))),
        "MultiPolygon" => Some(Geometry::MultiPolygon(
            c.as_array()
                .map(|a| a.iter().map(rings).collect())
                .unwrap_or_default(),
        )),
        "GeometryCollection" => match v["geometries"].as_array().and_then(|a| a.first()) {
            Some(g) => geometry(g)?,
            None => None,
        },
        "" => return Err(Error::Format("GeoJSON geometry has no type".into())),
        other => return Err(Error::Format(format!("GeoJSON geometry type {other:?} is not supported"))),
    })
}

fn field_value(v: &Value) -> FieldValue {
    match v {
        Value::Null => FieldValue::Null,
        Value::Bool(b) => FieldValue::Bool(*b),
        Value::Number(n) => n.as_f64().map_or(FieldValue::Null, FieldValue::Number),
        Value::String(s) => FieldValue::Text(s.clone()),
        other => FieldValue::Text(other.to_string()),
    }
}

/// The EPSG code named by a 2008-style `crs` member, if any.
pub fn crs_epsg(v: &Value) -> Option<u32> {
    let name = v["properties"]["name"].as_str()?;
    let upper = name.to_ascii_uppercase();
    if upper.contains("CRS84") {
        return Some(4326);
    }
    let tail = upper.rsplit(':').next()?;
    tail.parse().ok()
}

/// Parse GeoJSON text into a layer named `name`.
pub fn parse(text: &str, name: &str) -> Result<Layer> {
    let v: Value = serde_json::from_str(text).map_err(|e| Error::Format(format!("not JSON: {e}")))?;
    let ty = v["type"].as_str().unwrap_or("");
    let features: Vec<&Value> = match ty {
        "FeatureCollection" => v["features"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        "Feature" => vec![&v],
        "" => return Err(Error::Format("GeoJSON object has no type".into())),
        _ => Vec::new(),
    };
    let mut fields: Vec<Field> = Vec::new();
    let mut rows: Vec<(Option<Geometry>, Map<String, Value>)> = Vec::new();
    if features.is_empty() && ty != "FeatureCollection" && ty != "Feature" {
        rows.push((geometry(&v)?, Map::new()));
    }
    for f in features {
        let g = geometry(&f["geometry"])?;
        let mut props = f["properties"].as_object().cloned().unwrap_or_default();
        if let Some(id) = f.get("id") {
            if !props.contains_key("id") && !id.is_null() {
                props.insert("id".into(), id.clone());
            }
        }
        for (k, val) in &props {
            if !fields.iter().any(|x| x.name == *k) {
                let kind = match val {
                    Value::Bool(_) => FieldKind::Bool,
                    Value::Number(n) if n.is_i64() || n.is_u64() => FieldKind::Integer,
                    Value::Number(_) => FieldKind::Number,
                    _ => FieldKind::Text,
                };
                fields.push(Field::new(k, kind));
            }
        }
        rows.push((g, props));
    }
    let mut layer = Layer::new(name, fields);
    for (g, props) in rows {
        let values = layer
            .fields
            .iter()
            .map(|f| props.get(&f.name).map_or(FieldValue::Null, field_value))
            .collect();
        layer.features.push(Feature::new(g, values));
    }
    if let Some(code) = v.get("crs").and_then(crs_epsg) {
        layer.crs_text = Some(format!("EPSG:{code}"));
        layer.crs = Crs::from_epsg(code).ok();
    } else if let Some(name) = v["crs"]["properties"]["name"].as_str() {
        layer.crs_text = Some(name.to_string());
    } else if let Some((x0, y0, x1, y1)) = layer.bounds() {
        if x0 >= -180.0 && x1 <= 180.0 && y0 >= -90.0 && y1 <= 90.0 {
            layer.crs = Some(Crs::wgs84());
            layer.crs_text = Some("WGS 84 longitude/latitude (RFC 7946 default)".into());
        }
    }
    Ok(layer)
}

pub fn read(path: &std::path::Path) -> Result<Layer> {
    let text = std::fs::read_to_string(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("layer");
    parse(&text, name)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

fn pos(p: (f64, f64)) -> Value {
    json!([p.0, p.1])
}

fn ring_value(r: &[(f64, f64)]) -> Value {
    let closed = crate::gis::vector::close(r.to_vec());
    Value::Array(closed.into_iter().map(pos).collect())
}

/// A geometry as a GeoJSON object.
pub fn geometry_value(g: &Geometry) -> Value {
    match g {
        Geometry::Point(x, y) => json!({"type": "Point", "coordinates": pos((*x, *y))}),
        Geometry::MultiPoint(p) => json!({"type": "MultiPoint", "coordinates": p.iter().map(|q| pos(*q)).collect::<Vec<_>>()}),
        Geometry::LineString(p) => json!({"type": "LineString", "coordinates": p.iter().map(|q| pos(*q)).collect::<Vec<_>>()}),
        Geometry::MultiLineString(parts) => json!({"type": "MultiLineString", "coordinates": parts.iter().map(|p| Value::Array(p.iter().map(|q| pos(*q)).collect())).collect::<Vec<_>>()}),
        Geometry::Polygon(rings) => json!({"type": "Polygon", "coordinates": rings.iter().map(|r| ring_value(r)).collect::<Vec<_>>()}),
        Geometry::MultiPolygon(polys) => json!({
            "type": "MultiPolygon",
            "coordinates": polys.iter().map(|rings| Value::Array(rings.iter().map(|r| ring_value(r)).collect())).collect::<Vec<_>>()
        }),
    }
}

fn prop_value(v: &FieldValue) -> Value {
    match v {
        FieldValue::Null => Value::Null,
        FieldValue::Text(s) | FieldValue::Date(s) => Value::String(s.clone()),
        FieldValue::Number(x) if x.is_finite() => json!(x),
        FieldValue::Number(_) => Value::Null,
        FieldValue::Bool(b) => Value::Bool(*b),
    }
}

/// One feature object. `id_field` names the property also written as the
/// feature `id`.
pub fn feature_value(layer: &Layer, f: &Feature, id_field: Option<&str>) -> Value {
    let mut props = Map::new();
    for (i, field) in layer.fields.iter().enumerate() {
        props.insert(
            field.name.clone(),
            f.values.get(i).map_or(Value::Null, prop_value),
        );
    }
    let mut out = Map::new();
    out.insert("type".into(), json!("Feature"));
    if let Some(id) = id_field.and_then(|n| layer.field_index(n)).and_then(|i| f.values.get(i)) {
        if !id.is_null() {
            out.insert("id".into(), prop_value(id));
        }
    }
    out.insert(
        "geometry".into(),
        f.geometry.as_ref().map_or(Value::Null, geometry_value),
    );
    out.insert("properties".into(), Value::Object(props));
    Value::Object(out)
}

/// A `FeatureCollection` for the layer. `epsg` names a non-WGS-84 system
/// the coordinates are in (written as the 2008 `crs` member plus a
/// `note`); `None` means the coordinates are WGS 84 as RFC 7946 requires,
/// and no `crs` is written.
pub fn collection_value(layer: &Layer, epsg: Option<u32>, id_field: Option<&str>) -> Value {
    let features: Vec<Value> = layer
        .features
        .iter()
        .map(|f| feature_value(layer, f, id_field))
        .collect();
    let mut fc = Map::new();
    fc.insert("type".into(), json!("FeatureCollection"));
    fc.insert("name".into(), json!(layer.name));
    if let Some(code) = epsg {
        fc.insert(
            "crs".into(),
            json!({"type": "name", "properties": {"name": format!("urn:ogc:def:crs:EPSG::{code}")}}),
        );
        fc.insert(
            "note".into(),
            json!(format!("Coordinates are EPSG:{code}, not the WGS 84 longitude/latitude RFC 7946 requires; reproject to WGS 84 on export for a conforming file.")),
        );
    }
    fc.insert("features".into(), Value::Array(features));
    Value::Object(fc)
}

pub fn encode(layer: &Layer, epsg: Option<u32>, id_field: Option<&str>) -> String {
    serde_json::to_string_pretty(&collection_value(layer, epsg, id_field)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_collection_reads_every_geometry_type_and_its_properties() {
        let text = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","id":"a","geometry":{"type":"Point","coordinates":[-80.8,35.2,120]},"properties":{"name":"J1","invert":100.5,"n":3,"ok":true,"tags":["x"]}},
          {"type":"Feature","geometry":{"type":"LineString","coordinates":[[-80.8,35.2],[-80.7,35.3]]},"properties":{"name":"C1","extra":null}},
          {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[-80.8,35.2],[-80.7,35.2],[-80.7,35.3],[-80.8,35.2]]]},"properties":{"name":"S1"}},
          {"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]],[[[2,2],[3,2],[3,3],[2,2]]]]},"properties":{}},
          {"type":"Feature","geometry":{"type":"GeometryCollection","geometries":[{"type":"MultiPoint","coordinates":[[1,2],[3,4]]}]},"properties":{}},
          {"type":"Feature","geometry":null,"properties":{"name":"none"}}
        ]}"#;
        let l = parse(text, "t").unwrap();
        assert_eq!(l.features.len(), 6);
        // serde_json keeps object keys sorted, so a feature's properties
        // come out alphabetically; later features append new names.
        let names: Vec<&str> = l.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["id", "invert", "n", "name", "ok", "tags", "extra"]);
        assert_eq!(l.fields[2].kind, FieldKind::Integer);
        assert_eq!(l.fields[1].kind, FieldKind::Number);
        assert_eq!(l.features[0].geometry, Some(Geometry::Point(-80.8, 35.2)));
        assert_eq!(l.value(0, "id"), Some(&FieldValue::Text("a".into())));
        assert_eq!(l.value(0, "tags"), Some(&FieldValue::Text("[\"x\"]".into())));
        assert_eq!(l.value(1, "extra"), Some(&FieldValue::Null));
        assert!(matches!(l.features[3].geometry, Some(Geometry::MultiPolygon(_))));
        assert!(matches!(l.features[4].geometry, Some(Geometry::MultiPoint(_))));
        assert_eq!(l.features[5].geometry, None);
        assert_eq!(l.crs.as_ref().and_then(|c| c.epsg), Some(4326), "RFC 7946 default");
    }

    #[test]
    fn crs_member_names_the_system_and_projected_coordinates_without_one_stay_unknown() {
        let text = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::32617"}},"features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[514277.7,3898239.3]},"properties":{}}]}"#;
        let l = parse(text, "t").unwrap();
        assert_eq!(l.crs.as_ref().and_then(|c| c.epsg), Some(32617));
        let text = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[514277.7,3898239.3]},"properties":{}}]}"#;
        let l = parse(text, "t").unwrap();
        assert!(l.crs.is_none());
        let bare = r#"{"type":"Point","coordinates":[1,2]}"#;
        let l = parse(bare, "p").unwrap();
        assert_eq!(l.features.len(), 1);
        assert!(parse("{\"type\":\"Feature\",\"geometry\":{\"type\":\"Circle\"}}", "x").is_err());
        assert!(parse("not json", "x").is_err());
        assert_eq!(crs_epsg(&json!({"properties": {"name": "EPSG:2264"}})), Some(2264));
        assert_eq!(crs_epsg(&json!({"properties": {"name": "urn:ogc:def:crs:OGC:1.3:CRS84"}})), Some(4326));
    }

    #[test]
    fn encode_writes_a_collection_the_reader_accepts_and_closes_rings() {
        let mut l = Layer::new("subs", vec![Field::new("Name", FieldKind::Text), Field::new("Area", FieldKind::Number)]);
        l.features.push(Feature::new(
            Some(Geometry::Polygon(vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]])),
            vec![FieldValue::Text("S1".into()), FieldValue::Number(1.5)],
        ));
        let text = encode(&l, Some(2264), Some("Name"));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["crs"]["properties"]["name"], "urn:ogc:def:crs:EPSG::2264");
        assert_eq!(v["features"][0]["id"], "S1");
        let ring = v["features"][0]["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.len(), 4, "closed");
        let back = parse(&text, "back").unwrap();
        assert_eq!(back.crs.as_ref().and_then(|c| c.epsg), Some(2264));
        assert_eq!(back.value(0, "Area"), Some(&FieldValue::Number(1.5)));
        let wgs = encode(&l, None, None);
        assert!(!wgs.contains("\"crs\""));
    }
}
