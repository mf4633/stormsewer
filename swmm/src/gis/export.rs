// SPDX-License-Identifier: GPL-3.0-or-later

//! The model as GIS layers: nodes, links, subcatchments and rain gages
//! with their defining columns as attributes, plus the peaks of a loaded
//! run as extra numeric fields, written as a shapefile set (with `.prj`
//! from the model CRS) or as GeoJSON (optionally reprojected to WGS 84,
//! as RFC 7946 requires).

use std::path::{Path, PathBuf};

use crate::doc::build::{LinkType, NodeType};
use crate::doc::{unquote, InpDoc, Row};
use crate::gis::crs::Crs;
use crate::gis::vector::{close, Feature, Field, FieldKind, FieldValue, Geometry, Layer};
use crate::gis::{geojson, import, shapefile};
use crate::out::{LinkPeak, NodePeak};
use crate::Result;

/// Run peaks to attach, by object name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peaks<'a> {
    pub nodes: &'a [NodePeak],
    pub links: &'a [LinkPeak],
}

fn value_of(s: &str) -> FieldValue {
    let v = unquote(s);
    match v.parse::<f64>() {
        Ok(x) if x.is_finite() => FieldValue::Number(x),
        _ => FieldValue::Text(v.to_string()),
    }
}

fn kind_of(name: &str, values: &[FieldValue]) -> FieldKind {
    if name == "Name" || name == "Kind" || name == "Section" {
        return FieldKind::Text;
    }
    let mut numeric = true;
    let mut any = false;
    for v in values {
        match v {
            FieldValue::Number(_) => any = true,
            FieldValue::Null => {}
            _ => numeric = false,
        }
    }
    if numeric && any {
        FieldKind::Number
    } else {
        FieldKind::Text
    }
}

/// Collect rows of several sections into one attribute table whose
/// columns are the union of the rows' column names (first-seen order).
struct TableBuilder {
    columns: Vec<String>,
    rows: Vec<Vec<(String, FieldValue)>>,
    geoms: Vec<Option<Geometry>>,
}

impl TableBuilder {
    fn new() -> Self {
        Self {
            columns: vec!["Name".into(), "Kind".into(), "Section".into()],
            rows: Vec::new(),
            geoms: Vec::new(),
        }
    }

    fn push(&mut self, doc: &InpDoc, section: &str, kind: &str, row: &Row, geom: Option<Geometry>, extra: Vec<(String, FieldValue)>) {
        let cols = doc.columns(section, row);
        let mut vals: Vec<(String, FieldValue)> = vec![
            ("Name".into(), FieldValue::Text(row.value(0).unwrap_or("").to_string())),
            ("Kind".into(), FieldValue::Text(kind.to_string())),
            ("Section".into(), FieldValue::Text(section.to_string())),
        ];
        for (i, f) in row.fields.iter().enumerate().skip(1) {
            let name = cols.get(i).map(|c| c.to_string()).unwrap_or_else(|| format!("Field{i}"));
            vals.push((name, value_of(f)));
        }
        vals.extend(extra);
        for (name, _) in &vals {
            if !self.columns.contains(name) {
                self.columns.push(name.clone());
            }
        }
        self.rows.push(vals);
        self.geoms.push(geom);
    }

    fn finish(self, name: &str) -> Layer {
        let table: Vec<Vec<FieldValue>> = self
            .rows
            .iter()
            .map(|r| {
                self.columns
                    .iter()
                    .map(|c| r.iter().find(|(n, _)| n == c).map_or(FieldValue::Null, |(_, v)| v.clone()))
                    .collect()
            })
            .collect();
        let fields: Vec<Field> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let col: Vec<FieldValue> = table.iter().map(|r| r[i].clone()).collect();
                Field::new(c, kind_of(c, &col))
            })
            .collect();
        let mut layer = Layer::new(name, fields);
        for (values, geom) in table.into_iter().zip(self.geoms) {
            layer.features.push(Feature::new(geom, values));
        }
        layer
    }
}

fn num(v: f64) -> FieldValue {
    FieldValue::Number(v)
}

/// Nodes as points.
pub fn nodes_layer(doc: &InpDoc, peaks: &Peaks) -> Layer {
    let mut t = TableBuilder::new();
    for kind in NodeType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let Some(name) = row.value(0) else { continue };
            let geom = doc.coordinates(name).map(|(x, y)| Geometry::Point(x, y));
            let mut extra = Vec::new();
            if let Some(p) = peaks.nodes.iter().find(|p| p.id.eq_ignore_ascii_case(name)) {
                extra.push(("PeakDepth".into(), num(p.max_depth)));
                extra.push(("PeakInflow".into(), num(p.max_total_inflow)));
                extra.push(("PeakFlood".into(), num(p.max_flooding)));
            }
            t.push(doc, section, kind.label(), row, geom, extra);
        }
    }
    t.finish("nodes")
}

/// Links as lines through their vertices.
pub fn links_layer(doc: &InpDoc, peaks: &Peaks) -> Layer {
    let mut t = TableBuilder::new();
    for kind in LinkType::ALL {
        let section = kind.section();
        for (_, row) in doc.rows(section) {
            let (Some(name), Some(from), Some(to)) = (row.value(0), row.value(1), row.value(2)) else {
                continue;
            };
            let geom = match (doc.coordinates(from), doc.coordinates(to)) {
                (Some(a), Some(b)) => {
                    let mut line = vec![a];
                    line.extend(doc.vertices(name));
                    line.push(b);
                    Some(Geometry::LineString(line))
                }
                _ => None,
            };
            let mut extra = Vec::new();
            if let Some((_, xr)) = doc.find("XSECTIONS", name) {
                let xc = doc.columns("XSECTIONS", xr);
                for col in ["Shape", "Geom1", "Geom2", "Geom3", "Geom4", "Barrels"] {
                    if let Some(v) = xr.get(xc, col) {
                        extra.push((col.into(), value_of(v)));
                    }
                }
            }
            if let Some(p) = peaks.links.iter().find(|p| p.id.eq_ignore_ascii_case(name)) {
                extra.push(("PeakFlow".into(), num(p.max_flow)));
                extra.push(("PeakVel".into(), num(p.max_velocity)));
                extra.push(("PeakCap".into(), num(p.max_capacity)));
            }
            t.push(doc, section, kind.label(), row, geom, extra);
        }
    }
    t.finish("links")
}

/// Subcatchments as polygons (those without an outline have no geometry).
pub fn subcatchments_layer(doc: &InpDoc) -> Layer {
    let mut t = TableBuilder::new();
    for (_, row) in doc.rows("SUBCATCHMENTS") {
        let Some(name) = row.value(0) else { continue };
        let poly = doc.polygon(name);
        let geom = (poly.len() >= 3).then(|| Geometry::Polygon(vec![close(poly)]));
        let mut extra = Vec::new();
        if let Some((_, sr)) = doc.find("SUBAREAS", name) {
            let sc = doc.columns("SUBAREAS", sr);
            for col in ["NImperv", "NPerv", "SImperv", "SPerv", "PctZero", "RouteTo"] {
                if let Some(v) = sr.get(sc, col) {
                    extra.push((col.into(), value_of(v)));
                }
            }
        }
        t.push(doc, "SUBCATCHMENTS", "Subcatchment", row, geom, extra);
    }
    t.finish("subcatchments")
}

/// Rain gages as points (those without a symbol have no geometry).
pub fn gages_layer(doc: &InpDoc) -> Layer {
    let mut t = TableBuilder::new();
    for (_, row) in doc.rows("RAINGAGES") {
        let Some(name) = row.value(0) else { continue };
        let geom = doc.symbol(name).map(|(x, y)| Geometry::Point(x, y));
        t.push(doc, "RAINGAGES", "Rain Gage", row, geom, Vec::new());
    }
    t.finish("gages")
}

/// All four layers, in the order they are written.
pub fn layers(doc: &InpDoc, peaks: &Peaks) -> Vec<Layer> {
    vec![
        nodes_layer(doc, peaks),
        links_layer(doc, peaks),
        subcatchments_layer(doc),
        gages_layer(doc),
    ]
}

/// Write `<dir>/<stem>_nodes.shp` and siblings for the non-empty layers,
/// with a `.prj` when the model has a CRS. Returns the `.shp` paths.
pub fn write_shapefiles(dir: &Path, stem: &str, doc: &InpDoc, peaks: &Peaks, crs: Option<&Crs>) -> Result<Vec<PathBuf>> {
    let prj = crs.map(Crs::to_wkt);
    let mut out = Vec::new();
    for layer in layers(doc, peaks) {
        if layer.features.is_empty() {
            continue;
        }
        let path = dir.join(format!("{stem}_{}.shp", layer.name));
        shapefile::write(&path, &layer, prj.as_deref())?;
        out.push(path);
    }
    Ok(out)
}

/// The layers as GeoJSON text: one `FeatureCollection` per layer, or all
/// features in one collection when `combined`. With `to_wgs84` and a
/// model CRS the coordinates are reprojected to longitude/latitude and no
/// `crs` member is written; otherwise the model CRS's EPSG code (when
/// known) is declared and the file notes it is not RFC 7946 WGS 84.
pub fn geojson_texts(doc: &InpDoc, peaks: &Peaks, crs: Option<&Crs>, to_wgs84: bool, combined: bool) -> Result<Vec<(String, String)>> {
    let wgs = Crs::wgs84();
    let mut ls = layers(doc, peaks);
    let mut epsg = crs.and_then(|c| c.epsg);
    if let (true, Some(c)) = (to_wgs84, crs) {
        if !c.same_as(&wgs) {
            for l in &mut ls {
                *l = import::reproject(l, c, &wgs)?;
            }
        }
        epsg = None;
    } else if crs.is_some_and(|c| c.same_as(&wgs)) {
        epsg = None;
    }
    if combined {
        let mut all = Layer::new("model", vec![Field::new("Name", FieldKind::Text), Field::new("Kind", FieldKind::Text), Field::new("Section", FieldKind::Text)]);
        // A combined file keeps each feature's own attributes as properties
        // by building the union of the four field lists.
        for l in &ls {
            for f in &l.fields {
                if all.field_index(&f.name).is_none() {
                    all.fields.push(f.clone());
                }
            }
        }
        for l in &ls {
            for feat in &l.features {
                let values = all
                    .fields
                    .iter()
                    .map(|f| l.field_index(&f.name).and_then(|i| feat.values.get(i)).cloned().unwrap_or(FieldValue::Null))
                    .collect();
                all.features.push(Feature::new(feat.geometry.clone(), values));
            }
        }
        return Ok(vec![("model".into(), geojson::encode(&all, epsg, Some("Name")))]);
    }
    Ok(ls
        .iter()
        .filter(|l| !l.features.is_empty())
        .map(|l| (l.name.clone(), geojson::encode(l, epsg, Some("Name"))))
        .collect())
}

/// Write GeoJSON beside `path`: `path` itself when combined, else
/// `<stem>_<layer>.geojson` per layer. Returns the paths written.
pub fn write_geojson(path: &Path, doc: &InpDoc, peaks: &Peaks, crs: Option<&Crs>, to_wgs84: bool, combined: bool) -> Result<Vec<PathBuf>> {
    let texts = geojson_texts(doc, peaks, crs, to_wgs84, combined)?;
    let mut out = Vec::new();
    if combined {
        std::fs::write(path, &texts[0].1)?;
        out.push(path.to_path_buf());
        return Ok(out);
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for (name, text) in texts {
        let p = dir.join(format!("{stem}_{name}.geojson"));
        std::fs::write(&p, text)?;
        out.push(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pond() -> InpDoc {
        InpDoc::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epa-samples/Detention_Pond_Model.inp"),
        )
        .unwrap()
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("stormsewer-gis-tests").join(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn layers_carry_columns_and_peaks() {
        let doc = pond();
        let np = vec![NodePeak { id: "J1".into(), max_depth: 1.5, depth_at_s: 0.0, max_total_inflow: 3.0, max_flooding: 0.0 }];
        let lp = vec![LinkPeak { id: "C11".into(), max_flow: 12.5, flow_at_s: 0.0, max_velocity: 4.0, max_capacity: 0.8 }];
        let peaks = Peaks { nodes: &np, links: &lp };
        let n = nodes_layer(&doc, &peaks);
        assert_eq!(n.features.len(), 14);
        let j1 = n.features.iter().position(|f| f.values[0] == FieldValue::Text("J1".into())).unwrap();
        assert_eq!(n.value(j1, "PeakDepth"), Some(&FieldValue::Number(1.5)));
        assert_eq!(n.value(j1, "Kind"), Some(&FieldValue::Text("Junction".into())));
        assert!(n.value(j1, "Elevation").is_some());
        let su = n.features.iter().position(|f| f.values[0] == FieldValue::Text("SU1".into())).unwrap();
        assert_eq!(n.value(su, "PeakDepth"), Some(&FieldValue::Null));
        assert_eq!(n.value(su, "Shape"), Some(&FieldValue::Text("PYRAMIDAL".into())));
        let l = links_layer(&doc, &peaks);
        assert_eq!(l.features.len(), 14);
        let c11 = l.features.iter().position(|f| f.values[0] == FieldValue::Text("C11".into())).unwrap();
        assert_eq!(l.value(c11, "PeakCap"), Some(&FieldValue::Number(0.8)));
        assert_eq!(l.value(c11, "Geom1"), Some(&FieldValue::Number(4.75)));
        assert_eq!(l.value(c11, "Length"), Some(&FieldValue::Number(150.0)));
        assert!(matches!(l.features[c11].geometry, Some(Geometry::LineString(_))));
        let s = subcatchments_layer(&doc);
        assert_eq!(s.features.len(), 8);
        assert!(s.features.iter().all(|f| matches!(f.geometry, Some(Geometry::Polygon(_)))));
        assert!(s.value(0, "NImperv").is_some());
        let g = gages_layer(&doc);
        assert_eq!(g.features.len(), 1);
    }

    #[test]
    fn shapefile_set_round_trips_and_geojson_reprojects_to_wgs84() {
        let doc = pond();
        let dir = temp("export");
        let crs = Crs::from_epsg(2264).unwrap();
        let paths = write_shapefiles(&dir, "pond", &doc, &Peaks::default(), Some(&crs)).unwrap();
        assert_eq!(paths.len(), 4);
        assert!(dir.join("pond_nodes.prj").exists());
        let back = shapefile::read(&dir.join("pond_links.shp")).unwrap();
        assert_eq!(back.features.len(), 14);
        assert_eq!(back.crs.as_ref().and_then(|c| c.epsg), Some(2264));
        assert_eq!(back.value(0, "KIND"), Some(&FieldValue::Text("Conduit".into())));
        let subs = shapefile::read(&dir.join("pond_subcatchments.shp")).unwrap();
        let area_field = subs.fields.iter().position(|f| f.name == "AREA").unwrap();
        assert_eq!(subs.fields[area_field].kind, FieldKind::Number);
        assert_eq!(subs.value(0, "AREA"), Some(&FieldValue::Number(4.55)));

        let texts = geojson_texts(&doc, &Peaks::default(), Some(&crs), true, true).unwrap();
        assert_eq!(texts.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&texts[0].1).unwrap();
        assert!(v.get("crs").is_none(), "WGS 84 output declares no crs");
        let j1 = v["features"].as_array().unwrap().iter().find(|f| f["id"] == "J1").unwrap();
        let x = j1["geometry"]["coordinates"][0].as_f64().unwrap();
        let y = j1["geometry"]["coordinates"][1].as_f64().unwrap();
        // The pond's coordinates are small model units taken as NC feet:
        // they land near the zone's false origin, which is in the Atlantic
        // off Georgia — still a valid, finite longitude/latitude.
        assert!((-90.0..-70.0).contains(&x) && (25.0..40.0).contains(&y), "{x} {y}");
        assert_eq!(j1["properties"]["Kind"], "Junction");
        let per = geojson_texts(&doc, &Peaks::default(), Some(&crs), false, false).unwrap();
        assert_eq!(per.len(), 4);
        let v: serde_json::Value = serde_json::from_str(&per[0].1).unwrap();
        assert_eq!(v["crs"]["properties"]["name"], "urn:ogc:def:crs:EPSG::2264");
        let written = write_geojson(&dir.join("pond.geojson"), &doc, &Peaks::default(), None, true, false).unwrap();
        assert_eq!(written.len(), 4);
        assert!(dir.join("pond_nodes.geojson").exists());
    }
}
