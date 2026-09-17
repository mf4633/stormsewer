# 14. Preferences

StormSewer keeps a small preferences file per user:
`%APPDATA%\StormSewer\app_prefs.json` on Windows,
`~/Library/Application Support/StormSewer/app_prefs.json` on macOS,
`$XDG_CONFIG_HOME/stormsewer/app_prefs.json` (or `~/.config/stormsewer/`)
on Linux. It is JSON with defaults for every key, so deleting it resets
everything and an older file still loads. Nothing about a model is stored
in it.

## 14.1 What is saved

| Setting | Where you set it | Key |
| --- | --- | --- |
| Theme — Dark, Light, or the system's | View → Dark theme / Light theme | `theme` |
| SWMM layer settings — per-kind visibility, labels, symbol size and colour, arrows, grid, underlay opacity, results overlay | Layers tab ([chapter 7](07-layers.md)) | `swmm_layers` |
| Snap grid spacing for the storm-sewer plan | storm-sewer workspace | `snap_grid_ft` |
| Analyse automatically after an edit (storm-sewer workspace) | storm-sewer workspace | `auto_analyze` |
| Show the quick-start on launch; the interactive tutorial's "Don't show on startup" | Help → Interactive Tutorial | `show_quick_start`, `tutorial_done` |
| Draw zero-area catchments (storm-sewer workspace) | storm-sewer workspace | `draw_zero_area` |
| The support reminder's opt-out and last-shown time | the reminder | `coffee_optout`, `coffee_last_epoch` |

The SWMM workspace's grid-snap spacing, object snap and the View-menu
toggles (labels, arrows, grid) are per session and start from their
defaults each launch; the Layers tab's copies of arrows and grid are the
persisted ones.

Beside it: `recent.json` (storm-sewer projects) and `recent-inp.json`
(SWMM models), eight entries each, most recent first.

## 14.2 Environment variables

| Variable | Effect |
| --- | --- |
| `STORMSEWER_SWMM_ENGINE_DIR` | A folder searched first for `runswmm` ([§1.2](01-start-here.md)). |
| `STORMSEWER_ALR_SCRIPT`, `STORMSEWER_ALR_PYTHON` | Where ALR's `run_headless_swmm.py` is, and which interpreter runs it ([§9.5](09-run-and-engines.md)). |
| `STORMSEWER_SOFTWARE_GL=1` | Windows: skip the hardware renderers and start on the bundled Mesa software OpenGL. |

## 14.3 Command line

```
StormSewer [file]            open a .inp, .ssproj, .stm, .xml (LandXML) or .dxf
StormSewer --check-renderer  start, draw one frame, report the renderer, exit
StormSewer --version
StormSewer --help
```

A `.inp` opens in the SWMM workspace; the others open in the storm-sewer
workspace.
