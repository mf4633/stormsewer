# stormsewer

Native-Rust storm-sewer **hydrology & hydraulics** engine — an open recreation
of the standard, public-domain methods used by tools such as Autodesk Hydraflow
Storm Sewers.

`0.1.0` · GPL-3.0-or-later · engine only (no GUI, no CAD dependencies), so it
compiles to a native library, to WASM, and can be embedded in desktop or web
apps.

## Methods

- **Rational method** peak-flow accumulation (`Q = C·i·A`) down a dendritic pipe network.
- **Manning** open-channel / partial-flow hydraulics for circular conduits — exact
  geometry (no table lookups): normal depth, critical depth, full-flow and
  maximum (~0.94 d) capacity, velocity.
- **Time of concentration** — Kirpich, NRCS TR-55 sheet flow, FAA; travel time
  accumulated pipe-by-pipe.
- **HGL backwater** pass with junction losses (`H = K·V²/2g`), tailwater seeding,
  and surcharge / adverse-slope handling.
- **Standard-pipe sizing** — smallest catalog diameter meeting velocity and
  percent-full criteria (Hydraflow-style design checks).
- **HEC-22** inlet capacity (grate, curb opening, combination, sag) and
  multi–return-period IDF sets.

All units are US customary (feet, seconds, cfs) unless a metric Manning/gravity
constant is passed. Implementations are intentionally simple and standards-based
so they can be audited against hand calculations.

## Library usage

```rust
use stormsewer::{Network, Node, Pipe};

let net = Network {
    nodes: vec![
        Node::inlet("N1", 100.0, 105.0, 2.0, 0.7), // invert, rim, area (ac), C
        Node::inlet("N2", 99.0, 104.0, 3.0, 0.8),
        Node::outfall("OUT", 98.0, 103.0),
    ],
    pipes: vec![
        Pipe::new("P1", "N1", "N2", 100.0, 1.5, 0.013), // length, dia (ft), n
        Pipe::new("P2", "N2", "OUT", 100.0, 1.5, 0.013),
    ],
};

// Quick check at a constant intensity (i = 4 in/hr):
let results = net.analyze_rational(4.0).unwrap();

// Full analysis (Tc → IDF intensity → design Q → hydraulics → HGL):
// let analysis = net.analyze(&idf_curve, &AnalysisOptions::default()).unwrap();
```

See `src/lib.rs` for the full rustdoc, `src/network.rs` and `src/hydraulics.rs`
for the core, and `examples/sample.ssn` for an input file.

## CLI

A command-line binary is built from the `stormsewer-cli` bin target:

```bash
cargo run --bin stormsewer-cli -- examples/sample.ssn
```

## WASM

```bash
wasm-pack build --target web --out-dir pkg
```

Import the generated `pkg/stormsewer.js` and call the bound functions (e.g.
`rational_peak`, `manning_full_flow_circular`, `normal_depth_circular`). See
`examples/wasm-playground.html` for a working page.

## Build & test

```bash
cargo build
cargo test        # 96 tests across engine, I/O, and the GUI app
```

Requires stable Rust (edition 2021).

## Repository layout

| Path            | Contents                                                        |
| --------------- | --------------------------------------------------------------- |
| `src/hydraulics.rs` | Circular open-channel hydraulics (Manning, normal/critical depth) |
| `src/network.rs`    | Network model, Rational accumulation, HGL backwater pass       |
| `src/hydrology/`    | Tc estimators, TR-55, IDF curves and sets                     |
| `src/design/`       | Pipe sizing, design criteria, HEC-22 inlets, review, cost      |
| `src/io/`           | DXF, LandXML, PDF, HTML, project and `.stm` import/export      |
| `app/`              | egui desktop application (plan view, editing, reports)         |
| `examples/`         | Sample inputs and a WASM playground                            |

## License

GPL-3.0-or-later. See SPDX headers in each source file.
