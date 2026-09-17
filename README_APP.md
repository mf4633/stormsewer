# StormSewer Desktop Application

**StormSewer 0.9.8** — a free, GPL editor for EPA SWMM 5 models with the
open `stormsewer` storm-sewer design engine built in.

Two workspaces share one window. The **SWMM Model Editor** opens an `.inp`
losslessly, draws it, edits it on the map and in a property sheet and
attribute tables, runs it on the unmodified EPA engine installed on the
machine (as a child process, stamped with the engine version and binary
hash), and shows the results on the same map, in plots, tables and a
profile. The **storm-sewer design workspace** is the original tool: place
structures, draw pipes and catchments, run Rational-method analysis with
Manning hydraulics and HGL backwater, review design criteria, auto-size
pipes, estimate construction cost, and export CAD and report deliverables.
Tools → Storm Sewer Design runs that engine on a SWMM model's conduits.

The manual is at https://mf4633.github.io/stormsewer/manual/ (source in
`docs/`; `build-tools/build-docs.sh` builds it with pandoc). Help → Manual
(online) in the app opens it; Help → SWMM Model Editor is the offline
getting-started topic.

## Requirements

- [Rust](https://rustup.rs/) toolchain (edition 2021)
- Windows, macOS, or Linux with a GPU-capable display (eframe/glow backend)

## Build & Run

From the repository root:

```bash
cd stormsewer
cargo run -p stormsewer-app --bin StormSewer
```

Release build:

```bash
cargo build -p stormsewer-app --release --bin StormSewer
```

Windows installer (requires [Inno Setup 6](https://jrsoftware.org/isinfo.php)):

```powershell
.\scripts\build-installer.ps1
```

The executable is written to `target/release/StormSewer.exe`.

## Features

### SWMM model editor

- Lossless `.inp` document with undo/redo for every gesture, field, dialog,
  import and batch; autosave and recovery
- Map with the EPA GUI's tools and symbols; project browser; property sheet;
  attribute tables with replace-in-column and CSV round trip; layers
- Project dialogs: options (with a unit-switch wizard and offsets
  conversion), rain gages, curves, time series (edit, import, export to
  file), patterns, design storms, conduit lengths, controls, pollutants,
  land uses; `[BACKDROP]` images from world files
- Run on any registered EPA engine; pre-run QA; Run Status with continuity
  thresholds, full diagnostic lists and an index of all engine error codes
- Results on the map with a time slider and query; plots and scatter;
  report tables; profile with Max HGL; compare runs and engines; model
  report (HTML/PDF); CSV/GeoJSON export; Python terminal
- Storm-sewer design (Rational / Manning / HGL / HEC-22 / auto-size) on the
  SWMM model's conduits

The manual's chapters 3–14 cover each pane; Appendix A lists the shortcuts
(`S H + - Z R A J O D T C P I W U L` for the tools, `F5` run, `F` extents).

### Network editing (plan view)

- **Select** — pick nodes and pipes; inspect and edit properties
- **Place Inlet / Junction / Outfall** — add structures with snap-to-grid placement
- **Draw Pipe** — connect two nodes; circular, box, or elliptical sections
- **Draw Catchment** — polygon tributary areas linked to inlets
- Pan and zoom; zoom to extents (**F**) or selection (**G**)
- Undo/redo (**Ctrl+Z** / **Ctrl+Y**)
- Optional PNG or DXF background underlay for trace-over design

### Hydraulics & design

- Rational method peak-flow hydrology with IDF curve parameters
- Manning open-channel flow and HGL backwater analysis
- Design review against configurable municipal criteria
- Auto-size pipes to meet capacity and velocity limits
- HEC-22 inlet capacity checks
- Tc calculator and TR-55 multi-segment worksheet (FAA, Kirpich)
- Pipe construction cost estimation
- Network topology diagnostics
- Plan view and longitudinal profile view
- U.S. customary or SI units

### Project I/O

- Native `.ssproj` project save/open with recent files list
- Import/export DXF and LandXML
- Import Hydraflow `.stm` projects (IDF curves, inlet geometry, background DXF)
- Custom MyReport templates (`.srpt`) with column editor — CSV/HTML export
- Export PDF and HTML hydraulic reports
- Print report (**Ctrl+P**) opens PDF in default viewer

### User interface

- Tabbed left panel: Parameters, Tables, Design Review
- Right panel: hydraulic report and sizing summary
- Bottom inspector for selected node/pipe properties
- Full Help browser with Hydraflow migration guide
- Global pipe editing (Manning n, diameter)

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+Z / Ctrl+Y | Undo / Redo |
| Ctrl+O | Open project |
| Ctrl+S | Save project |
| Ctrl+A / F5 | Run analysis |
| Ctrl+P | Print report |
| Delete | Delete selected node or pipe |
| 1–6 | Select, Inlet, Junction, Outfall, Pipe, Catchment |
| F | Zoom to extents |
| G | Zoom to selection |
| F1 | Help |

## License

GPL-3.0-or-later. See `LICENSE` in the repository root.