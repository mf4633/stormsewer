# Appendix B. Menu reference

Every item of the SWMM workspace's menu bar, one line each, in menu order.
Items are greyed when they do not apply (no model open, nothing selected,
no run yet).

## File

| Item | Does |
| --- | --- |
| New SWMM Model | A blank model (asks about unsaved changes). `Ctrl+N` |
| Open .inp… | Open a model. `Ctrl+O` |
| Recent Models | The last eight models opened |
| Save | Save to the model's path. `Ctrl+S` |
| Save As… | Save to a new path. `Ctrl+Shift+S` |
| Import → StormSewer Project (.ssproj / .ssn)… | A new SWMM model from a storm-sewer project ([§13.1](13-import-export.md)) |
| Export → StormSewer Project (.ssproj)… | The model as a storm-sewer project ([§13.2](13-import-export.md)) |
| Export → Network CSV (nodes, links)… | Two CSV tables ([§13.3](13-import-export.md)) |
| Export → GeoJSON (map units, no CRS)… | One FeatureCollection ([§13.4](13-import-export.md)) |
| Load PNG Background… | The storm-sewer workspace's image underlay ([§3.9](03-map-and-tools.md)) |
| DXF Underlay… | A DXF drawn under the map |
| Storm Sewer Design Workspace | Switch workspaces; the model stays open |

## Edit

| Item | Does |
| --- | --- |
| Undo *label* | Undo the named step. `Ctrl+Z` |
| Redo *label* | Redo it. `Ctrl+Y` |
| Cut / Copy / Paste | The selection, with references remapped on paste |
| Delete | The selection, after the *Delete takes more with it* check |
| Select All | Every object |

## View

| Item | Does |
| --- | --- |
| Zoom In / Zoom Out / Zoom Extents / Zoom Window / Zoom to Selection | Navigation; Zoom Window and Pan are tools |
| Pan | The Pan tool |
| Backdrop → Load Image… | A PNG/JPEG placed from its world file into `[BACKDROP]` ([§3.9](03-map-and-tools.md)) |
| Backdrop → Georeference… | Extent or scale-and-offset; Fit to model; Write world file |
| Backdrop → Show | Toggle it |
| Backdrop → Remove | Delete `[BACKDROP]` |
| Map Dimensions… | `[MAP] DIMENSIONS` and `Units`; Set from model / from backdrop |
| Object Labels / Flow Arrows / Grid | Map toggles |
| Snap to Objects / Snap to Grid | Snapping toggles |
| Project Browser / Map Layers | The left panel's tabs |
| Attribute Table… | The selected object's section as a grid ([chapter 6](06-attribute-tables.md)) |
| Properties | Show or hide the property sheet |
| Profile from Selection | Two selected nodes become the profile path |
| Pick Profile Path… | Click a start and an end node on the map |
| Map / Chart / Profile / Results | The centre view |
| Storm Sewer Design Workspace | Switch workspaces |

## Project

| Item | Does |
| --- | --- |
| Title/Notes… | `[TITLE]` |
| Options… | `[OPTIONS]` in five tabs and `[FILES]`; opens the unit-switch wizard on a `FLOW_UNITS` change ([§8.2](08-project-dialogs.md)) |
| Rain Gages… | `[RAINGAGES]` |
| Curves… | `[CURVES]` as drafts |
| Time Series → Edit… | `[TIMESERIES]` as drafts |
| Time Series → Import… | Delimited or GHCN-Daily text into series and gages ([§8.5](08-project-dialogs.md)) |
| Time Series → Export to File… | A series as SWMM's external file, optionally replacing it with a `FILE` reference |
| Patterns… | `[PATTERNS]` |
| Design Storm… | NRCS/NOAA/alternating-block/Chicago/uniform hyetograph into a series and a gage ([§8.9](08-project-dialogs.md)) |
| Compute Conduit Lengths… | Geometric lengths against stored lengths; write the ticked ones ([§3.10](03-map-and-tools.md)) |
| Controls… | `[CONTROLS]` as text |
| Pollutants… / Land Uses… | Their sections |
| Validate Model | Refresh the findings and show the list |

## Run

| Item | Does |
| --- | --- |
| *Engine list* | Choose the engine; each shows version and architecture |
| Find Engines | Rescan the install locations and `PATH` |
| Live Results | Toggle: open the Live Results window and follow the growing `.out` during a run ([§23.2](23-live-runs.md)) |
| Check Model… | The QA pass: findings grouped by severity, clickable ([§9.2](09-run-and-engines.md)) |
| Autosave every N min | Snapshot interval for a dirty model; 0 = off |
| Run | Run the model. `F5` |
| Stop | Kill the running engine; the partial results are kept ([§23.1](23-live-runs.md)) |

## Results

| Item | Does |
| --- | --- |
| Map (Peaks) | Colour the map by run maxima |
| Chart | The single-series chart view |
| Report Summary… | Placeholder; the summary tables are in the Tables view |
| Run Status… | The last run's window: engine and hash, continuity with thresholds, every diagnostic list, warnings and errors explained ([§9.4](09-run-and-engines.md)) |
| Model Report… | HTML or PDF report of the model and its run ([§2.10](02-tutorials.md)) |
| Compare Runs… | Peak differences between two recorded runs ([§2.9](02-tutorials.md)) |
| Compare Engines… | Run the model on two engines and compare |

## Tools

| Item | Does |
| --- | --- |
| ALR Checks | The ALR post-processor on the last run ([§9.6](09-run-and-engines.md)) |
| Python Terminal… | A Python kernel with `model`, `out`, `rpt` bound ([chapter 18](18-python-cookbook.md)) |
| Tc Calculator… | Kirpich, TR-55 sheet flow, FAA, and a TR-55 worksheet |
| Storm Sewer Design → Design Panel… | The design panel ([chapter 12](12-design-panel.md)) |
| Storm Sewer Design → Analyse | Map and analyse |
| Storm Sewer Design → Auto-size… | Analyse and preview the sizing batch |
| Storm Sewer Design → Design Review | Analyse and show the findings |
| Storm Sewer Design → Design Report (HTML)… / (PDF)… | The design report for the mapped network |
| Storm Sewer Design Workspace | Switch workspaces |

## Help

Shared with the storm-sewer workspace.

| Item | Does |
| --- | --- |
| Interactive Tutorial | The storm-sewer workspace's guided first run |
| Getting Started / Quick Start Tutorial / Design Workflow / Computational Methods / File Import & Export / Hydraflow Migration Guide | Open the Help window on that topic (`F1` opens it too) |
| Keyboard Shortcuts… | [Appendix A](A1-keyboard-shortcuts.md) |
| Troubleshooting | The storm-sewer workspace's troubleshooting topic |
| SWMM Error Codes… | The engine's error and warning index with search ([Appendix C](A3-error-codes.md)) |
| Support StormSewer / Support & Custom Work… / About StormSewer… | Links and the About box |

The Help window's **Contents** list has two entries for this workspace at
its end: **SWMM Model Editor**, the offline getting-started topic that
mirrors [chapter 1](01-start-here.md), and **Manual (online)**, which
opens this manual in the browser and shows its address.

## Toolbar

Select · Pan · Zoom + · Zoom − · Zoom Win · Gage · Subcatch · Junction ·
Outfall · Divider · Storage · Conduit · Pump · Orifice · Weir · Outlet ·
Label · **Run** · Extents · Grid snap (spacing) · Object snap · … · SWMM /
Storm Sewer · ● Unsaved.

## Context menu (right-click on the map)

Zoom Extents · Zoom To · Edit Properties… · Attribute Table… · Profile from
Here… · Reverse Link · Convert Node Type → Junction / Outfall / Divider /
Storage · Delete.

## Dialog buttons that recur

OK · Cancel · Apply · Revert · Close · Add row · `+` (add) · `−` (delete) ·
Save model… / Discard changes / Keep editing (unsaved model) · Delete all of
it / Keep them (delete impact) · Show in findings list / Dismiss (run
refused) · Run anyway (warnings before running) · Restore autosave /
Discard it, keep the file (recovery) · Convert ticked / Copy report /
Revert (unit wizard) · Add to Model (design storm) · Import / Load file…
/ Save As… (time series) · Apply to n ticked (lengths) · Rebuild / Save
HTML… / Save PDF… / Open after saving (model report) · Run both / Export
CSV… / Swap A and B (compare) · Copy summary / Error codes… (run status)
· Run / Restart Kernel / Clear Output (Python) · Add label (label tool).
