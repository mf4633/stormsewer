# 17. File formats

## 17.1 The `.inp` file

A SWMM input file is plain text in sections. Each section starts with a
header in square brackets and holds whitespace-delimited rows; `;` starts
a comment, anywhere on a line. The engine's own definition is UM Appendix
D; this chapter adds what Appendix D leaves out and says what StormSewer
does with each section.

### How StormSewer reads it

- Fields are whitespace-delimited, never fixed-width. The `;;` rulers look
  like column guides but values overflow them freely.
- `;` starts a comment anywhere on a data line, even inside quotes: the
  engine truncates at the first `;` before it looks for quotes, so a
  `"label; with semicolon"` in `[LABELS]` is cut short by SWMM itself.
  StormSewer reads it the same way.
- Double quotes group a token (`"Site-Post.jpg"`), the closing quote ends
  the token even with no space after it, and `""` is a real empty field —
  `[DWF]` rows use it as a placeholder, and dropping it would shift the
  later columns.
- Object names are matched without regard to case, as the engine does.
- Section headers are matched uppercased (`[Polygons]` and
  `[POLYGONS]` are the same section); the file keeps its spelling.
- Blank lines occur inside sections; sections can be empty; the last line
  may lack a newline; lines may end CRLF or LF, mixed; a leading byte-order
  mark is allowed; text before the first header is kept.
- Everything is kept. Round-tripping any of the EPA sample models produces
  the identical bytes ([§1.6](01-start-here.md)).

### Sections with typed columns

These sections have a column layout the editor knows, so they show on the
property sheet, in attribute tables, and are checked by the validator.
Column names are Appendix D's, spelled without spaces.

| Section | Columns | Layout varies by |
| --- | --- | --- |
| `[RAINGAGES]` | Name Format Interval SCF Source SeriesOrFile [Station Units] | Source: `TIMESERIES` or `FILE` |
| `[SUBCATCHMENTS]` | Name RainGage Outlet Area PctImperv Width PctSlope CurbLen [SnowPack] | |
| `[SUBAREAS]` | Subcatchment NImperv NPerv SImperv SPerv PctZero RouteTo [PctRouted] | |
| `[INFILTRATION]` | Subcatchment p1 … p5 [Method] | `[OPTIONS] INFILTRATION`, or the row's own trailing method keyword, which wins |
| `[JUNCTIONS]` | Name Elevation MaxDepth InitDepth SurDepth Aponded | |
| `[OUTFALLS]` | Name Elevation Type [StageData] Gated [RouteTo] | Type: `FREE`/`NORMAL` have no stage field; `FIXED` a stage; `TIDAL` a curve; `TIMESERIES` a series |
| `[STORAGE]` | Name Elevation MaxDepth InitDepth Shape … SurDepth Fevap [Psi Ksat IMD] | Shape: `TABULAR` one curve name; `FUNCTIONAL` A0 A1 A2; `CYLINDRICAL`/`CONICAL`/`PARABOLOID`/`PYRAMIDAL` three dimensions |
| `[DIVIDERS]` | Name Elevation DivertedLink Type … | Type: `CUTOFF` one value, `TABULAR` a curve, `WEIR` three values, `OVERFLOW` none |
| `[CONDUITS]` | Name FromNode ToNode Length Roughness InOffset OutOffset InitFlow MaxFlow | |
| `[PUMPS]` | Name FromNode ToNode Curve Status Startup Shutoff | |
| `[ORIFICES]` | Name FromNode ToNode Type Offset Qcoeff Gated CloseTime | |
| `[WEIRS]` | Name FromNode ToNode Type CrestHt Qcoeff Gated EndCon EndCoeff Surcharge [RoadWidth RoadSurf CoeffCurve] | |
| `[OUTLETS]` | Name FromNode ToNode Offset Type … Gated | Type: `TABULAR/DEPTH`, `TABULAR/HEAD` a curve; `FUNCTIONAL/DEPTH`, `FUNCTIONAL/HEAD` a coefficient and exponent |
| `[XSECTIONS]` | Link Shape Geom1 Geom2 Geom3 Geom4 [Barrels Culvert] | Shape: `CUSTOM` and `IRREGULAR` name a curve or transect; `STREET` names a street; the rest take Geom columns |
| `[LOSSES]` | Link Kentry Kexit Kavg FlapGate Seepage | |
| `[INFLOWS]` | Node Constituent Series Type Mfactor Sfactor Baseline Pattern | |
| `[DWF]` | Node Constituent Baseline Pattern1 … Pattern4 | |
| `[RDII]` | Node UnitHydrograph SewerArea | |
| `[TREATMENT]` | Node Pollutant Expression | |
| `[TIMESERIES]` | Name [Date] Time Value | field count: with or without a date; a row may carry several time/value pairs |
| `[CURVES]` | Name Type X Y | the type is on the first row only; continuation rows have one field fewer; several pairs per row allowed |
| `[PATTERNS]` | Name Type Factors… | type on the first row only |
| `[POLLUTANTS]`, `[LANDUSES]`, `[COVERAGES]`, `[LOADINGS]` | as Appendix D | |
| `[GROUNDWATER]`, `[LID_USAGE]`, `[INLET_USAGE]` | as Appendix D | |
| `[COORDINATES]` | Node X Y | |
| `[VERTICES]` | Link X Y | one row per vertex, in order |
| `[POLYGONS]` | Subcatchment X Y | one row per corner |
| `[SYMBOLS]` | Gage X Y | |
| `[LABELS]` | X Y Label [Anchor Font Size Bold Italic] | the label is quoted; the fourth field is an anchor node that renaming follows |
| `[TAGS]` | Kind Name Tag | Kind is `Node`, `Link`, `Subcatch` or `Gage` |
| `[MAP]` | Option Value | `DIMENSIONS x1 y1 x2 y2`; `Units None/Feet/Meters/Degrees` |
| `[OPTIONS]`, `[REPORT]`, `[BACKDROP]`, `[EVAPORATION]`, `[TEMPERATURE]`, `[ADJUSTMENTS]` | Option Value(s) | key/value; key case varies between files and is kept |

Sections kept but not typed (edited by row, or in a text editor):
`[TITLE]`, `[FILES]`, `[CONTROLS]`, `[AQUIFERS]`, `[SNOWPACKS]`,
`[HYDROGRAPHS]`, `[LID_CONTROLS]`, `[BUILDUP]`, `[WASHOFF]`,
`[TRANSECTS]`, `[STREETS]`, `[INLETS]`, `[PROFILES]`, `[EVENTS]`, and any
section a later SWMM adds.

### The sections Appendix D omits

The EPA GUI writes these; the engine reads some of them; the 5.2 manual's
Appendix D does not describe them (reported as
[USEPA #96](https://github.com/USEPA/Stormwater-Management-Model/issues/96)).
What StormSewer knows of them, from the engine's reader and the GUI's
output:

**`[EVENTS]`** — read by the engine (5.1.013 and later). One event per
line: a start date and time and an end date and time, `MM/DD/YYYY HH:MM
MM/DD/YYYY HH:MM`. When present, routing runs only inside the listed
events; outside them only runoff is computed. StormSewer keeps the section
and does not edit it.

**`[PROFILES]`** — written by the GUI, ignored by the engine. Each line is a
quoted profile name followed by up to five link names in path order;
longer profiles continue on further lines repeating the name. StormSewer's
profile view does not read or write it; it picks paths by node.

**`[TAGS]`** — written by the GUI, ignored by the engine. `Kind Name Tag`,
one row per tagged object; a tag is a free label used for grouping. Shown
and edited on the property sheet; a rename follows.

**`[BACKDROP]`** — GUI only (described in UM Appendix D.1). `FILE
"name"`, `DIMENSIONS x1 y1 x2 y2` (the lower-left and upper-right map
coordinates of the image), `UNITS NONE|FEET|METERS|DEGREES`, `OFFSET x y`,
`SCALING xfactor yfactor`. StormSewer reads it to draw the backdrop and
writes it from View → Backdrop ([§3.9](03-map-and-tools.md)); the image
path is relative to the model unless absolute.

**`[MAP]`** — GUI only. `DIMENSIONS x1 y1 x2 y2` is the GUI's map extent;
`Units None|Feet|Meters|Degrees` its map length unit. StormSewer edits
both in View → Map Dimensions… and uses `Units` for the conduit-length
factor ([§3.10](03-map-and-tools.md)); its own view fits to the objects.

**`[STREETS]`, `[INLETS]`, `[INLET_USAGE]`** are in the 5.2 Appendix D.
StormSewer types `[INLET_USAGE]` (Link Inlet Node …) and reads `[INLETS]`
and a `STREET` cross-section for the design mapping's inlet geometry
([§12.2](12-design-panel.md)); it keeps `[STREETS]` and `[INLETS]` as
text.

### What a saved file looks like after an edit

An edited row is rewritten whitespace-delimited and padded to the column
widths of the section's own `;;----` ruler (or its `;;` header row, or
16/10 characters when there is neither), separated by tabs if the ruler
uses tabs, else spaces. A new row goes at the end of its section. A new
section goes at the end of the file. A key set in `[OPTIONS]` keeps its
existing spelling and place; a new key is appended to the section. Nothing
else changes.

## 17.2 The `.rpt` report

Plain text. StormSewer reads:

- the banner line, `EPA STORM WATER MANAGEMENT MODEL - VERSION 5.2 (Build
  5.2.4)`, for the engine version;
- `ERROR nnn: …` lines — any one is fatal — and `WARNING nn: …` lines;
- the `Continuity Error (%) …` line under each of the continuity
  blocks, with the block's title as its section;
- every summary table: a title boxed in asterisks, a dashed rule, two to
  four lines of right-aligned headings whose last line carries the units,
  a rule, then rows. Headings are stacked into column labels ("Maximum /
  Depth / Feet"); a table the engine replaced with a sentence ("No nodes
  were flooded.") has no rows and keeps the sentence; the Routing Time
  Step Summary's key/value lines become `Item` / `Value`;
- `Analysis begun on:` and the timing lines.

`runswmm` appends to an existing report, so StormSewer deletes the old one
before a run. A report with no `Analysis begun` and an empty `.out` is a
run that died before writing anything.

## 17.3 The `.out` results file

Binary, little-endian, as EPA's *SWMM 5 Interfacing Guide* describes it.
The layout (StormSewer reads the closing block, the opening block, the
ids, the start date and step, and the records; it skips the property
blocks and the pollutant unit codes):

```
offset 0        7 × i32: magic (516114522), version, flow-unit code,
                n_subcatch, n_nodes, n_links, n_pollutants
id_offset       object ids: i32 length + that many bytes, in order
                subcatchments, nodes, links, pollutants;
                then n_pollutants × i32 pollutant unit codes
input_offset    per-object property blocks (subcatchment area; node type,
                invert, max depth; link type, offsets, max depth, length)
                then the reporting-variable selections
output_offset − 12   f64 start date (days since 1899-12-30), i32 report step (s)
output_offset   n_periods records, each:
                  f64 date
                  f32 × n_subcatch × (8 + n_pollutants)
                  f32 × n_nodes    × (6 + n_pollutants)
                  f32 × n_links    × (5 + n_pollutants)
                  f32 × 15 system variables
size − 24       6 × i32: id_offset, input_offset, output_offset,
                n_periods, error_code, magic
```

Node variables, in order: depth, head, volume, lateral inflow, total
inflow, flooding, then pollutants. Link variables: flow, depth, velocity,
volume, capacity (fraction full), then pollutants. Subcatchment variables:
rainfall, snow depth, evaporation, infiltration, runoff, groundwater flow,
groundwater elevation, soil moisture, then pollutants.

The closing block is read first; it is the only place the period count
lives, and a file whose trailing magic is missing is a run that died
mid-write, reported as such rather than as garbage. Series are read one
object at a time by seeking, so the whole file is never loaded.

## 17.4 Hotstart `.hsf`

Binary, engine-specific, not read or written by StormSewer
([§16.11](16-troubleshooting.md)).

## 17.5 `.ssproj` — the storm-sewer project

JSON, written by the storm-sewer workspace and by File → Export →
StormSewer Project. It carries the design network (nodes with invert, rim,
area, C, inlet time, coordinates; pipes with length, section, n, end
inverts), the catchment polygons, the IDF curves and return period, the
design options (Min Tc, junction K, tailwater, min slope, HEC-22 defaults),
the report block, and the background placement. Every field has a
default, so older files load, and a `format_version` (currently 1) is
written; the promise is that any 1.x StormSewer opens any 1.x file. The
older `.ssn` text network is still read.

## 17.6 CSV and GeoJSON exports

[§13.3 and §13.4](13-import-export.md). Results CSVs from the views: a
plotted-series CSV has `time_s`, `time_h`, then one column per series
named `<object> · <variable> (<unit>)`; a table CSV is the report table's
columns; the profile CSV's columns are in [§11.4](11-profile.md); the
compare CSV has `Object`, `<metric> A`, `<metric> B`, `Delta`, `Percent`,
with `only in A` / `only in B` rows for objects one run lacks.

## 17.7 World files, images, DXF

**World files.** A backdrop image beside a `.pgw` (PNG), `.jgw` (JPEG) or
`.wld` is georeferenced from it: six lines — the pixel width `A`, the two
rotation terms `D` and `B`, the pixel height `E` (negative, rows run
down), and the centre of the top-left pixel `C`, `F` — the ESRI
convention every GIS writes. The image's `[BACKDROP] DIMENSIONS` follow
from those and its pixel size; rotation is ignored with a note. *Write
world file* in the Georeference dialog writes the same six lines back.

**Underlays.** A PNG background loaded from the File menu is placed by
the storm-sewer workspace's mechanism (width scaling and an offset in map
units) and remembered in the `.ssproj`, not in the `.inp`. A DXF underlay
is drawn from its `LINE`, `LWPOLYLINE`, `POLYLINE` (with `VERTEX`), `ARC`,
`CIRCLE`, `TEXT` and `INSERT` entities, in the DXF's own units,
untransformed.

**Rain and time-series files.** Import… reads delimited text (comma, tab,
semicolon or spaces; optional header; dates `M/D/YYYY`, `MM/DD/YYYY`,
`YYYY-MM-DD`, `YYYYMMDD`, `D.M.YYYY`; times `H:MM`, `HH:MM:SS`, or a
combined cell) and NOAA GHCN-Daily station csv (`STATION, DATE, PRCP` in
tenths of a millimetre). Export to File… writes SWMM's external
time-series file: `date time value` lines, `;` comments, as UM §11.5
defines it.
