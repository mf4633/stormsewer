# StormSewer v0.10.0

**StormSewer is now a full editor for EPA SWMM models, with 2D overland flow
coupled to the network.** Open an `.inp`, draw and edit it on the map with undo
on every gesture, run it on the EPA engine you already have, and put the water
that leaves the pipes onto a ground surface. It is free, GPL-licensed, and the
engine is EPA's own, unmodified, stamped with its version and hash on every run.

The manual is at <https://mf4633.github.io/stormsewer/manual/>. Every window and
menu item in the SWMM editor is documented there, and a test fails the build
when one is not.

## New

- **The SWMM model editor.** Map, property sheet, attribute tables, layers,
  project browser and every Project dialog, all editing one lossless document:
  bytes you did not change are written back exactly as they were read.
  Every map gesture, field edit, dialog OK, import and auto-size is one
  `Ctrl+Z`. Run Status shows the engine, its SHA-256, the continuity errors
  and each warning explained. Results colour the map; plots, the `.rpt`
  tables and a long-section profile with the HGL and its maximum envelope
  read the same run. The storm-sewer design panel (Rational, Manning,
  standard-step HGL, HEC-22 inlets, auto-size) runs on the SWMM network.
  Chapters 1–18.
- **2D overland flow** on a DEM: the local-inertial shallow-water scheme
  (Bates et al. 2010, with the de Almeida et al. 2012 stabilisation), and a
  full-dynamic HLL option. Rain on the grid, infiltration, roughness grids,
  open, closed and fixed-head edges. Checked against lake-at-rest,
  Manning uniform flow, the Ritter dam break, mass conservation and symmetry;
  chapter 20b gives the equations and the numbers each test asserts.
- **1D-2D interfaces** at manholes, inlets, channel banks and outfalls.
  Iterative coupling works with any installed engine. Tight coupling steps
  EPA's `swmm5.dll` in lockstep with the surface through a small 32-bit
  helper, `stormsewer-swmm-bridge32.exe`, installed beside the app. On a
  flooding test model the water put on the surface matches the engine's own
  flooding loss, the water returned matches its external inflow, and the
  surface mass balance closes to 0.0000 %. Chapter 20.
- **Stop works, and results update during a run.** Stop kills the engine and
  keeps the partial report and results. The Live Results window follows the
  growing results file. Chapter 23.
- **GIS.** Shapefile and GeoJSON import with field mapping (one undo step),
  export with run peaks, `.prj` coordinate systems with every State Plane
  1983 zone and UTM, GeoTIFF and ASCII DEMs, hillshade, and Set Ground From
  DEM. Every State Plane zone was checked against PROJ 9.5.1. Chapter 19.
- **Dialogs for the rest of the model**: LID controls and usage, aquifers,
  groundwater, snow packs, buildup, washoff, land-use coverages, initial
  loadings, treatment and RDII. Pressing OK without an edit leaves the EPA
  sample models byte for byte. Chapter 21.
- **Scenarios and calibration.** Named edit sets on a base model, run as a
  batch and compared. Observed data, NSE, KGE, PBIAS and RSR with the
  Moriasi et al. (2007) ratings, sensitivity screening, and the DDS
  optimiser (Tolson and Shoemaker 2007). Apply Best is one undo step.
  Chapter 22.

## Changed

- **A SWMM node's rim now follows EPA's full-depth rule** in the profile, the
  design panel and the 2D setup: `MaxDepth`, raised to the crown of every link
  that meets the node, except at storage units (EPA SWMM 5.2 `link.c`,
  `link_validate`). Before, a written `MaxDepth` below a pipe's crown was
  used as written. Design results on such nodes can change; they now agree
  with the engine. The storm-sewer workspace is not affected.

## Unchanged

- The storm-sewer engine's hydraulics, including Manning's K of 1.486.
  Sealed sheets produced with v0.9.8 reproduce here.
- **Results are in U.S. customary units only.** SI is still input-only; do
  not report StormSewer results in SI.

## Not in this release

2D results are checked against benchmarks and against the engine's own totals,
not yet against a real project. There is no mesh refinement around structures,
no buildings except as the DEM shows them, no evaporation on the 2D surface,
and no volume cap on bank-line exchange. Scenario batch runs go one at a time.
