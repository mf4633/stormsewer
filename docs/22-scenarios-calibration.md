# 22. Scenarios and calibration

Two tools that share one idea: the model you have open is the *base*, and
a set of edits laid on top of it is something you can name, keep, run and
compare. A **scenario** is such a set written by hand — "the pond with a
larger orifice", "the pre-development catchment". A **calibration** is a
set found by an optimiser — the parameter values that make the model's
answer look most like what was measured. Both live in small JSON files
beside the model, never inside the `.inp`, and both are on the Tools menu.

## 22.1 Concepts

- **Base model.** The document as saved. Every scenario is defined
  relative to it, and calibration starts from it.
- **Scenario.** A name, a description, and an ordered list of *edits*. An
  edit is one document command — set a field, add a row, delete an object,
  move a node, rename, and so on: the same commands every dialog and every
  map tool produce ([§17](17-file-formats.md) for the sections). Applying
  a scenario means: take a fresh copy of the base text, apply the edits in
  order. The base is never touched.
- **Active scenario.** The one you are working on. Activating it swaps the
  editor's document for base + scenario; while it is active, everything
  you do in the editor is recorded into the scenario. Deactivating puts the
  base back, byte for byte.
- **Stale edit.** An edit that names something the base no longer has — a
  subcatchment renamed or deleted since the scenario was written. Stale
  edits are flagged and a scenario with one cannot be activated until it
  is fixed or removed.
- **Observed series.** Measured values at a model object: a flow at an
  outfall, a depth at a manhole, runoff from a catchment. Imported from
  text, in the model's own units.
- **Parameter.** Something the optimiser may change: a group of objects,
  one column, a transform, and bounds. A group edits as *one* factor, so
  "Width of every subcatchment tagged `upper`" is a single parameter.
- **Objective.** One number that says how far the simulated series is
  from the observed one; the optimiser drives it to zero.

## 22.2 Scenarios window

Tools → **Scenarios…** opens the **Scenarios** window. It needs an open
model; scenarios are stored beside it, so an unsaved model keeps them in
memory only (the window says *unsaved model (save it to keep scenarios)*).

The left column lists **Base model** and every scenario with its edit
count; a `●` marks the active one. Click a row to select it. The right
column holds the buttons:

| Button | What it does |
|---|---|
| **New** | Adds a scenario named `Scenario` (numbered if taken) and opens the rename field. |
| **Duplicate** | Copies the selected scenario as `… copy`. |
| **Rename** | Opens a name field; **OK** or Enter applies, **Cancel** leaves it. Names are unique, case-insensitively. |
| **Delete** | Removes the selected scenario. Deleting the active one deactivates it first. |
| **Activate** | Switches the editor to base + the selected scenario (§22.3). Refused, with the reason, when the scenario has a stale edit. |
| **Deactivate** | Puts the base model back. |
| **Run All** | Runs the base and every scenario through the chosen engine (§22.4). Needs an engine (Run menu). |
| **Results…** | Reopens the **Scenario Results** table from the last Run All. |
| **Save** | Writes the `.scenarios.json` now. It is also written automatically on every change. |

Below the list, the selected scenario shows its **Description** (editable,
saved as you type) and its edit list, one line per edit in the form
`[SUBAREAS] S1 NPerv = 0.3`. A stale edit is marked `⚠` in red; hover it
for the reason (`[SUBCATCHMENTS] has no "S9"`). Selecting **Base model**
shows nothing to edit: the base is the file as saved.

## 22.3 Activating a scenario, and how edits are recorded

**Activate** applies the scenario to a copy of the base and makes that
copy the working document. The base document is parked, with its undo
history and its unsaved-changes flag, exactly as it was. Three things
change while a scenario is active:

1. The window title and the file name become `<stem>.scenario.<name>.inp`
   (spaces and punctuation in the name become `_`). **File → Save** and
   `Ctrl+S` therefore write that file, beside the model, and can never write
   the scenario over the base. The file is only created if you save.
2. **Every edit is recorded.** StormSewer cannot see the editor's command
   stream directly, so on every change of the document it recomputes the
   scenario as the *difference* between the base and the working document,
   section by section, keyed by object name: a row whose fields changed
   becomes a `SetFields` edit, a new row an `AddRow`, a missing one a
   `DeleteObject`, coordinates a `MoveNode`, vertices and polygons
   `SetVertices`/`SetPolygon`, the title a `SetTitle`, and free-text
   sections such as `[CONTROLS]` a delete-and-reinsert of their lines.
   Undoing an edit removes it from the scenario again. The result is
   written to the sidecar each time it changes, so a crash loses nothing.
   Comment and blank-line changes are not edits.
3. The status bar says `Scenario "…" active`, and the window's header line
   reads *Active: name — every edit is recorded into it*.

**Deactivate** records once more and restores the base document. The
restored text is the base's own text, not a re-serialisation, so it is
identical byte for byte; the undo stack from before activation is intact
(its labels read `edit`). Opening another model while a scenario is active
drops the activation, since the parked base belongs to the old model; the
window says so.

Because a scenario is a list of edits and not a copy of the file, a change
to the base flows through: widen a pipe in the base and every scenario
that does not itself set that pipe's width sees the wider pipe.

## 22.4 Run All and the results table

**Run All** writes base + each scenario to its own scratch folder under
`%TEMP%\StormSewer\run\` (the same layout as an ordinary run of an unsaved
model, [§9.2](09-run-and-engines.md); the model's `[FILES]` and data files
still resolve, since the scratch copy is keyed by a synthetic path beside
the model) and runs them one after another on a worker thread. The
**Running Scenarios** window shows *n of m done — running name*, a
progress bar, and **Stop after this run**, which lets the current engine
process finish and skips the rest.

Every run is recorded in the compare history with its scenario name —
Results → **Compare Runs…** lists them as `pond.inp [Big pipes]` — so any
two can be tabulated node by node and link by link as [§10](10-results-views.md)
describes. When the batch finishes the **Scenario Results** window opens:

| Column | Source |
|---|---|
| Scenario | `base`, then each scenario's name |
| OK | the run succeeded (no `ERROR` in the report and a results file) |
| Peak outfall flow | the largest *Max Flow* in the report's Outfall Loading Summary |
| Outfall volume | the sum of *Total Volume* in the same table |
| Flooding volume | the sum of *Total Flood Volume* in the Node Flooding Summary, `0` when no node flooded |
| Runoff cont. %, Routing cont. % | the two continuity errors from the report |
| ms | wall-clock time of the engine process |
| Error | why a run failed, if it did |

**Export CSV…** writes the table. A scenario whose edits no longer apply
(a stale edit) shows as failed with the edit named in the Error column.

## 22.5 The sidecar files

Both tools store their state beside the model, in the model's folder:

- `<stem>.scenarios.json` — `{"version": 1, "scenarios": [ {"name",
  "description", "edits": [ {"op": "SetField", "section": "SUBAREAS",
  "name": "S1", "field": "NPerv", "value": "0.3"}, … ]} ]}`. The `op` is the
  command name; the other keys are that command's fields. `Rename` carries
  its object kind as a string (`Node`, `Link`, `Subcatchment`, `Gage`,
  `Curve`, `Timeseries`, `Pattern`); `Batch` carries `edits`.
- `<stem>.calib.json` — the observed series (with their time stamps as
  SWMM days since 1899-12-30, or seconds from the start for time-only
  data), the parameters, the objective, the evaluation budget, seed, thread
  count, and the sensitivity settings.

The files are plain JSON, readable and editable by hand, and a missing file
is an empty set. They are not read by the engine. Renaming the model
orphans them; rename them to match.

## 22.6 Calibration window

Tools → **Calibration…** opens the **Calibration** window, in four parts.

### Observed data

The list shows each observed series (name, object · variable, point count)
with **Remove**. Below it, **Add observed data**:

- **Object** — the model object's name (`O2`, `J11`, `S1`).
- **Variable** — what was measured: *Node depth*, *Node head*, *Node total
  inflow*, *Node flooding*, *Link flow*, *Link depth*, *Link velocity*,
  *Subcatchment runoff*, or *System inflow at outfall* (the total inflow to
  an outfall node, which is the system's outflow there).
- **Name** — optional; defaults to `<object> observed`.
- The text box takes `datetime,value` rows. Accepted, auto-detected, no
  options: comma, tab, semicolon or space delimiters; an optional header
  row; dates `M/D/YYYY`, `YYYY-MM-DD`, `YYYYMMDD`, `D.M.YYYY`; times
  `H:MM` or `H:MM:SS`, in a second column or joined to the date by a
  space or `T`; time-only rows (`0:05,1.2`) as time from the simulation
  start; a numeric first column as elapsed seconds, or minutes/hours when
  the header says `min`/`hour`. Lines starting with `;` or `#` are
  comments. Values are taken as they are, in the model's units (CFS, feet
  … or CMS, metres): convert first if the gauge reports otherwise.
- **Add observed series** parses the text and attaches it; **Load CSV…**
  reads a file into the text box; **From last run** takes the last run's
  simulated series for the object and variable as the observations — the
  way to make a synthetic test (§22.10).

Observations outside the simulated period are ignored; if none fall
inside, the evaluation fails and says so.

### Parameters

The table lists each parameter: an editable **Parameter** name, the
**Field** (`[SECTION] Column`), the **Objects** it edits (`all`, `tag x`,
`n selected`, or one name), the **Transform**, **Lower** and **Upper**
bounds, and **Remove**.

Transforms decide what the bounds mean and what the optimiser changes:

| Transform | New value | Bounds are on |
|---|---|---|
| *multiply* | base × x | the factor x (`0.5`–`2` = halve to double) |
| *offset* | base + x | the offset x |
| *set* | x | the value itself, the same for every object of the group |

With *multiply* and *offset* each object keeps its own base value and the
group moves together — the right choice for a dozen subcatchments whose
widths were all estimated the same way. Percent columns (`PctImperv`,
`PctZero`, `PctRouted`, `PctSlope`) are clamped to 0–100 and other columns
to ≥ 0 after the transform.

**Suggested** offers the usual calibration parameters as multipliers over
every object of their section: `%Imperv`, `Width`, `N-Imperv`, `N-Perv`,
`S-Imperv`, `S-Perv`, `PctZero`; the infiltration parameters for the
model's `[OPTIONS] INFILTRATION` method — Horton `MaxRate`, `MinRate`,
`Decay`; Green-Ampt `Suction`, `Ksat`, `IMD`; or the curve number;
conduit `Roughness`; junction ponded area (`Aponded`); storage surcharge
depth; and aquifer conductivity (`[AQUIFERS]` column 4). **Add suggestion**
adds the chosen one; edit its bounds in the table.

**Compose a parameter** builds any other: **Name** (optional), **Section**
(`SUBCATCHMENTS`, `SUBAREAS`, `INFILTRATION`, `JUNCTIONS`, `STORAGE`,
`OUTFALLS`, `CONDUITS`, `AQUIFERS`, `GROUNDWATER`), **Column** (a column
name as the property sheet spells it — `Width`, `NPerv`, `Roughness` — or
a 0-based index for sections without names), **Objects** as **All**,
**Selection** (the objects of that section selected on the map when you
press the button; a selected subcatchment also serves `[SUBAREAS]` and
`[INFILTRATION]`), **Tag** (every object of the section carrying that
`[TAGS]` tag — the Detention Pond model tags its conduits `Swale`,
`Gutter` and `Culvert`) or **Object** (one name), then **Transform**,
**Lower**, **Upper** and **Add parameter**. The button refuses a parameter
that matches no object or whose bounds are inverted.

### Objective and budget

- **Objective** — which statistic (§22.7) the optimiser minimises. With
  several observed series the objective is the mean over series.
- **Evaluations** — the DDS budget: how many engine runs, counting the
  first one at the current values.
- **Seed** — the random seed; the same seed on the same model repeats the
  run exactly.
- **Threads** — how many engine processes run at once. Each evaluation is
  a separate `runswmm` process in its own scratch folder, so this is real
  parallelism; the default is the machine's core count, capped at 8.
- **OAT swing %**, **Morris trajectories**, **levels** — for §22.8.

**Save setup** writes `<stem>.calib.json` now (it is also written when a
series is added).

### Buttons and results

**Sensitivity (OAT)**, **Morris screening** and **Calibrate (DDS)** start
a worker (§22.8, §22.9); only one runs at a time. Results appear at the
bottom of the window: the tornado table with **Export tornado CSV…**, the
Morris table, and after a calibration the best objective, a
*Parameter / Start / Best* table, and for each observed series every
statistic of §22.7 with its Moriasi rating coloured green (very good,
good), amber (satisfactory) or red (unsatisfactory). Then:

- **Apply Best** sets the working document to the best parameter set as
  **one undo step** labelled `apply calibration` — one `Ctrl+Z` takes every
  changed field back, however many objects the groups covered. Nothing is
  saved until you save.
- **Save as Scenario** adds a scenario named `Calibrated` (numbered if
  taken) whose single edit is that batch, with the objective, evaluations,
  seed and engine version in its description. The base is untouched.
- **Report…** opens the **Calibration Report** window: the report as
  Markdown, with **Save Markdown…**, **Save HTML…** and **Export series
  CSV…** (time, observed, simulated for every series). The report carries
  the engine version and the SHA-256 of its executable, the optimiser
  settings, the parameter table with start and best values, the objective
  table at the start and at the best point with ratings, the convergence
  history, and the paired observed/simulated values.

## 22.7 Objectives, formulas and ratings

All statistics are computed on `n` pairs `(oᵢ, sᵢ)`: the observed value
and the simulated series *linearly interpolated to the observed time*.
`ō`, `s̄` are means; `σ` is the population standard deviation; `r` the
Pearson correlation; peaks and volumes are over the paired points, volumes
by the trapezoidal rule over time.

| Statistic | Formula | Minimised as | Reference |
|---|---|---|---|
| NSE | `1 − Σ(oᵢ−sᵢ)² / Σ(oᵢ−ō)²` | `1 − NSE` | Nash & Sutcliffe (1970) |
| KGE | `1 − √((r−1)² + (α−1)² + (β−1)²)`, `α = σ_s/σ_o`, `β = s̄/ō` | `1 − KGE` | Gupta et al. (2009) |
| RMSE | `√(Σ(oᵢ−sᵢ)²/n)` | RMSE | — |
| PBIAS | `100 · Σ(oᵢ−sᵢ) / Σoᵢ` (positive = the model under-estimates) | `|PBIAS|` | Moriasi et al. (2007) |
| RSR | `RMSE / σ_o` | RSR | Moriasi et al. (2007) |
| Peak error % | `100 · (peak_s − peak_o) / peak_o` | `|…|` | — |
| Volume error % | `100 · (V_s − V_o) / V_o` | `|…|` | — |
| Time-to-peak error | `t_peak,s − t_peak,o` (seconds) | `|…|` in hours | — |

NSE = 1 is a perfect fit and NSE = 0 means the model does no better than
the observed mean; the KGE components are shown in the report (`r / α /
β`) so a poor KGE can be blamed on timing, variability or bias.

Ratings follow Moriasi, D. N., Arnold, J. G., Van Liew, M. W., Bingner,
R. L., Harmel, R. D. and Veith, T. L. (2007), "Model evaluation guidelines
for systematic quantification of accuracy in watershed simulations",
*Transactions of the ASABE* 50(3), 885–900, Table 4 (streamflow):

| Rating | NSE | RSR | PBIAS |
|---|---|---|---|
| very good | 0.75 < NSE ≤ 1.00 | 0.00 ≤ RSR ≤ 0.50 | \|PBIAS\| < 10 |
| good | 0.65 < NSE ≤ 0.75 | 0.50 < RSR ≤ 0.60 | 10 ≤ \|PBIAS\| < 15 |
| satisfactory | 0.50 < NSE ≤ 0.65 | 0.60 < RSR ≤ 0.70 | 15 ≤ \|PBIAS\| < 25 |
| unsatisfactory | NSE ≤ 0.50 | RSR > 0.70 | \|PBIAS\| ≥ 25 |

Moriasi's thresholds were set for monthly streamflow from watershed
models; they are shown as a familiar yardstick, not as a standard for
event hydrographs at five-minute steps. The other statistics have no
published rating and show none.

References: Nash, J. E. and Sutcliffe, J. V. (1970), "River flow
forecasting through conceptual models part I — A discussion of
principles", *J. Hydrol.* 10(3), 282–290. Gupta, H. V., Kling, H., Yilmaz,
K. K. and Martinez, G. F. (2009), "Decomposition of the mean squared error
and NSE performance criteria: Implications for improving hydrological
modelling", *J. Hydrol.* 377(1–2), 80–91.

## 22.8 Sensitivity

**Sensitivity (OAT)** evaluates, for each parameter, the model with that
parameter alone moved to `x·(1 − p/100)` and `x·(1 + p/100)` around its
current value `x` (`p` = **OAT swing %**; a parameter whose current value
is zero moves by `p/100` of its range instead), clamped to its bounds —
`2·k` runs for `k` parameters. The tornado table lists each parameter's
low and high factor, the objective at each, and the **swing**
`f(high) − f(low)`, largest |swing| first. A parameter with a small swing
is not worth the optimiser's budget; remove it.

**Morris screening** is the elementary-effects method of Morris, M. D.
(1991), "Factorial sampling plans for preliminary computational
experiments", *Technometrics* 33(2), 161–174: `r` random trajectories
(**Morris trajectories**) on a `p`-level grid (**levels**) over the
normalised `[0, 1]` parameter space, each stepping one parameter at a time
by `Δ = p / (2(p − 1))`, so `r·(k + 1)` runs. For each parameter it
reports `μ` (the mean elementary effect), `μ*` (the mean absolute effect,
after Campolongo, Cariboni and Saltelli, 2007 — the ranking statistic), and
`σ` (the spread of the effects: interaction with other parameters or
non-linearity). Effects are per unit of the normalised range, so
parameters with different bounds compare directly.

Both run on the worker with the thread count from the settings and show
the **Calibration Progress** window; **Stop** ends them early with what
is done.

## 22.9 The optimiser: DDS

**Calibrate (DDS)** runs the dynamically dimensioned search of Tolson,
B. A. and Shoemaker, C. A. (2007), "Dynamically dimensioned search
algorithm for computationally efficient watershed model calibration",
*Water Resources Research* 43, W01413. DDS is a greedy, single-solution
search built for exactly this situation — a few hundred expensive model
runs at most. It starts from the current parameter values (the factors
that reproduce the model unchanged) and, at each evaluation `i` of a
budget of `m`, perturbs a random subset of the parameters around the best
point found so far: each parameter is included with probability
`1 − ln(i)/ln(m)`, so early on almost all move and near the end only one
or two do — the search narrows from global to local by itself, with no
other tuning. A chosen parameter moves by `r·(upper − lower)·N(0, 1)` with
`r = 0.2`, and a value pushed past a bound is reflected back about that
bound (or set to it, if the reflection would cross the other bound). The
candidate replaces the best whenever its objective is no worse. With
**Threads** set above one, that many candidates are drawn around the
current best per round and evaluated at once, the best of them kept; with
one thread the run is exactly the paper's algorithm. A failed engine run
counts against the budget and never becomes the best.

The **Calibration Progress** window shows evaluations done of the budget
with elapsed seconds, a progress bar, the best objective so far, the best
parameter set, and a convergence plot of best objective against
evaluation; failed evaluations are counted with the last engine error.
**Stop** finishes the evaluations in flight, keeps the best, and produces
the result and report marked *(stopped early)*.

What DDS does not do: it gives one good point, not a distribution, and
says nothing about how well-determined each parameter is. Look at the
sensitivity screen, at the KGE components, and at the paired series in
the report before believing a number.

## 22.10 Tutorial: recover a perturbed parameter on the Detention Pond model

This walks the whole loop on `docs/datasets/Detention_Pond_Model.inp`
without needing a gauge: the "observations" come from a run of the model
with parameters you changed, and the calibration has to find them again.

1. Open the model and run it once (`F5`) so an engine is chosen and the
   model is known to run.
2. **Make the truth.** Tools → **Scenarios…**, **New**, name it `Truth`,
   **Activate**. In the attribute table for subcatchments
   ([§6](06-attribute-tables.md)) multiply every `Width` by 1.5 (a global
   edit) and set every `PctImperv` to 0.8 of its value. The window's edit
   list fills as you go — one `SetFields` per subcatchment. Run the model
   (`F5`): this runs `Detention_Pond_Model.scenario.Truth.inp` from a
   scratch copy.
3. **Take the synthetic observations.** Tools → **Calibration…**. Object
   `O2`, variable *System inflow at outfall*, name `truth`, **From last
   run**. The series has 144 points at five minutes.
4. **Back to the base.** In the Scenarios window, **Deactivate**. The
   model text is the file's again; check the window title lost its
   `.scenario.Truth` suffix.
5. **Parameters.** In the Calibration window pick **Suggested** → `Width`,
   **Add suggestion**; then `%Imperv`, **Add suggestion**. Leave the
   bounds (0.25–4 and 0.5–1.5).
6. **Screen.** **Sensitivity (OAT)**. Both parameters swing the objective
   (the pond routes the hydrograph, but `Width` still moves the peak
   noticeably); neither is worth dropping.
7. **Calibrate.** Objective *NSE*, Evaluations 40, Seed 1, Threads as many
   as you have cores. **Calibrate (DDS)**. Watch the progress window: the
   first evaluation is the base (`1 − NSE` well above zero), and the curve
   steps down as candidates land nearer 1.5 and 0.8. Forty runs of this
   model take about a minute with four threads.
8. **Read the result.** The best point should be close to `Width × 1.5`
   and `%Imperv × 0.8` with `1 − NSE` near zero and every rated statistic
   *very good*. Because the two parameters trade off (a wider, less
   impervious catchment can mimic a narrower, more impervious one), the
   exact factors may differ from the truth while the fit is nearly
   perfect — a useful thing to have seen before calibrating to a real
   gauge.
9. **Keep it.** **Save as Scenario** makes `Calibrated`; **Run All** in the
   Scenarios window runs base, `Truth` and `Calibrated`, and Results →
   **Compare Runs…** with `Truth` as A and `Calibrated` as B shows the node
   depths agreeing to the tolerance the fit achieved. **Report…** → **Save
   HTML…** keeps the evidence with the engine hash in it.
10. **Apply Best** if you want the calibrated values in the base model;
    `Ctrl+Z` once takes them all back.

## 22.11 Limits and cautions

- Scenario recording is a diff of rows, so an edit that only changes
  spacing or a comment is not recorded, and a row moved within its section
  is not an edit. Replaying a scenario reproduces the rows, not the
  original whitespace of the rows it touches.
- A scenario's edits reference objects by name; rename an object in the
  base and the scenario's edits on it go stale. Fix the base or edit the
  JSON.
- Calibration runs the model as many times as the budget says. Each run is
  a full engine process; a long continuous simulation with a budget of
  500 is hours. Screen first, keep the budget honest, and use the seed to
  repeat a run.
- The optimiser sees only the objective. Parameters that are physically
  wrong but fit the gauge will be found if the bounds allow them; the
  bounds are where your judgement goes in.
- Everything here is StormSewer's own code; the engine is EPA's and is
  not modified. No part of this chapter was derived from any other
  vendor's product.
