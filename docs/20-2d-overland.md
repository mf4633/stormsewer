# 20. 2D overland flow and 1D-2D interfaces

SWMM routes water through a network of nodes and links. What happens when a
manhole surcharges and water runs down the street, pools in a car park, and
finds its way back into a downstream inlet is not something the 1D network
can tell you: the engine reports the flooded volume at the node and loses
it (or, with `Aponded`, stores it in a column of air). The **2D** menu adds a
surface to the model — a grid of cells on a digital elevation model (DEM) —
solves the shallow-water equations on it, and couples the two so that
surcharge leaves the network at the nodes and bank lines, flows over the
ground, and is captured again where the surface meets an inlet.

The equations, the numerical scheme, the wet/dry treatment and the tests
that show the solver reproduces known solutions are in
[20b](20b-2d-methods.md). This chapter is the user's side: what to load,
what to set, how to run, and what the map shows afterwards.

Everything 2D lives **outside the `.inp`**. The settings are kept in a
*sidecar* text file `<model>.2d` beside `<model>.inp`, and the results go to
`<model>.2d.out`. The `.inp` stays EPA's format, so the model still opens in
the EPA GUI and runs on any engine; the sidecar is simply ignored by
software that does not know it. StormSewer never writes anything 2D into the
`.inp`.

## 20.1 Workflow

1. **DEM.** Get a DEM of the site in the model's map units and coordinate
   system, as an ESRI ASCII grid (`.asc`) or a GeoTIFF (`.tif`). Chapter
   [19](19-gis.md) covers coordinate systems and where DEMs come from.
2. **Setup.** 2D → **2D Setup…**: point at the DEM, choose the cell size
   and window, roughness, rain on the grid, boundaries and time stepping.
   **Save** writes the sidecar.
3. **Interfaces.** 2D → **Interfaces…**: say which nodes exchange water
   with the surface (manhole, inlet, sealed), and draw bank lines along
   open channels. **Sources…** adds point inflows that are not nodes.
4. **Run.** 2D → **Run 2D Only** runs the surface by itself (rain on grid
   and sources only), or **Run Coupled (1D-2D)…** runs the network and the
   surface together.
5. **Results.** The depth, velocity and hazard grids are painted on the
   map under the network, driven by the same time slider as the SWMM
   results; the interface markers show which nodes are surcharging and
   which are capturing; the grids export as ASCII rasters for a GIS.

## 20.2 The 2D menu

| Item | What it does |
| --- | --- |
| **2D Setup…** | The setup window ([§20.3](#203-2d-setup)). |
| **Interfaces…** | Node interfaces and bank lines ([§20.4](#204-interfaces)). |
| **Sources…** | Point sources ([§20.5](#205-sources)). |
| **Run 2D Only** | Runs the surface alone ([§20.6](#206-running)). |
| **Run Coupled (1D-2D)…** | Picks the engine and the coupling mode, then runs both. |
| **Stop 2D** | Asks a running 2D or coupled run to stop at its next progress report. |
| **Load 2D Results** | Opens `<model>.2d.out` beside the model (a finished run loads it by itself). |
| **Export Max Depth Grid…** | Writes the run's maximum depth as an ESRI ASCII grid. |
| **Export Frame Grid…** | Writes the depth of the frame the overlay is showing. |
| **Export Hazard Grid…** | Writes the run's maximum depth × velocity. |
| **2D Legend** | Shows or hides the 2D legend in the map's corner. |
| **Show Interfaces** | Shows or hides the interface markers and bank lines on the map. |

Every item needs a model open. The run items need the model saved to a
file: the run reads `<model>.inp` from disk and writes `<model>.2d.out`
beside it. A model with unsaved changes is saved first, exactly as the
Run menu does before an ordinary run; an untitled model is refused with
"Save the model first". A model whose validation finds errors is refused
too, with the count, as Run → Run Model is.

## 20.3 2D Setup

The window's first line names the sidecar the settings go to. Nothing in
this window touches the `.inp`.

**DEM** — the path to the elevation grid. Type it, drop the file on the
window, or use **…** to browse. **Load DEM** reads it; once loaded the
window reports its size and cell, its bounds, the elevation range, and how
much of the model's drawn extent it covers ("covers the whole model
extent", a percentage, or "does NOT overlap the model: check the coordinate
system" — the usual first sign of a DEM in a different projection or
different units from the `.inp`'s coordinates). ESRI ASCII grids (`.asc`)
and GeoTIFFs (`.tif`, read by the GIS chapter's reader) are accepted; the
run reads the DEM with the same reader, so what loads here is what runs.

**Grid**

- **Resample to** — a cell size in map units. Off: the DEM's own cell. A
  coarser cell runs faster; the time step scales with the cell size and the
  cell count with its square, so halving the cell costs about eight times
  the run time.
- **Window** — restrict the grid to a rectangle of the DEM (x and y of two
  opposite corners). **From map view** fills the rectangle with the extent
  the map shows now. Off: the whole DEM.

**Roughness (Manning's n)** — one of:

- **Uniform** — a single **n** for the whole grid (0.05 by default, a
  reasonable rough street-and-yard value; paved surfaces are nearer 0.015,
  grass 0.03–0.05, brush 0.1).
- **Raster of n** — a grid of n values on the DEM's grid (or resampled to
  it).
- **Land-cover classes** — a land-cover raster of class codes plus a table
  of class → n. **Add class** adds a row; **−** removes one. A row whose
  class is not a whole number or whose n is not a number is refused on Save
  with its row number.

**Rain on grid** — **None** (water reaches the surface only through the
interfaces and sources), a **Rain gage** from the model's `[RAINGAGES]`
(its time series falls on every cell), or a **Constant** intensity in the
model's rain units (in/hr or mm/hr). Direct rainfall on the grid is the
choice when the surface itself is the catchment; with subcatchments already
draining to the nodes, leave it off or the rain is counted twice.

**Infiltration** from the surface — **None**, a **Constant loss** rate in
rain units, or **Horton** (f0, fc, k in 1/hr).

**Boundary** — how the edge of the grid behaves: **Closed** (a wall),
**Open** (free outflow at the local slope), or **Fixed head** (a receiving
water level; water flows out or in to hold it).

**Time**

- **Duration** — hours; off: the model's own simulation length.
- **Output step** — seconds between saved frames. Every frame stores depth
  and velocity for every cell, so 300 s on a 1000 × 1000 grid over six
  hours is about 900 MB; use a coarser step or a window for large grids.
- **Dry depth** — below this depth (in the DEM's units) a cell is dry: it
  neither flows nor is drawn. 0.003 by default.
- **Courant** — the Courant number of the adaptive time step (0.7 by
  default; lower is safer and slower).
- **Max dt** — an explicit cap on the time step, seconds; off: the CFL
  condition alone sets it.
- **Threads** — worker threads; 0 uses every core.

**Save** writes the sidecar and reports the path; **Revert** reads it
again, discarding the edits. An "● unsaved" mark shows while the draft
differs from the file. The same two buttons, acting on the same file, are
in the Interfaces and Sources windows.

## 20.4 Interfaces

The **2D Interfaces** window: where the 1D network and the 2D surface
exchange water.

The table has every node with coordinates: its name and type, **Ground**
(the DEM's elevation at the node once a DEM is loaded), **Rim** (the node's
invert plus its maximum depth, from the `.inp`), and the interface:

- **Manhole** — the default. Surcharge leaves the network when the node's
  head rises above ground; ponded water re-enters through the lid only when
  **Lid open** is ticked (a lid that has been lifted or a grated cover).
- **Inlet** — captures surface water by a weir/orifice with the given
  **Perimeter** (ft or m) and clear opening **area** (ft² or m²); the
  perimeter governs shallow capture, the area deep capture.
- **Sealed** — no exchange at all.

**Weir C** is the coefficient for surcharge and capture; **auto** uses the
default (0.6 of the unit-system constant), untick it to type one.

A ground elevation well below the rim means the DEM and the model disagree
about where the surface is, and the node will surcharge onto a surface
lower than its own rim. Check the DEM's datum and units before running.

**Selection** — pick a kind and **Apply to selection** to set every node
selected on the map at once. **Seal all outfalls** sets every outfall
Sealed; an outfall is where water leaves the model, not somewhere it
comes back onto the ground.

**Bank lines** couple an open channel to the cells beside it: water spills
over the crest when the channel's head exceeds it and flows back when the
surface is higher. **Draw bank line** starts the pick tool on the map: click
near the conduit to choose it (or select the conduit first), then click the
vertices along the bank; Enter or a double-click finishes, Escape cancels.
Each line then has its **Left**/**Right** bank (as seen looking
downstream), a **Crest** elevation (**auto** uses the DEM along the line),
a **Weir C**, its vertex count, and **Remove**. While the pick tool is on,
the ordinary map tools do not act; the wheel still zooms and the middle
button still pans.

Everything here edits the sidecar draft; **Save** writes it. The `.inp` is
untouched: the Unsaved mark in the status bar stays off.

## 20.5 Sources

The **2D Sources** window: point inflows onto the surface that are not network nodes — a culvert
outlet, a pump discharge, an upstream catchment represented by a
hydrograph. Each has a **Name**, a place (**X**, **Y**), and a **Series**:
a `[TIMESERIES]` in the model, in the model's flow units (Project → Time
Series adds one). Type the name and choose the series, then **Place
source** and click the map (Escape cancels); a name already in use gets the
next free number. The table edits the place and series in place; **Remove**
deletes a row.

## 20.6 Running

**Run 2D Only** runs the surface by itself: rain on the grid, infiltration
and sources apply; the interfaces feed from nothing because there is no
network run to feed them. It is the run for a pure overland-flow question
(where does the rain go on this DEM?) and the quick check that a setup is
sound before a coupled run.

**Run Coupled (1D-2D)…** opens a dialog with the **Engine** (the same
registry as the Run menu) and the mode:

- **Tight** — the engine is driven step by step through the engine bridge;
  every *sync* interval the node heads are read and the exchange flows set
  as lateral inflows, so surcharge and re-entry are resolved within the
  same time step. This is the mode to use when the surface feeds the
  network back — inlets capturing street flow, ponds draining into
  manholes. It needs the bridge helper `stormsewer-swmm-bridge32.exe`
  (chapter [23](23-live-runs.md) says where it is looked for, and
  `scripts\build-bridge.ps1` builds it in a source tree) and a 32-bit
  engine with its `swmm5.dll` beside `runswmm.exe`, as EPA SWMM 5.2's
  Windows install has; without them the option is greyed with "Tight
  coupling needs the engine bridge; iterative is available". The engine
  runs to the end of its own simulation after the surface finishes, so
  the report is complete; its continuity errors, when above 5 %, are
  among the run's warnings.
- **Iterative** — `runswmm` runs the whole 1D model, its node overflow
  hydrographs drive the surface, the surface's captured flows are written
  back as `[INFLOWS]` time series into a scratch copy of the model, and the
  pair is re-run until the exchanged volume changes by less than the
  tolerance or the pass limit is reached. It works with any engine and no
  bridge. It converges quickly when the surface mostly *receives* water
  (surcharge that ponds and drains away over the surface) and slowly, or
  not at all, when the surface and the network trade the same water back
  and forth, which is the tight mode's job. The scratch model, report and
  results are in the same per-model run folder as ordinary runs
  (chapter [9](09-run-and-engines.md)); the model's own `.inp` is never
  rewritten.

**Run** starts; **Cancel** closes the dialog.

Both runs open the **Run 2D** window and report as they go: simulation
time against the duration as a progress bar, the current **Time step**,
**Wet cells**, **Surface volume**, the running **Mass error** (coloured
green, amber and red at the same 1 % and 10 % thresholds as the Run Status
window's continuity errors), and the **Elapsed** wall clock. **Stop 2D**
(also in the menu) raises a flag the solver checks at every progress
report. When a run finishes the window shows the summary — results path,
final mass error, steps and frames, elapsed, peak depth, largest wet area,
the inflow, outflow, infiltrated and stored volumes, and for a coupled run
the iterations, the surcharged and captured volumes and the 1D run's files
— plus every warning; **Load 2D Results** brings the results onto the map
(a finished run does this by itself). Anything that stops a run from
starting — no DEM, no saved model, validation errors, a DEM that does not
cover the model — is written on the window's status line; the window
never disappears on an error. **Close** closes it.

The run happens on its own thread; the editor stays live and the map can
be panned while it goes.

## 20.7 Results on the map

With results loaded, the map paints a grid over the DEM's extent under the
network. The **Layers** tab's **2D overland** section controls it:

- **2D results** — the overlay on or off.
- The mode: **Depth**, **Velocity** or **Hazard (depth × velocity)** in the
  current frame; **Max depth**, **Max velocity**, **Max hazard** and
  **Arrival time** over the whole run.
- **opacity**.
- **2D frame** — a slider over the saved frames, shown when the frame is
  not already set by the SWMM time slider (below).
- **2D Legend** and **Show Interfaces**, the same toggles as the menu.

Cells below the dry depth are transparent, so the DEM, backdrop and GIS
layers show through the dry ground. The colour scale of a frame mode is
the run's maximum of that quantity, so the same depth is the same blue in
every frame; the legend in the map's lower-right corner gives the scale,
the unit (ft or m, and the corresponding velocity and hazard units, after
the model's flow units) and the frame's time. Move the pointer over the
grid and the depth, velocity and hazard under it are written beside the
cursor (the static modes read out their own grid).

**The time slider.** When the SWMM results are loaded and the Results view
is showing an instant (the period slider, Play, ◀ ▶), the 2D overlay shows
the 2D frame nearest that instant: SWMM's reporting period *p* is at
(p + 1) × report step seconds, and the nearest saved 2D frame by time is
chosen. Playback of the SWMM results therefore animates the surface as
well. With the SWMM results showing the run's peaks, or with no SWMM
results loaded, the **2D frame** slider in the Layers tab chooses the frame
instead.

**Interface markers** (**Show Interfaces**): a circle at every manhole
interface (a dot inside it when the lid is open), a square at every inlet,
a cross at a sealed node, and the bank lines as thick orange polylines
labelled with their link and bank. With results loaded the node markers are
coloured by the exchange in the current frame: red when water is leaving
the network onto the surface, blue when the surface is being captured, grey
when nothing is passing; the whole-run modes colour by the net exchange.
Sources are purple triangles.

## 20.8 Exporting

The three export items write ESRI ASCII grids (`.asc`) with the results'
geometry and `-9999` for no-data, which any GIS opens directly: the run's
**maximum depth**, the **depth of the current frame** (whichever frame the
overlay is showing), and the **maximum hazard**. Arrival time and maximum
velocity are on the map but not yet exported.

## 20.9 The sidecar file

`<model>.2d` is a plain text file beside the model holding everything in
the three windows: the DEM path and grid, roughness, rain, infiltration,
boundary, time stepping, the node interfaces, the bank lines and their
vertices, the sources and the sealed list. Its format (sections, keywords
and an example) is in [20b](20b-2d-methods.md), §5.1. Opening a model reads its sidecar; a model without one starts
from the defaults (a manhole at every node, uniform n = 0.05, no rain on
grid, closed boundary). A different model gets its own sidecar; the
results and DEM loaded for one model are dropped when another is opened.

## 20.10 Limitations

- DEMs are read from ESRI ASCII grids and GeoTIFFs; other raster formats
  have to be converted first.
- Cells are square, as every DEM's are.
- The overlay is sampled down to at most 4096 cells along either side for
  drawing (the results themselves keep every cell); exports are full
  resolution.
- Tight coupling needs the 32-bit engine bridge (EPA's Windows engine is
  32-bit and StormSewer 64-bit; chapter [23](23-live-runs.md) explains
  the bridge). Without it the coupled dialog offers iterative coupling
  only.
- The results file stores depth and the two velocity components per cell
  per frame; there is no compression, so budget the output step for the
  grid size.
- A node's **Rim** in the Interfaces table is `Elevation + MaxDepth`; for
  a junction with MaxDepth 0 (SWMM then uses the crown of its highest
  pipe) that is the invert. The exchange compares the network's head with
  the DEM's **Ground**, not the rim, so what matters is that the DEM is
  right at the node: a DEM that puts the ground at such a junction's
  invert makes it surcharge whenever any water flows through it.
- Iterative coupling feeds the surface's *captured* flows back to the
  network, but surcharge worked out from the manhole formula (as opposed
  to the engine's own reported flooding) is not taken out of the
  network's run, so where nodes surcharge without the engine flooding
  them the iterative mode counts that water on both sides. In tight mode
  the same surcharge is withdrawn from the node as a negative inflow,
  which the engine cannot always supply: watch the run's warnings for an
  engine continuity error.

## 20.11 Tutorial: rain on the Site Drainage model

This tutorial is the one the editor's end-to-end tests run, step by step,
through the same menus and buttons (`app/src/swmm_twod_e2e_tests.rs`); the
numbers quoted are from those runs with EPA SWMM 5.2.4.

The EPA Site Drainage sample (`Site_Drainage_Model.inp`, in the EPA
install's Samples folder) is a small development drained by swales,
culverts and street gutters to one outfall, with coordinates in feet and
junction inverts from 4963 to 4973 ft. Copy it to a folder of its own as
`site.inp`.

1. **Make a DEM.** The model has no DEM, so build one from the network:
   interpolate a surface through the ground at every node (its
   `Elevation + MaxDepth`, or for the junctions with MaxDepth 0 the
   invert plus the crown of the highest conduit that meets it, SWMM's own
   rule for a node's full depth) at a 20 ft cell, with a cell of margin
   round the model's drawn extent. Any GIS does this: inverse-distance
   weighting from a point layer of the nodes (File → **Export GIS
   Layers…**), saved as `.asc` or `.tif`. Save it as `ground.asc` beside
   `site.inp`.

2. **Setup.** Open `site.inp`, 2D → **2D Setup…**, type the DEM's path
   (or drop the file on the window) and **Load DEM**: it covers the whole
   model extent. Set rain on grid to **Constant**, 3 in/hr; boundary
   **Open**; tick **Duration** and set 0.333 h (20 minutes); output step
   300 s. Leave roughness uniform 0.05 and infiltration **None**.
   **Save**: the status line reads "Saved …\site.2d".

3. **Run 2D Only.** 2D → **Run 2D Only**. The **Run 2D** window counts to
   00:20:00 in about a second and reports 5 frames (0, 5, 10, 15 and 20
   minutes), mass error +0.000 %, inflow 210,267 ft³ (3 in/hr for a third
   of an hour over the grid), outflow 32,993 ft³ over the open edge, and a
   peak depth of about 1.4 ft in the low spots between the nodes. The
   results load by themselves; the status bar says "2D run finished".

4. **Look and export.** The map shows the depth over the DEM. In the
   **Layers** tab's **2D overland** section drag **2D frame** to the last
   frame, or switch to **Max depth**. 2D → **Export Max Depth Grid…**
   writes `site_max_depth.asc`, a grid the size of the DEM with the
   deepest water of the run in each cell; **Export Hazard Grid…** writes
   the depth × velocity maximum. 2D → **Load 2D Results** re-reads the
   file at any time.

5. **Inlets.** Now let the network take the water. 2D → **Interfaces…**,
   select everything on the map (Ctrl+A), choose **Inlet** and **Apply to
   selection** (10 ft perimeter and 2 ft² opening unless you change them
   in the table), then **Seal all outfalls**. Back in 2D Setup, set the
   duration to 0.5 h and **Save**.

6. **Coupled, iterative.** 2D → **Run Coupled (1D-2D)…**, choose the
   engine, **Iterative**, up to 3 passes, tolerance 0.020, **Run**. The
   run takes 2 passes (the second changes the exchanged volume by less
   than 2 %), the surface mass error stays at +0.000 %, and the inlets
   capture 23,276 ft³ of the 315,400 ft³ that fell on the grid in the half
   hour; nothing surcharges. The summary's **Surcharged / captured** and
   **1D run** rows show the volumes and the scratch model's files.

7. **Coupled, tight.** The same dialog, **Tight**, sync every 30 s,
   **Run** (it needs the bridge, see [§20.6](#206-running)). The engine is
   stepped alongside the surface and the captured water enters the
   network as it is captured: 32,954 ft³ captured, nothing surcharged,
   surface mass error +0.000 %, no engine continuity warning. Tight
   captures more than iterative here because the network heads it sees
   are the ones the capture itself produces, not a previous pass's.

**When the network surcharges.** The sample's pipes carry the storm, so
nothing leaves the network above. On a stressed copy (every conduit
narrowed 2.5 times, the same kind of DEM through the rims, manholes at
the junctions) the engine alone reports a flooding loss of 0.040 MG
(about 5,350 ft³). Coupled, that water goes onto the surface instead:
5,229 ft³ iterative, 5,360 ft³ tight. With every lid ticked **Lid open**,
tight coupling captured 1,454 ft³ of it back, which the engine booked as
0.011 MG of external inflow. The surface balance was 0.0000 % in every
case.

**Stop 2D** ends a run early at its next progress report. Try it with a
ten-hour duration: the summary then carries "2D run stopped by the caller
at … s" among its warnings, and the frames written so far load as usual.
