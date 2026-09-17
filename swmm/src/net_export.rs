// SPDX-License-Identifier: GPL-3.0-or-later

//! Network exports from a model: node and link tables as CSV, and the map
//! as GeoJSON. Coordinates are the model's own map units; no CRS is
//! assumed or written, and the GeoJSON says so in a top-level `note`.

use serde_json::{json, Value};

use crate::doc::build::{LinkType, NodeType};
use crate::doc::InpDoc;
use crate::rpt::csv_line;

fn num(s: Option<&str>) -> Option<f64> {
    s.and_then(|s| s.trim().parse::<f64>().ok())
}

fn opt_str(v: Option<f64>) -> String {
    v.map(|x| format!("{x}")).unwrap_or_default()
}

/// `Name,Kind,X,Y,Invert,MaxDepth,Type` — one row per node in file order.
/// `Type` is the outfall type, storage shape or divider type where the
/// section has one.
pub fn nodes_csv(doc: &InpDoc) -> String {
    let mut out = csv_line(&["Name", "Kind", "X", "Y", "Invert", "MaxDepth", "Type"]);
    for kind in NodeType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let cols = doc.columns(section, row);
            let Some(name) = row.value(0) else { continue };
            let (x, y) = doc.coordinates(name).map_or((None, None), |(x, y)| (Some(x), Some(y)));
            let invert = num(row.get(cols, "Elevation"));
            let max_depth = num(row.get(cols, "MaxDepth"));
            let ty = match kind {
                NodeType::Outfall | NodeType::Divider => row.get(cols, "Type"),
                NodeType::Storage => row.get(cols, "Shape"),
                NodeType::Junction => None,
            }
            .unwrap_or("")
            .to_string();
            out.push_str(&csv_line(&[
                name.to_string(),
                kind.label().to_string(),
                opt_str(x),
                opt_str(y),
                opt_str(invert),
                opt_str(max_depth),
                ty,
            ]));
        }
    }
    out
}

/// `Name,Kind,From,To,Length,Roughness,InOffset,OutOffset,Shape,Geom1,Geom2,Vertices`
/// — one row per link in file order; `Vertices` is the count of interior
/// bend points.
pub fn links_csv(doc: &InpDoc) -> String {
    let mut out = csv_line(&[
        "Name", "Kind", "From", "To", "Length", "Roughness", "InOffset", "OutOffset", "Shape",
        "Geom1", "Geom2", "Vertices",
    ]);
    for kind in LinkType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let cols = doc.columns(section, row);
            let (Some(name), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2))
            else {
                continue;
            };
            let (length, n, in_off, out_off) = if kind == LinkType::Conduit {
                (
                    num(row.get(cols, "Length")),
                    num(row.get(cols, "Roughness")),
                    num(row.get(cols, "InOffset")),
                    num(row.get(cols, "OutOffset")),
                )
            } else {
                (None, None, None, None)
            };
            let (shape, g1, g2) = doc
                .find("XSECTIONS", name)
                .map(|(_, xr)| {
                    let xc = doc.columns("XSECTIONS", xr);
                    (
                        xr.get(xc, "Shape").unwrap_or("").to_string(),
                        num(xr.get(xc, "Geom1")),
                        num(xr.get(xc, "Geom2")),
                    )
                })
                .unwrap_or_default();
            out.push_str(&csv_line(&[
                name.to_string(),
                kind.label().to_string(),
                from.to_string(),
                to.to_string(),
                opt_str(length),
                opt_str(n),
                opt_str(in_off),
                opt_str(out_off),
                shape,
                opt_str(g1),
                opt_str(g2),
                doc.vertices(name).len().to_string(),
            ]));
        }
    }
    out
}

fn point(x: f64, y: f64) -> Value {
    json!([x, y])
}

/// A GeoJSON `FeatureCollection`: nodes as `Point`s, links as
/// `LineString`s through their vertices, subcatchments with polygons as
/// `Polygon`s (closed), rain gages with symbols as `Point`s. Properties
/// carry the defining row by column name. Pretty-printed.
pub fn geojson(doc: &InpDoc) -> String {
    let mut features: Vec<Value> = Vec::new();
    let props = |section: &str, row: &crate::doc::Row| -> serde_json::Map<String, Value> {
        let cols = doc.columns(section, row);
        let mut m = serde_json::Map::new();
        for (i, f) in row.fields.iter().enumerate() {
            let key = cols
                .get(i)
                .map(|c| c.to_string())
                .unwrap_or_else(|| format!("field{i}"));
            let v = crate::doc::unquote(f);
            m.insert(
                key,
                v.parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite())
                    .map_or_else(|| Value::String(v.to_string()), |x| json!(x)),
            );
        }
        m
    };
    let node_xy = |name: &str| doc.coordinates(name);

    for kind in NodeType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let Some(name) = row.value(0) else { continue };
            let Some((x, y)) = node_xy(name) else { continue };
            let mut p = props(section, row);
            p.insert("kind".into(), json!(kind.label()));
            p.insert("section".into(), json!(section));
            features.push(json!({
                "type": "Feature",
                "id": name,
                "geometry": {"type": "Point", "coordinates": point(x, y)},
                "properties": Value::Object(p),
            }));
        }
    }
    for kind in LinkType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let (Some(name), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2))
            else {
                continue;
            };
            let (Some(a), Some(b)) = (node_xy(from), node_xy(to)) else {
                continue;
            };
            let mut line = vec![point(a.0, a.1)];
            line.extend(doc.vertices(name).into_iter().map(|(x, y)| point(x, y)));
            line.push(point(b.0, b.1));
            let mut p = props(section, row);
            p.insert("kind".into(), json!(kind.label()));
            p.insert("section".into(), json!(section));
            if let Some((_, xr)) = doc.find("XSECTIONS", name) {
                let xc = doc.columns("XSECTIONS", xr);
                if let Some(shape) = xr.get(xc, "Shape") {
                    p.insert("Shape".into(), json!(shape));
                }
                if let Some(g1) = num(xr.get(xc, "Geom1")) {
                    p.insert("Geom1".into(), json!(g1));
                }
                if let Some(g2) = num(xr.get(xc, "Geom2")) {
                    p.insert("Geom2".into(), json!(g2));
                }
            }
            features.push(json!({
                "type": "Feature",
                "id": name,
                "geometry": {"type": "LineString", "coordinates": line},
                "properties": Value::Object(p),
            }));
        }
    }
    for (_, row) in doc.rows("SUBCATCHMENTS") {
        let Some(name) = row.value(0) else { continue };
        let poly = doc.polygon(name);
        if poly.len() < 3 {
            continue;
        }
        let mut ring: Vec<Value> = poly.iter().map(|(x, y)| point(*x, *y)).collect();
        if poly.first() != poly.last() {
            ring.push(point(poly[0].0, poly[0].1));
        }
        let mut p = props("SUBCATCHMENTS", row);
        p.insert("kind".into(), json!("Subcatchment"));
        p.insert("section".into(), json!("SUBCATCHMENTS"));
        features.push(json!({
            "type": "Feature",
            "id": name,
            "geometry": {"type": "Polygon", "coordinates": [ring]},
            "properties": Value::Object(p),
        }));
    }
    for (_, row) in doc.rows("RAINGAGES") {
        let Some(name) = row.value(0) else { continue };
        let Some((x, y)) = doc.symbol(name) else { continue };
        let mut p = props("RAINGAGES", row);
        p.insert("kind".into(), json!("Rain Gage"));
        p.insert("section".into(), json!("RAINGAGES"));
        features.push(json!({
            "type": "Feature",
            "id": name,
            "geometry": {"type": "Point", "coordinates": point(x, y)},
            "properties": Value::Object(p),
        }));
    }
    let map_units = doc.key_value("MAP", "Units").unwrap_or_else(|| "None".into());
    let fc = json!({
        "type": "FeatureCollection",
        "note": format!(
            "Coordinates are the SWMM model's map units ([MAP] Units {map_units}); no coordinate reference system is assumed or declared."
        ),
        "title": doc.title(),
        "features": features,
    });
    serde_json::to_string_pretty(&fc).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pond() -> InpDoc {
        InpDoc::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/epa-samples/Detention_Pond_Model.inp"),
        )
        .unwrap()
    }

    #[test]
    fn csv_tables_list_every_node_and_link() {
        let doc = pond();
        let nodes = nodes_csv(&doc);
        assert_eq!(nodes.lines().count(), 1 + 14);
        assert!(nodes.lines().any(|l| l.starts_with("SU1,Storage Unit,") && l.ends_with(",PYRAMIDAL")));
        assert!(nodes.lines().any(|l| l.starts_with("O2,Outfall,") && l.ends_with(",FREE")));
        let links = links_csv(&doc);
        assert_eq!(links.lines().count(), 1 + 14);
        let c11 = links.lines().find(|l| l.starts_with("C11,")).unwrap();
        assert_eq!(c11, "C11,Conduit,J11,SU1,150,0.016,0,1,CIRCULAR,4.75,0,0");
        assert!(links.lines().any(|l| l.starts_with("W1,Weir,SU1,J_out,,,,,RECT_OPEN,2,5,")));
    }

    #[test]
    fn geojson_is_a_feature_collection_without_a_crs() {
        let doc = pond();
        let text = geojson(&doc);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["type"], "FeatureCollection");
        assert!(v["note"].as_str().unwrap().contains("no coordinate reference system"));
        assert!(v.get("crs").is_none());
        let features = v["features"].as_array().unwrap();
        // 14 nodes + 14 links + 8 subcatchment polygons + 1 gage.
        assert_eq!(features.len(), 14 + 14 + 8 + 1);
        let c11 = features.iter().find(|f| f["id"] == "C11").unwrap();
        assert_eq!(c11["geometry"]["type"], "LineString");
        assert_eq!(c11["properties"]["Shape"], "CIRCULAR");
        assert_eq!(c11["properties"]["Geom1"], 4.75);
        assert_eq!(c11["properties"]["Length"], 150.0);
        let s1 = features.iter().find(|f| f["id"] == "S1").unwrap();
        assert_eq!(s1["geometry"]["type"], "Polygon");
        let ring = s1["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.first(), ring.last(), "ring is closed");
        assert_eq!(s1["properties"]["Area"], 4.55);
        let j1 = features.iter().find(|f| f["id"] == "J1").unwrap();
        assert_eq!(j1["geometry"]["type"], "Point");
        assert_eq!(j1["properties"]["kind"], "Junction");
    }
}
