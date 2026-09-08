# Validation

StormSewer implements public-domain methods, so every number it reports can be
reproduced by hand. This document does exactly that for a reference network:
each engine output below is followed by the hand calculation that produces it,
and the two agree to six decimal places.

Reproduce the engine side yourself:

```sh
cargo run --example validation_dump      # every number below, full precision
stormsewer-cli examples/sample.ssn       # the same run through the CLI
```

The numbers here are also asserted by `validation_reference_network` in
`tests/headless_suite.rs`, so a change to the engine that moves any of them
fails the build rather than silently invalidating this page.

## Constants

| Symbol | Value | Where |
| --- | --- | --- |
| `K` (Manning, US customary) | 1.49 | `hydraulics::K_MANNING_US` |
| `g` | 32.2 ft/s² | `hydraulics::G_US` |

A note on `K`: Manning's conversion factor is often written 1.486 (exactly
3.2808¹ᐟ³). StormSewer uses **1.49**, matching the value used in FHWA HDS-5 and
HEC-22 and by the commercial storm sewer packages this is meant to be checked
against. The difference is 0.27%. If you are reconciling against a spreadsheet
that uses 1.486, expect capacities to differ by that amount.

## Reference network

[`examples/sample.ssn`](examples/sample.ssn) — three pipes down a trunk to a
fixed-tailwater outfall.

> **On the demo project.** The GUI's built-in demo is this same trunk *plus* a
> drawn catchment (C1), and the app merges catchment areas into inlet
> hydrology before analyzing. That is correct behaviour, but it means the demo
> reports higher flows than the numbers below (P3 = 8.663 rather than 8.477).
> This page uses `sample.ssn`, which has no catchment, so the hand calculation,
> the CLI, the Python bindings, and this document all describe one network.

```
IDF        i = 60 / (t + 10)^0.8      (in/hr, t in minutes)
TAILWATER  100.5 ft
MINTC      10 min
JUNCTIONK  0.5
```

| Node | Kind | Invert | Rim | Area | C | Inlet Tc |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| N1 | inlet | 104.00 | 110.00 | 1.00 ac | 0.70 | 12.0 min |
| N2 | inlet | 102.50 | 108.50 | 1.00 ac | 0.70 | 10.0 min |
| N3 | junction | 101.20 | 107.00 | 0.50 ac | 0.80 | 8.0 min |
| OUT | outfall | 100.00 | 106.00 | — | — | — |

| Pipe | From | To | Length | Diameter | n |
| --- | --- | --- | ---: | ---: | ---: |
| P1 | N1 | N2 | 300 ft | 1.25 ft (15 in) | 0.013 |
| P2 | N2 | N3 | 250 ft | 1.50 ft (18 in) | 0.013 |
| P3 | N3 | OUT | 180 ft | 1.75 ft (21 in) | 0.013 |

## 1. Rainfall intensity

`i = a / (t + b)^c` with a = 60, b = 10, c = 0.8.

| t (min) | Engine i (in/hr) | Hand |
| ---: | ---: | ---: |
| 10.000000 | 5.461693 | 60 / 20^0.8 = 5.461693 |
| 12.000000 | 5.060729 | 60 / 22^0.8 = 5.060729 |
| 13.213436 | 4.847968 | 60 / 23.213436^0.8 = 4.847968 |
| 14.070853 | 4.709319 | 60 / 24.070853^0.8 = 4.709319 |

## 2. Manning full-flow capacity

`Q_full = (K/n) · A · R^(2/3) · √S`, with `A = πD²/4` and `R = D/4`.

P1: D = 1.25 ft, n = 0.013, S = (104.00 − 102.50)/300 = 0.005

```
A = π(1.25)²/4          = 1.227185 ft²
R = 1.25/4              = 0.312500 ft
Q = (1.49/0.013)(1.227185)(0.312500^0.6667)(√0.005)
                        = 4.580060 cfs
```

| Pipe | S | Engine capacity | Hand |
| --- | ---: | ---: | ---: |
| P1 | 0.005000 | 4.580060 | 4.580060 |
| P2 | 0.005200 | 7.595176 | 7.595176 |
| P3 | 0.006667 | 12.972250 | 12.972250 |

## 3. Rational method accumulation

`Q = ΣCA · i`, with `i` taken at the pipe's own time of concentration.

| Pipe | ΣCA | Contributors | t (min) | i | Engine Q | Hand Q |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| P1 | 0.70 | N1 (0.70 × 1.00) | 12.000000 | 5.060729 | 3.542510 | 3.542510 |
| P2 | 1.40 | + N2 (0.70 × 1.00) | 13.213436 | 4.847968 | 6.787155 | 6.787155 |
| P3 | 1.80 | + N3 (0.80 × 0.50) | 14.070853 | 4.709319 | 8.476773 | 8.476773 |

Note that ΣCA accumulates but the intensity **falls** as Tc grows, which is why
Q rises less than proportionally with area — the behaviour the Rational method
is supposed to show.

## 4. Time of concentration accumulation

Each pipe's Tc is the larger of its own inlet Tc and the upstream Tc plus the
travel time through the upstream pipe, `L / V`.

```
P2:  max(10.0, 12.000000 + 300/4.120529/60) = max(10.0, 13.213436) = 13.213436
P3:  max( 8.0, 13.213436 + 250/4.859559/60) = max( 8.0, 14.070853) = 14.070853
```

Both match the engine exactly. The N2 and N3 inlet times (10 and 8 min) lose to
the accumulated upstream path, which is correct for a trunk line.

## 5. Partial-flow velocity

Velocity is the design flow over the **actual** flow area at normal depth, not
the full-barrel area. For a circular section at depth `y` in diameter `D`:

```
θ = 2·arccos((r − y)/r),   A = r²(θ − sin θ)/2
```

P1 at normal depth y = 0.825392 ft in D = 1.25 ft:

```
A = 0.859722 ft²
V = Q/A = 3.542510 / 0.859722 = 4.120529 ft/s      (engine 4.120529)
```

Percent full is a **discharge** ratio, not a depth ratio:
`3.542510 / 4.580060 = 0.773464` → 77% (engine 0.773464).

## 6. Hydraulic grade line

The HGL is computed by a standard-step backward pass from the outfall.

**Outfall boundary.** P3's downstream HGL is the specified tailwater, 100.500000.

**Open-channel reach.** Where a pipe flows part full and is not surcharged, the
upstream HGL is the invert plus normal depth:

```
P3 upstream = 101.200000 + 1.031259 = 102.231259    (engine 102.231259)
P2 upstream = 102.500000 + 1.105908 = 103.605908
```

**Junction loss.** At N2 the entering flow loses `K·V²/2g` with K = 0.5:

```
V²/2g = 4.859559² / (2 × 32.2) = 0.366697 ft
0.5 × 0.366697                 = 0.183348 ft
103.605908 + 0.183348          = 103.789257    (engine 103.789256)
```

**Freeboard.** Every structure's HGL stays below its rim, so nothing floods:

| Node | Rim | HGL | Freeboard |
| --- | ---: | ---: | ---: |
| N1 | 110.00 | 104.957218 | 5.04 |
| N2 | 108.50 | 103.789256 | 4.71 |
| N3 | 107.00 | 102.231259 | 4.77 |
| OUT | 106.00 | 100.500000 | 5.50 |

## 7. HEC-22 inlet interception

The inlet pass is a **surface** check and is deliberately separate from the pipe
design flows. Pipes are sized on the full Rational `ΣCA·i` — the conservative
assumption that everything reaches the system — while the inlet schedule reports
what a given grate or curb opening actually captures, and routes the remainder
to the next inlet you designate.

Local flow arriving at N1, at the minimum Tc:

```
i(10 min) = 5.461693 in/hr
Q_local   = 0.70 × 1.00 × 5.461693 = 3.823185 cfs    (engine 3.823185)
```

With the default grate geometry the engine reports 1.419061 cfs intercepted,
2.404124 cfs bypassing, and 11.94 ft of spread — flagged as exceeding the
allowable spread, which is the correct outcome for a single default grate under
3.8 cfs. That is the schedule doing its job: the pipe design is unaffected, and
the drawing needs a second inlet or a larger opening.

## 8. Cross-check against Hydraflow Storm Sewers

Sections 1–7 prove the engine computes the published equations. This section
compares it with the tool most reviewers already trust: **Hydraflow Storm
Sewers Extension for Civil 3D, v2026.00**, on a network that was designed in
Civil 3D, exported to a `.stm` project file, and printed by Hydraflow on
2026-08-16. The `.stm` file is checked in as
`tests/fixtures/civil3d-2015-storm-sewers.stm` (coordinates moved to a local
origin; every hydraulic value untouched), and `tests/civil3d_formats.rs` holds
the assertions.

**The network.** A four-line trunk: 12-18-18-18 in RCP at 0.50 % grades,
186–175 ft reaches, n = 0.012, four sag grate inlets (0.42–0.57 ac, C 0.67–0.78,
5-min inlet time), a 10-yr NOAA Atlas 14 IDF fitted by Hydraflow as
i = 62.50296 / (t + 8.799997)^0.7942153, a starting HGL (tailwater) of 757.365,
and 0.10 ft drops through each structure (the incoming pipe invert sits above
the outlet invert).

Reproduce: `cargo run --example stm_dump -- tests/fixtures/civil3d-2015-storm-sewers.stm`.

The full comparison is the end-to-end suite `cargo test --test e2e_reference`:
both Hydraflow runs (Run 2 is the same trunk with a 24-in outfall pipe and
different areas, `civil3d-2015-storm-sewers-run2.stm`) go import → engine →
inlet pass → text / HTML / PDF reports → `.ssproj` save and reload → LandXML
export and re-import → DXF export and re-import → the `stormsewer-cli`
binary, and every stage is held to the table below within the tolerances
stated in the file header. The GUI does the same on real frames in
`e2e_gui_opens_civil3d_stm_and_matches_the_hydraflow_report`
(`cargo test -p stormsewer-app e2e_gui`). The 2012-format layout file of the
same project (`civil3d-2012-storm-layout.stm`, both runs, six FHA storms) is
checked for import fidelity and monotone flows across return periods.

| Line | Quantity | Hydraflow | StormSewer | Δ |
|---|---|---|---|---|
| all | Slope, ft/ft (from each line's own inverts) | 0.00500 / 0.00497 / 0.00497 / 0.00502 | 0.00500 / 0.00497 / 0.00497 / 0.00502 | 0 |
| all | ΣC·A, ac | 1.50 / 1.09 / 0.71 / 0.38 | 1.497 / 1.087 / 0.710 / 0.382 | rounding |
| 4 (top) | Tc, min → i, in/hr → Q, cfs | 5.0 → 7.77 → 2.97 | 5.000 → 7.773 → 2.969 | 0.03 % |
| 3 | Tc → i → Q | 5.8 → 7.44 → 5.28 | 5.77 → 7.444 → 5.282 | 0.04 % |
| 2 | Tc → i → Q | 6.8 → 7.07 → 7.68 | 6.37 → 7.209 → 7.836 | +2.0 % |
| 1 (outfall) | Tc → i → Q | 7.4 → 6.83 → 10.23 | 6.94 → 7.004 → 10.49 | +2.5 % |
| all | Full-flow capacity, cfs | 8.04 / 8.02 / 8.02 / 2.73 | 8.07 / 8.04 / 8.05 / 2.74 | +0.3 % |
| 1, 4 | Surcharged (Q > capacity) | yes / yes | yes / yes | agree |
| 2, 3 | Surcharged | no / no | no / no | agree |
| outfall | HGL, ft | 757.37 | 757.365 | 0 |
| AI-4 | HGL after junction loss, ft | 759.08 | 759.35 | +0.27 |
| AI-3 | HGL, ft | 760.03 | 760.38 | +0.35 |
| AI-2 | HGL, ft | 760.44 | 760.94 | +0.50 |
| AI-1 | HGL, ft | 761.69 | 762.08 | +0.39 |

**What agrees exactly.** Slopes, ΣC·A, intensity for a given Tc, the Rational
flow for a given Tc, the surcharge calls, the tailwater seed, and the terminal
lines' flows (2.97 and 5.28 cfs to the reported precision). The 0.3 % on
capacity is the Manning constant: Hydraflow uses 1.486, this engine 1.49.
The inlet schedule agrees too: each 4 × 4 ft sag grate captures its local
flow in full (3.19 / 2.93 / 2.55 / 2.97 cfs, Hydraflow "Incr Q", 100 %
efficiency) once the grate size is imported and the local flow uses the
intensity at the inlet's own inlet time — the app had been using the pipe
system's accumulated Tc, which under-stated AI-4's approach flow as 2.87 cfs.
Run 2 (`civil3d-2015-storm-sewers-run2.stm`, printed with a 757.62 starting
HGL) behaves the same: 3.73 and 6.49 cfs exact on the terminal lines, 9.20
and 11.90 within 2.5 %, capacities 2.71 / 8.02 / 8.01 / 17.25 within 0.8 %,
HGLs within 0.6 ft, inlets 3.30 / 3.23 / 2.98 / 3.73 exact.

**What differs, and why.** Every remaining difference traces to three method
choices, in decreasing order of effect:

1. **Velocity for travel time.** Hydraflow computes each line's velocity from
   the *actual* flow depth found by its HGL pass — a line drowned by downstream
   backwater flows full, so V = Q / A_full (line 3: 2.99 ft/s in Hydraflow,
   4.86 ft/s here at normal depth). That lengthens travel time, lengthens Tc
   downstream, and lowers intensity and Q on the lower lines. It is why lines 1
   and 2 carry 2–2.5 % less flow in Hydraflow, and it needs an outer iteration
   (Q → HGL → V → Tc → Q) that this engine does not yet run. The normal-depth
   velocity used here is the conservative side for pipe sizing.
2. **Surcharged-reach friction.** For a pressurised line Hydraflow steps the
   *energy* grade line up the reach with the average of the two end friction
   slopes (line 1: 0.706 % at the partially-full outlet, 0.808 % full at the
   inlet), then subtracts the velocity head. This engine starts a surcharged
   line at the higher of the downstream HGL and the pipe crown and applies the
   full-flow friction slope at the design Q over the whole length. On line 1
   that is 0.27 ft more head, and it is carried up the run.
3. **Structure loss coefficient.** Hydraflow assigns K per structure and puts
   K = 1.0 on the terminal inlet of each run (0.5 elsewhere). This engine uses
   one K for the project; the importer takes the most common value (0.5), so the
   top inlet here loses 0.11 ft instead of 0.22 ft.

Items 2 and 3 are on the roadmap; item 1 is a method decision recorded here so
that a reviewer comparing the two reports can account for the gap line by line.

**What the file taught the importer.** The same exercise fixed the `.stm`
reader, which had been written against a synthetic sample rather than real
Civil 3D output: the Civil 3D signature ("Storm Sewers for AutoCAD Civil 3D")
was rejected outright; the per-line `"Gutter N-Value"` record overwrote the
pipe n (0.013 for 0.012); "Starting HGL" was ignored (free outfall instead of
the 757.365 tailwater); Rise/Span are feet in this format, not inches; "Return
Period Index" is the period in years; and each line's own inverts are now kept,
so a drop through a structure no longer re-slopes the pipe (0.00554 for
0.00497 on line 2). The LandXML export of the same network from Civil 3D 2026
(`tests/fixtures/civil3d-2026-pipenetwork.xml`) imports to the same inverts,
lengths, and diameters, which `landxml_and_stm_exports_of_one_network_agree`
asserts.

## What this does and does not establish

It establishes that the implementation computes the published equations
correctly, and that the pieces compose — Tc feeds intensity, intensity feeds
flow, flow feeds velocity, velocity feeds the next Tc, and the HGL walks back
up through them.

It does not establish that the **method** suits your site. The Rational method
carries its own assumptions about drainage area, uniform rainfall, and constant
runoff coefficient. Choosing it, choosing C, and choosing a design storm remain
engineering judgment, and the engineer sealing the drawing owns them.

Nor is this a substitute for your own agency's design standards. Reconcile the
Manning K, the junction loss coefficients, and the spread criteria against your
own before relying on the output.

## Reporting a disagreement

If you run a network you have already designed and stamped and the numbers come
out different from your usual tool, that is the most valuable thing you can
send: [open an issue](https://github.com/mf4633/stormsewer/issues) with the
inputs and both sets of results.
