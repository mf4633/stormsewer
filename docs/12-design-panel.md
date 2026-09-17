# 12. Storm-sewer design panel

Tools → Storm Sewer Design runs StormSewer's own storm-sewer engine —
Rational method, Manning, standard-step HGL, HEC-22 inlets, catalog sizing,
design review — on the open SWMM model, and can write the sizes it
recommends back into `[XSECTIONS]` as one undo step. It is the same engine
the storm-sewer workspace and the `stormsewer-cli` use; the equations are
in [chapter 15](15-methods.md) and the hand checks in `VALIDATION.md`.

The menu:

| Item | Does |
| --- | --- |
| Design Panel… | opens the panel |
| Analyse | maps the model and runs the analysis |
| Auto-size… | analyses and opens the sizing preview |
| Design Review | analyses and opens the Findings tab |
| Design Report (HTML)… | writes the design report for the mapped network as HTML |
| Design Report (PDF)… | the same report as PDF |

## 12.1 The design basis

The engine needs what a SWMM model does not have: an IDF curve, a return
period, a minimum time of concentration, a junction loss coefficient, a
minimum slope for flat pipes, and optionally a tailwater. These are a
project of their own, shown at the top of the panel:

- **IDF a, b, c** — `i = a / (t + b)^c`, in/hr with t in minutes; the line
  under the fields restates it (`i = 60.0/(t + 10.0)^0.80 in/hr, 10-yr`).
- **Return period (yr)**.
- **Min Tc (min)** — the floor on every inlet time.
- **Junction K** — the loss `K·V²/2g` at every structure.
- **Min slope (ft/ft)** — used for Manning capacity where the inverts give
  a slope of zero or less.
- **Tailwater (ft)** — a checkbox and a value; the starting HGL at the
  outfall when on.

The basis is seeded from the storm-sewer workspace's project the first
time the panel opens; **Copy from Storm Sewer workspace** does it again.
Editing the basis never touches that project. It is not saved in the `.inp`
(there is no section for it); it lives with the session, and the design
report prints it.

## 12.2 The mapping

Everything the panel computes depends on how a SWMM model becomes a
storm-sewer network. The rules, and what each one assumes:

**Units.** `FLOW_UNITS` CFS, GPM or MGD mean feet, acres and in/hr, which
is what the engine computes in. CMS, LPS and MLD are refused: the engine's
results are U.S. customary only, and a silent conversion would still report
feet and cfs for a metric model.

**Nodes.** `[JUNCTIONS]` become junctions, or inlets once a subcatchment or
an `[INLET_USAGE]` row drains to them; `[OUTFALLS]` become outfalls;
`[STORAGE]` and `[DIVIDERS]` become junctions with a note, because the
engine routes peaks and does not store or divert. Invert is `Elevation`;
rim is `Elevation + MaxDepth`, or the highest connecting crown when
`MaxDepth` is zero — the same rule the profile uses.

**Conduits.** `CIRCULAR`, `FORCE_MAIN` and `FILLED_CIRCULAR` map to circular
pipes by diameter; `RECT_CLOSED` to a box (`Geom1` rise × `Geom2` span);
`HORIZ_ELLIPSE` and `VERT_ELLIPSE` to elliptical (rise × span); `ARCH` to
arch. Every other shape — trapezoidal channels, custom, irregular, street —
and every pump, orifice, weir and outlet is listed on the **Skipped** tab
with the reason, never dropped silently. Length and Manning's n come from
`[CONDUITS]`; pipe end inverts are the node invert plus the offset,
honouring `LINK_OFFSETS`. A conduit with no `[XSECTIONS]` row is skipped.

**Subcatchments.** Each folds into the node it drains to, through other
subcatchments when `Outlet` names one: its area in acres; a runoff
coefficient from `%Imperv` by the straight line

```
C = 0.20 + 0.75 · (%Imperv / 100)
```

(0.20 fully pervious, 0.95 fully impervious — the usual Rational-table
band for lawns and pavement), area-weighted when several drain to one node;
and an inlet time by Kirpich over the overland flow length `Area / Width`
at `%Slope`, the largest kept when several drain to one node, floored at
Min Tc. The SWMM `Area` is authoritative; polygons are not re-measured. A
subcatchment whose outlet chain never reaches a node is skipped with
`outlet "…" does not reach a node`.

**Inlets.** `[INLET_USAGE]` rows mark their node as an inlet and carry the
`[INLETS]` grate length and width (or curb length) and `ON_SAG` placement
into the per-inlet HEC-22 overrides; a `STREET` cross-section on the
conduit supplies the cross slope. Inlet count and clogging are noted, not
carried.

**Tailwater.** A `FIXED` outfall's stage becomes the tailwater; other
outfall types keep the basis's value.

**Hydrology** (IDF, return period, Min Tc, junction K, min slope) comes
from the basis, since a SWMM model has none of it.

The **Notes** tab prints these assumptions as they applied to the open
model.

## 12.3 The tabs

- **Pipes** — per mapped conduit: slope, ΣCA, Tc, intensity, design Q,
  just-full capacity, velocity, percent full, surcharge flag, HGL at both
  ends.
- **Nodes** — invert, rim, HGL, freeboard.
- **Sizing** — the smallest catalog RCP (8 to 72 in: 8, 10, 12, 15, 18, 21,
  24, 27, 30, 33, 36, 42, 48, 54, 60, 66, 72) that carries the design flow
  within the criteria, beside the current size, with velocity and percent
  full at that size.
- **Inlets** — HEC-22 interception per inlet: gutter flow, spread,
  efficiency, intercepted and bypassed flow.
- **Skipped** — what did not map, and why.
- **Findings** — the design review (§12.5).
- **Notes** — the mapping's assumptions.

Click an id in any tab to select it on the map.

## 12.4 Auto-size

**Auto-size…** opens the *Auto-size preview* window: `N conduit(s) change
in [XSECTIONS]; one undo step.` and a table of conduit, shape, design Q,
before, after, note.
Only recommendations with a solution and a size different from the current
one are listed. **Apply — Auto-size N conduits** writes them:

- `CIRCULAR` and `FORCE_MAIN` get `Geom1` = the recommended diameter;
- `RECT_CLOSED` gets `Geom1` and `Geom2` = the recommended diameter (a
  square box of that side, which carries at least what the circle does);
- ellipses get `Geom1` = d and `Geom2` = 1.5 d, as the storm-sewer
  workspace does;
- arches are left as they are and noted.

`Ctrl+Z` reverts the whole batch. The sizing rule is in
[§15.5](15-methods.md).

## 12.5 Design review

The **Findings** tab checks the analysed network against the review
criteria (defaults in brackets):

- velocity below the minimum [2 ft/s] — warning; above the maximum
  [10 ft/s] — warning;
- design flow above the percent-full limit [85 %] — warning; surcharge
  (design flow above open-channel capacity) — error;
- slope below the minimum [0.0005] — warning; adverse (uphill) — error;
- cover from rim to crown below the minimum [1 ft] at either end — warning;
- a pipe smaller than the pipe feeding it — warning;
- HGL freeboard below the minimum [0.5 ft] — warning.

These are the storm-sewer workspace's criteria; the SWMM design panel uses
their defaults.

## 12.6 The design report

**Report HTML…** and **Report PDF…** write the storm-sewer design report
for the mapped network: the basis, pipe and structure and inlet schedules,
a plan schematic, a profile with HGL, and the review findings. It is the
same report the storm-sewer workspace produces. It is about the *mapped*
network; the Skipped list belongs beside it in any submittal.

## 12.7 Read this before you submit

The Rational pass and the SWMM run answer different questions. The
Rational pass gives a peak flow for sizing under one intensity chosen by
the pipe's own Tc; the SWMM run gives a hydrograph routed through storage,
surcharge and backwater under a hyetograph. They will not agree on peaks,
and the disagreement is informative
([§16.13](16-troubleshooting.md)). Use the design panel to size and to
check cover, velocity and freeboard; use the SWMM run to see what the sized
system does.
