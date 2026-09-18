# Roadmap to 1.0

StormSewer is at 0.9.2 and is already doing production work. The version number
is the thing holding it back: `0.9.x` tells a conservative engineer "not ready,"
and that is exactly the audience this is for. This is what 1.0 has to mean
before the number changes.

The bar for 1.0 is not "no more features." It is **an engineer can rely on it,
verify it, and install it without a warning dialog.**

## Blocking

### 1. Code signing and notarization

Today every Windows user gets a SmartScreen warning and every macOS user is told
the app is damaged or from an unidentified developer. This is the single largest
source of friction in the funnel, it hits users of *all five* install paths, and
no amount of documentation fixes it — a firm's IT policy may simply refuse.

- **Windows:** Azure Trusted Signing, roughly $10/month. The release workflow
  already has a `SIGNTOOL` hook; it needs credentials and a CI secret.
- **macOS:** Apple Developer Program, $99/year. Needs `codesign` plus
  `notarytool` submission and stapling in the macOS job.

Requires money and accounts, so it cannot be done unattended.

### 2. Validation against a commercial package

[VALIDATION.md](VALIDATION.md) proves the engine computes the published
equations correctly. It does not prove agreement with the tools reviewers
already trust. A 1.0 claim should include a reference network run through
Hydraflow Storm Sewers or Stormwater Studio side by side, with the differences
explained rather than hidden. The Manning K question is settled: 1.486, matching Hydraflow,
which is a known 0.27% offset.

**Done for Hydraflow Storm Sewers (2026-09-08).** A real Civil 3D network with
its Hydraflow report was run side by side; [VALIDATION.md §8](VALIDATION.md)
tabulates the two and explains every difference (Manning constant, travel-time
velocity, surcharged-reach friction, per-structure K). Flows agree to 0.04 % on
the terminal lines and 2.5 % on the outfall line; HGLs within 0.5 ft. The
comparison is one four-line trunk under one storm — more networks, branches,
and box sections would strengthen it. Stormwater Studio remains uncompared.

### 3. A file format promise

`.ssproj` is JSON with `serde` defaults, and old files load today. That is a
happy accident, not a commitment. 1.0 should state that 1.x will open any 1.x
project, add a `format_version` field, and have a test that loads a checked-in
0.9 file.

**Done (2026-09-18).** The promise is carried in the type rather than in prose:
`Project::format_version` with `FORMAT_VERSION = 1`, `#[serde(default)]` on
every field added since, and the rule written at the field itself — a field may
not change meaning without bumping the version (`src/io/project.rs`). The
checked-in 0.9 file is `tests/fixtures/legacy-0.9-project.ssproj`, loaded by
`tests/headless_suite.rs`; `legacy_files_get_the_default_structure_diameter`
and `p2_rainfall_defaults_on_legacy_json` additionally cover individual fields
added after 0.9 defaulting correctly.

### 4. Crash-free on the unhappy paths

The suite covers a great deal, but 1.0 should also survive deliberate abuse:
malformed `.ssproj`, a truncated DXF, a network with a cycle, an outfall higher
than its upstream invert, zero-length pipes, a 10,000-pipe network. Some of
these are already handled; none are systematically fuzzed.

**Largely done (2026-09-18).** `malformed_inputs_error_rather_than_panic` in
`tests/headless_suite.rs` covers the file paths — not JSON at all, valid JSON of
the wrong shape, truncated mid-object, a missing file, and a truncated DXF —
asserting an error rather than a panic. The same file covers a zero-length pipe
and a cycle (the last pipe pointed back at the head node), and
`tests/robustness.rs` covers the degenerate networks: no pipes at all, zero
contributing area, wholly adverse slopes, and supercritical throughout, each
required to stay finite rather than produce NaN.

Two gaps remain against the wording above: `large_network_analyzes` builds
**500** pipes, not 10,000, and none of this is *fuzzed* — every case is a
hand-written input, so it protects the failures we thought of. The SWMM
readers (`.inp`, `.out`) have no equivalent abuse coverage at all.

### 5. Software rendering on Windows

**Done (0.9.5).** StormSewer tries Direct3D 12 first and OpenGL second, which
covers remote desktop and virtual desktops that expose any display driver at
all. A Windows machine with **no display driver whatsoever** — a bare VM,
Microsoft's package-validation sandbox — has zero graphics adapters and only
the generic OpenGL 1.1, so neither backend can start there.

Since 0.9.5 the Windows installer bundles Mesa's llvmpipe software rasteriser
(`mesa\opengl32.dll` + `libgallium_wgl.dll`, ~62 MB on disk, pinned by hash in
`scripts/fetch-mesa.ps1`) together with a second copy of the executable, and
the app re-executes that copy when the hardware renderers fail. The copy is
what makes it work: the executable imports `opengl32.dll` statically, so the
system DLL is mapped before `main` and nothing loaded later can replace it —
only the application directory is searched ahead of System32. The Windows job
in `.github/workflows/smoke.yml` runs on exactly such a driverless machine and
now asserts that the self-test **starts on llvmpipe**, by name; a second step
asserts `STORMSEWER_SOFTWARE_GL=1` forces the same path on any machine.
`app/src/software_gl.rs` has the details.

## Not blocking

These are real gaps, but none of them makes the current build unreliable, and
none should hold the version number hostage.

- **Multi-barrel pipes.** The HGL friction pass is unvalidated for parallel
  barrels — deliberately deferred rather than shipped wrong.
- **Flow splits.** The network model is dendritic. Loops and splits are a
  different solver.
- **Hydrograph routing.** Rational peak flows only; no storage routing.
- **HEC-14 riprap sizing.** Needs transcription from the primary source, which
  has not been to hand.
- **TIN / surface model.** Ground elevations come from the structures.
- **Editable schedule grid.** Tables are read-only; editing happens in the
  inspector.
- **SI output.** The SI toggle converts inputs (m, ha, mm/hr, metric pipe
  sizes) but results — schedules, inspector, reports — are still cfs, ft/s and
  ft, and the web build and CLI are US customary only. Needs a results-side
  conversion layer and an SI reference network in `VALIDATION.md`.

## After 1.0

- **Python bindings.** The engine is `std`-only and binds cleanly through PyO3;
  wheels via maturin would reach the PySWMM and notebook audience. Started —
  see `python/`.
- **Civil 3D round-trip.** The connector exists separately; sharing the engine
  is the obvious next step.
- **Homebrew core and winget maturity.** The tap works now; homebrew-cask proper
  needs the notability bar (75 stars / 30 forks / 30 watchers).

## The PCSWMM-class editor (direction set 2026-09-17)

StormSewer's interface becomes a model editor for EPA SWMM in the shape people
know from PCSWMM: a map you draw on, an attribute grid, a property sheet, a run
button that reports which engine ran, and results that come back onto the same
map. The storm-sewer design and reporting that StormSewer already does becomes
one feature inside that editor, not a separate product. The engines stay EPA's
own, unmodified, run as subprocesses and stamped with version and hash. The
name stays StormSewer.

The order below is dependency order, not preference. Nothing in the editor is
worth building on a document that cannot write a model back exactly as it read
it, and every editing gesture from the first one has to be undoable.

1. **Lossless `.inp` document with undo and redo** (`swmm/src/doc.rs`).
   Parse, edit, and serialise a model with every comment, blank line, unknown
   section and formatting quirk preserved; typed field access by the SWMM 5.2
   column definitions; rename that follows every reference; commands with
   inverses, batched per gesture, bounded history. Proven by round-tripping
   the EPA sample models byte for byte and by undoing random edit sequences
   back to the original text.
2. **Map editing on the existing plan canvas.** Place, move, and delete nodes;
   draw links with vertices; draw subcatchment polygons; snap, rubber-band
   select, pan and zoom as the storm-sewer view does today. Every gesture is
   one undo step. A background image or DXF underlay, which the plan view
   already supports.
3. **Attribute grid and property sheet.** One grid per object type, sortable
   and filterable, with in-place editing; a property sheet for the selection
   with unit labels; both driven from the same column table as the document,
   so a field cannot exist in one and not the other.
4. **Run and results on the map.** The existing engine registry and runner
   become the Run button; results colour nodes and links by a chosen variable
   with a time slider; time-series plots and profile plots for a selected
   path; the `.rpt` summary tables as grids. Continuity error and warnings
   surface where the user is looking, not in a file.
5. **Storm-sewer design as a feature.** The Rational / Manning / HEC-22 /
   backwater engine runs on a SWMM network's conduits and inlets and writes
   its schedules and the design report from the same model, so one drawing
   serves both the design submittal and the SWMM analysis.
6. **Scenarios and the report.** Named scenarios that differ by a set of
   edits (a command list on top of a base model, which the undo design gives
   for free); a report that a reviewer can read, in the spirit of the
   StreamStats appendix, with the engine version and hash on it.

Explicitly not in this plan: a 2D overland solver of our own, a modified
engine, or anything that reads PCSWMM's files or binaries. Interop with
Civil 3D comes through the existing connector once the document exists.

**Status, 2026-09-17.** Steps 1 to 5 are in master and documented in the
manual (`docs/`, published at `/manual/`): the lossless document with undo,
map editing with every gesture one undo step, grids and the property sheet
driven from one column table, Run with the engine's version and hash on the
Run Status window and results on the map, plots, profile with the HGL and
its maximum envelope, `.rpt` tables, and the storm-sewer design panel running
on the SWMM network. Also landed from the community research: autosave and
recovery, a units wizard that converts values rather than relabelling them,
depth/elevation offset conversion, world-file backdrops, computed conduit
lengths, design storms, rain-gauge import, an explained error index, and
run-to-run comparison. Step 6 is partly there (compare runs and engines;
the model report) and partly not (named scenarios as command lists). Still
open and stated plainly in the manual: Run → Stop cannot interrupt the
engine, no live results while a run is in progress, no batch runs from the
GUI, no GIS import or coordinate systems, no dialogs for LID, groundwater,
snowmelt or water quality (attribute tables only), and design output in U.S.
customary units only.

## How to read this

If you are evaluating StormSewer for real work today: items 1 and 2 are about
trust and installation, not correctness. The hydraulics are tested — 284 tests,
with every reference number worked by hand in VALIDATION.md — and the engine
does not change when the version number does.
