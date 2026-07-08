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
- **Non-circular sections.** Box (rectangular) and elliptical conduits are solved
  on their own geometry — exact area/top-width and, for the ellipse, a
  numerically integrated wetted perimeter — through the full network analysis,
  not an equal-area circle. Validated by hand calc (rectangular Q and critical
  depth) and by the ellipse collapsing exactly onto the circle at equal axes
  (`tests/sections.rs`, `hydraulics.rs`).
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

- HGL / backwater against a **published** HEC-22 profile. Single-reach and
  multi-structure backwater are now validated by independent hand calculation
  (§1), but no specific published FHWA example has been reproduced table-for-table.
- HEC-22 inlet capacities (grate/curb/combination/sag) — the forms are
  simplified; no check against the FHWA chart examples.
- PDF/HTML report output — content is correct but layout/print fidelity is
  unreviewed against what an engineer would stamp and submit.

## 3. Feature gaps vs. Autodesk Hydraflow Storm Sewers

The engine covers the Rational + Manning + HGL spine. A production peer also
provides, and these are **not** here yet:

- **Hydrograph routing** — Hydraflow routes hydrographs (not just Rational peak)
  and combines them at junctions. We compute steady peak flows only.
- **Rigorous structure losses** — FHWA HEC-22 access-hole/junction energy-loss
  methodology (entrance/exit, bend, plunging flow) rather than a single K·V²/2g.
- **Inlet computations on grade** — gutter spread, bypass/carryover chained
  downstream, sag ponding. We size a pipe and check an inlet in isolation.
- **Rainfall** — NOAA Atlas 14 / regional IDF ingestion and multiple design
  storms; user-defined intensity tables.
- **Section library breadth** — box and elliptical are solved on their own
  geometry; arch and other special shapes, material-based Manning n libraries,
  and shape/gauge catalogs are not yet covered, and pipe *sizing* still
  recommends circular catalog diameters only.
- **QA/reporting** — code-compliant report templates per DOT, plan/profile sheet
  output, batch runs, and an audit trail.

## 4. The licensing / IP question (read before pitching Autodesk)

This is a strategic blocker, not a technical one, and it is easy to miss:

- The project is **GPL-3.0-or-later**. GPL is strong copyleft — it cannot be
  absorbed into a proprietary product. An acquirer/partner who ships closed
  software cannot simply take GPL code in. If the goal is to license or sell to
  a proprietary vendor, the licensing has to be resolved first (dual-licensing
  requires that *we* own or can relicense 100% of the code).
- The product is a **re-creation of a specific commercial product** (Hydraflow
  Storm Sewers) and includes a **`.STM` importer** for that product's format.
  Interop is a feature, but before an external presentation we should be able to
  state plainly that the implementation is clean-room — derived from public
  standards (Manning, Rational, HEC-22), not from the original product's code or
  proprietary outputs — and that the `.STM` format support was built from
  independent inspection. Have that provenance answer ready.

## 5. Getting to a credible presentation — recommended order

1. **Provenance + licensing memo.** One page: clean-room basis, standards cited,
   and the licensing position. This gates everything else for an external pitch.
2. **Reference validation against a published example.** Take one fully worked
   HEC-22 / textbook storm-sewer example and reproduce its table end-to-end
   (flows, depths, HGL) to stated tolerance. One credible side-by-side is worth
   more than a hundred internal tests.
3. **One polished demo project + stamped-quality report.** A realistic network,
   run start-to-finish, exported to a report that looks like a submittal.
4. **Close the top scope gap** the audience will ask about first — most likely
   FHWA structure losses or hydrograph routing — or state clearly that it is
   roadmap, not hidden.

Items 1 and 2 are the difference between "a promising prototype" and "numbers a
professional engineer can trust." The rest is polish.
