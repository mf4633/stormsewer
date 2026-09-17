# 15. Methods

This chapter says what StormSewer computes itself, with the equations and
where they come from, and what it does not compute. The dividing line is
simple: the SWMM engine does the simulation; StormSewer does the
storm-sewer design arithmetic, the conversions in the mapping between the
two, and nothing else. Every number below is worked by hand in
`VALIDATION.md` and `WORKED_EXAMPLE.md` in the repository, and the tests
that pin them run on every commit.

## 15.1 What the SWMM engine computes (and this manual does not re-explain)

Rainfall, evaporation and climate; surface runoff by the nonlinear
reservoir; infiltration by Horton, modified Horton, Green-Ampt, modified
Green-Ampt and Curve Number; groundwater; snowmelt; RDII; flow routing by
steady, kinematic and dynamic wave; conduit geometry, storage geometry,
critical and normal depth; pumps, orifices, weirs and outlets; minor
losses, force mains, culverts, roadway weirs; water quality and LID;
streets and inlets in 5.2. StormSewer runs the engine and reads what it
wrote. Read the engine's own documents:

| Topic | Where |
| --- | --- |
| Object model, process models, simulation loop, interpolation and units | RM I Ch. 1 (1.2–1.5) |
| Precipitation, temperature, evaporation, wind | RM I Ch. 2 |
| Surface runoff: governing equations, subcatchment partitioning, time-step considerations, discretization, parameter estimates (width, n, depression storage) | RM I Ch. 3 (3.2–3.8) |
| Infiltration: Horton, modified Horton, Green-Ampt, Curve Number | RM I Ch. 4 (4.2–4.5) |
| Groundwater; snowmelt; RDII | RM I Ch. 5, 6, 7 |
| Hydraulic model: network components, analysis methods, boundary and initial conditions | RM II Ch. 2 |
| Dynamic wave: governing equations, solution, computational details, numerical stability | RM II Ch. 3 (3.1–3.4) |
| Kinematic wave, and its stability | RM II Ch. 4 |
| Cross-section geometry, custom shapes, irregular channels, storage geometry, critical and normal depths | RM II Ch. 5 |
| Pumps, orifices, weirs, outlets | RM II Ch. 6 |
| Evaporation and seepage, minor losses, force mains, culverts, roadway weirs | RM II Ch. 7 |
| Input file format | UM Appendix D |
| Streets and inlets | UM §3.3 (5.2) and the 2022 hydraulics addendum |

These are public-domain U.S. Government documents; the citations are the
whole of what this manual says about the engine's physics.

## 15.2 Storm-sewer engine: overview

The design tool ([chapter 12](12-design-panel.md)) is a peak-flow, steady
pipe-network calculation of the kind a drainage manual describes:

1. Rational peak flow accumulated down a dendritic network, with the
   intensity at each pipe taken from an IDF curve at that pipe's own time
   of concentration.
2. Manning capacity, normal and critical depth, velocity and percent full
   per pipe.
3. A standard-step hydraulic grade line from the outfall upstream, with
   junction losses.
4. HEC-22 inlet interception at inlets, with bypass carried to the next
   inlet.
5. Catalog sizing and a design review.

Units are U.S. customary throughout (feet, acres, cfs, in/hr, minutes).
Constants: Manning's `K = 1.486` (`3.280840^(1/3)`, the exact conversion,
and the value Hydraflow Storm Sewers uses); `g = 32.2 ft/s²`.

The network must be a tree draining to outfalls; loops and splits are
rejected by the topological sort. Storage, pumps and diversions are not
modelled here — that is the engine's job.

## 15.3 Hydrology

**IDF curve.**

```
i = a / (t + b)^c        i in in/hr, t in minutes
```

Coefficients come from the design basis, from a NOAA Atlas 14 PFDS CSV
fitted by least squares per return period (storm-sewer workspace, File →
Import NOAA Atlas 14 IDF), or from a Hydraflow `.stm`. A frequency factor
`Cf` may raise the effective coefficient, `min(C·Cf, 1)`, for the higher
return periods, as HEC-22 and most DOT manuals require.

**Rational method.**

```
Q = ΣCA · i(t_c)        Q in cfs, ΣCA in acres, i in in/hr
```

The acre-inch-per-hour to cfs factor of 1.008 is taken as 1, the usual
drainage-manual practice. ΣCA accumulates downstream; `i` falls as `t_c`
grows, so `Q` rises less than proportionally with area.

**Time of concentration.** Each pipe's `t_c` is the larger of its own inlet
time and the upstream `t_c` plus the travel time through the upstream pipe
at its design velocity, `L / V`, floored at the project's minimum Tc.
Inlet times are entered, or estimated:

- Kirpich (1940), `l` in ft, `s` in ft/ft;
- NRCS TR-55 (1986) Eq. 3-3 sheet flow, `T_t = 0.42 (nL)^0.8 / (P_2^0.5
  s^0.4)` in minutes, with `P_2` the 2-year 24-hour depth in inches;
- FAA (1970), `T_c = 1.8 (1.1 − C) L^0.5 / S^(1/3)`, `S` in percent.

Tools → Tc Calculator… computes all three and a TR-55 multi-segment
worksheet.

**The C-from-%Imperv mapping** (SWMM → design):

```
C = 0.20 + 0.75 · (%Imperv / 100)
```

A straight line from 0.20 (fully pervious) to 0.95 (fully impervious),
the band the Rational-C tables give for lawns and pavement. Area-weighted
when several subcatchments drain to one node. The inverse,
`%Imperv = 100 (C − 0.20) / 0.75` clamped to 0–100, is used on import. The
inlet time for a subcatchment is Kirpich over the flow length `Area /
Width` at `%Slope`. This is a stated assumption, not a derivation: a SWMM
subcatchment has no C, and a C has to come from somewhere. Override it by
editing the design basis or the imported project.

## 15.4 Hydraulics

**Manning full-flow capacity.**

```
Q_full = (K/n) · A · R^(2/3) · √S       A = πD²/4,  R = D/4 for a circle
```

Box, elliptical and arch sections use their exact geometry (no table
lookups). The maximum open-channel capacity of a circular pipe is about
1.076 × `Q_full`, at y/D ≈ 0.938; flows above it are surcharged.

**Slope** is from the inverts, `(inv_up − inv_dn) / L`; a pipe's own end
inverts, where the SWMM offsets give them, override the node inverts. A
zero or adverse slope uses the basis's minimum slope for capacity and is
flagged.

**Normal depth** is found by iteration on the section's `A R^(2/3)` at
the design flow; **critical depth** by iteration on `Q² T / (g A³) = 1`.
For a circular section at depth `y`:

```
θ = 2 arccos((r − y)/r),   A = r²(θ − sin θ)/2
```

**Velocity** is `Q / A(y_n)`, the actual area at normal depth, not the full
barrel. **Percent full** is a discharge ratio, `Q / Q_full`, not a depth
ratio.

**Hydraulic grade line.** A backward pass from the outfall upstream, one
pipe at a time:

- the outfall HGL is the tailwater when set, else the outfall invert (a
  free outfall);
- each pipe is classified by its normal depth against its critical depth,
  with a ±2 % dead band, and solved accordingly:
  - **pressurised** (design flow above the open-channel maximum, or the
    downstream HGL above the crown): full-pipe friction over the reach,
    `S_f = (Q / K_conv)²` with the full-section conveyance
    `K_conv = (K/n) A R^(2/3)`;
  - **supercritical** (normal depth below critical) and not drowned from
    downstream: controlled from upstream; the upstream water surface is
    the reach's own normal depth and tailwater does not back up through it
    (no hydraulic-jump analysis);
  - **subcritical**: a true standard-step gradually-varied-flow profile
    from the downstream water surface (at least critical depth), the
    reach divided into sub-reaches, the friction slope `S_f = (Q/K_conv)²`
    evaluated at the *actual* local depth, and the energy balance
    `EGL_up = EGL_dn + S_f,avg · Δx` solved for the subcritical depth at
    each station; when no root exists the profile relaxes to normal depth;
  - **adverse** slope: the downstream HGL is recorded and no head loss is
    propagated upstream, since `S_f` is undefined;
- at each structure the entering flow loses `H = K · V²/2g` with the
  basis's junction K (plus a geometry-aware bend term for the deflection
  between the incoming and outgoing pipe when the workspace's bend-loss
  coefficient is non-zero); a supercritical reach is upstream-controlled,
  so a downstream structure loss does not raise its upstream HGL. When
  HEC-22 access-hole losses are switched on in the storm-sewer workspace,
  `K_ah = K_o C_D C_d C_Q C_p C_B` with `K_o = 0.1 (b/D_o)(1 − sin θ) + 1.4
  (b/D_o)^0.15 sin θ` and the HEC-22 correction factors for relative
  diameter, flow depth and plunging, `C_Q = 1` and benching `C_B` supplied
  (HEC-22 Ch. 7). The access-hole method is opt-in and is not yet pinned to
  a published FHWA worked example; the simple K is the default.

Freeboard is rim minus HGL at each structure; a negative freeboard is a
surcharge to the surface.

`VALIDATION.md` §6 works this on a three-pipe trunk to six decimal places,
and §8 runs a real Civil 3D network side by side with Hydraflow Storm
Sewers, agreeing to 0.04 % on the terminal lines and 2.5 % on the outfall
line with every difference explained.

## 15.5 Sizing and review

**Catalog.** RCP 8, 10, 12, 15, 18, 21, 24, 27, 30, 33, 36, 42, 48, 54, 60,
66, 72 in. (Metric sizes exist for the storm-sewer workspace's SI input
mode: 300 to 1800 mm.)

**Rule.** For each pipe, the smallest catalog diameter for which, at the
design flow and the pipe's slope:

- the flow does not exceed the open-channel maximum (no surcharge);
- velocity does not exceed the maximum (10 ft/s);
- percent full does not exceed the limit (85 %).

Minimum velocity (2 ft/s) is *not* a sizing filter: velocity falls as
diameter grows, so rejecting a size for low velocity would reject every
larger size too and report "no solution" on flat sewers. Low velocity is a
review finding that says "steepen the pipe" instead. A pipe already meeting
the criteria is reported as adequate at its current size. If no catalog
size works, the largest is reported with its hydraulics as a diagnostic.

**Review** ([§12.5](12-design-panel.md)): velocity band, percent full,
surcharge, slope (adverse is an error), cover from rim to crown at each end
using the section's real rise, size progression downstream, HGL freeboard.

## 15.6 HEC-22 inlets

On-grade inlets (HEC-22 Ch. 4, gutter flow and inlet interception):

- Gutter spread from Izzard's form of Manning for a triangular section,
  `Q = (0.56/n) S_x^1.67 S_L^0.5 T^2.67`, solved for `T`; gutter velocity
  `V = Q / (½ S_x T²)`.
- Grate on grade: `E = R_f E_o + R_s (1 − E_o)`, with `E_o` the frontal
  fraction, `R_f` the frontal capture (1 below the grate's splash-over
  velocity, which is grate-specific and is an input), `R_s` the side
  capture.
- Curb opening on grade: `L_T = 0.6 Q^0.42 S_L^0.3 (1/(n S_x))^0.6`,
  `E = 1 − (1 − L/L_T)^1.8`, or 1 when `L ≥ L_T`.
- Combination on grade is taken as the grate's efficiency; summing the
  two would count the same gutter flow twice.

Sag inlets: grate weir `Q = 3.0 P d^1.5` with `P = L + 2W` (the curb side
excluded), transitioning to orifice `Q = 0.67 A √(2g d)`, the smaller
governing; curb opening weir `Q = 2.3 (L + 1.8W) d^1.5` for `d ≤ h`, orifice
`Q = 0.67 h L √(2g (d − h/2))` when submerged. A clogging fraction reduces
open area and perimeter.

The flow arriving at an inlet is its local catchment's peak at the
intensity for its *own* inlet time (floored at Min Tc), not the pipe
system's accumulated Tc — HEC-22 §4 and Hydraflow's "i Inlet". Intercepted
flow is `E·Q` on grade or the sag capacity capped at `Q`; the bypass is
routed to the next designated inlet. The inlet pass is a surface check and
does not change the pipe design flows, which assume everything reaches
the system.

## 15.7 Design storms

Project → Design Storm… ([§8.9](08-project-dialogs.md)) builds a
hyetograph by one of six methods. All sources are public; the tables are
checked in the tests against the cited figures. Depths are in the model's
rain unit and nothing is converted.

**NRCS (SCS) 24-hour Type I, IA, II, III.** The cumulative mass curves
are the NRCS TR-20 tabular rainfall distributions (Type IA and II from
the 1982 tables, Type I and III from the 1992 tables shipped with TR-20
2/92) at 0.1-hour points (Type IA at 0.5-hour points), scaled by the
24-hour depth you enter and differenced at your time step. Their
derivation and the hourly Type II ratios are in USDA-NRCS *National
Engineering Handbook* Part 630, Chapter 4, §630.0403 and figure 4-36; the
0.1-hour tables are also reproduced in WSDOT *Highway Runoff Manual*
M 31-16.04 Appendix 4C, tables 4C-3 and 4C-4. The Type II nested ratios
of figure 4-31 (5 min 0.114, 10 min 0.201, 15 min 0.270, 30 min 0.380,
1 h 0.454, 2 h 0.538, 3 h 0.595, 6 h 0.707, 12 h 0.841) are kept as a
check on the table.

**NRCS NOAA Atlas 14 regional 24-hour, regions A–D.** NEH 630 Chapter 4
§630.0408, figure 4-72: the ratio of the 5-minute … 12-hour depth to the
24-hour depth for each region, built into a curve by the chapter's nesting
rule — every shorter duration's depth centred inside the longer one
(§630.0403 A(9), §630.0407) — with log-linear interpolation between the
tabulated durations. The handbook additionally smooths in WinTR-20; this
does not.

**Alternating block from an IDF curve.** For blocks of `Δt` minutes over
a duration `D`: the cumulative depth at each block boundary, `P(t) = i(t)
· t / 60` with `i = a/(t+b)^c`; the incremental depth of block `k`,
`P(kΔt) − P((k−1)Δt)`, never negative; the increments sorted descending,
the largest at the centre block and the rest alternately right and left of
it, spilling to the other side at an edge; each block's intensity `depth /
(Δt/60)`. The storm reproduces the curve's average intensity over every
duration up to `D`. Chow, Maidment & Mays, *Applied Hydrology* (1988)
§14.4. The same routine makes the storm when a storm-sewer project is
imported ([§13.1](13-import-export.md)), with `Δt` = 5 min and `D` =
120 min.

**Alternating block from a NOAA Atlas 14 PFDS row.** The same placement,
but the cumulative depths at the tabulated durations come straight from
the pasted PFDS csv (`5-min:,d1,d2,…` under `by duration for ARI
(years):,1,2,5,…`) for the chosen return period, interpolated between
durations; no curve is fitted.

**Chicago (Keifer & Chu 1957).** From `i = a/(t+b)^c`, a duration `D`
and a peak position `r` (the peak is at `t_p = r·D`). With `P(t) =
i(t)·t/60` the IDF depth over `t` minutes, the cumulative depth in the
`t_b` minutes before the peak is `r · P(t_b / r)` and in the `t_a` minutes
after it is `(1 − r) · P(t_a / (1 − r))`; the mass curve is `r·P(D) −
r·P((t_p − t)/r)` before the peak and `r·P(D) + (1 − r)·P((t − t_p)/(1 −
r))` after it, and the total is `P(D)`. Differenced at the time step, that
gives the hyetograph whose average intensity over any interval centred on
the peak equals the IDF curve. Chow, Maidment & Mays §14.4.

**Uniform.** The depth spread evenly over the duration.

## 15.8 Conduit lengths

Not automatic. A conduit drawn on the map is given its drawn length, in
map units, once, at creation. Project → Compute Conduit Lengths…
([§3.10](03-map-and-tools.md)) computes the geometric length through the
vertices, `L = Σ √(Δx² + Δy²)` over the segments from the start node
through each vertex to the end node, times a map-to-model factor from
`[MAP] Units` and `FLOW_UNITS` (1 for feet→feet or metres→metres,
0.3048 for map feet in a metric model, 3.28084 for map metres in a US
model, undefined for degrees), and writes the ticked ones. It is never
run without being asked.

## 15.9 Units

**SWMM.** `FLOW_UNITS` decides the unit system of the whole model (RM I
§1.5): CFS, GPM and MGD are U.S. customary (feet, acres, inches, in/hr),
CMS, LPS and MLD are SI (metres, hectares, millimetres, mm/hr). The engine
converts nothing when the keyword changes; it interprets every number in
the declared system, so a weir coefficient of 3.33 in a CMS model is read
as a metric coefficient. StormSewer labels fields by the keyword and,
when you change it in the Options dialog, opens the unit-switch wizard
([§8.2](08-project-dialogs.md)), which lists the groups of values the
engine will misread and converts the ones you tick with

| | |
| --- | --- |
| 1 ft | 0.3048 m |
| 1 acre | 0.40468564 ha |
| 1 in | 25.4 mm |
| 1 cfs | 0.028316847 m³/s = 448.83117 gpm = 0.64631689 mgd |
| weir `Cw` | 3.33 ft^0.5/s ↔ 1.84 m^0.5/s (× 0.5521) |
| Manning's n, orifice `Cd` | unchanged |

The checklist is [§16.7](16-troubleshooting.md).

**Storm-sewer engine.** Computes in U.S. customary. The storm-sewer
workspace's SI input mode converts inputs on the way in:

| Input | Factor |
| --- | --- |
| length, m → ft | × 3.280840 |
| area, ha → ac | × 2.471054 |
| IDF `a`, mm/hr → in/hr | ÷ 25.4 |
| pipe size, mm catalog → ft | nearest metric size, then ÷ 304.8 |

Results (schedules, reports) remain U.S. customary; full SI output is on
the roadmap. The SWMM design panel refuses metric models rather than
convert silently ([§12.2](12-design-panel.md)).

**Offsets.** With `LINK_OFFSETS DEPTH` a link's `InOffset`/`OutOffset` are
heights above the node invert. With `ELEVATION` they are absolute
elevations, and StormSewer converts them to depths as `offset − node
invert`, never below zero, wherever it needs a pipe invert (profile, design
mapping, CSV). Reading never writes anything back. Changing the keyword in
the Options dialog offers to rewrite every offset with its node's invert
in the same undo step (conduits from the from-node for `InOffset` and the
to-node for `OutOffset`; orifices, weirs and outlets from their inlet
node), so the geometry keeps its meaning; without that the pipes move.
The engine's own silent corrections are in [§16.6](16-troubleshooting.md).

## 15.10 Result statistics

The Results views compute only: five-class breaks by equal interval or
quantile over the frame's values; per-series min, max, mean, sum and the
top-N local maxima; peak depth, inflow, flooding, flow, velocity and
capacity per object over the run (the maximum of the reported values);
and the difference and percent difference between two runs' peaks. Nothing
is interpolated between reporting periods.
