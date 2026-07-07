# StormSewer v0.1.0

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
