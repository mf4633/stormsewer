# Worked-Example Validation

A single end-to-end storm-sewer calculation, solved **by hand** and reproduced
by the engine column-for-column. The point is not that the code runs — it is
that the numbers a professional engineer would compute with a pencil are the
numbers the engine returns.

Reproduce it:

```bash
cargo run --example worked_example      # prints the engine column below
cargo test --test worked_example        # asserts agreement with the hand calc
```

---

## The network

Two 24-inch reinforced-concrete pipes (n = 0.013) draining two inlet
catchments to an outfall. A **constant design intensity** i = 5.0 in/hr is used
deliberately, so the hydrology (Rational Q = i·C·A) is exact and every
hydraulic quantity is closed-form — nothing depends on an iterative
time-of-concentration loop.

| Structure | Invert (ft) | Area (ac) | C    | C·A  |
| --------- | ----------- | --------- | ---- | ---- |
| N1 (inlet)  | 100.0     | 4.0       | 0.60 | 2.40 |
| N2 (inlet)  | 98.0      | 3.0       | 0.70 | 2.10 |
| OUT         | 97.4      | —         | —    | —    |

| Pipe | From → To  | Length | Dia | Slope (from inverts)     |
| ---- | ---------- | ------ | --- | ------------------------ |
| P1   | N1 → N2    | 200 ft | 24″ | (100.0−98.0)/200 = 0.010 |
| P2   | N2 → OUT   | 200 ft | 24″ | (98.0−97.4)/200 = 0.003  |

## The hand calculation

**Design flows** (Rational, C·A accumulates downstream):

- P1: Q = i·(C·A)₁ = 5.0 × 2.40 = **12.00 cfs**
- P2: Q = i·(C·A₁ + C·A₂) = 5.0 × 4.50 = **22.50 cfs**

**Full-flow capacity** (Manning, Q = (k/n)·A·R^⅔·√S, k = 1.486):

For a 24″ pipe: A = πD²/4 = π = 3.141593 ft², R = D/4 = 0.5 ft,
R^⅔ = 0.629961, k/n = 1.486/0.013 = 114.3077.

- P1 (S = 0.010): 114.3077 × 3.141593 × 0.629961 × √0.010 = **22.62 cfs**
- P2 (S = 0.003): 114.3077 × 3.141593 × 0.629961 × √0.003 = **12.39 cfs**

**Peak (max) open-channel capacity** ≈ 1.076 × full (occurs at y/D ≈ 0.938):

- P1 → 24.34 cfs P2 → 13.33 cfs

**Full-flow velocity** = Q_full / A:

- P1 → 22.62 / π = 7.20 ft/s P2 → 12.39 / π = 3.94 ft/s

**Result** (compare design Q to capacity):

- P1: 12.00 / 22.62 = **53.0 % full** — flows open-channel, adequate.
- P2: 22.50 cfs exceeds P2's peak capacity of 13.33 cfs → **P2 surcharges**
  (182 % of just-full). The flat 0.3 % reach cannot pass the accumulated flow.

## Hand calc vs. engine

| Quantity           | Pipe | Hand    | Engine  | Match |
| ------------------ | ---- | ------- | ------- | ----- |
| Slope (ft/ft)      | P1   | 0.010   | 0.010   | ✓ |
|                    | P2   | 0.003   | 0.003   | ✓ |
| Design Q (cfs)     | P1   | 12.00   | 12.00   | ✓ |
|                    | P2   | 22.50   | 22.50   | ✓ |
| Full capacity (cfs)| P1   | 22.62   | 22.62   | ✓ |
|                    | P2   | 12.39   | 12.39   | ✓ |
| Max capacity (cfs) | P1   | 24.34   | 24.34   | ✓ |
|                    | P2   | 13.33   | 13.33   | ✓ |
| Full velocity (ft/s)| P1  | 7.20    | 7.20    | ✓ |
|                    | P2   | 3.94    | 3.94    | ✓ |
| Percent full       | P1   | 53.0 %  | 53.0 %  | ✓ |
|                    | P2   | 182 %   | 182 %   | ✓ |
| Surcharged?        | P1   | no      | no      | ✓ |
|                    | P2   | yes     | yes     | ✓ |

Every independently hand-derived value is reproduced by the engine, and the
undersized reach is correctly flagged. This validates the Rational + Manning
spine end-to-end on a realistic network.

## Scope of this validation

This example exercises the closed-form quantities: Rational accumulation,
Manning capacity, peak capacity, full velocity, percent full, and the
surcharge decision. It does **not** by itself validate the iterative pieces —
normal-depth solving, the HGL/backwater pass, or time-of-concentration
coupling — which are covered separately (`tests/validation.rs` pins normal
depth via a round-trip and critical depth via the Froude-unity condition). The
next validation milestone (see `READINESS.md`) is to reproduce a published
HEC-22 example that includes the full HGL profile.
