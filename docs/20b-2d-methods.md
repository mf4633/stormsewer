# 20b. 2D methods: equations, numerics and validation

This chapter is for the engineer who has to defend a 2D result. It gives the
equations the solver integrates, the exact update formulas it applies, how
the time step, wet/dry cells, boundaries, rain and infiltration are handled,
the 1D-2D interface formulas with their coefficients and sources, the two
file formats the 2D engine owns, the validation cases with the numbers the
test suite asserts, and the limitations. The user-facing side — how to set
up a grid, draw interfaces and read the results in the app — is in
[chapter 20](20-2d-overland.md).

The solver is written from the published papers cited at the end; no code
was taken from any other flood model.

## 1. Governing equations

Depth-averaged flow over a fixed bed obeys the shallow-water equations. With
`h` the depth, `z` the bed elevation, `η = z + h` the water surface, `q = (qx, qy)`
the unit discharge (m²/s or ft²/s) and `u = q / h` the velocity:

```
∂h/∂t + ∂qx/∂x + ∂qy/∂y = R − I                                (continuity)
∂q/∂t + ∂(q u)/∂x + ∂(q v)/∂y + g h ∇η + g h S_f = 0            (momentum)
```

`R` is the rain rate on the cell, `I` the infiltration rate, `g` gravity and
`S_f` the friction slope from Manning's equation,

```
S_f = n² |q| q / (k² h^{10/3})
```

with `n` Manning's roughness and `k` the unit constant: `k = 1` in metric
units (`g = 9.81 m/s²`) and `k = 1.486` in US customary units
(`g = 32.174 ft/s²`). The system is chosen by the model's `FLOW_UNITS`:
`CMS`, `LPS` and `MLD` are metric, everything else is US. All lengths on the
grid are the DEM's units, which must be the model's length units.

The solver offers two momentum treatments:

- **Local inertial** (the default). Bates, Horritt & Fewtrell (2010) drop
  the convective acceleration `∂(q u)/∂x + ∂(q v)/∂y`, keeping local
  acceleration, pressure (free-surface slope) and friction. The result is a
  scheme that is explicit, needs no Riemann solver, stays stable at the
  gravity-wave CFL limit and reproduces flood spreading, ponding and
  drainage where the Froude number is low (`Fr < ~0.5` to `0.8`), which is
  the case for urban surface flooding and overbank spreading. de Almeida,
  Bates, Freer & Souvignet (2012) add a θ-weighted average of the
  neighbouring fluxes that removes the checkerboard instability of the
  original scheme on fine grids with low friction.
- **Full dynamic (HLL)**. A first-order Godunov scheme with the HLL
  approximate Riemann solver of Toro (2001) and the hydrostatic
  reconstruction of Audusse et al. (2004). It keeps the convective terms
  and therefore handles dam breaks, hydraulic jumps and supercritical
  street flow at the cost of more diffusion of the free surface and more
  work per step. It is selected on the `Simulator` (`scheme = Scheme::Hll`)
  and by the `--scheme hll` switch of the CLI; the sidecar has no key for
  it yet.

## 2. Discretisation

### 2.1 Grid

The DEM's cells are the control volumes: square, side `Δx`, row-major with
the top row first (the raster convention). Depth `h`, bed `z` and roughness
`n` live at cell centres. For the local-inertial scheme the unit discharges
live on the faces, staggered: `qx` on the `(ncols+1) × nrows` vertical faces
(positive to +x, east) and `qy` on the `(nrows+1) × ncols` horizontal faces
(positive towards increasing row index, i.e. south; the results file
converts to world +y). A cell whose DEM value is no-data is a wall: every
face it touches carries zero flux and it is never wet.

### 2.2 Local-inertial flux update

For the face between cells `L` and `R` (west and east, or north and south),
Bates et al. (2010) eq. 11–12 with de Almeida et al. (2012) eq. 12:

```
h_f  = max(η_L, η_R) − max(z_L, z_R)                    (effective face depth)
q_c  = θ q^n + ½ (1 − θ) (q^n_prev + q^n_next)          (q-centred average)
q^{n+1} = [ q_c − g h_f Δt (η_R − η_L) / Δx ]
          / [ 1 + g Δt n_f² |q^n| / (k² h_f^{7/3}) ]
```

where `q^n_prev` and `q^n_next` are the fluxes on the two neighbouring
faces along the same axis, `n_f = ½(n_L + n_R)`, and `θ = 0.7` (θ = 1
recovers the original Bates scheme). The friction term is evaluated
implicitly in `q^{n+1}`, which is what keeps thin sheets of water on rough
ground stable. If `h_f ≤ dry_depth` the flux is zero.

The scheme is *well balanced* by construction: the driving term is the
free-surface gradient, which vanishes on a lake at rest whatever the bed
does (§6.1).

### 2.3 Continuity update and the outflow limiter

```
h^{n+1} = h^n + (Δt/Δx) (q_W − q_E + q_N − q_S) + R Δt − I Δt
```

with the four face fluxes taken as inflow-positive. Before the update, every
cell's total outflow over the step is compared with what it holds; if
`Δt/Δx · Σ q_out > h`, all of that cell's outgoing fluxes are scaled by
`h / (Δt/Δx · Σ q_out)`. A face is scaled by its *upwind* cell only, so the
volume leaving one cell is exactly the volume entering its neighbour and
the limiter conserves mass. The scaled fluxes are the ones carried into the
next step's momentum equation. The limiter is a safety net for the rare
cell that empties within a step; it is never active in the validation runs
except at a dam-break tip.

### 2.4 Full-dynamic (HLL) update

The conserved variables `(h, hu, hv)` live at cell centres. For each face
the two bed elevations are reconciled by hydrostatic reconstruction
(Audusse et al. 2004):

```
z_f = max(z_L, z_R),  h_L* = max(0, η_L − z_f),  h_R* = max(0, η_R − z_f)
```

and the HLL flux `F = (F_h, F_hu, F_hv)` is formed from `(h_L*, u_L, v_L)` and
`(h_R*, u_R, v_R)` with wave speeds `S_L = min(u_L − c_L, u_R − c_R)`,
`S_R = max(u_L + c_L, u_R + c_R)`, `c = √(g h*)`, and Toro's dry-bed
estimates (`S_L = u_R − 2c_R, S_R = u_R + c_R` when the left is dry, mirrored
when the right is dry):

```
F = F_L               if S_L ≥ 0
F = F_R               if S_R ≤ 0
F = [S_R F_L − S_L F_R + S_L S_R (U_R − U_L)] / (S_R − S_L)   otherwise
```

The transverse momentum is carried as `F_hv = F_h · v_upwind`. The cell
update is

```
h^{n+1}   = h  − (Δt/Δx) Σ F_h (outward)
hu^{n+1}  = hu − (Δt/Δx) (F_hu,E − F_hu,W + F_hv,S − F_hv,N)
            + (Δt/Δx) ½ g [(h_E,L*)² − (h_W,R*)²]
```

(likewise for `hv` with the north/south faces), where `h_E,L*` is this cell's
reconstructed depth on its east face and `h_W,R*` on its west face; the
pressure difference of the reconstructed depths is what balances the bed
slope exactly at rest. Manning friction is applied semi-implicitly after
the update, `hu ← hu / (1 + Δt g n² |u| / (k² h^{4/3}))`. Walls (closed edges
and no-data neighbours) use the reflecting flux `(0, ½ g h², 0)`. The same
outflow limiter as §2.3 is applied to the mass flux and, with the same
factor, to the momentum fluxes.

### 2.5 Time step

Adaptive, from the CFL condition on the fastest wave:

```
Δt = α Δx / √(g h_max)                       (local inertial)
Δt = ½ α Δx / max(|u| + √(g h))              (HLL)
```

with `α` the Courant number from the sidecar (`COURANT`, default 0.7). The
HLL step carries the ½ because the unsplit two-dimensional update is
stable for `ν_x + ν_y ≤ 1`; without it round-off on a lake at rest grows
exponentially (that is how the factor was found).
`h_max` is the largest depth on the grid after the previous step. `MAX_DT`
caps the step; while the whole grid is dry the step is 1 s (capped the same
way). Steps are shortened to land exactly on every output frame and every
1D synchronisation time, so the frames are at the times they claim.

### 2.6 Wet and dry cells

A cell is wet when `h > dry_depth` (`DRY_DEPTH`, default 0.003 in the DEM's
units). Faces whose effective depth `h_f` is at or below it carry no flux;
velocities are reported as zero on dry cells; the arrival-time grid records
the first step at which a cell exceeds the threshold. Water below the
threshold is still stored and still counted in the balance; it just does
not move.

### 2.7 Boundaries

The four grid edges share one setting (`[BOUNDARY]`):

- `CLOSED`: walls; no flux (default).
- `OPEN`: free outflow at normal depth. The edge face carries
  `q = (k/n) h^{5/3} S^{1/2}` outward, with `S` the larger of the bed slope
  and the water-surface slope towards the edge, never negative, so still
  water against a level edge does not leak. (HLL: zero-gradient outflow,
  the ghost cell copying the cell; flow pointing into the grid meets a
  wall, or the edge would manufacture water.)
- `FIXED_HEAD H`: a ghost cell with the edge cell's bed and depth
  `max(0, H − z)`; water flows in or out according to the head difference,
  which represents a receiving water body or a tide level.

Boundary fluxes are booked in the mass balance as inflow or outflow.

### 2.8 Rain on the grid, infiltration, sources and sinks

- `[RAIN] GAGE name` reads the named `[RAINGAGES]` row's `TIMESERIES` at the
  gage's `Interval`, with the `SCF` applied. `INTENSITY` values are used as
  they are, `VOLUME` values are divided by the interval, `CUMULATIVE` values
  are differenced. Each value is held for one gage interval after its time
  (the engine's own convention) and then the rate is zero. `CONSTANT i` is a
  steady intensity. Rates are in the model's rain units (in/hr for US,
  mm/hr for metric) and are converted to DEM length per second. Dated
  series are placed on the model's `START_DATE`/`START_TIME` clock; undated
  times are hours from the start.
- `[INFILTRATION] CONSTANT f` removes `f` per hour (same rain units) from
  every wet cell, never more than it holds. `HORTON f0 fc k` uses
  `f(t) = fc + (f0 − fc) e^{−k t}` with `t` the time the cell has been wet
  (hours, `k` in 1/hr).
- `[SOURCES]` add a `[TIMESERIES]` of flow, in the model's flow units, to
  one cell; the series is interpolated linearly and integrated over each
  step (the engine's treatment of inflow series). `GPM`, `MGD`, `LPS` and
  `MLD` are converted to cubic length per second.
- Outfall nodes on the grid act as **sinks**: water above the outfall's
  stage (its `FIXED` stage, else its invert; in a coupled run the engine's
  head) drains through the interface opening by the inlet formula of §4.2
  and leaves the system, as it would through the outfall.

### 2.9 Mass balance

The solver keeps four volumes in the DEM's cubic units: inflow (rain,
sources, boundary inflow, exchange onto the surface), outflow (boundary
outflow, sinks, exchange off the surface), infiltrated, and the storage at
the start. The reported error is

```
ε = (inflow − outflow − infiltrated − (stored − stored₀)) / max(inflow, stored₀) × 100 %
```

Because every transfer is an addition to one cell and a subtraction from
another (or from a booked total), the error is round-off: below `1e-9 %`
in every validation run.

### 2.10 Threads

The four passes of a step (fluxes, limiter, update, limiter applied) each
run over bands of rows with `std::thread::scope`, every band writing a
disjoint slice and reading the previous pass's arrays. Results do not
depend on the thread count (`THREADS`, 0 = all cores; grids under 40 000
cells run on one thread because spawning costs more than the work).

## 3. Grid setup

`Setup::build` resolves the sidecar against the model:

1. The DEM is read (`.asc`; `.tif` through the GIS chapter's GeoTIFF reader),
   cropped to `WINDOW` (cells whose centre lies inside) and resampled to
   `CELL` by bilinear interpolation between cell centres (nearest for the
   roughness-class raster, which must not be interpolated).
2. The roughness grid is `UNIFORM n`, a `GRID` sampled onto the DEM cells
   (missing or non-positive values become 0.05 with a warning), or a
   `CLASSES` raster looked up in the `CLASS id n` table (unknown classes
   become 0.05 with a warning).
3. Every node in `[JUNCTIONS]`, `[STORAGE]`, `[OUTFALLS]` and `[DIVIDERS]`
   with `[COORDINATES]` is placed in its cell. Its interface is the
   `[NODES]` row if there is one, `SEALED` if it is listed under `[SEALED]`,
   else a default `MANHOLE`. The rim is `Elevation + MaxDepth`
   (`Elevation` alone for outfalls). Nodes outside the DEM or on a no-data
   cell are warnings, not errors.
4. Each `[BANKS]` polyline (or the conduit's own centreline when no points
   are given) is rasterised with Bresenham's line algorithm between the
   cells of consecutive vertices, segments clipped to the grid; the crest
   at each cell is the given `crest` or the DEM.

## 4. 1D-2D interfaces

### 4.1 Where the formulas come from

The exchange formulas are those of Chen, Djordjević, Leandro & Savić (2007)
and Leandro, Chen, Djordjević & Savić (2009): a free weir while the
receiving side is below the crest, an orifice once the opening is
submerged, and Villemonte's (1947) reduction for a submerged weir. Written
with the constants explicit so one dimensionless coefficient `c` serves both
unit systems:

```
free weir      Q = c · (2/3) · √(2g) · P · Δh^{3/2}
orifice        Q = c · A · √(2g Δh)
submerged weir Q = Q_free · [1 − (h₂/h₁)^{3/2}]^{0.385}     (Villemonte)
```

`c = 0.6` unless the interface's `weir_coeff` says otherwise. In metric
units `c (2/3) √(2g) = 1.77`, the familiar broad-crested constant; in US
units it is `3.21`. `P` is the opening's perimeter and `A` its clear area:
the inlet's own for an `INLET`; for a `MANHOLE`, a round lid of 0.6 m
(metric) or 2 ft (US), `P = π D`, `A = π D² / 4`.

### 4.2 Node interfaces

At each synchronisation time both models are at the same `t`. With `H` the
node's head, `z` the DEM ground at its cell, `d` the surface depth there and
`η = z + d`:

- **Surcharge, engine-decided.** If the engine reports a flooding
  (overflow) rate at the node, that water has already left the network —
  the engine's own continuity counts it as flooding loss — so it is placed
  on the surface as it is, `rate × Δt_sync` into the cell. Nothing is set
  back on the engine for it. This is the usual path: the stock engine caps
  the head at the rim and reports the excess as flooding.
- **Surcharge, formula-decided.** If no flooding is reported but `H > η`
  (a rim above the DEM ground, or a node given `SurDepth`), the outflow is
  the free weir over `P` with head `H − z` while the cell is dry
  (`d ≤ dry_depth`), else the orifice through `A` with head `H − η`. It is
  placed on the surface and, in tight mode, withdrawn from the node as a
  negative lateral inflow. The engine caps a withdrawal at the node's
  stored volume, so this path can put slightly more on the surface than the
  node gave up; the coupled summary's `surcharged` is what the surface
  received.
- **Capture.** An `INLET`, or a `MANHOLE` with `OPEN`, takes water while
  `H < η`: the weir over `P` with head `d`, capped by the orifice through `A`
  with head `d` (node below ground) or `η − H` (node surcharged but below the
  surface). The volume `Q Δt_sync` is withdrawn from the cell *now*, bounded
  by what the cell holds, and exactly that volume, as a rate over the
  coming interval, is handed to the node: a positive lateral inflow in
  tight mode, an `[INFLOWS]` series in iterative mode. Capture is therefore
  conserved exactly.
- **Sealed.** Nothing.

A sealed node is one whose lid is bolted or that is not on the surface at
all (an underground storage). Outfall interfaces are sinks (§2.8).

### 4.3 Bank interfaces

For every cell under a bank line, the channel water surface is the head at
the conduit's from-node and to-node interpolated by the cell's fraction of
the line, `η₁ = H_from + f (H_to − H_from)`. With `η₂` the cell's surface
and `z_c` the crest, `h₁ = max(η₁, η₂) − z_c` and `h₂ = min(η₁, η₂) − z_c`:

```
Q = c (2/3) √(2g) Δx h₁^{3/2} · [1 − (h₂/h₁)^{3/2}]^{0.385}   if h₂ > 0, else no reduction
```

positive from channel to surface when `η₁ > η₂`. Each cell's volume is
split between the two end nodes by `(1 − f, f)` and set as (negative)
lateral inflow in tight mode. Flow back into the channel is bounded by what
the cell holds.

### 4.4 Modes

- **Tight** (`Mode::Tight`): the engine runs through the bridge
  (`crate::bridge::Session`). Every `sync_s` seconds (default: the engine's
  routing step, at least 1 s) the engine is stepped to the surface's time,
  heads and overflow rates are read, the exchange above is applied, lateral
  inflows are set for the coming interval, and the surface advances. At the
  end the engine runs out to its own end so the report is complete.
- **Iterative** (`Mode::Iterative`): the model is copied to the scratch
  folder (`engine::prepare`, always scratch), `runswmm` runs it, the node
  head and flooding series are read from the `.out` (variables 1 and 5 of
  each node record, interpolated linearly between report steps and held at
  the ends), the surface runs against them with a synchronisation of
  `min(OUTPUT_STEP, 60 s)`, and the captured flows are written back into the
  scratch `.inp` as `[TIMESERIES] SS2D_<node>` rows (decimal hours) with one
  `[INFLOWS] <node> FLOW SS2D_<node> FLOW 1.0 1.0` row each. The pair is
  re-run until the total exchanged volume changes by less than `tolerance`
  or `iterations` is reached. A node that already has a `FLOW` inflow is
  left alone (the engine would replace the user's inflow) and reported.

Both modes write the surface results file and return a `CoupledSummary`
with the 1D files, the iterations run, and the total volumes surcharged and
captured.

## 5. File formats

### 5.1 The sidecar `<model>.2d`

Plain text beside the `.inp`, which stays EPA's format untouched. Sections
and keywords are case-insensitive, `;` starts a comment, paths or names with
spaces are double-quoted, a missing file is the default configuration.

```
[GRID]
DEM          "C:/data/site dem.asc"   ; .asc or .tif, in the model's map units
CELL         2                        ; resample to this cell size (optional)
WINDOW       xmin ymin xmax ymax      ; restrict to this window (optional)

[ROUGHNESS]
UNIFORM      0.05                     ; or:
GRID         n.asc                    ; a raster of Manning's n
CLASSES      landcover.asc            ; a class raster with a table:
CLASS        11 0.03
CLASS        22 0.1

[RAIN]
NONE | GAGE RainGage | CONSTANT 25    ; in/hr or mm/hr

[INFILTRATION]
NONE | CONSTANT 5 | HORTON f0 fc k    ; rain units; k in 1/hr

[BOUNDARY]
CLOSED | OPEN | FIXED_HEAD 98.25

[RUN]
DURATION     7200                     ; seconds (default: the model's)
OUTPUT_STEP  300                      ; seconds between frames
DRY_DEPTH    0.003                    ; DEM units
COURANT      0.7                      ; 0 < α ≤ 1
MAX_DT       2                        ; seconds (optional)
THREADS      0                        ; 0 = all cores

[SOURCES]
;;Name  X  Y  Series
hose    12.5 40 Q1

[NODES]
;;Node  MANHOLE [coeff|*] [OPEN] | INLET perimeter area [coeff] | SEALED
J1      MANHOLE 0.5 OPEN
J2      INLET 3.0 0.2
J3      SEALED

[BANKS]
;;Link  LEFT|RIGHT  crest|DEM  coeff|*  x1 y1 x2 y2 ...
C1      RIGHT DEM 0.7 0 0 10 5

[SEALED]
O1
```

`Config::to_text` writes exactly this layout and `Config::parse` reads it;
the round trip is tested.

### 5.2 The results file `<model>.2d.out`

Little-endian binary, written frame by frame so a stopped run still opens.

```
header   "SS2D"  u32 version=1  u32 ncols  u32 nrows  f64 x0  f64 y0  f64 cell
         u8 metric  f64 frame_step_s  u32 n_frames  f64 dry_depth
         u32 n_nodes  { u16 len, utf-8 name } × n_nodes
frame    f64 time_s  f32 depth[n]  f32 vx[n]  f32 vy[n]  f32 node[n_nodes]
trailer  f32 max_depth[n]  f32 max_speed[n]  f32 max_hazard[n]  f32 arrival[n]
         u32 n_frames  "2DSS"
```

`n = ncols × nrows`, row-major top row first, `x0, y0` the lower-left corner
as in an ESRI ASCII grid. Depth is NaN on no-data cells; `vx, vy` are world
axes (x east, y north); `node[i]` is the average exchange rate at interface
`i` over the frame interval, positive onto the surface, in cubic DEM units
per second. The trailer holds the maxima over every step (not just the
frames): depth, speed, depth × speed (the hazard index), and the arrival
time in seconds (NaN where never wet). The header's `n_frames` is 0 until
the file is sealed; a reader that finds no `2DSS` tail counts frames from the
file size and computes the maxima from the frames it has. The CLI writes
the maximum-depth grid or one frame as `.asc` (`twod max`, `twod frame`).

## 6. Validation

The cases below are the integration tests in `swmm/tests/twod_validation.rs`
and `swmm/tests/twod_coupling.rs`; the thresholds are what the tests assert
on every build.

### 6.1 Lake at rest on a random bed

50 × 50 cells of 2 m, bed uniformly random in 100–103 m (a fixed LCG seed),
one no-data cell as an island, filled still to 104 m, 1000 steps, both
schemes. **Asserted:** every cell velocity `≤ 1e-12`, depth drift
`≤ 1e-12`, balance error `< 1e-9 %`. The local-inertial scheme is exactly
balanced because its driving term is the surface slope; HLL is balanced by
the hydrostatic reconstruction (§2.4).

### 6.2 Steady uniform flow (Manning)

A 200 m plane of 1 m cells at `S = 0.01`, `n = 0.03`, `q = 0.1 m²/s` fed at the
top edge, open downstream boundary, run to 2000 s. Manning's normal depth
is `h_n = (q n / (k √S))^{3/5} = 0.03^{0.6} = 0.12203 m`. **Asserted:** depth
at 80, 100, 120 and 150 m within **1 %** of `h_n` for both schemes; unit
discharge within 1 % of `q` (local inertial) and 2 % (HLL, whose first-order
diffusion costs about 1.5 % here); no cross-slope difference (`< 1e-6`).

### 6.3 Dam break on a dry frictionless bed (Ritter)

Ritter (1892), in the form given by Stoker (1957, *Water Waves*, §10.8):
with `c₀ = √(g h₀)` the depth at time `t` is

```
h = h₀                          x < −c₀ t
h = (2c₀ − x/t)² / (9g)         −c₀ t ≤ x ≤ 2c₀ t
h = 0                           x > 2c₀ t
```

1200 × 3 cells of 1 m, `h₀ = 1 m` behind a dam at 600 m, `n = 0`, checked at
`t = 40 s` for `−0.8 c₀t ≤ x ≤ 1.5 c₀t`, i.e. away from the negative wave
and the dry tip where any first-order scheme smears the solution.
**Asserted:** depth error `< 5 %` of `h₀` everywhere in that range; the still
water and the dry bed far from the dam untouched (`1e-9`); balance
`< 1e-6 %`. This case runs the **HLL** scheme. It is not a test of the
local-inertial scheme: Ritter's solution has Froude numbers up to 2 at the
tip and depends on the convective term that scheme omits, which is exactly
the regime Bates et al. (2010) exclude; on this case the local-inertial
profile is wrong by tens of percent, as expected, so a dam break must be
run with `Scheme::Hll`.

### 6.4 Mass conservation in a closed basin

A 60 × 60 parabolic bowl, closed edges, a triangular source hydrograph
peaking at 2 m³/s over 600 s (600 m³ in all), run to 900 s. **Asserted:**
inflow booked `= 600 m³` to `1e-9`, balance error `< 0.1 %` (achieved: round-off),
stored volume within 0.1 % of 600 m³, zero boundary outflow.

### 6.5 Symmetry

A constant 0.5 m³/s source at the centre of a flat 41 × 41 plate, 120 s.
**Asserted:** the depth field is mirror-symmetric in x, in y and under the
diagonal swap to `1e-9`, and the water has spread past 5 cells.

### 6.6 Time step

A 10 × 10 plate filled to 0.25 m then 4 m. **Asserted:**
`Δt = 0.7 Δx / √(g h)` to `1e-12`, the ratio of the two steps is exactly 4
(`√16`), and `MAX_DT` caps it.

### 6.7 Rain, infiltration and a fixed-head edge

A 30 × 30 plate under 36 mm/h with 7.2 mm/h infiltration for 600 s, then one
edge opened to a head below the bed. **Asserted:** rain and infiltration
volumes booked exactly (`1e-9`), water leaves through the edge, balance
`< 1e-9 %` throughout.

### 6.8 Interfaces

- **Node conservation:** engine-reported overflow of 0.4 m³/s for 5 s puts
  exactly 2 m³ on the cell; the formula surcharge equals the free-weir /
  orifice value for the lid dimensions; re-entry through an open lid
  removes from the cell exactly the volume handed to the network and never
  more than the cell holds; a sealed node exchanges nothing. All to `1e-12`.
- **Inlet limits:** for depths 0.01–2 ft over a 6 ft × 1.5 ft² inlet the
  capture equals `min(weir, orifice)` and never exceeds either; a node whose
  head is at the surface takes nothing.
- **Sink:** an outfall interface drains a ponded cell, takes no more than
  it holds, and is booked as outflow (balance `< 1e-9 %`).
- **Bank:** the volume put on the cells equals the sum handed to the two
  end nodes (`1e-12`), more of it at the higher end; return flow is bounded
  by the cells.
- **Recording:** a coupled-style run writes the per-frame exchange rate
  into the results file (0.25 m³/s over the frames it was applied).
- **Iterative write-back:** captured series become `SS2D_<node>` rows and
  one `[INFLOWS]` row each, replaced (not duplicated) on the next
  iteration, and a node with its own `FLOW` inflow is skipped with a
  warning.

### 6.9 Performance

300 × 300 cells with 1 m of water over a third of the grid, 60 s of flow
(several hundred CFL steps): **asserted** under 5 s in a release build
(ignored in debug builds, where it is about ten times slower).

## 7. Limitations

- First-order in space and time: fronts and hydraulic jumps are smeared
  over a few cells; halve the cell size to check a result's sensitivity.
- The local-inertial scheme is for low Froude numbers. Steep chutes,
  dam breaks and supercritical gutter flow need the HLL scheme, which in
  turn is more diffusive of the free surface in slow ponds.
- No turbulence or eddy viscosity, no wind stress, no sediment, no
  temperature; depth-averaged velocities only.
- Buildings, walls and kerbs exist only insofar as the DEM has them; a
  bare-earth DEM lets water run through houses. Burn buildings in as high
  cells (or no-data to block them) before running.
- Manhole lids have one default size; give inlets their real perimeter and
  area. The exchange coefficients (0.6) are the literature defaults, not
  calibrated values.
- Rain falls on the grid at the same rate everywhere (one gage); no
  spatial rain field, no evaporation, no snow.
- Iterative coupling cannot represent a surface that feeds a node whose
  own inflow the user already specified, and converges slowly when the
  captured flow changes the network's behaviour strongly; use tight
  coupling there.
- Formula-decided surcharge in tight mode is bounded by the engine's cap on
  withdrawals, so a few percent of non-conservation is possible on that
  path (the summary reports what the surface received).

## 8. References

- Audusse, E., Bouchut, F., Bristeau, M.-O., Klein, R. & Perthame, B. (2004).
  A fast and stable well-balanced scheme with hydrostatic reconstruction
  for shallow water flows. *SIAM J. Sci. Comput.* 25(6), 2050–2065.
- Bates, P. D., Horritt, M. S. & Fewtrell, T. J. (2010). A simple inertial
  formulation of the shallow water equations for efficient two-dimensional
  flood inundation modelling. *J. Hydrol.* 387, 33–45.
- Chen, A. S., Djordjević, S., Leandro, J. & Savić, D. (2007). The urban
  inundation model with bidirectional flow interaction between 2D overland
  surface and 1D sewer networks. *NOVATECH 2007*, Lyon, 465–472.
- de Almeida, G. A. M., Bates, P., Freer, J. E. & Souvignet, M. (2012).
  Improving the stability of a simple formulation of the shallow water
  equations for 2-D flood modeling. *Water Resour. Res.* 48, W05528.
- Leandro, J., Chen, A. S., Djordjević, S. & Savić, D. A. (2009). Comparison
  of 1D/1D and 1D/2D coupled (sewer/surface) hydraulic models for urban
  flood simulation. *J. Hydraul. Eng.* 135(6), 495–504.
- Ritter, A. (1892). Die Fortpflanzung der Wasserwellen. *Z. Ver. Dtsch.
  Ing.* 36(33), 947–954.
- Rossman, L. A. & Simon, M. A. (2022). *Storm Water Management Model
  User's Manual Version 5.2*. US EPA, EPA/600/R-22/030 (rain gage formats,
  time series conventions, flooding as reported in the binary output).
- Stoker, J. J. (1957). *Water Waves: The Mathematical Theory with
  Applications*. Interscience, New York (§10.8, the dam-break problem).
- Toro, E. F. (2001). *Shock-Capturing Methods for Free-Surface Shallow
  Flows*. Wiley, Chichester (ch. 10, the HLL solver and dry-bed wave speeds).
- Villemonte, J. R. (1947). Submerged-weir discharge studies. *Engineering
  News-Record* 139, 866–869.
