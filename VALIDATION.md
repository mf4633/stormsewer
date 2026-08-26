# Validation

StormSewer implements public-domain methods, so every number it reports can be
reproduced by hand. This document does exactly that for a reference network:
each engine output below is followed by the hand calculation that produces it,
and the two agree to six decimal places.

Reproduce the engine side yourself:

```sh
cargo run --example validation_dump
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

The built-in demo: three pipes down a trunk to a fixed-tailwater outfall.

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
