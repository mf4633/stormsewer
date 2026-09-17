# 2. Tutorials

Ten exercises, each building on the last, on one model: the EPA Detention
Pond sample (`docs/datasets/Detention_Pond_Model.inp`). This follows the
shape of EPA's *SWMM Applications Manual*, which works nine examples on one
watershed. Copy the file somewhere you can write to before you start; the
tutorials save to it.

What you should already have: StormSewer installed, EPA SWMM 5.2 installed,
and [chapter 1](01-start-here.md) read. Every step below names the menu item
or key it uses, and every number is one you can check in the file.

## 2.1 Draw a network

The pond model drains eight subcatchments through open channels and a few
pipes into storage unit `SU1`. You will add one junction upstream of `J1` and
a pipe to connect it.

1. Open the model. Press `F` to fit it to the window.
2. Press `J` (the **Junction** tool) and click on the map a short way
   north-west of `J1`. A junction appears with the next free name, `J12`
   is taken so it is `J13`. The status bar says `Added junction J13` and the
   Edit menu says `Undo Add junction J13`.
3. With `J13` selected, set **Elevation** to `4976` in the Properties sheet
   and press Enter. Leave **MaxDepth** at `0`: the engine then takes the
   node's depth from the highest connecting conduit crown (UM §3.3, and
   [chapter 15](15-methods.md) for how StormSewer draws it).
4. Press `C` (the **Conduit** tool). Click `J13`, then click `J1`. The link
   `C12` appears. Its `[CONDUITS]` row has the length you drew, in map units,
   Manning's n `0.01`, and offsets of `0`; its `[XSECTIONS]` row is
   `CIRCULAR 1`. Click once between the nodes before clicking `J1` if you
   want a vertex; `Esc` abandons the link.
5. Set **Length** to `200`, **Roughness** to `0.016`, and under the
   cross-section set **Geom1** to `2`. Each commit is one undo step.
6. Press `Ctrl+Z` three times and watch the fields revert in order; press
   `Ctrl+Y` three times to put them back.
7. Read the line under the map. It still says `Validation: no findings`. Now
   try to break it: select `C12`, and in the sheet change **ToNode** to `J99`.
   The line becomes `Validation: 1 error`. Click it, then click the finding
   (`[CONDUITS] C12: to node "J99" does not exist`) — the conduit is selected
   and the map zooms to it. Fix it back to `J1`.

What happened in the file: three rows were added (`[JUNCTIONS] J13`,
`[COORDINATES] J13`, `[CONDUITS] C12`, `[XSECTIONS] C12`), padded to the
columns of each section's `;;----` ruler. Nothing else in the file changed.
Open it in a text editor and see.

**Rubber-band, move, delete.** With the **Select** tool (`S`) drag a box
around `J13` and `C12`; both select. Drag either to move both — one undo
step however many pointer moves it took. Press `Delete` on `J13` alone: a
dialog, *Delete takes more with it*, says the conduit goes too. Choose *Keep
them* for now.

## 2.2 Subcatchments and a rain gage

1. Press `A` (**Subcatch**). Click four corners around `J13` and click the
   first corner again (or press Enter, or double-click) to close. The
   subcatchment `S9` appears: 5 acres, 25 % impervious, width 500, slope
   0.5 %, draining to the node nearest its centroid, which should be `J13`.
   It takes the model's first rain gage, `RainGage`.
2. Fix the numbers. Select `S9`; set **Area** to `2.1`, **PctImperv** to
   `40`, **Width** to `600`. Width is the overland flow width; see
   [chapter 16](16-troubleshooting.md) for how to estimate it.
3. Every subcatchment needs a `[SUBAREAS]` row and an `[INFILTRATION]` row.
   `S9` got both, with the EPA GUI's defaults and Horton parameters because
   the model's `[OPTIONS] INFILTRATION` is `HORTON`. They are on the same
   sheet, under the `[SUBCATCHMENTS]` fields.
4. Add a second gage to see how gages work: press `R` (**Gage**) and click
   west of the pre-development site. `R1` appears, reading the model's first
   time series, `2-yr`, at a 1-hour interval. A gage is a point symbol that
   does nothing until a subcatchment names it; set `S9`'s **RainGage** to
   `R1`.
5. Open the subcatchment table: View → Attribute Table… with `S9` selected
   (or the **Table** button in the Project tab with Subcatchments chosen).
   Sort by **Width** by clicking the header. Type `S` in the filter box to see
   only the S-names. Use **Replace in column**: choose `PctSlope`, enter
   `2`, and click **Set all** — every filtered row's slope becomes 2, one
   undo step (`Set PctSlope on 9 rows`). Undo it.

## 2.3 A design storm

The pond model's gage reads the `2-yr` series. You will build a 100-year
24-hour storm and give it its own gage.

1. Project → Design Storm…. Choose the **Distribution**: *NRCS (SCS)
   24-hour Type I / IA / II / III*. Pick Type II and enter the 100-year
   24-hour depth for the site (say `6.0` in — NOAA Atlas 14 gives it for
   your point). **Time step (min)** `5`. The plot shows the hyetograph and
   the cumulative curve; the table lists every interval; the line under
   them reads `Total 6.000 in over 1440 min in 288 steps of 5 min; peak … in/h`.
2. Name the **Series** `TS_TypeII_6in` and the **Gage** `RG_Design`; leave
   **Series format** `INTENSITY`. Tick **Assign to every subcatchment** so
   the eight subcatchments and your `S9` switch to the new gage, and **Place
   the gage symbol on the map**. Click **Add to Model**. That is one undo
   step: a `[TIMESERIES]`, a `[RAINGAGES]` row, a `[SYMBOLS]` row, and the
   subcatchments' `RainGage` column.
3. The other distributions in the same dialog: the NRCS NOAA Atlas 14
   regional 24-hour curves (regions A–D, from NEH 630 Chapter 4); the
   alternating-block method from an IDF curve `i = a/(t+b)^c` or from a
   pasted NOAA Atlas 14 PFDS csv row for the return period; Chicago
   (Keifer–Chu) from an IDF curve with a peak position `r`; and a uniform
   storm. Each names its source on the dialog; the equations are in
   [§15.7](15-methods.md). Depths are in the model's rain unit (inches for a
   CFS model); nothing is converted.
4. Observed rain instead: Project → Time Series → Import…. **Load file…**
   or paste a delimited table with a date, a time and one value column per
   series — a hundred radar cells is one paste — or a NOAA GHCN-Daily
   station csv (`STATION, DATE, PRCP` in tenths of a millimetre, imported as
   daily `VOLUME`). Delimiters and date formats are detected. Tick **Also
   add a rain gage per series** and **place on the map** if you want gages
   made; click **Import**. One undo step.
5. A series too long for the `.inp`: Project → Time Series → Export to
   File… writes it as SWMM's external time-series file (`date time value`
   lines) and, if you ask, replaces its rows with a `FILE` reference the
   engine reads at run time. [Chapter 16](16-troubleshooting.md) covers
   ERROR 363 when the engine cannot read such a file.

The gage's **Format** and **Interval** must match the series: INTENSITY at
the storm's time step for a design storm, VOLUME at 24 h for GHCN daily
totals. The Design Storm dialog writes them consistently; check them in
Project → Rain Gages… after an import.

## 2.4 Run, and read the run status

1. Save (`Ctrl+S`). Run → Check Model… first: the pre-run QA pass lists
   the validator's findings grouped as *Errors — the engine will refuse or
   misread the model* and *Warnings — the model runs, but check these*,
   each clickable to select the object. `S9` from §2.2 gives none; a
   subcatchment with `Width 0` would.
2. Press `F5`. The toolbar spinner turns; the left panel says `running…`.
   (Had there been warnings, a *Warnings before running* prompt would show
   them once with **Run anyway**; errors refuse the run outright.)
3. When it stops the **Run Status** window opens. Read it top to bottom:
   - **Engine** — `EPA SWMM 5.2.4` and the SHA-256 of the `runswmm`
     executable; **Elapsed**; **Model read** — the file the engine actually
     read, and `Ran a scratch copy` when the path forced one.
   - **Result** — `Run finished and wrote results`, or `Run failed: …`.
   - **Continuity** — each section's error with a colour: green below 1 %,
     amber 1–10 %, red above 10 % (absolute). The unmodified sample gives
     −0.023 % for runoff and +0.092 % for routing, both green.
   - **Highest continuity errors by node**, **Highest flow instability
     indexes**, **Time-step critical elements**, **Most frequent
     non-converging nodes** and the **Routing time step summary** — every
     entry the report carries, not the GUI's five; each names an object you
     can click.
   - **Warnings** and **Errors** — every `WARNING nn` / `ERROR nnn` line
     with what the code means, the usual cause and the fix, from the
     built-in index ([Appendix C](A3-error-codes.md)), and the object it
     names selected on click.
   - **Copy summary** puts a plain-text version on the clipboard for an
     e-mail or a submittal note. Results → Run Status… reopens the window.
4. The left panel's **Last run** block keeps the short form — `worst
   continuity +0.092% (Flow Routing Continuity)` — and **Results** says
   `144 periods every 300 s`, `14 nodes, 14 links, CFS`, and the start date.
5. Click **Tables**. Every summary table the engine printed is a grid:
   Subcatchment Runoff Summary, Node Depth Summary, Node Inflow Summary, Node
   Flooding Summary (or its sentence, "No nodes were flooded."), Storage
   Volume Summary, Outfall Loading Summary, Link Flow Summary, Flow
   Classification Summary, Conduit Surcharge Summary, and the Routing Time
   Step Summary as Item/Value rows. A table's title bar shows the units the
   engine printed, which change with `FLOW_UNITS`.

What "success" means here: `runswmm` exits 0 whether the run worked or not,
so StormSewer decides by reading the report. A run succeeded when the report
has no `ERROR` line and a non-empty `.out` exists. Everything else is a
failure and says why in red.

**A run that is refused.** Make an error on purpose: set `C12`'s **ToNode**
to `J99` again and press `F5`. A dialog, *The model has errors*, lists the
validation errors and refuses to run; click a line to select the object,
*Show in findings list* to keep the list open, or *Dismiss*. StormSewer
refuses only what the engine would refuse or misread (undefined references,
duplicate names, rows too short to parse); it does not second-guess the
hydraulics. Fix the node and run again.

**Stop.** Run → Stop is present but the engine runs to completion; it cannot
be interrupted in this build. The pond model is quick; a large continuous
model is not, so check the options before running.

## 2.5 A profile with HGL, rim and crown

1. Click `J1`, Shift-click `J_out`. View → Profile from Selection. (Two other
   routes: View → Pick Profile Path… then click a start and an end node on
   the map; or right-click a node → Profile from Here… and click the end
   node.)
2. The profile view opens: ground line through the node rims, each conduit's
   invert and crown, a shaft at each node from invert to rim, and the HGL at
   the reporting period on the Time slider. The path is found through the
   links between the two nodes; if there is none the view says so.
3. Tick **Max HGL**. A second line is the maximum head at every node over
   the whole run, read from the `.out`. This is the envelope; the animated
   line is a snapshot at one report step and can miss the true peak.
4. Set **V. exag.** to stretch elevation. **Swap** reverses the path. The
   **From** / **To** dropdowns choose another path without touching the map.
5. **Export PNG** writes the view; **Export CSV** writes the station table:
   one row per node with its kind, station, invert, rim, HGL and max HGL,
   and the invert and crown of the link entering and the link leaving it.

Read the elevations the engine's way: a link end's invert is the node invert
plus the link's offset (`LINK_OFFSETS DEPTH`), or the absolute offset
elevation converted to a depth (`LINK_OFFSETS ELEVATION`); a node with
`MaxDepth 0` takes its rim from the highest connecting crown. Conduit `C2`
in this model has an outlet offset of 4 ft above `J11`'s invert; look at the
drop.

## 2.6 Results on the map

1. Click **Results** in the left panel (or View → Results). The map is
   repainted with nodes coloured by **Depth** and links by **Flow**.
2. Change the variables with the **Nodes** and **Links** pickers: Depth,
   Head, Volume, Lateral inflow, Total inflow, Flooding for nodes; Flow,
   Depth, Velocity, Volume, Capacity for links; plus each pollutant when the
   model has any.
3. The legend has five classes. **Auto breaks** recomputes them from the
   data, **Equal** or **Quantile**; untick it and edit the **Node breaks** and
   **Link breaks** by hand.
4. **Width by flow** scales link width; **Arrows** draws flow direction,
   reversing where the flow is negative.
5. **Play** animates through the reporting periods at the **speed** factor;
   `◀` and `▶` step; **Peaks** colours by each object's run maximum instead
   of a period, and the legend says `showing run maxima`.
6. **Query**: choose nodes or links, a variable, a comparison and a value.
   The matching objects are highlighted and counted (`3 match`). Use it to
   find every node whose flooding exceeds zero, or every link over 90 %
   capacity.
7. Go back to **Map**. The colouring stays as a layer on the editing map;
   the **Layers** tab's results entries turn it off.

## 2.7 Plots and tables

1. Click **Plots**. Pick a target (node or link), an object and a variable in
   the strip and add the series; or select `C11` on the map and click **Add
   selected**. Several series share one plot.
2. Add `SU1` **Depth** and `O1` **Flow**. The cursor readout follows the
   pointer. The statistics block under the plot gives each series' maximum,
   minimum, mean and the top-N peaks with their times.
3. The scatter mode plots one variable against another for the same object
   — `SU1` depth against `W1` flow gives the weir's rating as the engine
   computed it.
4. **Export CSV** writes every plotted series with a shared time column;
   **Export PNG** writes the plot as drawn.
5. Click **Tables**. Sort by any column; filter by name; click a row to select
   the object on the map. Each table has its own CSV export.

## 2.8 Storm-sewer design and auto-size on the SWMM network

The storm-sewer engine sizes gravity pipes by the Rational method. Running
it on a SWMM model needs a mapping, and the mapping is where the assumptions
are; the panel shows them rather than hiding them.

1. Tools → Storm Sewer Design → Design Panel….
2. The **design basis** at the top is a project of its own: IDF `a`, `b`,
   `c`, the return period, minimum Tc, junction K, minimum slope, and an
   optional tailwater. It is seeded from the storm-sewer workspace's project
   the first time the panel opens (*Copy from Storm Sewer workspace* does it
   again). Set `a = 60`, `b = 10`, `c = 0.8`, 10-year, Min Tc 10 — the
   values `VALIDATION.md` works by hand.
3. Click **Analyse**. The tabs fill:
   - **Pipes** — each mapped conduit's slope, ΣCA, Tc, intensity, design Q,
     capacity, velocity, percent full, HGL at both ends.
   - **Nodes** — invert, rim, HGL, freeboard.
   - **Sizing** — the smallest catalog pipe that carries each design flow
     within the criteria, beside the current size.
   - **Inlets** — HEC-22 interception for nodes marked as inlets by
     `[INLET_USAGE]` rows or by a subcatchment draining to them.
   - **Skipped** — everything the mapping could not carry, with a reason. In
     the pond model that is most of the network: the trapezoidal channels
     (`C1`, `C2`, `C4`–`C6`, `C8`–`C10`) are not pipes, the storage unit is
     treated as a junction with a note, and the orifice and weir are not
     conduits. Only `C3`, `C7`, `C11`, `C_out` and your `C12` are sized.
   - **Findings** — the design review: velocity outside 2–10 ft/s, more than
     85 % full, cover under 1 ft, slope under 0.0005, a pipe smaller than the
     one feeding it, freeboard under 0.5 ft.
   - **Notes** — the mapping's assumptions for this model, in words.
4. Click **Auto-size…**. A preview lists each conduit whose recommended size
   differs from its current size: shape, design Q, before, after, note. Click
   **Apply — Auto-size N conduits**. That writes `Geom1` (and `Geom2` for a
   box) in `[XSECTIONS]` as one undo step. `Ctrl+Z` takes it all back.
5. **Report HTML…** and **Report PDF…** write the storm-sewer design report
   for the mapped network: schedules, plan schematic, profile.

Read [chapter 12](12-design-panel.md) for the mapping rules and
[chapter 15](15-methods.md) for the equations before you put a design
schedule from a SWMM model in a submittal. The two engines answer different
questions: the Rational pass gives a peak for sizing; the SWMM run gives a
hydrograph and a routed HGL. Run both.

## 2.9 Compare two runs, or two engines

Every finished run is copied to `%TEMP%\StormSewer\runs\<n>\` with the
engine's id, version and binary hash, the SHA-256 of the `.inp` text, and the
elapsed time. The last ten are kept.

1. You have at least two runs now (2.4 and the auto-sized model). Results →
   Compare Runs…. Pick run **A** and run **B** from the lists; `⇄` swaps
   them.
2. Choose what to compare: Max depth, Max total inflow or Max flooding for
   nodes; Max flow, Max velocity or Max capacity (fraction full) for links.
   Sort by `|Δ|`, by percent, or by name. Click a row to select that object
   on the map. **Export CSV…** writes the table.
3. Results → Compare Engines… needs two registered engines (say 5.1.015 and
   5.2.4). Pick the second engine and click **Run both**: the current model
   runs on each in turn on a worker thread, both runs are recorded, and the
   comparison opens. This is how you find out whether an engine update moved
   your calibrated peaks; [chapter 16](16-troubleshooting.md) says what to
   expect.

## 2.10 The model report, and the Python terminal

**Model report.** Results → Model Report… builds one document about the open
model and its last run: title and notes; engine version and binary hash,
model name and text hash; the options that decide the answer; the inventory;
the engine's continuity errors; every summary table it printed; the editor's
validation findings; a profile station table; and the exact `.inp` text as
an appendix. Give it a **Title** and **Notes**, click **Rebuild** after
changing anything, then **Save HTML…** or **Save PDF…**; *Open after saving*
launches the viewer. The report says which binary produced the numbers so a
reviewer can reproduce them.

**Python terminal.** Tools → Python Terminal…. The kernel starts your own
`python` with three names bound: `model`, `out` and `rpt`, each the path of
the current model, results file and report as a string (or `None` before a
run). Type a block and click **Run** (or `Ctrl+Enter`):

```python
import swmmio
m = swmmio.Model(model)
m.inp.conduits.head()
```

Anything your interpreter has — numpy, pandas, swmmio, pyswmm — is available.
**Restart Kernel** starts a fresh namespace bound to the current run; **Clear
Output** empties the transcript. [Chapter 18](18-python-cookbook.md) has ten
recipes, starting with reading `out` without any third-party package.

## Where to go next

- The panes, one chapter each: [3](03-map-and-tools.md) to [14](14-preferences.md).
- The equations StormSewer computes itself: [15](15-methods.md).
- When a run misbehaves: [16](16-troubleshooting.md).
