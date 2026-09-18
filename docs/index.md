# StormSewer manual

StormSewer is a free, GPL-licensed editor for EPA SWMM 5 models, with a
storm-sewer design tool (Rational method, Manning, standard-step HGL, HEC-22
inlets, auto-sizing) built in. It draws the model on a map, edits it in a
property sheet and attribute tables, runs it on the unmodified EPA engine you
already have installed, and brings the results back onto the same map.

This manual is written for the engineer who has to defend the numbers. Every
claim in it can be checked against the source code, and where the software
does not do something yet, the manual says so.

## Contents

**Part 1 — Start here**

- [1. Start here](01-start-here.md) — install, open a model in fifteen minutes,
  the workspace tour, undo, saving, and what "lossless" means.

**Part 2 — Tutorials**

- [2. Tutorials](02-tutorials.md) — ten cumulative exercises on the EPA
  Detention Pond model: draw a network, add subcatchments and a rain gage,
  build a design storm, run and read the status, profile, results on the map,
  plots and tables, storm-sewer design and auto-size, compare two runs, the
  model report, and the Python terminal.

**Part 3 — Panes and tools**

- [3. Map and tools](03-map-and-tools.md)
- [4. Project browser](04-project-browser.md)
- [5. Property sheet](05-property-sheet.md)
- [6. Attribute tables](06-attribute-tables.md)
- [7. Layers](07-layers.md)
- [8. Project dialogs](08-project-dialogs.md)
- [9. Run and engines](09-run-and-engines.md)
- [10. Results views](10-results-views.md)
- [11. Profile](11-profile.md)
- [12. Storm-sewer design panel](12-design-panel.md)
- [13. Import and export](13-import-export.md)
- [14. Preferences](14-preferences.md)

**Part 3b — GIS, 2D and the rest of the model**

- [19. GIS: shapefiles, GeoJSON, coordinate systems and DEMs](19-gis.md)
- [20. 2D overland flow and 1D-2D interfaces](20-2d-overland.md)
- [20b. 2D methods: equations, numerics and validation](20b-2d-methods.md)
- [21. LID, groundwater, snowmelt and water quality](21-lid-groundwater-quality.md)
- [22. Scenarios and calibration](22-scenarios-calibration.md)
- [23. Stopping a run and watching it live](23-live-runs.md)

**Part 4 — Methods**

- [15. Methods](15-methods.md) — what StormSewer computes itself, with
  equations and citations, and what it leaves to the SWMM engine.

**Part 5 — Troubleshooting**

- [16. Troubleshooting](16-troubleshooting.md) — continuity error, silent
  failures, instability, surcharge versus flooding, offsets, units, width,
  ERROR 209/335/363, hotstart, missing coordinates, non-ASCII paths, engine
  versions.

**Part 6 — File formats**

- [17. File formats](17-file-formats.md) — `.inp` section by section
  (including the ones EPA's Appendix D omits), what StormSewer preserves,
  `.out`, `.rpt`, `.ssproj`, CSV, GeoJSON, world files.

**Part 7 — Python**

- [18. Python terminal cookbook](18-python-cookbook.md) — the bound `model`,
  `out` and `rpt` names and ten recipes.

**Appendices**

- [A. Keyboard shortcuts](A1-keyboard-shortcuts.md)
- [B. Menu reference](A2-menu-reference.md)
- [C. Error codes](A3-error-codes.md)
- [D. Glossary](A4-glossary.md)
- [E. Credits and licences](A5-credits.md)

## Conventions

- Menu paths are written `File → Open .inp…`.
- Keys are written `Ctrl+Z`. Single letters such as `J` are tool shortcuts
  and work only when no text field has the keyboard.
- "The engine" means the EPA SWMM executable (`runswmm.exe` or `runswmm`)
  that StormSewer runs as a child process. StormSewer never modifies it.
- Where the manual cites the SWMM manuals it uses these abbreviations:
  *UM* — Storm Water Management Model User's Manual Version 5.2
  (EPA/600/R-22/030); *RM I* — Reference Manual Volume I, Hydrology
  (EPA/600/R-15/162A); *RM II* — Reference Manual Volume II, Hydraulics
  (EPA/600/R-17/111); *HEC-22* — FHWA Urban Drainage Design Manual,
  Hydraulic Engineering Circular 22, 3rd edition (FHWA-NHI-10-009). All four
  are public documents.

## Versions

This manual describes StormSewer 0.9.8 and was checked against EPA SWMM
5.2.4. The tutorial datasets are in `docs/datasets/`; they are the EPA sample
models and are public domain.
