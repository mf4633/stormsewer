// SPDX-License-Identifier: GPL-3.0-or-later

//! GIS input and output: rasters, vector layers, and coordinate systems.
//!
//! `raster` is the shared grid type every other module reads and writes.
//! The readers for shapefiles, GeoJSON, `.prj` coordinate systems and
//! GeoTIFF live beside it (see the GIS chapter of the manual).

pub mod raster;
