// SPDX-License-Identifier: GPL-3.0-or-later

//! GIS input and output: rasters, vector layers, and coordinate systems.
//!
//! `raster` is the shared grid type every other module reads and writes.
//! The readers for shapefiles, GeoJSON, `.prj` coordinate systems and
//! GeoTIFF live beside it (see the GIS chapter of the manual).
//!
//! Every reader is written from the public specification named at the
//! top of its file; no third-party format library is used.

pub mod crs;
pub mod dbf;
pub mod export;
pub mod geojson;
pub mod geotiff;
pub mod import;
pub mod inflate;
pub mod lzw;
pub mod prj;
pub mod raster;
pub mod shapefile;
pub mod sidecar;
pub mod stateplane;
pub mod vector;

use std::path::Path;

use crate::{Error, Result};

/// The vector formats the importer reads, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorFormat {
    Shapefile,
    GeoJson,
}

/// The raster formats the DEM loader reads, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterFormat {
    GeoTiff,
    EsriAscii,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn vector_format(path: &Path) -> Option<VectorFormat> {
    match ext_of(path).as_str() {
        "shp" => Some(VectorFormat::Shapefile),
        "geojson" | "json" => Some(VectorFormat::GeoJson),
        _ => None,
    }
}

pub fn raster_format(path: &Path) -> Option<RasterFormat> {
    match ext_of(path).as_str() {
        "tif" | "tiff" | "gtif" => Some(RasterFormat::GeoTiff),
        "asc" | "txt" | "grd" => Some(RasterFormat::EsriAscii),
        _ => None,
    }
}

/// Read a vector layer by its extension.
pub fn read_vector(path: &Path) -> Result<vector::Layer> {
    match vector_format(path) {
        Some(VectorFormat::Shapefile) => shapefile::read(path),
        Some(VectorFormat::GeoJson) => geojson::read(path),
        None => Err(Error::Format(format!(
            "{}: not a .shp or .geojson file",
            path.display()
        ))),
    }
}

/// Read a raster by its extension. A GeoTIFF also yields its CRS when the
/// file carries one; an ESRI ASCII grid takes its CRS from a `.prj`
/// beside it when present.
pub fn read_raster(path: &Path) -> Result<(raster::Raster, Option<crs::Crs>)> {
    match raster_format(path) {
        Some(RasterFormat::GeoTiff) => {
            let g = geotiff::read(path)?;
            Ok((g.raster, g.crs))
        }
        Some(RasterFormat::EsriAscii) => {
            let r = raster::Raster::read_asc(path)?;
            let crs = std::fs::read_to_string(path.with_extension("prj"))
                .ok()
                .and_then(|t| prj::parse_wkt(&t).ok());
            Ok((r, crs))
        }
        None => Err(Error::Format(format!(
            "{}: not a .tif or .asc file",
            path.display()
        ))),
    }
}
