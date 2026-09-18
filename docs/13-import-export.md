# 13. Import and export

File → Import and File → Export move a network between the SWMM model and
StormSewer's own project format, and write the network out as tables or
GeoJSON. All of the mapping lives in one place (`swmm/src/design.rs`) and
its assumptions are stated here.

## 13.1 Import → StormSewer Project (.ssproj / .ssn)…

Makes a **new SWMM model** from a storm-sewer project — the nodes,
conduits, catchments, and a design storm from the project's IDF curve. One
undo step (`import project`) over a blank model, so `Ctrl+Z` empties it
again.

What is written:

- **Options**: dynamic-wave defaults, `FLOW_UNITS CFS`. Values are written
  in U.S. customary units whatever the project's unit system.
- **Nodes**: `[JUNCTIONS]` and `[OUTFALLS]` with the project's inverts and
  rims (`MaxDepth` = rim − invert), and `[COORDINATES]`.
- **Conduits**: `[CONDUITS]` with length, n and offsets from the project's
  per-pipe inverts; `[XSECTIONS]` with the project's shape (circular, box,
  elliptical, arch).
- **Subcatchments**: catchment polygons become subcatchments with the area
  by the polygon, `%Imperv` by the inverse of the C line
  (`%Imperv = 100 · (C − 0.20) / 0.75`, clamped 0–100), width = area / flow
  length, and `[POLYGONS]`; a node's own local area becomes a polygon-less
  subcatchment `<node>_DA` with width √area and 2 % slope, since the project
  holds no slope for it.
- **Rain gage and storm**: one gage reading a `[TIMESERIES]` built by the
  **alternating-block method** from the project's design-return-period IDF
  curve — 5-minute blocks over 2 hours by default. The IDF curve is what the
  project has, it is valid over that duration range, and the alternating-
  block storm reproduces the curve's intensity at every duration up to the
  storm length ([§15.7](15-methods.md)).

The status bar reports the import with the first of the mapping's notes;
the full list of assumptions is the one in §12.2 and §15.7. The older
`.ssn` text network imports the same way.

## 13.2 Export → StormSewer Project (.ssproj)…

The reverse: the open SWMM model becomes a storm-sewer project, by the
mapping in [§12.2](12-design-panel.md) — circular/box/elliptical/arch
conduits, junctions and outfalls, subcatchments folded into C·A and inlet
times. Objects that do not map are listed in the status bar
(`… 3 object(s) not exported: …`) and the count is in the message. The
hydrology (IDF, return period, Min Tc, K) comes from the storm-sewer
workspace's current project, which is the template for the mapping; the
design panel's own basis is not used here. Metric models are refused.

## 13.3 Export → Network CSV (nodes, links)…

Two files, `<name>-nodes.csv` and `<name>-links.csv`:

```
Name,Kind,X,Y,Invert,MaxDepth,Type
Name,Kind,From,To,Length,Roughness,InOffset,OutOffset,Shape,Geom1,Geom2,Vertices
```

`Kind` is Junction / Outfall / Divider / Storage or Conduit / Pump /
Orifice / Weir / Outlet; `Type` is the outfall or divider type; `Vertices`
is the *count* of the link's interior vertices (the coordinates themselves
are in the GeoJSON). Length, roughness and offsets are filled for conduits
only. Cells with commas or quotes are quoted.

## 13.4 Export → GeoJSON (map units, no CRS)…

One `FeatureCollection`: a `Point` per node and rain gage, a `LineString`
per link (through its vertices), a `Polygon` per subcatchment. Properties
carry `name`, `kind`, and the object's row columns. Coordinates are the
model's own map units; no CRS is assumed or written, and the file says so
in a top-level `note`. Assign the CRS in your GIS when you load it.

## 13.5 Images and rain

- View → Backdrop → Load Image… places a PNG or JPEG in model coordinates
  from its world file and records it in `[BACKDROP]`; Georeference… can
  write the world file back ([§3.9](03-map-and-tools.md)). File → Load PNG
  Background… and DXF Underlay… are drawing aids that import nothing.
- Project → Time Series → Import… reads delimited rain and flow tables and
  NOAA GHCN-Daily station files into `[TIMESERIES]` (and gages);
  Export to File… writes a series as SWMM's external file
  ([§8.5](08-project-dialogs.md)).
- Project → Design Storm… builds a hyetograph from a NOAA Atlas 14 depth
  or PFDS row, or an IDF curve ([§8.9](08-project-dialogs.md)).

## 13.6 What there is no importer for

- **Shapefile and GeoJSON layers** import through File → Import GIS Layer…
  ([§19.3](19-gis.md)). **GeoPackage** and **DXF as objects** are not in this
  build. The DXF underlay is a picture to draw over. The storm-sewer
  workspace's DXF and LandXML importers make a storm-sewer project, which
  §13.1 then turns into a SWMM model — that is the route from Civil 3D
  today: Civil 3D → LandXML → storm-sewer project → SWMM model.
- **Hydraflow `.stm`**: the storm-sewer workspace imports it; then §13.1.
- **Results**: the `.out` and `.rpt` are read, never written or converted.
  CSV of results is per view ([chapter 10](10-results-views.md)).
- **Other engines' project files**: none are read.
