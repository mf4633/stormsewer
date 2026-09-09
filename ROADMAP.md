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

Doable unattended. This is the most valuable blocking item I can do without
spending your money.

### 4. Crash-free on the unhappy paths

The suite covers a great deal, but 1.0 should also survive deliberate abuse:
malformed `.ssproj`, a truncated DXF, a network with a cycle, an outfall higher
than its upstream invert, zero-length pipes, a 10,000-pipe network. Some of
these are already handled; none are systematically fuzzed.

Doable unattended.

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

## After 1.0

- **Python bindings.** The engine is `std`-only and binds cleanly through PyO3;
  wheels via maturin would reach the PySWMM and notebook audience. Started —
  see `python/`.
- **Civil 3D round-trip.** The connector exists separately; sharing the engine
  is the obvious next step.
- **Homebrew core and winget maturity.** The tap works now; homebrew-cask proper
  needs the notability bar (75 stars / 30 forks / 30 watchers).

## How to read this

If you are evaluating StormSewer for real work today: items 1 and 2 are about
trust and installation, not correctness. The hydraulics are tested — 284 tests,
with every reference number worked by hand in VALIDATION.md — and the engine
does not change when the version number does.
