# 16. Troubleshooting

Questions in the order people ask them on the SWMM forums, answered for
StormSewer. Where the answer is the engine's behaviour the manual cites the
EPA document; where it is a forum finding the thread is linked and
paraphrased, never copied. Engine error numbers are indexed in
[Appendix C](A3-error-codes.md).

## 16.1 The continuity error: what it means and what is acceptable

The report ends each of Runoff Quantity Continuity, Flow Routing
Continuity, Quality Routing Continuity and Groundwater Continuity with a
`Continuity Error (%)` line: the mismatch between what came in and what
went out plus what is stored, as a percentage of the inflow. StormSewer
shows the largest absolute value in the run panel as `worst continuity`.

A model can **run and be wrong**. `runswmm` exits 0 regardless, and a
finished run with a 40 % continuity error is still a finished run. The
forum's own words for this are that SWMM lets a model *silently fail*
([r/civilengineering](https://www.reddit.com/r/civilengineering/comments/1oo11a1/comment/nn5nxxb/)).
Always read the number.

Working thresholds from the OpenSWMM threads
([4223](https://www.openswmm.org/Topic/4223/interpreting-continuity-error),
[3925](https://www.openswmm.org/Topic/3925/high-continuity-error-in-flow-routing)):
under 1 % is good; under 2 % is usually fine; 5 % needs a reason;
anything above that means the routing did not converge somewhere and the
results at that place are not trustworthy. Some reviewers ask for 0.5 %.
The sign is only which side the imbalance fell on; a negative error is not
better than a positive one
([11479](https://www.openswmm.org/Topic/11479/meaning-of-negative-value-in-continuity-error)).

Where it comes from, in the order to check:

1. **Routing time step too long** for the shortest conduit or steepest
   pipe (RM II §3.4, §4.4). Halve `ROUTING_STEP`, or set `VARIABLE_STEP`
   (0.5–0.75) and a `MINIMUM_STEP`.
2. **Very short links** next to long ones; use `LENGTHENING_STEP` (10–30 s
   is a common setting) or merge them.
3. **Weirs and orifices on a junction with no surface area**: a junction
   whose only outgoing links are a weir or orifice has nowhere to put water
   below the crest, so it fills and floods at low flow
   ([28820](https://www.openswmm.org/Topic/28820/node-continuity-errors-weirs)).
   StormSewer's validator warns about this. Make the node a storage unit
   with a real area.
4. **Storage with a throttled outlet** under `SURCHARGE_METHOD EXTRAN`
   ([14273](https://www.openswmm.org/Topic/14273/storage-node-yielding-large-continuity-error),
   [USEPA #210](https://github.com/USEPA/Stormwater-Management-Model/issues/210));
   try `SLOT`, and check `MIN_SURFAREA`.
5. **Pumps** cycling on and off each step; see §16.9.
6. **Flooding with `ALLOW_PONDING NO`**: water lost to flooding is
   accounted as `Flooding Loss`, not an error — but a huge flooding loss
   with a small error is its own warning sign.

The engine prints only the five worst nodes in *Highest Continuity Errors*
and the five worst links in *Highest Flow Instability Indexes*
([USEPA #32](https://github.com/USEPA/Stormwater-Management-Model/issues/32)).
The Run Status window ([§9.4](09-run-and-engines.md)) shows every entry
the report carries, colours each continuity section green below 1 %,
amber to 10 % and red above, and selects a listed node or link on click;
the Model Report keeps the whole report text.

## 16.2 The run "worked" but nothing is right

Check, in order: the `ERROR`/`WARNING` lines in the run panel; the
continuity error; the Node Flooding Summary and the Conduit Surcharge
Summary in Tables; then the Profile with Max HGL along the trunk. A model
whose every node floods is telling you about an offset or a unit, not
about drainage.

## 16.3 Instability: "the results oscillate"

An oscillating hydrograph, a link with a large instability index, or a
step count that never converges (`% of Steps Not Converging` in the
Routing Time Step Summary) is a numerical problem, not a hydraulic one.
RM II §3.4 is the reference. What people report helping, in rough order of
effect
([2346](https://www.openswmm.org/Topic/2346/stability-problems),
[3938](https://www.openswmm.org/Topic/3938/inertial-term-dampening),
[25086](https://www.openswmm.org/Topic/25086/kinematic-wave-vs-dynamic-wave-routing-question)):

- a smaller `ROUTING_STEP`, or `VARIABLE_STEP` with a `MINIMUM_STEP`;
- `LENGTHENING_STEP` so short links are computed as if longer;
- `INERTIAL_DAMPING PARTIAL` or `FULL` (`FULL` drops the inertial terms as
  flow approaches critical; RM II §3.2);
- `NORMAL_FLOW_LIMITED BOTH`;
- `MIN_SURFAREA` raised on tiny junctions;
- removing an adverse-slope conduit under kinematic wave, which the KW
  solver cannot route (RM II §4.1); dynamic wave can.

Kinematic wave is faster and cannot surcharge or back up; if your model
has surcharge, backwater or loops it needs `DYNWAVE` (RM II §2.2).

## 16.4 Surcharged, flooded, or ponded

Three words the engine uses with exact meanings (UM §3.3 and the Node
Surcharge and Node Flooding summaries):

- **Surcharged** — the node's water level is above the crown of the
  highest connecting conduit; the pipe is under pressure. Reported as
  hours surcharged and the height above the crown. A node can be
  surcharged with the pipes half empty at the start if the initial depths
  are wrong under kinematic wave
  ([4224](https://www.openswmm.org/Topic/4224/node-surcharge-flooding-issue)).
- **Flooded** — the water level reached the rim (invert + `MaxDepth`,
  plus `SurDepth` if set) and water left the system. With `ALLOW_PONDING
  NO` that water is gone (counted as Flooding Loss). The HGL in a profile
  stops at the rim.
- **Ponded** — with `ALLOW_PONDING YES` *and* an `Aponded` area on the
  node, flooded water sits on the node in a pond of that area and drains
  back in when it can. The HGL can then rise above the rim
  ([29608](https://www.openswmm.org/Topic/29608/node-flooding-with-ponded-area),
  [15558](https://www.openswmm.org/Topic/15558/hydraulic-grade-line-above-junctions)).
  Without an `Aponded` area, `ALLOW_PONDING YES` does nothing at that node.

`SurDepth` (surcharge depth) is an extra height above the rim before
flooding is declared — a sealed manhole cover.

## 16.5 ERROR 209 and other "undefined object" errors

`ERROR 209: undefined object X at line N` means a row names an object that
does not exist. The usual culprits
([16685](https://www.openswmm.org/Topic/16685/undefined-object-error-209),
[13117](https://www.openswmm.org/Topic/13117/assigning-rain-gage-error-209),
[USEPA #171](https://github.com/USEPA/Stormwater-Management-Model/issues/171)):

- a subcatchment whose `RainGage` or `Outlet` was deleted or renamed by
  hand;
- a link whose node was deleted;
- an `[INFLOWS]`, `[DWF]` or `[TAGS]` row for a deleted node;
- a `[REPORT]` line (`NODES J5 J6`) that still lists a deleted object —
  the engine treats that as fatal.

StormSewer's validator catches all of these before the run (`link "…" does
not exist`, `rain gage "…" does not exist`, `outlet "…" is neither a node
nor a subcatchment`, `to node "…" does not exist`) and refuses to run
until they are fixed, and its delete and rename commands keep references
in step so they do not arise from editing here. They arise from files
edited elsewhere. Click the finding to go to the row.

Also fatal at read time: a duplicate name (the engine ignores case, so
`J1` and `j1` are the same object — the validator says so), a row with too
few fields, a zero or negative length (ERROR 111) or area (ERROR 211), a
`REPORT_STEP` shorter than `ROUTING_STEP` (ERROR 195), an outfall with more
than one link (ERROR 141), an end date before the start (ERROR 191). The
validator checks each of these and names the engine's code.

## 16.6 Offsets, inverts and rims: what the engine silently changes

A link's `InOffset` and `OutOffset` are the heights of its ends above the
node inverts when `LINK_OFFSETS DEPTH`, or absolute elevations when
`ELEVATION` (UM §3.3, Appendix D `[CONDUITS]`). The forum history says
the manual's first definition was unclear and people still get it
backwards
([3507](https://www.openswmm.org/Topic/3507/inlet-offset-and-outlet-offset-meaning),
[9489](https://www.openswmm.org/Topic/9489/conduit-depth-with-offsets)).

Two things the engine does without asking, noted only by a `WARNING` line
in the report that is easy to miss
([swmm5.org](https://swmm5.org/2013/08/23/what-node-and-link-invert-elevations-does-swmm-5-use/)):

- a link end below its node's invert (a negative offset, or an `ELEVATION`
  offset under the invert) is raised to the node invert — the offset
  becomes 0;
- a conduit crown above the node's rim (invert + `MaxDepth`) raises the
  node's rim to the crown; with `MaxDepth 0` the rim *is* the highest crown.

StormSewer's validator flags a negative offset and an `ELEVATION` offset
below the node invert before the run; the profile draws what the engine
will use ([chapter 11](11-profile.md)). A drop through a structure is a
pipe entering above the node invert — an `InOffset` on the downstream
pipe's upstream end is not how to say that; put the offset on the
*incoming* pipe's `OutOffset`.

Switching `LINK_OFFSETS` between DEPTH and ELEVATION is the same trap in
miniature: the keyword alone changes what every offset in the file means,
and every pipe moves. The Options dialog says so beside the field and
offers *Convert existing offsets with the node inverts (same undo step)*
([§8.2](08-project-dialogs.md)). Take it unless the numbers are already
in the new convention.

## 16.7 I changed the flow units and the results changed

Changing `FLOW_UNITS` changes how every number in the file is *read*, and
converts none of them
([4235](https://www.openswmm.org/Topic/4235/changing-flow-units),
[32845](https://openswmm.org/Topic/32845/how-to-unify-units)). The engine
does the same: the labels change, the numbers do not. When you change the
keyword in Project → Options…, StormSewer opens the unit-switch wizard
([§8.2](08-project-dialogs.md)): it lists the groups below with the number
of rows each would touch, converts the ones you tick as one undo step, and
can copy its report. Anything it does not cover you convert by hand or
with *Replace in column* → **Scale** ([§6.4](06-attribute-tables.md)).
The checklist of what a switch between U.S. and SI touches:

| Section | What must change |
| --- | --- |
| `[JUNCTIONS]`, `[OUTFALLS]`, `[STORAGE]`, `[DIVIDERS]` | elevations and depths: ft ↔ m |
| `[CONDUITS]` | length ft ↔ m; offsets; `Roughness` is Manning's n and is the **same** in both systems |
| `[XSECTIONS]` | every Geom column ft ↔ m |
| `[SUBCATCHMENTS]` | area ac ↔ ha; width ft ↔ m |
| `[SUBAREAS]` | depression storage in ↔ mm |
| `[INFILTRATION]` | Horton rates in/hr ↔ mm/hr; Green-Ampt suction in ↔ mm, conductivity in/hr ↔ mm/hr |
| `[RAINGAGES]` and their series | in/hr ↔ mm/hr, or in ↔ mm for VOLUME |
| `[INFLOWS]`, `[DWF]` | baseline flows and any FLOW series in the flow unit |
| `[PUMPS]` curves | head ft ↔ m, flow in the flow unit |
| `[WEIRS]` `Qcoeff` | dimensional: 3.33 (U.S.) ↔ 1.84 (SI) for a sharp-crested transverse weir; the engine expects the metric form in an SI model ([3620](https://www.openswmm.org/Topic/3620/swmm-5-11-picks-up-weir-cd-in-us-units-when-using-si-units)) |
| `[ORIFICES]` `Qcoeff` | dimensionless — unchanged |
| `[STORAGE]` curves and `[CURVES]` | depth ft ↔ m; area ft² ↔ m²; flows |
| `[DIVIDERS]` cutoff flows, tabular curves | flow unit |
| `[OPTIONS] MIN_SURFAREA` | ft² ↔ m² |
| `[LID_CONTROLS]`, `[LID_USAGE]` | thicknesses in ↔ mm; areas ft² ↔ m² |
| `[EVAPORATION]`, `[TEMPERATURE]`, `[SNOWPACKS]` | in/day ↔ mm/day, °F ↔ °C |
| `[LOADINGS]`, pollutant concentrations | usually unchanged (mg/L) — check |

The storm-sewer design panel refuses metric models rather than convert
([§12.2](12-design-panel.md)).

## 16.8 Subcatchment width

`Width` is the width of the overland flow path: conceptually the
subcatchment's area divided by the length of the longest overland flow
path to the collector (RM I §3.8). It sets the runoff hydrograph's timing;
the same area with half the width peaks later and lower. Its estimation
is the most-asked question on the forums
([3818](https://www.openswmm.org/Topic/3818/estimating-subcatchment-width),
[4862](https://www.openswmm.org/Topic/4862/estimating-width-for-a-subcatchment),
[11520](https://www.openswmm.org/Topic/11520/characteristic-width-of-subcatchments))
and Guo (2012) called it a frequent source of user error. Rules of thumb
people use: area ÷ maximum overland flow length; for a rectangular lot
draining to a gutter along one side, the length of that side; for a
subcatchment draining to a pipe running through it, about twice the pipe
length (flow comes from both sides), with a skew factor when the two sides
are unequal. Flow lengths over about 500 ft of true sheet flow are rare.

The design mapping uses `Area / Width` as the flow length for Kirpich
([§15.3](15-methods.md)), so a wrong width also moves the design Tc. The
validator warns when a width is zero: no runoff leaves such a
subcatchment.

## 16.9 Pumps cycle on and off; control rules are rejected

A Type 2 pump (flow versus wet-well depth, stepwise) starts at `Startup`
and stops at `Shutoff` depth; with the two close together and a small
wet well it cycles every step and the continuity error climbs
([2773](https://www.openswmm.org/Topic/2773/pump-curves-in-swmm5)).
Separate the depths, enlarge the wet well, or use a Type 3 or 4 curve.

`ERROR 2xx: … clause invalid or out of sequence` in `[CONTROLS]` is nearly
always a second `IF` inside one rule: every `IF` needs its own `RULE
name` line
([9552](https://www.openswmm.org/Topic/9552/pump-control-rule-problem),
[16749](https://www.openswmm.org/Topic/16749/pump-control-rules)). The
syntax is UM Appendix C. StormSewer does not parse rules; it patches
object names in them on rename.

## 16.10 Rain and time-series files: ERROR 363, GHCN names, long comments

`ERROR 363: invalid data in rain gage file` (or in a time-series file)
means the engine's reader could not parse a line
([r/stormwater](https://www.reddit.com/r/stormwater/comments/2wd0w4/rainfall_data_set_swmm/)).
The file formats are UM §11.5. Common causes: a header line the format
does not allow, a blank line, a date in the wrong order, a station name
over 50 characters in a GHCN download
([USEPA #224](https://github.com/USEPA/Stormwater-Management-Model/issues/224)),
a comment line long enough to overflow the reader's buffer
([USEPA #165](https://github.com/USEPA/Stormwater-Management-Model/issues/165)),
or an interval that does not match the gage's `Interval`. Bring the data
into a `[TIMESERIES]` instead with Project → Time Series → Import…
([§8.5](08-project-dialogs.md)): it detects the delimiter and date format,
reads a GHCN-Daily station csv directly (and cuts the over-long station
name to SWMM's limit, saying so), and can make the gage with a matching
format and interval. The engine's own parser then reads the series with
the model and any problem is reported with a line number. Export to File…
goes the other way for series too long for the `.inp`, writing exactly the
file format the engine expects.

`WARNING 09: time series interval greater than recording interval` means
the gage's `Interval` is shorter than the series' spacing; set them equal.

## 16.11 Hotstart: ERROR 335, and when to regenerate

A hotstart file (`[FILES] SAVE HOTSTART x.hsf` then `USE HOTSTART x.hsf`)
carries the end state of one run into the start of another (UM §11.6).
It is binary, model-specific, and has no checker. `ERROR 335: error
reading hotstart file` means the file was made by a model with a
different object count or order — any add or delete since it was saved
([19483](https://www.openswmm.org/Topic/19483/how-to-use-hotstart-file),
[USEPA #214](https://github.com/USEPA/Stormwater-Management-Model/issues/214)).
Regenerate it after every structural edit; there is no way to inspect it.
StormSewer does not manage hotstart files; the `[FILES]` lines are text in
the Options dialog.

The common uses: a warm-up run of a day or two of dry weather to fill the
pipes and wet wells before the event, and a tidal outfall's initial
condition.

## 16.12 Nothing is on the map, or it is in the wrong place

- A node with no `[COORDINATES]` row is not drawn; the validator lists
  each one (`no [COORDINATES] row, so it cannot be drawn`) and the Project
  browser still counts it. Add the row in the `[COORDINATES]` attribute
  table, or select the object in the browser and place it.
- A model whose coordinates are all zero draws as a heap at the origin.
  Files exported from some tools do this.
- A backdrop image is placed by `[BACKDROP] DIMENSIONS`, which View →
  Backdrop → Load Image… fills from a world file beside the image and
  Georeference… edits by hand ([§3.9](03-map-and-tools.md)). An image
  that lands in the wrong place has a world file in other units or none;
  fix the extent in the dialog and *Write world file* so the next load is
  right. Rotation in a world file is ignored (SWMM cannot draw a rotated
  backdrop). Lengths are never measured from the picture; Compute Conduit
  Lengths measures from the drawn objects.
- `[MAP] DIMENSIONS` is the EPA GUI's map extent; StormSewer's view fits
  to the objects regardless, but the dialog (View → Map Dimensions…) sets
  it from the model or the backdrop so the EPA GUI opens the file sensibly,
  and `[MAP] Units` feeds the length factor.

## 16.13 Two engines disagree, or the design panel disagrees with SWMM

Different engine versions give different numbers, because bugs were fixed:
5.0 → 5.1 changed evaporation and infiltration accounting; 5.1 → 5.2
fixed elliptical-pipe geometry, changed the inlet handling, and more
([4769](https://www.openswmm.org/Topic/4769/differences-in-results-between-versions-of-swmm),
[USEPA #144](https://github.com/USEPA/Stormwater-Management-Model/issues/144);
EPA's own change lists are at epa.gov). A calibrated model is calibrated to
an engine. Results → Compare Engines… shows the difference object by
object, and every StormSewer run and report carries the engine version and
binary hash so a number can be tied to the binary that produced it.

The storm-sewer design panel and the SWMM run will not agree on peaks
either, and should not: one is a Rational peak at one intensity, the other
a routed hydrograph under a hyetograph. Typical reasons for a 2× gap
([r/Hydrology](https://www.reddit.com/r/Hydrology/comments/1nlc7bp/)):
the Rational Tc floor; a subcatchment width that gives a very different
time to peak; a `WET_STEP` or rain interval too coarse for a small
catchment; storage or surcharge attenuating the SWMM peak. Use the design
panel to size and check; use the run to see what the sized system does.

## 16.14 The model is slow

Runtime is the routing step count times the network size. Continuous
models with LID controls are the usual complaint
([5464](https://www.openswmm.org/Topic/5464/extremely-long-model-runtime));
`THREADS` in Options uses more cores for the routing; `REPORT_STEP` does
not change runtime but changes the `.out` size; `DRY_STEP` can be long.
The Stop item cannot interrupt a run in this build, so check the dates
before pressing F5.

## 16.15 Crashes, and where unsaved work is

The engine runs as a separate process: an engine crash cannot take the
editor down, and the run is reported as failed with the report's last
lines. If StormSewer itself stops, the model on disk is whatever you last
saved, plus the autosave snapshot: while the model is dirty its text is
written every *N* minutes (Run → Autosave every N min, default 2) to
`<model>.inp.autosave` beside the file, or to
`%APPDATA%\StormSewer\recovery` for a model that has no file yet. On the
next open of that file StormSewer asks *Recover unsaved changes?* with
the file's and the snapshot's timestamps: *Restore autosave* or *Discard
it, keep the file*. Saving or closing the model removes the snapshot.
`Ctrl+S` is still cheap and lossless.

## 16.16 Non-ASCII characters in the path

The stock EPA engine can fail to open a model whose path contains
characters outside ASCII (`õ ä ö ü`, CJK) — reported against the Windows
build and the Python bindings
([swmm-python #71](https://github.com/pyswmm/swmm-python/issues/71),
[#70](https://github.com/pyswmm/swmm-python/issues/70)). StormSewer
sidesteps it: a model on a non-ASCII path is run from a scratch copy under
`%TEMP%\StormSewer\run\<hash>\`, with its `[FILES]` and data files copied
beside it so relative paths still resolve, and the Run Status window says
`Ran a scratch copy` and names the path the engine read
([§9.2](09-run-and-engines.md)). The `.rpt` and `.out` are beside the copy,
and the run history, Compare Runs and the Python terminal use that
location. Only the engine sees the copy; your model stays where it is.

## 16.17 What the validator checks

Live, on every edit, shown under the map and in Project → Validate Model.
Errors refuse a run; warnings do not.

Errors: a link whose from/to node does not exist; a subcatchment whose
outlet is neither a node nor a subcatchment, or whose rain gage does not
exist; a duplicate name within a section, across the node sections or
across the link sections, including names that differ only by case; a
conduit without an `[XSECTIONS]` row; a row shorter than its section
allows; `[COORDINATES]`, `[VERTICES]`, `[POLYGONS]`, `[SYMBOLS]` or
`[XSECTIONS]` rows for objects that do not exist; a zero or negative
conduit length or subcatchment area; `REPORT_STEP` shorter than
`ROUTING_STEP`; an outfall with more than one link, or with an outgoing
link; a simulation that ends before it starts.

Warnings: a node without coordinates; more than one coordinate row for a
node; a node no link connects to; a rain gage no subcatchment uses; a
negative offset, or an `ELEVATION` offset below the node invert (the
engine silently uses 0 and raises the rim if it must — WARNING 03); a
subcatchment width of zero; a curve or series with no points; a junction
whose only outgoing links are weirs or orifices; `DRY_STEP` shorter than
`WET_STEP` (the engine raises it — WARNING 06).

It does not check hydraulics, units, or anything inside `[CONTROLS]`,
`[LID_CONTROLS]`, `[STREETS]` or the climate sections.
