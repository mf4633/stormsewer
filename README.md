# stormsewer

Native-Rust storm-sewer network **hydrology & hydraulics** engine.

**0.1.0** (GPL-3.0-or-later). WASM-friendly (cdylib + web target). Engine only — no GUI, no CAD deps. Compiles to native library, WASM (for web apps like HydroComplete), and embeddable modules.

Part of the open hydrology core (with hydro-tools Python + hc-refactored JS calc mirrors) for **Phase 3 realization**: verifiable, auditable public-domain methods (Rational, SCS, Manning) made visible and consumable.


Ties directly to the project [STRATEGY.md](../STRATEGY.md) three goals:
- **Knowledge**: Open, mirrored, documented implementations of standard methods (no black boxes).
- **Openness**: Free to use/audit/extend. Exact consumption + contribute path documented.
- **Profit**: Credible foundation for pro layers (FieldHydro AR/field tools + HydroComplete professional modeling, provenance, batch, CAD).

See the canonical publish polish artifacts for full details:
- [RELEASE_NOTES.md](../RELEASE_NOTES.md) (0.1 status, polished commands/examples, pro notes, 3-goal tie-in)
- [0.1-QUICKSTART.md](../0.1-QUICKSTART.md) (beginner consumption + "how to contribute a method")
- [OUTREACH-TEMPLATE.md](../OUTREACH-TEMPLATE.md) (pilot/contribute/feedback template for Phase 3 activation + 5 leads)
- WASM playground example: `examples/wasm-playground.html` (or the path under dev/OpenCADStudio/crates/stormsewer in the broader tree)


## Quick Consumption (0.1)

### WASM for web / JS
```bash
cd stormsewer
wasm-pack build --target web --out-dir pkg
```
Then import the generated `pkg/stormsewer.js` (see lib.rs docs and the playground HTML for bound functions like `rational_peak`, `manning_full_flow_circular` (0.2 open core spike), `manning_friction_head_loss` (0.2 additional HGL/energy step for network)).


### Native Rust (library usage)
```rust
use stormsewer::{Network, Node, NodeKind, Pipe};

let net = Network {
    nodes: vec![
        Node::inlet("N1", 100.0, 105.0, 2.0, 0.7),
        Node::outfall("OUT", 99.0, 104.0),
    ],
    pipes: vec![Pipe::new("P1", "N1", "OUT", 100.0, 1.5, 0.013)],
};
let results = net.analyze_rational(4.0).unwrap();
```

See `src/lib.rs` (full rustdoc with inline example + Phase 3 notes + pointer to 0.1-QUICKSTART), `Cargo.toml`, `src/network.rs`, `src/hydraulics.rs`.



CLI binary also available via the bin target.

## Methods (0.1 Scope)
- Rational method peak-flow accumulation across pipe network.
- Manning open-channel / partial-flow hydraulics for circular conduits (normal depth, critical depth, full-flow capacity, velocity).
- **0.2 spike (Phase 3)**: `manning_full_flow_circular` (simple full circular capacity primitive) added + WASM exported for open consumption (mirrored in hydro-tools + hc calc). See lib.rs (dev crate) + root 0.1-QUICKSTART/RELEASE_NOTES.
- **0.2 additional (Phase 3 / STRATEGY "next wave 0.2 methods" + Priya "network hydraulics extension")**: `manning_normal_flow_trapezoidal` (trapezoidal/rect channel normal flow capacity) added mirrored + WASM exported in dev crate src/lib.rs (demo fn too). Complements circular. Pure, standard Manning+trap geo. See dev crate lib.rs (WASM), hydro-tools/rational.py, hc calc; exposed in wasm-playground + pe-calc mannings blurb; usage in root quickstart/RELEASE + this. Exact cross-verif (e.g. 17.656 cfs test case). Fully open.

All implementations are intentionally simple, public-domain, and mirrored in the Python/JS siblings for cross-verification and education.

## Contribute
See root [RELEASE_NOTES.md](../RELEASE_NOTES.md) and [0.1-QUICKSTART.md](../0.1-QUICKSTART.md) for concrete steps (add primitive + test + docs across mirrors; use engine-feedback template).

Issue template: [.github/ISSUE_TEMPLATE/engine-feedback.md](../.github/ISSUE_TEMPLATE/engine-feedback.md)

## Pro Context
The engine is the free open core. Real impact + revenue come from:
- FieldHydro (mobile/AR dam safety, field verification — see fieldhydro/ and pe-calc/field/).
- HydroComplete (hc-refactored — full modeling, gated provenance exports, batch, CAD integration via OpenCAD).


**0.1 publish polish** (this README + root RELEASE_NOTES + cross-links) makes the Rust/WASM engine a first-class published, consumable component. Previously visibility was mostly in lib.rs rustdoc; now it has a proper crate README tying everything together.

For the bigger picture: [STRATEGY.md](../STRATEGY.md) (Phase 3 section).


This README + crate (C:\Users\michael.flynn\dev\OpenCADStudio\crates\stormsewer\) + top stormsewer/ ensure latest 0.2 (Manning full + trap + routing) + Tauri/FieldHydro pro + dispatch package refs visible. Open core (knowledge) + contribute (openness via template) + pro on top (profit: FieldHydro/HydroComplete/Tauri desktop without gating). 


*stormsewer — 0.1 engine component (2026).*





## Latest 0.2 (normal_depth_circular ~1.000 ft, energy_grade_line_step / EGL ~0.500 ft + priors HGL~0.500, critical~0.658, routing 6.321 + full set) + Pro Integration Update (targeted append per task)

**Exact usage/numerics (matching across mirrors; priors no breakage: manning_full_flow_circular ~15.996 cfs; manning_normal_flow_trapezoidal ~17.656 cfs trap; simple_linear_reservoir_routing ~6.321 cfs; manning_friction_head_loss ~0.500 ft HGL; critical_depth_circular ~0.658 ft; EGL matches HGL for uniform):**
```bash
# Python (hydro-tools)
pip install -e hydro-tools
python -c "
from hydro_tools.rational import normal_depth_circular, energy_grade_line_step, manning_normal_flow_trapezoidal, manning_friction_head_loss, critical_depth_circular
print(normal_depth_circular(2.0, 0.013, 0.005, 25.393))  # ~1.000 ft
print(energy_grade_line_step(17.656, 0.013, 3.0, 0.6708, 100.0))  # ~0.500 ft
print('priors:', manning_normal_flow_trapezoidal(2.0,1.0,1.0,0.013,0.005), manning_friction_head_loss(17.656,0.013,3.0,0.6708,100.0), critical_depth_circular(10.0,2.0))
"
# or CLI: python -m hydro_tools.cli normal-depth --d 2 --n 0.013 --s 0.005 --q 25.393 ; python -m hydro_tools.cli egl-step --q 17.656 --n 0.013 --a 3 --r 0.6708 --l 100
```

```bash
# WASM / Rust (stormsewer; after build in dev/OpenCADStudio/crates/stormsewer or equiv)
cd stormsewer
wasm-pack build --target web --out-dir pkg
# import init, { normal_depth_circular, energy_grade_line_step, ... } from './pkg/stormsewer.js'; await init(); ...
```

```js
// JS (hc-refactored; direct, used in apps)
import { normalDepthCircular, energyGradeLineStep } from './src/calc/index.js';
// or via window.HC
const yn = normalDepthCircular(2.0, 0.013, 0.005, 25.393);
const de = energyGradeLineStep(17.656, 0.013, 3.0, 0.6708, 100.0);
```

Consumption commands (mirrors note): pip install -e hydro-tools; python -c "from hydro_tools.rational import *"; cd stormsewer; wasm-pack build --target web; JS import from pkg or hc calc. See hydro-tools/rational.py + cli.py, stormsewer (top + dev/OpenCADStudio/crates/stormsewer/src/lib.rs + Cargo), hc, pe-calc/tools, wasm-playground.html, 0.1-QUICKSTART/RELEASE.


