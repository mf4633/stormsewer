// SPDX-License-Identifier: GPL-3.0-or-later

//! The one vector-layer model every reader fills and every writer takes:
//! features with a geometry and a row of attribute values, plus the field
//! list that names the columns. Shapefile, GeoJSON and the importer all
//! speak this and nothing else.

use crate::gis::crs::Crs;

/// A geometry in the layer's own coordinates. Polygons are rings, outer
/// first and holes after; rings are closed (first point repeated last)
/// as both shapefiles and GeoJSON store them.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point(f64, f64),
    MultiPoint(Vec<(f64, f64)>),
    LineString(Vec<(f64, f64)>),
    MultiLineString(Vec<Vec<(f64, f64)>>),
    Polygon(Vec<Vec<(f64, f64)>>),
    MultiPolygon(Vec<Vec<Vec<(f64, f64)>>>),
}

/// The three shapes the importer cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryKind {
    Point,
    Line,
    Polygon,
}

impl GeometryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Point => "points",
            Self::Line => "lines",
            Self::Polygon => "polygons",
        }
    }
}

impl Geometry {
    pub fn kind(&self) -> GeometryKind {
        match self {
            Self::Point(..) | Self::MultiPoint(_) => GeometryKind::Point,
            Self::LineString(_) | Self::MultiLineString(_) => GeometryKind::Line,
            Self::Polygon(_) | Self::MultiPolygon(_) => GeometryKind::Polygon,
        }
    }

    /// Every coordinate, in storage order.
    pub fn points(&self) -> Vec<(f64, f64)> {
        match self {
            Self::Point(x, y) => vec![(*x, *y)],
            Self::MultiPoint(p) | Self::LineString(p) => p.clone(),
            Self::MultiLineString(parts) | Self::Polygon(parts) => parts.concat(),
            Self::MultiPolygon(polys) => polys.iter().flat_map(|p| p.concat()).collect(),
        }
    }

    /// The same geometry with every coordinate mapped through `f`.
    pub fn map(&self, f: &dyn Fn((f64, f64)) -> (f64, f64)) -> Geometry {
        let ring = |r: &Vec<(f64, f64)>| r.iter().map(|p| f(*p)).collect::<Vec<_>>();
        match self {
            Self::Point(x, y) => {
                let (x, y) = f((*x, *y));
                Self::Point(x, y)
            }
            Self::MultiPoint(p) => Self::MultiPoint(ring(p)),
            Self::LineString(p) => Self::LineString(ring(p)),
            Self::MultiLineString(parts) => Self::MultiLineString(parts.iter().map(ring).collect()),
            Self::Polygon(rings) => Self::Polygon(rings.iter().map(ring).collect()),
            Self::MultiPolygon(polys) => {
                Self::MultiPolygon(polys.iter().map(|p| p.iter().map(ring).collect()).collect())
            }
        }
    }

    /// Bounding box (xmin, ymin, xmax, ymax), or None for an empty geometry.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        bounds_of(&self.points())
    }

    /// A representative point: the point itself, the first point of a
    /// line, or the centroid of the outer ring of a polygon.
    pub fn anchor(&self) -> Option<(f64, f64)> {
        match self {
            Self::Point(x, y) => Some((*x, *y)),
            Self::MultiPoint(p) | Self::LineString(p) => p.first().copied(),
            Self::MultiLineString(parts) => parts.iter().find_map(|p| p.first().copied()),
            Self::Polygon(rings) => rings.first().and_then(|r| ring_centroid(r)),
            Self::MultiPolygon(polys) => polys
                .first()
                .and_then(|p| p.first())
                .and_then(|r| ring_centroid(r)),
        }
    }

    /// The outer ring of the largest polygon, for conversion to a single
    /// SWMM subcatchment outline. Returned unclosed (SWMM style).
    pub fn outer_ring(&self) -> Option<Vec<(f64, f64)>> {
        let ring = match self {
            Self::Polygon(rings) => rings.first()?.clone(),
            Self::MultiPolygon(polys) => polys
                .iter()
                .filter_map(|p| p.first())
                .max_by(|a, b| {
                    ring_area(a)
                        .abs()
                        .partial_cmp(&ring_area(b).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })?
                .clone(),
            _ => return None,
        };
        Some(unclose(ring))
    }

    /// Signed-area sum of every polygon (outer rings positive, holes
    /// subtracted), in squared layer units. Zero for points and lines.
    pub fn area(&self) -> f64 {
        let poly = |rings: &Vec<Vec<(f64, f64)>>| {
            let mut a = 0.0;
            for (i, r) in rings.iter().enumerate() {
                let s = ring_area(r).abs();
                if i == 0 {
                    a += s;
                } else {
                    a -= s;
                }
            }
            a.max(0.0)
        };
        match self {
            Self::Polygon(rings) => poly(rings),
            Self::MultiPolygon(polys) => polys.iter().map(poly).sum(),
            _ => 0.0,
        }
    }

    /// Total length of every line part, in layer units. Zero otherwise.
    pub fn length(&self) -> f64 {
        match self {
            Self::LineString(p) => path_length(p),
            Self::MultiLineString(parts) => parts.iter().map(|p| path_length(p)).sum(),
            _ => 0.0,
        }
    }

    /// The longest line part as one path (for a conduit's route).
    pub fn main_path(&self) -> Option<Vec<(f64, f64)>> {
        match self {
            Self::LineString(p) => Some(p.clone()),
            Self::MultiLineString(parts) => parts
                .iter()
                .max_by(|a, b| {
                    path_length(a)
                        .partial_cmp(&path_length(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned(),
            _ => None,
        }
    }
}

pub fn bounds_of(points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let mut it = points.iter();
    let &(x, y) = it.next()?;
    Some(it.fold((x, y, x, y), |(x0, y0, x1, y1), &(x, y)| {
        (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
    }))
}

/// Shoelace signed area: positive for counter-clockwise rings.
pub fn ring_area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        s += x0 * y1 - x1 * y0;
    }
    s / 2.0
}

/// Area-weighted centroid of a ring; the vertex mean for a degenerate one.
pub fn ring_centroid(ring: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = ring.len();
    if n == 0 {
        return None;
    }
    let a = ring_area(ring);
    if a.abs() < 1e-12 {
        let (sx, sy) = ring.iter().fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
        return Some((sx / n as f64, sy / n as f64));
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    Some((cx / (6.0 * a), cy / (6.0 * a)))
}

pub fn path_length(p: &[(f64, f64)]) -> f64 {
    p.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Drop a repeated closing point.
pub fn unclose(mut ring: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring
}

/// Repeat the first point last when it is not already.
pub fn close(mut ring: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if ring.len() > 1 && ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    ring
}

/// One attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Null,
    Text(String),
    Number(f64),
    Bool(bool),
    /// `YYYYMMDD` as dBASE stores it, or ISO text from GeoJSON.
    Date(String),
}

impl FieldValue {
    /// The value as it would be written into a SWMM row: numbers without a
    /// trailing `.0`, booleans as `YES`/`NO`, null as empty.
    pub fn to_field(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Text(s) | Self::Date(s) => s.trim().to_string(),
            Self::Number(v) => crate::doc::format_number(*v),
            Self::Bool(b) => if *b { "YES" } else { "NO" }.to_string(),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(v) => Some(*v),
            Self::Text(s) => s.trim().parse().ok(),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null) || matches!(self, Self::Text(s) if s.trim().is_empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Bool,
    Date,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
}

impl Field {
    pub fn new(name: &str, kind: FieldKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub geometry: Option<Geometry>,
    /// One value per layer field, in field order.
    pub values: Vec<FieldValue>,
}

impl Feature {
    pub fn new(geometry: Option<Geometry>, values: Vec<FieldValue>) -> Self {
        Self { geometry, values }
    }
}

/// A vector layer as read from disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
    /// The coordinate system when the file declared one (`.prj`, GeoJSON
    /// `crs`), parsed.
    pub crs: Option<Crs>,
    /// The declaration as text, kept even when it could not be parsed so
    /// the dialog can show it.
    pub crs_text: Option<String>,
}

impl Layer {
    pub fn new(name: &str, fields: Vec<Field>) -> Self {
        Self {
            name: name.to_string(),
            fields,
            features: Vec::new(),
            crs: None,
            crs_text: None,
        }
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
    }

    pub fn value(&self, feature: usize, field: &str) -> Option<&FieldValue> {
        let i = self.field_index(field)?;
        self.features.get(feature)?.values.get(i)
    }

    /// The kind of the first feature with a geometry, or None if the layer
    /// has no geometry at all.
    pub fn geometry_kind(&self) -> Option<GeometryKind> {
        self.features
            .iter()
            .find_map(|f| f.geometry.as_ref().map(Geometry::kind))
    }

    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let pts: Vec<(f64, f64)> = self
            .features
            .iter()
            .filter_map(|f| f.geometry.as_ref())
            .flat_map(Geometry::points)
            .collect();
        bounds_of(&pts)
    }

    /// The layer with every geometry mapped through `f`.
    pub fn map_geometry(&self, f: &dyn Fn((f64, f64)) -> (f64, f64)) -> Layer {
        let mut out = self.clone();
        for feat in &mut out.features {
            if let Some(g) = &feat.geometry {
                feat.geometry = Some(g.map(f));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_area_and_centroid_of_a_square() {
        let sq = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        assert!((ring_area(&sq) - 100.0).abs() < 1e-12);
        assert_eq!(ring_centroid(&sq), Some((5.0, 5.0)));
        let cw: Vec<_> = sq.iter().rev().copied().collect();
        assert!((ring_area(&cw) + 100.0).abs() < 1e-12);
        let g = Geometry::Polygon(vec![sq.clone(), vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0), (2.0, 2.0)]]);
        assert!((g.area() - 96.0).abs() < 1e-12);
        assert_eq!(g.outer_ring().unwrap().len(), 4, "unclosed");
        assert_eq!(g.anchor(), Some((5.0, 5.0)));
    }

    #[test]
    fn length_and_main_path() {
        let g = Geometry::MultiLineString(vec![
            vec![(0.0, 0.0), (3.0, 4.0)],
            vec![(0.0, 0.0), (1.0, 0.0)],
        ]);
        assert!((g.length() - 6.0).abs() < 1e-12);
        assert_eq!(g.main_path().unwrap().len(), 2);
        assert_eq!(g.main_path().unwrap()[1], (3.0, 4.0));
        assert_eq!(g.bounds(), Some((0.0, 0.0, 3.0, 4.0)));
    }

    #[test]
    fn field_values_format_like_swmm_fields() {
        assert_eq!(FieldValue::Number(4.0).to_field(), "4");
        assert_eq!(FieldValue::Number(0.013).to_field(), "0.013");
        assert_eq!(FieldValue::Bool(true).to_field(), "YES");
        assert_eq!(FieldValue::Null.to_field(), "");
        assert_eq!(FieldValue::Text(" 12 ".into()).as_f64(), Some(12.0));
        assert!(FieldValue::Text("  ".into()).is_null());
    }
}
