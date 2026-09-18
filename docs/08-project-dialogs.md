# 8. Project dialogs

The Project menu opens one dialog per section that is not a map object.
The menu item and the window it opens:

| Menu item | Window |
| --- | --- |
| Title/Notes… | Title / Notes |
| Options… | Simulation Options |
| Rain Gages… | Rain Gages |
| Curves… | Curves |
| Time Series → Edit… | Time Series |
| Patterns… | Time Patterns |
| Controls… | Control Rules |
| Pollutants… | Pollutants |
| Land Uses… | Land Uses |
| LID Controls…, LID Usage…, Aquifers…, Groundwater…, Snow Packs… | [chapter 21](21-lid-groundwater-quality.md) |
| Buildup / Washoff…, Land Use Coverages…, Initial Loadings…, Treatment… | [chapter 21](21-lid-groundwater-quality.md) |
| Unit Hydrographs…, RDII Inflow… | [chapter 21](21-lid-groundwater-quality.md) |

Every **OK** or **Apply** is one command and one undo step; **Cancel** or
closing the window discards the draft. The same dialogs open from the
Project browser's **Edit…** button and by double-clicking an entry there.

## 8.1 Title/Notes

The `[TITLE]` section as free text. Lines beginning with `;` and blank lines
are not part of the title as the engine reads it (it skips them), so the
browser's count is of data lines only.

## 8.2 Options

`[OPTIONS]` grouped the way the EPA GUI groups them, five tabs:

| Tab | Keys |
| --- | --- |
| General | `FLOW_UNITS` (CFS, GPM, MGD, CMS, LPS, MLD), `INFILTRATION` (HORTON, MODIFIED_HORTON, GREEN_AMPT, MODIFIED_GREEN_AMPT, CURVE_NUMBER), `FLOW_ROUTING` (STEADY, KINWAVE, DYNWAVE), `LINK_OFFSETS` (DEPTH, ELEVATION), `FORCE_MAIN_EQUATION`, `MIN_SLOPE`, `ALLOW_PONDING`, `SKIP_STEADY_STATE`, `IGNORE_RAINFALL`, `IGNORE_SNOWMELT`, `IGNORE_GROUNDWATER`, `IGNORE_RDII`, `IGNORE_ROUTING`, `IGNORE_QUALITY` |
| Dates | `START_DATE`, `START_TIME`, `REPORT_START_DATE`, `REPORT_START_TIME`, `END_DATE`, `END_TIME`, `SWEEP_START`, `SWEEP_END`, `DRY_DAYS` |
| Time Steps | `REPORT_STEP`, `WET_STEP`, `DRY_STEP`, `ROUTING_STEP`, `RULE_STEP` |
| Dynamic Wave | `LENGTHENING_STEP`, `VARIABLE_STEP`, `MINIMUM_STEP`, `INERTIAL_DAMPING` (NONE, PARTIAL, FULL), `NORMAL_FLOW_LIMITED` (SLOPE, FROUDE, BOTH), `SURCHARGE_METHOD` (EXTRAN, SLOT), `MIN_SURFAREA`, `MAX_TRIALS`, `HEAD_TOLERANCE`, `SYS_FLOW_TOL`, `LAT_FLOW_TOL`, `THREADS` |
| Files | the `[FILES]` section as text (`USE`/`SAVE` `RAINFALL`/`RUNOFF`/`HOTSTART`/`RDII`/`INFLOWS`/`OUTFLOWS` lines) |

A key the file does not have is written when you set it; a key it has keeps
its spelling and its place, including the blank-line grouping the GUI
writes. Every value is written as text; the engine's own rules for each
(UM §3.4 and Appendix D `[OPTIONS]`) decide what is valid.

Three of these change how StormSewer itself reads the file:

- `FLOW_UNITS` decides the unit labels everywhere, and whether the
  storm-sewer design tool will accept the model (it refuses metric models);
- `LINK_OFFSETS` decides whether a link's `InOffset`/`OutOffset` are depths
  above the node invert or absolute elevations, for the profile and the
  design mapping;
- `INFILTRATION` decides the column layout of `[INFILTRATION]` rows (a row
  may also end with its own method keyword, which wins).

**The unit-switch wizard.** Changing `FLOW_UNITS` changes only a keyword:
the engine reads every number in the file as if it were already in the new
system. So when you OK a change of `FLOW_UNITS`, the *Flow units changed*
window opens and says so (`FLOW_UNITS is now CMS (was CFS).`). A change
within one system (CFS to GPM) affects only flow-valued fields; a change of
system (US customary to SI) affects lengths, areas, depths, rates and
coefficients as well. The wizard lists each group of values the engine
does not convert, with how many rows it would touch:

Node elevations and depths · Conduit geometry · Weirs, orifices, outlets ·
Storage units · Pump start/stop depths · Subcatchment area, width, storage
· Infiltration parameters · Rain gage time series · External inflows ·
Dry-weather flows · Evaporation · Curves (by type).

Tick what to convert and click **Convert ticked**: one undo step, using
1 ft = 0.3048 m, 1 acre = 0.40468564 ha, 1 in = 25.4 mm, 1 cfs =
0.028316847 m³/s = 448.83117 gpm = 0.64631689 mgd. Manning's n is the
same in both systems; orifice discharge coefficients are dimensionless;
weir coefficients are not (3.33 ft^0.5/s against 1.84 m^0.5/s) and are in
the *Weirs, orifices, outlets* group. **Copy report** puts the list on the
clipboard; **Revert** puts `FLOW_UNITS` back and changes nothing else.
[§16.7](16-troubleshooting.md) is the checklist behind the groups.

**The offsets note.** On the General tab, next to `LINK_OFFSETS`, a note
explains that DEPTH means each link offset is a height above its node's
invert and ELEVATION means an absolute elevation, and that the keyword
alone changes what every offset means. Changing it offers **Convert
existing offsets with the node inverts (same undo step)**: every
`InOffset`/`OutOffset` is rewritten with its node's invert (conduits from
the from-node and to-node; orifices, weirs and outlets from their inlet
node) so the pipes stay where they are. Leave it unticked only if the
numbers in the file are already in the new convention.

## 8.3 Rain Gages

The list of `[RAINGAGES]` on the left; `+` adds one (*Add a rain gage*),
`−` deletes the chosen one. The fields are the row's columns: **Format**
(INTENSITY, VOLUME, CUMULATIVE), **Interval**, **SCF**, **Source**
(TIMESERIES with a series picker, or FILE with a file name, station id and
rain units). A new gage is placed on the map by the browser or the Gage
tool; this dialog edits the row.

## 8.4 Curves

`[CURVES]`. `+` adds a curve (*Add a curve*), `−` deletes. Choose the
**Type**: STORAGE, SHAPE, DIVERSION, TIDAL, PUMP1–PUMP5, RATING, CONTROL,
WEIR. The points are a draft table with **Add row**, plotted beside it;
**Apply** writes the whole curve back as one undo step, keeping its rows in
their place in the file; **Revert** discards the draft; **Close** closes.
The type is written on the first row only, as the engine expects.

## 8.5 Time Series

Project → Time Series is a submenu: **Edit…**, **Import…**, **Export to
File…**.

**Edit…** — `[TIMESERIES]`. `+` adds a series (*Add a time series*), `−`
deletes. Rows are `time value` or `date time value`; edit them in the
draft table (**Add row**) or paste a block into *Paste rows* and click
**Replace with pasted** or **Append pasted**. **From an external file**
turns the series into a `FILE` reference the engine reads at run time (UM
§11.5 for the file format). **Apply** writes the series back; **Revert**;
**Close**. The plot under the table draws the draft. Times are the
engine's hours:minutes (or days:hours:minutes past 24 h); a series without
dates is relative to the simulation start.

**Import…** — the *Import Time Series* window. **Load file…** or paste a
delimited table: a date column, a time column (or a combined `date time`
cell), and one value column per series, so a hundred radar cells import
as a hundred series in one go. Detected without options: comma, tab,
semicolon or space delimiters; an optional header row; dates `M/D/YYYY`,
`MM/DD/YYYY`, `YYYY-MM-DD`, `YYYYMMDD`, `D.M.YYYY`; times `H:MM` or
`HH:MM:SS`; a table with no date column is a relative series. A NOAA
GHCN-Daily station csv (`STATION, DATE, PRCP` in tenths of a millimetre)
is recognised and imported as a daily `VOLUME` series. Names longer than
SWMM's limit are cut and suffixed, and the preview says so. Tick **Also
add a rain gage per series** (and **place on the map**) to get
`[RAINGAGES]` rows whose format and interval match; click **Import**. One
undo step.

**Export to File…** — the *Export Time Series to File* window: choose a
series, **Save As…** writes it as SWMM's external time-series file (`date
time value` lines with `;` comments), and, when asked, replaces its rows
in the `.inp` with a `FILE` reference. For series too long to keep in the
model.

## 8.6 Patterns

`[PATTERNS]`. `+` adds (*Add a pattern*), `−` deletes. **Kind** is MONTHLY
(12 factors), DAILY (7), HOURLY (24) or WEEKEND (24); the table has that
many cells. **Apply**, **Revert**, **Close** as for curves.

## 8.7 Controls

The `[CONTROLS]` section as text, with **OK** and **Cancel**. The rule
syntax is the engine's (UM §3.3 and Appendix C); StormSewer does not parse
it, except that a rename patches the object name after `NODE`, `LINK`,
`PUMP`, `ORIFICE`, `WEIR`, `OUTLET`, `CONDUIT` or `GAGE`. Each `IF` needs
its own `RULE name` line; a second `IF` inside a rule is the usual cause of
the engine's *clause invalid or out of sequence* error
([chapter 16](16-troubleshooting.md)).

## 8.8 Pollutants and Land Uses

`[POLLUTANTS]` and `[LANDUSES]` as grids with **OK** and **Cancel**. The
related `[BUILDUP]` and `[WASHOFF]` rows are edited per land use in
Project → Buildup / Washoff… ([§21.7](21-lid-groundwater-quality.md)),
`[COVERAGES]` and `[LOADINGS]` per subcatchment (§21.8, §21.9) and
`[TREATMENT]` per node (§21.10).

## 8.9 Design Storm…

Builds a hyetograph and writes it as a `[TIMESERIES]` plus a `[RAINGAGES]`
row in one undo step. The **Distribution** choices, each with its source
printed on the dialog:

| Distribution | Inputs | Source |
| --- | --- | --- |
| NRCS (SCS) 24-hour Type I / IA / II / III | 24-hour depth, time step | NRCS TR-20 tabular distributions; NEH 630 Ch. 4 (2019) §630.0403, fig. 4-36 |
| NRCS NOAA Atlas 14 regional 24-hour (A–D) | region, 24-hour depth, time step | NEH 630 Ch. 4 §630.0408, fig. 4-72 ratios, nested (no WinTR-20 smoothing) |
| Alternating block from an IDF curve | `a`, `b`, `c`, duration, time step | Chow, Maidment & Mays, *Applied Hydrology* §14.4 |
| Alternating block from a NOAA Atlas 14 PFDS row | the pasted PFDS csv, return period, duration, time step | depths from the PFDS csv; same method |
| Chicago (Keifer–Chu) from an IDF curve | `a`, `b`, `c`, peak position `r`, duration, time step | Keifer & Chu (1957) |
| Uniform | depth, duration, time step | constant intensity |

The plot shows the hyetograph and its cumulative curve; the table lists
each interval; the line under them totals the depth and names the peak.
Name the **Series** and the **Gage**, choose the **Series format**
(INTENSITY or VOLUME), and optionally **Assign to every subcatchment** (or
to the selected ones) and **Place the gage symbol on the map**. **Add to
Model** writes it all. Depths are in the model's rain unit — inches for a
CFS/GPM/MGD model, millimetres otherwise — and nothing is converted.
[§15.7](15-methods.md) has the equations.

## 8.10 Compute Conduit Lengths…

The geometric length of every conduit from the map against its stored
`Length`, with a factor from `[MAP] Units`, and one batch that writes the
ticked ones. Described in [§3.10](03-map-and-tools.md). Nothing runs on its
own: drawing writes a length once and nothing touches it again until you
ask.

## 8.11 Backdrop and Map Dimensions

View → Backdrop → Load Image…, Georeference…, Show, Remove; and View → Map
Dimensions…. They edit `[BACKDROP]` and `[MAP]`. Described in
[§3.9](03-map-and-tools.md).

## 8.12 LID, groundwater, snow, water quality and RDII

`[LID_CONTROLS]`, `[LID_USAGE]`, `[AQUIFERS]`, `[GROUNDWATER]`, `[GWF]`,
`[SNOWPACKS]` (with `[TEMPERATURE]` and `[ADJUSTMENTS]`), `[BUILDUP]`,
`[WASHOFF]`, `[COVERAGES]`, `[LOADINGS]`, `[TREATMENT]`, `[HYDROGRAPHS]`
and `[RDII]` each have a dialog on the Project menu's second group, and
the per-object ones open from the subcatchment and node property sheets.
[Chapter 21](21-lid-groundwater-quality.md) describes every field.

## 8.13 What has no dialog

`[EVAPORATION]`, `[TRANSECTS]`, `[STREETS]`, `[INLETS]`, `[REPORT]`,
`[PROFILES]`, `[EVENTS]`. All are kept byte for byte and listed in the
Project browser where they have a node; edit them in a text editor. The
engine reads them; StormSewer's validator does not check their contents.
