# StormSewer — Readiness Assessment

An honest accounting of where this engine stands, written for the question
"could we put this in front of a serious engineering-software audience?" It
separates what is **proven**, what is **implemented but unvalidated**, and what
is **missing** relative to a production storm-sewer design product such as
Autodesk Hydraflow Storm Sewers.

Status date: 2026-07. Version 0.7.

---

## 1. What is solid today

- **Core hydraulics, analytically validated.** Manning capacity, partial-flow
  geometry, normal depth, critical depth, and max open-channel capacity are each
  pinned to a closed-form/hand-derived value in `tests/validation.rs` (e.g. a
  24-in pipe at n=0.013, S=0.005 → 16.04 cfs full-flow; half-full carries exactly
  ½ capacity; peak flow at y/D≈0.938 is 1.076× full). These are correctness
  proofs, not range checks.
- **Network method.** Rational C·A accumulation, Tc propagation with pipe travel
  time, and an HGL backwater pass with junction losses, over a topologically
  sorted dendritic network (loops rejected). The HGL pass is validated against
  hand-derived backwater calculations for both a single surcharged reach and a
  **multi-structure** system — two reaches in series through a junction manhole,
  with two friction segments and two structure losses (`tests/hgl_validation.rs`).
- **A full worked example** (`WORKED_EXAMPLE.md`) reproduces an independent
  hand calculation of a two-pipe network column-for-column.
- **Unit consistency** — the analysis is invariant under the U.S. ↔ SI toggle:
  design flows are identical and Manning capacity stays within metric-catalog
  snap tolerance (`tests/units_si.rs`), so the engine's internal US-customary
  computation is unit-correct.
- **Non-circular sections.** Box (rectangular), elliptical, and arch (vertical
  walls + semicircular top) conduits are solved on their own geometry — exact
  area/top-width and, for the ellipse, a numerically integrated wetted perimeter
  — through the full network analysis, not an equal-area circle. Validated by
  hand calc (rectangular Q and critical depth; arch full area/perimeter and
  springline continuity), by the ellipse collapsing exactly onto the circle at
  equal axes, and by the arch reducing to a semicircle when rise = span/2
  (`tests/sections.rs`, `hydraulics.rs`).
- **External published-example validation.** Beyond the internal hand calcs, the
  engine independently reproduces standard *published* worked examples: a
  partially-full circular sewer's normal depth (8-in, S=0.0033, n=0.013,
  Q=0.525 cfs → published 0.433 ft; engine 0.432 ft) and a Rational-method peak
  flow (0.813 ac, C=0.85, i=5.1 in/hr → published 3.52 cfs)
  (`tests/published_examples.rs`).
- **Hydrology.** Kirpich, TR-55 sheet flow, and FAA Tc validated against their
  published formulas; multi-return-period IDF sets.
- **Design + interoperability.** Standard-pipe sizing to velocity/percent-full
  criteria; HEC-22 inlet capacity; DXF / LandXML / Hydraflow `.STM` import and
  DXF / LandXML / PDF / HTML export; a desktop GUI (plan + profile + inspector,
  light/dark) and a CLI.
- **Engineering hygiene.** Builds clean on stable Rust (debug + release), 118
  tests pass (unit/integration + analytical validation suites), consistent
  GPL-3.0-or-later headers.

## 2. Implemented but NOT independently validated

These run and look right, but nothing yet pins them to an authoritative
reference (a published worked example or a Hydraflow run on the same input):

- HGL / backwater against a **published multi-structure HEC-22 profile**. The
  open-channel HGL is now a true **standard-step gradually-varied-flow backwater**
  (relaxing toward normal depth, with supercritical reaches controlled from
  upstream — `tests/hgl_validation.rs`), and pressurized reaches plus
  multi-structure surcharge are validated against independent hand calcs. Manning
  and Rational fundamentals are validated against published worked examples (§1).
  What remains: reproducing a full FHWA HEC-22 EGL profile with several
  access-hole losses in series table-for-table (needs the composite structure-loss
  corrections above; the FHWA HEC-22 PDFs were not fetchable from this build
  environment).
- HEC-22 inlet interception is now the real gutter-spread method (Izzard spread,
  frontal/side-flow split, curb `L_T`, sag weir/orifice; `src/design/inlets.rs`),
  validated for internal behaviour (bounded efficiency, splash-over effect,
  spread criterion, weir→orifice transition) but not yet pinned to the FHWA
  Chapter-4 chart examples. Grate splash-over velocity and clogging are inputs.
- PDF/HTML report output — content is correct but layout/print fidelity is
  unreviewed against what an engineer would stamp and submit.

## 3. Feature gaps vs. Autodesk Hydraflow Storm Sewers

The engine covers the Rational + Manning + HGL spine. A production peer also
provides, and these are **not** here yet:

- **Hydrograph routing** — Hydraflow routes hydrographs (not just Rational peak)
  and combines them at junctions. We compute steady peak flows only.
- **Rigorous structure losses** — the loss model supports a base junction K, a
  geometry-aware bend term (validated, `tests/bend_loss.rs`), and an opt-in
  **HEC-22 access-hole coefficient K₀** (relative access-hole size + deflection
  angle, with a plunging-flow factor; `src/access_hole.rs`). The remaining HEC-22
  composite corrections — flow-depth (C_d), relative-diameter (C_D), relative-flow
  (C_Q), and benching (C_B) — are not yet implemented, and the method is not yet
  pinned to a published FHWA worked example (the HEC-22 PDFs were not fetchable
  from this build environment).
- **Inlet bypass routing** — the HEC-22 gutter spread, interception efficiency,
  and bypass are now computed per inlet from its LOCAL gutter flow, but the
  bypass is not yet carried over to the next downstream inlet (each inlet is
  evaluated on its own local runoff).
- **Rainfall** — NOAA Atlas 14 / regional IDF ingestion and multiple design
  storms; user-defined intensity tables.
- **Section library breadth** — circular, box, elliptical, and arch are solved
  on their own geometry; other special shapes (e.g. horseshoe, low-profile arch),
  material-based Manning n libraries, and shape/gauge catalogs are not yet
  covered, and pipe *sizing* still recommends circular catalog diameters only.
- **QA/reporting** — code-compliant report templates per DOT, plan/profile sheet
  output, batch runs, and an audit trail.

## 4. Licensing & provenance (decided: free for the world)

The project is released **free for everyone under GPL-3.0-or-later** — an
open-source tool, not a product to be sold or licensed to a proprietary vendor.
That decision makes the licensing question simple: GPL is the correct license,
and no relicensing is needed.

- GPL copyleft is a feature here, not an obstacle: it keeps the methods open and
  auditable and any derivative distributions open too.
- Provenance is documented in [`PROVENANCE.md`](PROVENANCE.md): every method is a
  public, published standard (Manning, Rational, NRCS TR-55, Kirpich, FAA, FHWA
  HEC-22), and the file-format importers target documented/observed formats. The
  author-attestation section there should be completed and signed by the author.
- `Hydraflow`/`Autodesk` are trademarks of Autodesk, Inc.; StormSewer is an
  independent project, not affiliated with or endorsed by Autodesk.

## 5. Building trust with the world — recommended order

The goal is adoption by practicing engineers, so the priorities are the things
that let a stranger trust and use the tool:

1. **Provenance + license** — done (`PROVENANCE.md`, `LICENSE`); the author
   should complete the attestation section.
2. **Reference validation against published examples.** Partly done: the engine
   reproduces published Manning circular partial-flow and Rational-method worked
   examples (`tests/published_examples.rs`). Remaining: one fully worked HEC-22
   storm-drain example reproduced *table-for-table* end-to-end (flows, depths,
   HGL) — this needs the FHWA HEC-22 example tables, which were not fetchable
   here.
3. **One polished demo project + stamped-quality report.** A realistic network,
   run start-to-finish, exported to a report that looks like a submittal.
4. **Close the top scope gap** users ask about first — most likely FHWA structure
   losses or hydrograph routing — or state clearly that it is roadmap, not hidden.

Item 2 is the difference between "a promising prototype" and "numbers a
professional engineer can trust." The rest is reach and polish.
