# StormSewer v0.9.5

One change, for Windows: StormSewer now starts on machines that have no GPU
driver at all.

## Fixed

- **Starts with no graphics adapter.** 0.9.3 made StormSewer try Direct3D 12
  and then OpenGL, which covers remote desktop and virtual desktops that
  present any display driver. A machine with none — a bare virtual machine,
  Microsoft's winget validation sandbox — has zero adapters and only OpenGL
  1.1, and StormSewer could only explain why it would not start. The Windows
  installer now bundles Mesa's llvmpipe software renderer and falls back to
  it automatically when the hardware renderers fail. Nothing changes on a
  machine with a GPU; `STORMSEWER_SOFTWARE_GL=1` forces the software path,
  and `--check-renderer` now prints the OpenGL renderer it actually used.
  The installer grows by about 20 MB. Not bundled in the portable zip, the
  Homebrew, Linux, or web builds, where the OS already supplies Mesa or no
  driver is needed.

The engine crate and the Python package are unchanged and stay at 0.9.4.

## Install

```sh
brew tap mf4633/tap
brew install --cask mf4633/tap/stormsewer   # macOS app
brew install mf4633/tap/stormsewer-cli      # macOS + Linux CLI
```

| Platform | Download |
| --- | --- |
| Windows | `StormSewer-0.9.5-setup.exe` |
| macOS (Intel + Apple Silicon) | `StormSewer-macos-universal.zip` |
| Linux | `StormSewer-x86_64.AppImage` or `StormSewer-linux-x64.tar.gz` |
| Command line | `stormsewer-cli-linux-x64.tar.gz` / `stormsewer-cli-macos.tar.gz` |
| Browser build (engine only) | `stormsewer-web.zip` |

# StormSewer v0.9.4

Import fixes found by running real Civil 3D files from a live project through
the app and comparing the results with the Hydraflow Storm Sewers report for
the same network. Every finding below is now covered by a regression test on a
checked-in Civil 3D file (`tests/civil3d_formats.rs`, `tests/dxf_underlay.rs`),
and the side-by-side comparison is written up in VALIDATION.md §8.

## Fixed

- **Civil 3D `.stm` files open.** The importer required the words "Hydraflow
  Storm Sewers" and the Civil 3D extension writes "Storm Sewers for AutoCAD
  Civil 3D", so every real project file was rejected as "not a Hydraflow Storm
  Sewers STM file".
- **Pipe roughness is the pipe's, not the gutter's.** Each line block ends with
  a `Gutter N-Value` record that matched the `N-Value` key, so every pipe took
  the gutter n (0.013 for 0.012 here: 8 % less capacity).
- **Starting HGL becomes the tailwater.** It was ignored and the outfall
  treated as free, which moved the whole HGL profile.
- **Pipe sizes in the Civil 3D format are feet.** A 1.5 ft rise was
  treated as inches for anything over 3, so a 42-in pipe would have imported
  as 3.5 in. "Return Period Index" in that format is the period in years.
- **Drops through structures are kept.** A structure has one invert in this
  model and Hydraflow gives every pipe its own end inverts. Pipes now carry
  optional inverts of their own (used by the engine, the profile, and both
  importers), so a pipe entering a manhole above the outlet invert keeps its
  real slope instead of being re-sloped to the structure.
- **Outfall rim, unset inlet time, and zero K.** An outfall with no rim read as
  hundreds of feet of negative freeboard; an inlet time of 0 became 10 min
  instead of the file's minimum Tc; an all-zero (auto) junction K became a
  lossless network.
- **LandXML from Civil 3D imports with its data.** Civil 3D writes rims and
  sumps as attributes, one `<Invert>` per connected pipe, pipe lengths,
  `<PipeFlow>` catchment data, and a "null structure" for the outfall, and
  never writes a `role`. The reader looked for element text and role
  attributes, so a real export imported as nameless junctions at invert 0
  with no outfall. It now reads all of the above, infers the outfall from the
  structure that only receives flow, keeps Civil 3D's names ("AI-1", "P5"),
  and picks Manning's n from the pipe material.
- **DXF underlays match the drawing.** The reader walked every section of the
  file, so block definitions drawn at the origin stretched the site extents
  to (0,0); INSERTs were never expanded (a QGIS tree export produced two
  segments); old-style POLYLINE/VERTEX runs were dropped; every open polyline
  was closed; ARCs and paper-space title blocks were mis-handled. Fixed all
  six, with nested blocks bounded. A 51 MB site plan loads in 0.2 s.
- **The tailwater field no longer rewrites your elevation.** The Parameters
  panel clamped tailwater to 0–500 ft, so an imported 757.365 ft starting HGL
  was silently changed to 500 on the first frame and the outfall HGL moved
  with it. Ticking the tailwater box now starts at the outfall invert.
- **Opened networks are zoomed to fit.** A file in state-plane coordinates
  opened off-screen with an empty plan view.
- **Inlet gutter flow uses the inlet time.** The app computed each inlet's
  local flow C·i·A with the intensity at the pipe system's accumulated Tc,
  so every downstream inlet's approach flow was under-stated (2.87 cfs where
  Hydraflow and HEC-22 give 3.19 at a 5-min inlet on a 7-min system). The
  app, the CLI, and the reports now share one inlet pass at the inlet's own
  time. Imported STM inlets also carry their grate size and cross slope, so
  a 4 × 4 sag grate captures what Hydraflow says it captures.

## Added

- **`stormsewer-cli` reads project files.** Besides `.ssn`, the CLI now
  takes a `.ssproj`, a Hydraflow / Civil 3D `.stm`, a LandXML `.xml`, or a
  StormSewer `.dxf`, analyzes it exactly as the desktop app does, and prints
  the inlet schedule.
- **End-to-end reference suite.** `tests/e2e_reference.rs` runs two real
  Civil 3D project files through every input and output channel (engine,
  inlet pass, text/HTML/PDF, project/LandXML/DXF round-trips, CLI) against
  the Hydraflow Storm Sewers reports for those files; the GUI does the same
  on real frames. LandXML export now writes Civil 3D-style per-pipe inverts
  and DXF export carries them in XDATA, so drops survive both round-trips.
- **Open a file from the command line.** `StormSewer <file>` opens a
  `.ssproj`, a Hydraflow / Civil 3D `.stm`, a LandXML `.xml`, or a `.dxf`
  (a StormSewer network export, else the drawing becomes the site underlay)
  and skips the tutorial. Groundwork for double-click file association.
- **Per-pipe inverts** in the project format (`invert_up` / `invert_dn`,
  optional, older files unchanged) and in the Python/engine `Pipe`.
- **VALIDATION.md §8**: side-by-side with Hydraflow Storm Sewers v2026.00 on
  a real Civil 3D network, with every difference explained.



# StormSewer v0.9.3

Fixes three things that were invisible from a normal desktop: StormSewer would
not start at all on machines without an OpenGL driver, it said nothing when that
happened, and its in-app support links were dead.

## Fixed

- **Starts on remote desktop, Citrix/VDI, and virtual machines.** The app was
  built against OpenGL only. On a machine with no usable GL driver — RDP
  sessions, virtual desktops, plain VMs, and Microsoft's own winget validation
  sandbox — it simply failed to launch. Both renderers now ship, and startup
  tries Direct3D 12 (with a software adapter as a last resort) before falling
  back to OpenGL.
- **Says something when it cannot start.** A failed launch used to print to a
  console that a double-click does not have, so nothing appeared to happen at
  all. It now explains why, names the likely cause, and points at the browser
  build, which needs no graphics driver.
- **The support links work.** The Help menu item and the About dialog button
  pointed at a Buy Me a Coffee account that does not exist, so anyone who tried
  to say thanks hit a dead page. Both now open the real checkout.
- **The version is read from the build.** The window title, the document title,
  and the About dialog carried a hardcoded `v0.9` that had already gone stale
  once. All three now read the released version, with a test that fails on any
  new literal.

## Install

```sh
brew tap mf4633/tap
brew install --cask mf4633/tap/stormsewer   # macOS app
brew install mf4633/tap/stormsewer-cli      # macOS + Linux CLI
```

The engine is on crates.io (`cargo add stormsewer`) and compiles to WebAssembly.

| Platform | Download |
| --- | --- |
| Windows | `StormSewer-0.9.3-setup.exe` |
| macOS (Intel + Apple Silicon) | `StormSewer-macos-universal.zip` |
| Linux | `StormSewer-x86_64.AppImage` or `StormSewer-linux-x64.tar.gz` |
| Command line | `stormsewer-cli-linux-x64.tar.gz` / `stormsewer-cli-macos.tar.gz` |
| Browser build (engine only) | `stormsewer-web.zip` |

Windows and macOS builds are unsigned — SmartScreen and Gatekeeper will warn on
first run. On macOS, right-click the app and choose Open.

## Also since 0.9.1

- [**VALIDATION.md**](https://github.com/mf4633/stormsewer/blob/master/VALIDATION.md)
  works every number on a reference network by hand — intensity, Manning
  capacity, Rational accumulation, Tc accumulation, partial-flow velocity, HGL
  with junction loss, HEC-22 interception — and matches the engine to six
  decimal places, with a test that fails if any published number moves.
- **Python bindings.** `pip install stormsewer` gives the same engine as a
  native extension: primitives for scripting, and whole-network analysis
  returning dictionaries that drop straight into pandas.
- **Project files carry a format version**, with the promise that any 1.x
  StormSewer opens any 1.x project.
- Preferences and unsaved-work recovery now work on macOS and Linux (0.9.2).

293 tests.

## Support

Bugs and feature requests belong in
[Issues](https://github.com/mf4633/stormsewer/issues) — free, and the fastest
way to get something fixed. For commercial support, custom modules, or
firm-wide rollouts: support@hydrocomplete.com. If StormSewer saves you an
afternoon, [buy me a coffee](https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-release).

Built by Michael Flynn, PE — see also [HydroComplete](https://hydrocomplete.com)
for browser-based hydrology and hydraulics.
