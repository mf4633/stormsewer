# StormSewer v0.8.0

Second release — a major step toward a defensible professional tool. Deeper
hydraulics, real HEC-22 methods, NOAA rainfall ingestion, and a correctness
audit of it all. GPL-3.0-or-later; still free for the world.

## New in 0.8.0

**Hydraulics**
- **True gradually-varied-flow (GVF) backwater** — the HGL is now a real
  standard-step water-surface profile (M1/M2), not a single friction step.
- **Flow-regime classification** — every reach is reported as subcritical,
  critical, supercritical, or pressurized (from normal vs critical depth), and
  the HGL model routes each regime correctly. A supercritical reach whose outlet
  is drowned by a downstream surcharge is now backed up rather than ignored.

**HEC-22**
- **Access-hole structure losses** (opt-in) — composite energy-loss coefficient
  from relative access-hole size, flow deflection, plunging, and benching.
- **Real inlet interception** — Izzard gutter spread, frontal/side-flow grate
  efficiency, curb-opening length for full interception, and weir/orifice sag
  capacity, replacing the earlier surrogate.

**Rainfall**
- **NOAA Atlas 14 import** — load or paste a NOAA PFDS precipitation CSV and
  StormSewer fits `a/(t+b)^c` IDF coefficients for every return period; fitted
  curves are shown in the IDF panel.
- **Rational frequency factor (Cf)** — raises the effective runoff coefficient
  for rarer storms (25/50/100-yr).

**Reports & review**
- Submittal metadata (engineer, firm, project number, jurisdiction) on reports.
- HGL freeboard check and non-circular cover (uses section rise) in the review.
- Real FAA Tc formula; TR-55 channel segments use Manning velocity.

**Quality**
- A multi-agent correctness audit of the new GVF, HEC-22, and NOAA code, with
  every real finding fixed (see the audit-hardening commit).
- Test suite expanded to 180+ tests (integration, robustness, unit-conversion,
  and golden-value coverage).

**Editor**
- Draw pipe runs by clicking the canvas, snap highlighting, finish-on-right-
  click/double-click, drag-to-merge, split-by-drop, reverse, duplicate, and
  place-on-empty context menus.

---

# StormSewer v0.7.0

First public release of the StormSewer hydrology & hydraulics engine — an open
recreation of the standard methods used by tools like Autodesk Hydraflow Storm
Sewers. GPL-3.0-or-later.

## What's in it

- **Engine** (`stormsewer`) — Rational-method peak-flow accumulation, Manning
  circular open-channel hydraulics (exact geometry, normal/critical depth, full
  and peak capacity), time-of-concentration (Kirpich, TR-55, FAA), HGL backwater
  with junction losses, standard-pipe sizing, and HEC-22 inlet capacity.
- **Desktop app** (`StormSewer`) — plan + profile + inspector, live analysis,
  editing with undo, light/dark themes, DXF/LandXML/`.STM` import and
  DXF/LandXML/PDF/HTML export.
- **CLI** (`stormsewer-cli`) — analyze a `.ssn` network, with `--size` and
  `--review`.
- **Web** (`stormsewer-wasm`) — the same engine compiled to WebAssembly; a
  browser playground with live calculators and full-network analysis, no server.

## Validation

Correctness is pinned to hand-derived reference values, not ranges:

- `tests/validation.rs` — Manning full-flow (16.04 cfs), half-full = ½ capacity,
  peak capacity 1.076× full at y/D≈0.938, critical depth at Froude = 1, Kirpich,
  TR-55.
- `tests/worked_example.rs` + `WORKED_EXAMPLE.md` — a full two-pipe network
  reproduced column-for-column.
- `tests/hgl_validation.rs` — HGL backwater (friction + structure loss +
  tailwater → 111.81 ft).

106 tests pass; debug and release builds are clean on stable Rust.

## Try it

```bash
# CLI
cargo run --bin stormsewer-cli -- examples/sample.ssn --size --review

# Web (or open the GitHub Pages URL)
./wasm/build.sh && cd wasm && python3 -m http.server
```

Import an existing Hydraflow `.STM` project and compare — that's the fastest way
to see the numbers line up with what you already trust.

## Known limitations

See `READINESS.md`. In brief: steady Rational peak flow only (no hydrograph
routing), single-K junction losses (not full FHWA structure-loss methodology),
non-circular shapes solved as equivalent circular, and multi-structure HGL not
yet validated against a published HEC-22 profile.
