# 19. GIS: shapefiles, GeoJSON, coordinate systems and DEMs

The SWMM editor reads and writes the everyday GIS formats itself: ESRI
shapefiles, GeoJSON, `.prj` coordinate systems, GeoTIFF and ESRI ASCII
elevation grids. There is no GDAL, PROJ or other GIS library underneath;
every reader and writer is written from the public specification named at
the top of its source file under `swmm/src/gis/`, and every formula in the
projection code is cited to Snyder (1987) or Karney (2011). That makes the
reach deliberately narrower than a desktop GIS, and this chapter says
exactly where the edges are.

What the GIS tools do:

- **Import GIS Layer…** turns shapefile or GeoJSON features into junctions,
  outfalls, storage units, dividers, rain gages, conduits or subcatchments,
  with a field-mapping table ([§19.3](#193-import-gis-layer)).
- **Import DEM…** draws a GeoTIFF or ASCII grid under the map as a
  hillshade ([§19.5](#195-dems-import-dem)), and **Set Ground From DEM…**
  writes node depths from it ([§19.6](#196-set-ground-from-dem)).
- **Export GIS Layers…** writes the model as a shapefile set or GeoJSON,
  with the peaks of a loaded run as fields ([§19.7](#197-export-gis-layers)).
- **Model CRS…** records which coordinate system the map is in, in a
  sidecar file beside the `.inp` ([§19.2](#192-the-model-crs)).

All five are on the **File** menu while a model is open, and a shapefile,
GeoJSON file, GeoTIFF or ASCII grid dropped on the window (or opened from
the command line) with the SWMM workspace active opens the matching dialog.

## 19.1 Coordinate systems: what is supported

A coordinate reference system (CRS) is read from a `.prj` beside a
shapefile or ASCII grid (OGC WKT1 or the ESRI dialect ArcGIS writes), from a
GeoJSON `crs` member, from a GeoTIFF's GeoKeys, or typed as an EPSG code.

**Projections.** Transverse Mercator (Krüger series, after Karney 2011 —
accurate to well under a millimetre across a UTM zone), Lambert Conformal
Conic (one and two standard parallels), Albers Equal Area, Mercator, Web
Mercator (EPSG:3857), Polar Stereographic (variant B) and Hotine Oblique
Mercator (State Plane Alaska zone 1), plus plain geographic
longitude/latitude. The formulas are from Snyder, *Map Projections — A
Working Manual*, USGS Professional Paper 1395 (1987).

**EPSG codes built in.** 4326 (WGS 84), 4269 (NAD83), 4267 (NAD27,
recognised only), 3857, every UTM zone on WGS 84 (326xx north, 327xx south)
and NAD83 (269xx), 5070 (CONUS Albers), 3031/3413/3995 (polar), and every
State Plane 1983 zone in metres (26929–26998, 32100–32161, and 2205 for
Kentucky North) and, where the
registry has one, in US survey or international feet (2222–2289, 2965,
2966). A `.prj` with no authority code (the ESRI form) is matched to a UTM
or State Plane zone by its parameters, so it still gets its code.

**The State Plane table.** Zone constants (projection, origin, standard
parallels, scale, false easting and northing) are the defining values in
NOAA Manual NOS NGS 5, *State Plane Coordinate System of 1983* (James E.
Stem, 1990), Appendix A. On 2026-09-18 every one of the 122 zones, and
every foot-unit code, was checked against PROJ 9.5.1: each projects a test
point to within 1 cm of the EPSG definition, and each foot code carries
the right foot. The one disagreement was the registry's own: EPSG 26979
(Kentucky North) is deprecated because it put both standard parallels at
37°58′, which misplaces points by metres. StormSewer uses the current code
2205, with Stem's parallels 37°58′ and 38°58′, and reads 26979 as 2205.

**Units.** Metres, US survey feet (1200/3937 m) and international feet
(0.3048 m) are told apart. A US-survey-foot system on a model in feet is
not quite 1:1 — lengths computed from it are multiplied by 1.000002 — which
is correct and far below anything a drainage model notices.

**Datums — read this.**

- **NAD83 and WGS 84 are treated as one datum.** They differ by about 1–2 m
  across the conterminous United States (the NAD83(2011) to WGS 84 shift).
  That is inside the accuracy of the data drainage models are built from,
  but it is a real offset: moving data between a NAD83 State Plane system
  and WGS 84 longitude/latitude here is *not* survey-grade. If you need
  survey-grade positions, transform in a GIS that carries the time-dependent
  datum transformation.
- **NAD27 is not supported.** A NAD27 `.prj` is recognised and named, but
  anything that would transform it is refused with "unsupported datum
  shift". NAD27 to NAD83 needs the NADCON grid shift (tens of metres, up to
  about 100 m), which this build does not carry. Reproject NAD27 data to
  NAD83 in a GIS first.
- Other datums (ED50, GDA, ETRS89 and so on) are read and shown by name, and
  are only transformed to or from a system on the same datum.

**Not supported.** WKT2 (`PROJCRS[…]`), compound and vertical systems,
datum grid shifts, and any projection not in the list above; each is
refused with a message naming what it found.

## 19.2 The model CRS

The model's coordinate system is kept in a sidecar file beside the model,
`<model>.crs` (for `site.inp`, `site.crs`) — never inside the `.inp`, because
the EPA engine refuses a section it does not know and the `.inp` must stay a
file every SWMM tool reads. The sidecar is plain text: comment lines start
with `#`, an `EPSG:nnnn` line gives the code when there is one, and the rest
is the WKT. Either half is enough to read it back. It is re-read whenever the
model's path changes; setting a CRS on a model that has never been saved
holds it until the first save, then writes the sidecar.

**File → Model CRS…** (also the **Model CRS…** button in the GIS section of
the Layers tab) opens the **Model CRS** dialog:

- The text box takes `EPSG:2264`, a bare `2264`, or the WKT pasted from a
  `.prj` file.
- **Parse** checks it and shows a one-line description (name, projection,
  datum, unit), or the error.
- **State Plane 1983 zone** — type part of a state name (`carolina`,
  `texas`) to list its zones; click the metre form or the foot form to fill
  in its EPSG code.
- **Set** saves it as the model CRS (writing the sidecar), **Clear CRS**
  removes the CRS and deletes the sidecar, **Cancel** closes without a
  change.

With a CRS set, the Layers tab's GIS section shows **Cursor lat/lon**:
the map cursor's latitude and longitude, updated as you move the mouse
(WGS 84/NAD83, five decimals, about a metre). A model with no CRS works
exactly as before; only the tools that need to know the unit or to
reproject ask for one.

## 19.3 Import GIS Layer

**File → Import GIS Layer…** asks for a `.shp`, `.geojson` or `.json` file
and opens the **Import GIS Layer** dialog. The header shows the file, the
feature count, the geometry type, the field count, the layer's CRS (or
"not declared", or "not recognised" with the start of the text) and the
model's.

**What is read.** Shapefiles per the *ESRI Shapefile Technical Description*
(1998): Point, MultiPoint, PolyLine and Polygon, with their Z and M variants
(the Z and M values are dropped); MultiPatch is refused. Attributes come from
the `.dbf` (dBASE III/IV: character, numeric, float, logical and date
fields), decoded as UTF-8 when the `.cpg` or the language driver says so,
Windows-1252 for the ANSI drivers, else Latin-1. The `.shx` is optional on
read. GeoJSON per RFC 7946: `FeatureCollection`, `Feature` or a bare
geometry; a `GeometryCollection` contributes its first member. A GeoJSON
file with a `crs` member naming an EPSG code (the pre-2016 convention many
tools still write) is in that system; without one, coordinates inside
±180/±90 are taken as WGS 84 and anything else as having no CRS.

**Import as.** The targets the layer's geometry can become: points →
Junction, Outfall, Storage Unit, Divider or Rain Gage; lines → Conduit;
polygons → Subcatchment. The first is chosen for you. Changing it rebuilds
the mapping table.

**Name from / Name prefix.** The field that names each object. A field
called `name`, `id`, `label`, `objectid` or `fid` (any case) is picked
automatically. "(numbered)" names them from the prefix (`J1`, `J2`, … for
junctions, `C` conduits, `S` subcatchments, `R` gages, `O` outfalls, `ST`
storage, `D` dividers). Names are made legal and unique: spaces become
underscores (`Basin A` → `Basin_A`), a blank or null name is numbered, and a
name already in the model or used earlier in the same import counts up to
the next free one (`MH-1` → `MH-3` when `MH-1` and `MH-2` are taken).

**CRS options.**

- **Reproject to the model CRS** — shown when the layer and the model both
  have a CRS and they differ; on by default. Every coordinate is transformed
  before the features are mapped. A datum the build cannot shift (NAD27)
  stops the import with the reason.
- **Use the layer's CRS as the model CRS** — shown when the layer has a CRS
  and the model has none; on by default. The model takes the layer's system
  (and writes the sidecar) as part of the import, so lengths and areas use
  its unit.

**Conduit options.**

- **Snap tolerance** — a conduit end connects to the nearest existing node
  (or one created earlier in the same import) within this distance, in map
  units. Default 1.
- **Create junctions at unsnapped ends** — on: an end with no node within
  the tolerance gets a new junction there (named from `J`). Off: that
  feature is skipped, with the reason in the preview.
- A line whose two ends snap to the same node is skipped. For a multipart
  line, the longest part is the conduit's route and the others are dropped;
  interior points become the conduit's vertices.

**Subcatchment options.**

- **Outlet to the nearest node** — the outlet is the node nearest the
  polygon's centroid (of its first outer ring); off, the outlet is `*`.
  A mapped `Outlet` column wins over either.
- The outline is the outer ring of the polygon (of the largest part for a
  multipart polygon). Holes are not part of a SWMM outline, but they *are*
  subtracted from the area.

**Keep as a reference layer** keeps the layer on the map after the import
(see [§19.4](#194-reference-layers)).

**Field mapping.** One row per SWMM column the target has — for example
`JUNCTIONS Elevation`, `MaxDepth`, `InitDepth`, `SurDepth`, `Aponded`; for
conduits `CONDUITS Length`, `Roughness`, `InOffset`, `OutOffset`,
`InitFlow`, `MaxFlow` and `XSECTIONS Shape`, `Geom1`–`Geom4`, `Barrels`; for
subcatchments `SUBCATCHMENTS RainGage`, `Outlet`, `Area`, `PctImperv`,
`Width`, `PctSlope`, `CurbLen` and `SUBAREAS NImperv`, `NPerv`, `SImperv`,
`SPerv`, `PctZero`. Each row's source is:

- **(default)** — leave the value a new object of that kind gets when drawn
  on the map;
- a layer field — its value for each feature;
- **(constant)** — the same text for every feature, typed beside it (for
  example `CIRCULAR` for `XSECTIONS Shape`).

Rules:

- A field fills a column automatically when their names match ignoring case
  and punctuation (`MAX_DEPTH` → `MaxDepth`, `pct_imperv` → `PctImperv`), and
  a field called `INVERT` fills `Elevation`. Check the table: an automatic
  match is a guess.
- A null or blank value keeps the default for that one object.
- Numbers are written without a trailing `.0`; logical fields as `YES`/`NO`;
  text containing spaces is quoted.
- No unit conversion is applied to mapped values: a `DIAM` field in inches
  mapped to `Geom1` of a model in feet must be converted first. The
  geometry-derived values below *are* converted.

**Area and length units.** With nothing mapped to them, a subcatchment's
`Area` and a conduit's `Length` come from the geometry:

- The map unit is the model CRS's linear unit when the model has one, else
  `[MAP] Units` (`Feet` or `Meters`).
- US flow units (CFS, GPM, MGD) give areas in acres (43,560 ft²) and
  lengths in feet; SI flow units give hectares and metres.
- If the map unit is unknown (no CRS and `[MAP] Units` of `None`), or the
  model CRS is geographic (degrees), nothing is computed, the default stays,
  and the preview says "map units unknown".
- A mapped `Area` or `Length` field wins over the geometry.

**Preview** builds the import without touching the model and shows what it
will create ("3 junction(s)", "2 conduit(s) + 1 junction(s) at conduit
ends, 1 skipped"), any notes, and the first skipped features with the
reason (no geometry, wrong geometry type, a coordinate that is not a number,
a line with fewer than two points, no node within the tolerance, both ends
on one node). **Import** applies it as **one undo step** labelled "import
GIS layer" — conduits, the junctions created at their ends and every mapped
field together — selects everything it created, and reports the counts in
the status bar and the Layers tab. **Cancel** closes without a change.

## 19.4 Reference layers

A layer kept with **Keep as a reference layer** is drawn under the network
in the model's coordinates (reprojected when the import reprojected). The
GIS section of the Layers tab lists each one with a visibility checkbox, a
colour, a label field (a field's values drawn beside each feature, or "no
labels"), a **Remove** button, and its feature count and source file.
Reference layers are a view setting: they are not written to the `.inp` and
are not kept between sessions.

## 19.5 DEMs: Import DEM

**File → Import DEM…** reads a digital elevation model and draws it under
the map; the status bar reports how it was stored, its CRS and its
elevation range. One DEM is loaded at a time; loading another replaces it.

**GeoTIFF** (TIFF 6.0 with the GeoTIFF 1.1 keys, OGC 19-008r4):

- classic TIFF, either byte order;
- one band, or the first band of an interleaved (PlanarConfiguration 1)
  multi-band image;
- unsigned and signed 8, 16 and 32-bit integers, 32 and 64-bit floats;
- strips or tiles;
- compression none, **LZW** (5), **Deflate** (8, and the older 32946) and
  **PackBits** (32773);
- predictor 2 (horizontal differencing) and 3 (floating-point
  differencing);
- georeferencing from ModelPixelScale + ModelTiepoint or an axis-aligned
  ModelTransformation, with PixelIsArea or PixelIsPoint;
- no-data from the GDAL_NODATA tag;
- the CRS from the EPSG codes in the GeoKeys, or a user-defined projected
  system from its projection keys, with the linear unit.

**Not supported**, each with its own message: **BigTIFF** (rewrite as
classic TIFF, e.g. `gdal_translate -co BIGTIFF=NO`), JPEG and CCITT
compression, separate-plane multi-band images, rotated or sheared
transformations, non-square cells and sub-byte samples.

**ESRI ASCII grid** (`.asc`): the `ncols`/`nrows`/`xllcorner`/`yllcorner`
(or `xllcenter`/`yllcenter`)/`cellsize`/`NODATA_value` header, square cells
only, and its CRS from a `.prj` of the same name when there is one.

**Drawing.** A DEM in a different CRS from the model's is drawn at its
reprojected corners (fine for the extents a drainage model covers); a DEM
with no CRS, or with a datum that cannot be transformed, is drawn as if its
coordinates were the model's. Grids larger than 2048 cells a side are
thinned for display only; sampling always reads the full grid. In the GIS
section of the Layers tab:

- the **DEM: name** checkbox shows or hides it, **Remove** unloads it;
- **Hillshade** — grey relief by Horn's method (the ESRI/GDAL formulation),
  sun from the north-west (azimuth 315°) at 45°, elevation and cell size in
  the same unit;
- **Elevation tint** — a green–tan–brown–white ramp over the DEM's range,
  shaded by the same hillshade;
- the slider sets its opacity.

No-data cells are transparent.

## 19.6 Set Ground From DEM

**File → Set Ground From DEM…** (enabled once a DEM is loaded) sets each
node's `MaxDepth` so that its rim is on the ground:
**MaxDepth = DEM ground − invert Elevation.** The DEM is sampled at the
node by bilinear interpolation between cell centres (the nearest cell at
the grid's edge and beside no-data), after transforming the node's position
into the DEM's CRS when the two differ.

The **Set Ground From DEM** dialog:

- **All nodes** / **Selected nodes** — the scope.
- **DEM-to-model elevation factor** — multiplies DEM values into the
  model's elevation unit. It starts at the ratio of the DEM CRS's unit to
  the model's (feet for US flow units, metres for SI), and at 1 when they
  agree or the DEM declares no unit. Use 3.2808 for a DEM in metres on a
  model in feet, 0.3048 the other way.
- A before/after table: node, invert, ground, MaxDepth now, new MaxDepth,
  and a note. A node outside the DEM or on no-data is left alone ("node is
  outside the DEM or on no-data"); a ground below the invert gives a depth
  of 0 with a note; a node with no invert is left alone; an outfall has no
  depth column and its ground is only listed.
- **Write MaxDepth** applies every change as **one undo step** ("set
  ground from DEM"); **Cancel** closes without a change.

The DEM is not written into the model; only the depths are.

## 19.7 Export GIS Layers

**File → Export GIS Layers…** opens the **Export GIS Layers** dialog. The
model becomes up to four layers — nodes (points), links (lines through their
vertices), subcatchments (polygons from their outlines) and rain gages
(points) — each object with its defining columns as attributes plus `Name`,
`Kind` and `Section`; links also carry their cross-section (`Shape`,
`Geom1`–`Geom4`, `Barrels`) and subcatchments their subareas. A column that
is numeric in every row is written as a number, otherwise as text. Objects
without a position (a subcatchment with no outline, a gage with no symbol)
are written with no geometry.

- **Shapefile set** — pick a folder; `<model>_nodes.shp`,
  `<model>_links.shp`, `<model>_subcatchments.shp` and `<model>_gages.shp`
  are written, each with `.shx`, `.dbf` (UTF-8, with a `.cpg`) and, when the
  model has a CRS, a `.prj`. Empty layers are skipped. dBASE field names are
  10 characters, upper case (`PEAKDEPTH`).
- **GeoJSON** — pick a file name.
  - **Reproject to WGS 84 (RFC 7946)** — coordinates are transformed to
    longitude/latitude and no `crs` member is written, as RFC 7946 requires.
    Needs a model CRS. Off: the model's own coordinates, with the model's
    EPSG code declared in a `crs` member (`urn:ogc:def:crs:EPSG::2264`) and a
    note that the file is not RFC 7946 WGS 84.
  - **One combined file** — every feature in one collection at the chosen
    name; off, one `<name>_<layer>.geojson` per layer.
- **Include run peaks** — enabled when a run is loaded: nodes get
  `PeakDepth`, `PeakInflow` and `PeakFlood`, links `PeakFlow`, `PeakVel` and
  `PeakCap`, from the run's summary. Objects the run did not report get
  null.

**Export…** writes and reports the files; **Close** closes the dialog. The
older File → Export → GeoJSON (map units, no CRS)… of
[§13.4](13-import-export.md) is still there and unchanged.

## 19.8 Tutorial: a network from GIS layers, grounded on a DEM

The files are in the source tree under `swmm/tests/fixtures/gis/`: three
manholes and two pipes as shapefiles in NC State Plane feet (EPSG:2264),
two catchments as GeoJSON, and small GeoTIFF DEMs.

1. **New model.** File → New. Save it once (File → Save As…) so the CRS has
   somewhere to live.
2. **Manholes.** File → Import GIS Layer… → `manholes.shp`. The dialog
   picks **Junction**, names from `NAME`, maps `INVERT` to `JUNCTIONS
   Elevation`, and — because the layer has a `.prj` and the model has no
   CRS — ticks **Use the layer's CRS as the model CRS**. Click **Preview**
   ("3 junction(s)"), then **Import**. The model CRS is now EPSG:2264 and a
   `.crs` file sits beside your `.inp`.
3. **Pipes.** File → Import GIS Layer… → `pipes.shp`. **Conduit** is
   picked; map `DIAM` to `XSECTIONS Geom1` and `N` to `CONDUITS Roughness`.
   Both ends of each pipe land on a manhole, so no junctions are created;
   pipe P2 is multipart and its longer part becomes the route. Tick **Keep
   as a reference layer** and **Import**. Lengths are the drawn lengths in
   feet.
4. **Catchments.** File → Import GIS Layer… → `basins.geojson`.
   **Subcatchment** is picked; map `imperv` to `PctImperv`. Import: `B1`'s
   area is 660 × 660 ft less its 100 × 100 ft hole = 9.77 acres, and its
   outlet is the manhole nearest its centroid. One Edit → Undo removes the
   whole import.
5. **Check the position.** In the Layers tab's GIS section, turn on
   **Cursor lat/lon** and hover over MH-1: about 35.2271° N, 80.8431° W
   (Charlotte).
6. **DEM.** File → Import DEM… → `dem_deflate_pred3_f32.tif` (a
   Deflate-compressed float grid in the same CRS). Switch between
   **Hillshade** and **Elevation tint** in the Layers tab. The fixture DEM is
   tiny and lies south of the manholes; with your own DEM covering the
   network, File → Set Ground From DEM…, check the table, and **Write
   MaxDepth**.
7. **Export.** File → Export GIS Layers… → **GeoJSON**, **Reproject to WGS
   84 (RFC 7946)**, **Export…**. Open the file in any web map: it lands on
   Charlotte.

## 19.9 Known limits

- Datums: NAD83 = WGS 84 (1–2 m), NAD27 refused ([§19.1](#191-coordinate-systems-what-is-supported)).
- No GeoPackage, File Geodatabase, KML, DWG or WFS; convert to shapefile or
  GeoJSON first.
- No BigTIFF, JPEG-compressed or rotated GeoTIFFs.
- Mapped attribute values are not unit-converted.
- Reference layers and the DEM are not saved with the model; the model CRS
  is (in the sidecar).
- Import creates objects; it does not update existing objects that share a
  name — they are counted up to a new name instead.
