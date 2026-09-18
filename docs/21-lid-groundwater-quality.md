# 21. LID, groundwater, snowmelt and water quality

The Project menu's second group of dialogs edits the sections that
describe what happens on a subcatchment beyond runoff, and what a node
does to the water passing through it: LID controls and their placement,
aquifers and groundwater flow, snow packs, pollutant buildup and washoff,
land use coverages and initial loadings, treatment, and the RDII unit
hydrographs. Each opens from the Project menu; the per-object ones also
open from the property sheet of the subcatchment or node they belong to.

| Menu item | Window | Section(s) | Also from |
| --- | --- | --- | --- |
| LID Controls… | LID Controls | `[LID_CONTROLS]` | — |
| LID Usage… | LID Usage | `[LID_USAGE]` | subcatchment sheet, **LID Usage…** |
| Aquifers… | Aquifers | `[AQUIFERS]` | — |
| Groundwater… | Groundwater | `[GROUNDWATER]`, `[GWF]` | subcatchment sheet, **Groundwater…** |
| Snow Packs… | Snow Packs | `[SNOWPACKS]`, `[TEMPERATURE]`, `[ADJUSTMENTS]` | — |
| Buildup / Washoff… | Buildup / Washoff | `[BUILDUP]`, `[WASHOFF]` | — |
| Land Use Coverages… | Land Use Coverages | `[COVERAGES]` | subcatchment sheet, **Land Use Coverages…** |
| Initial Loadings… | Initial Loadings | `[LOADINGS]` | subcatchment sheet, **Initial Loadings…** |
| Treatment… | Treatment | `[TREATMENT]` | node sheet, **Treatment…** |
| Unit Hydrographs… | Unit Hydrographs | `[HYDROGRAPHS]` | — |
| RDII Inflow… | RDII Inflow | `[RDII]` | node sheet, **RDII…** |

Column names and meanings below are the SWMM 5.2 User's Manual
(EPA/600/R-22/030), Appendix D, section by section; the physics is in the
Reference Manual Volume I (Hydrology) for LID, groundwater, snowmelt and
RDII and Volume III (Water Quality) for buildup, washoff and treatment.
Where a rule is the engine's rather than the manual's (how many fields a
row must have, which trailing fields may be left out) it comes from the
EPA source: `lid.c`, `gwater.c`, `snow.c`, `landuse.c`, `subcatch.c`,
`treatmnt.c`, `rdii.c`.

## 21.1 How these dialogs edit the file

Every dialog works on a *draft* of the rows that belong to one name — the
LID control, the subcatchment, the land use, the node — as the raw fields
of each row. **OK** (and **Apply**) writes the draft back as one command
and one undo step; **Cancel** or closing the window drops it. A row the
draft did not change is compared field for field with the file's and left
alone, so:

- OK on a dialog you did not edit writes nothing, adds no undo step and
  leaves the file byte for byte as it was (the test suite opens every
  dialog on the EPA sample models `LID_Model.inp`,
  `Groundwater_Model.inp` and `Site_Drainage_Model.inp`, presses OK, and
  compares the text);
- an edited row is rewritten in place, keeping its line and any trailing
  `;` comment; new rows go in after the object's last row, so an LID's
  layers stay together; a removed row is deleted;
- a trailing optional field you clear is dropped from the row rather than
  written empty, so the engine reads the row the way it did before.

The list dialogs (LID Controls, Aquifers, Snow Packs, Unit Hydrographs)
have `+` and `−` on the left: `+` adds an object with the defaults below
as its own undo step, `−` deletes the chosen one together with the rows
that use it (`[LID_USAGE]` rows for an LID control, `[GROUNDWATER]` rows
for an aquifer, `[RDII]` rows for a unit hydrograph set; a snow pack's
subcatchments keep the name and the validator reports them). The **Name**
field renames the object and every column that refers to it (the engine
has no rename of its own for these; StormSewer rewrites each referring
row). Choosing another object in the list while the draft has edits
applies them first, as the Curves dialog does.

Cells are the same widgets as the property sheet: a number, a keyword
list, or a combo over the names the model has (`*` for none). Units
follow `[OPTIONS] FLOW_UNITS`: inches and in/hr for a CFS/GPM/MGD model,
millimetres and mm/hr for CMS/LPS/MLD.

## 21.2 LID Controls

`[LID_CONTROLS]` (UM Appendix D; RM I §3.6, *LID Controls*). One process
is a `Name Type` row followed by one row per layer:
`Name Layer p1 p2 …`. Types: **BC** bio-retention cell, **RG** rain
garden, **GR** green roof, **IT** infiltration trench, **PP** permeable
pavement, **RB** rain barrel, **VS** vegetative swale, **RD** rooftop
disconnection. The dialog shows, for the chosen type, each layer it uses
with a tick box (ticked = the row exists) and the layer's fields:

| Layer | Fields (units: US / SI) | Meaning |
| --- | --- | --- |
| SURFACE | StorHt (in / mm), VegFrac (fraction), Rough (Manning n), Slope (%), Xslope (run per unit rise) | berm height, vegetation volume fraction (must be below 1), surface roughness, surface slope, swale side slope |
| SOIL | Thick (in / mm), Por, FC, WP (fractions), Ksat (in/hr / mm/hr), Kslope, Suct (in / mm) | thickness, porosity, field capacity, wilting point, saturated conductivity, conductivity slope, wetting-front suction head |
| PAVEMENT | Thick (in / mm), Vratio (voids/solids), FracImp (fraction), Perm (in/hr / mm/hr), Vclog, Treg (days), Freg (fraction) | thickness, void ratio, impervious fraction, permeability, clogging factor; regeneration interval and degree (optional, SWMM 5.2) |
| STORAGE | Height (in / mm), Vratio, Seepage (in/hr / mm/hr), Vclog, Covrd (YES/NO) | thickness, void ratio, seepage into the native soil, clogging factor; covered (rain barrels) |
| DRAIN | Coeff (in/hr / mm/hr), Expon, Offset (in / mm), Delay (hr), Hopen, Hclose (in / mm), Qcurve | underdrain flow = Coeff × head^Expon; drain height; rain-barrel drain delay; open/close heads and a control curve (optional) |
| DRAINMAT | Thick (in / mm), Vratio, Rough | green-roof drainage mat thickness, void fraction, Manning n |
| REMOVALS | Pollutant, Removal (%) pairs | pollutant removal from the LID's outflow |

Which layers a type uses, and which the engine insists on (`lid.c`
`validateLidProc`, ERROR 184 when missing):

| Type | Layers | Required |
| --- | --- | --- |
| BC | SURFACE, SOIL, STORAGE, DRAIN, REMOVALS | SOIL |
| RG | SURFACE, SOIL, STORAGE, REMOVALS | SOIL |
| GR | SURFACE, SOIL, DRAINMAT, REMOVALS | SOIL, DRAINMAT |
| IT | SURFACE, STORAGE, DRAIN, REMOVALS | STORAGE |
| PP | SURFACE, PAVEMENT, SOIL, STORAGE, DRAIN, REMOVALS | PAVEMENT |
| RB | STORAGE, DRAIN, REMOVALS | — |
| VS | SURFACE, REMOVALS | — |
| RD | SURFACE, DRAIN, REMOVALS | — |

A layer row the type does not use is shown as *ignored* and kept (the
engine reads and ignores it). Changing the **Type** keeps every row and
adds, with defaults, the layers the new type requires. **As the engine
reads it** at the bottom prints the rows as they will be written and
names any required layer still missing. `+` adds a bio-retention cell
named `LID1`, `LID2`, …; **Duplicate** copies the chosen control under a
new name; `−` deletes it with the `[LID_USAGE]` rows that place it. The
defaults for a new layer are: surface 0 / 0.0 / 0.1 / 1.0 / 5; soil 12 /
0.5 / 0.2 / 0.1 / 0.5 / 10.0 / 3.5 (the soil of the planter boxes in the
EPA sample `LID_Model.inp`); pavement 6 / 0.15 / 0 / 100 / 0; storage 12 /
0.75 / 0.5 / 0 / NO; drain 0 / 0.5 / 0 / 6; drainage mat 3 / 0.5 / 0.1.
Ticking REMOVALS adds an empty row and **Add pollutant** adds a
pollutant / percent pair to it; a REMOVALS row still without a pair when
you press OK is left out, because a two-field row is what the engine reads
as the type row.

Checks: the type is one of the eight; each layer keyword is one of the
seven; a layer row has at least the fields its reader needs (SURFACE and
PAVEMENT 7, SOIL 9, STORAGE and DRAIN 6, DRAINMAT 5, REMOVALS 4, counting
the name and keyword); parameters are numbers; VegFrac is below 1;
REMOVALS name existing pollutants with percentages between 0 and 100; a
control without a type row (ERROR 183) and a required layer missing
(ERROR 184) are errors; an ignored layer is a warning.

## 21.3 LID Usage

`[LID_USAGE]` (UM Appendix D; RM I §3.6). One row per LID placement in a
subcatchment: `Subcatchment LID Number Area Width InitSat FromImp ToPerv
[RptFile DrainTo FromPerv]`. Pick the subcatchment at the top (the dialog
opens on the selected one, or from its sheet), edit the rows, **Add LID
unit** adds one.

| Field | Unit | Meaning |
| --- | --- | --- |
| LID | — | the `[LID_CONTROLS]` name |
| Number | — | how many identical units |
| Area | ft² / m² | surface area of one unit |
| Width | ft / m | width of the overland flow path across one unit (0 for a rain barrel or anything with no surface flow) |
| InitSat | % | initial saturation of the soil (or storage) layer |
| FromImp | % | percent of the subcatchment's *impervious* area whose runoff is sent to this LID; 0 when the LID takes only the rain falling on it |
| ToPerv | 0/1 | 1 returns the LID's surface and drain outflow to the subcatchment's pervious area instead of its outlet |
| RptFile | — | a file to receive the LID's water balance each step, or `*` |
| DrainTo | — | a node or subcatchment that receives the underdrain flow, or `*` for the subcatchment's outlet |
| FromPerv | % | percent of the *pervious* area's runoff sent to this LID (SWMM 5.2) |

The dialog totals Number × Area against the subcatchment's `Area` (acres
× 43 560 ft², or hectares × 10 000 m²) and says so in red when the LID
units are larger than the subcatchment; the engine refuses that model
with ERROR 187. Checks: the LID exists; Number, Area, Width are numbers;
InitSat, FromImp and FromPerv are between 0 and 100; DrainTo, when not
`*`, is a node or subcatchment. The sheet's **LID Usage…** button shows
the count (*2 LID units*).

## 21.4 Aquifers

`[AQUIFERS]` (UM Appendix D; RM I §5, *Groundwater*). One row per
aquifer, fourteen columns:

| Field | Unit | Meaning |
| --- | --- | --- |
| Por | fraction | porosity |
| WP | fraction | wilting point |
| FC | fraction | field capacity (WP < FC < Por) |
| Ksat | in/hr / mm/hr | saturated hydraulic conductivity |
| Kslope | — | slope of log(conductivity) against soil moisture deficit |
| Tslope | ft / m | slope of soil tension against moisture content (`gwater.c` reads it with the length factor, so the sheet shows ft or m) |
| ETu | fraction | fraction of total evaporation taken from the upper (unsaturated) zone |
| ETs | ft / m | depth into the lower saturated zone over which evaporation can occur |
| Seep | in/hr / mm/hr | seepage to deep groundwater at full saturation |
| Ebot | ft / m | elevation of the aquifer bottom |
| Egw | ft / m | initial water table elevation |
| Umc | fraction | initial upper-zone moisture content |
| ETupat | — | a monthly `[PATTERNS]` name scaling ETu (optional) |

`+` adds `Aquifer1`, … with the values of the EPA sample
`Groundwater_Model.inp`; the **Name** field renames the aquifer in every
`[GROUNDWATER]` row. Checks (`gwater.c`, ERROR 109): porosity positive,
field capacity below porosity, wilting point below field capacity,
conductivity positive; the pattern exists.

## 21.5 Groundwater

`[GROUNDWATER]` and `[GWF]` (UM Appendix D; RM I §5). Tick **Groundwater
flow from this subcatchment** to give the subcatchment a row
(`Subcatchment Aquifer Node Esurf A1 B1 A2 B2 A3 Dsw [Egwt Ebot Wgr
Umc]`):

| Field | Unit | Meaning |
| --- | --- | --- |
| Aquifer | — | the `[AQUIFERS]` row |
| Node | — | the node that receives the groundwater flow |
| Esurf | ft / m | ground surface elevation of the subcatchment |
| A1, B1, A2, B2, A3 | — | the flow equation's coefficients: flow = A1·(Hgw − Hcb)^B1 − A2·(Hsw − Hcb)^B2 + A3·Hgw·Hsw, heights above the aquifer bottom |
| Dsw | ft / m | fixed surface water depth at the receiving node, above its invert (0 = use the node's routed depth) |
| Egwt | ft / m | the water table elevation groundwater flow starts at (optional; blank or `*` means the receiving node's invert, as in `gwater.c`) |
| Ebot, Wgr, Umc | ft / m, ft / m, fraction | this subcatchment's aquifer bottom, initial water table and initial upper moisture, overriding the aquifer's (optional; blank or `*` inherits) |

Under it, two text boxes hold the `[GWF]` expressions: **LATERAL**
replaces the lateral flow equation, **DEEP** the deep percolation; a
blank box means the built-in equation and no row. The variables the
engine allows (`gwater.c`): `Hgw` water table height, `Hsw` surface water
height, `Hcb` channel bottom height, `Hgs` ground surface height, `Ks`
saturated conductivity, `K` unsaturated conductivity, `Theta` upper-zone
moisture, `Phi` porosity, `Fi` surface infiltration, `Fu` upper-zone
percolation, `A` subcatchment area; and the parser's functions (`sin`,
`cos`, `tan`, `cot`, `abs`, `sgn`, `sqrt`, `log`, `exp`, `asin`, `acos`,
`atan`, `acot`, `sinh`, `cosh`, `tanh`, `coth`, `log10`, `step`). The
dialog checks each identifier as you type and the validator reports an
unknown one as an error (the engine's ERROR 233). An expression is written
token by token; one you did not change keeps its original row. Checks:
the aquifer exists, the node exists, the numbers are numbers, the `[GWF]`
type is LATERAL or DEEP.

## 21.6 Snow Packs

`[SNOWPACKS]` (UM Appendix D; RM I §4, *Snowmelt*). A pack is up to four
rows, one per keyword, each with a tick box in the dialog:

| Row | Fields | Meaning |
| --- | --- | --- |
| PLOWABLE | Cmin, Cmax (in/hr·°F / mm/hr·°C), Tbase (°F / °C), FWF (fraction), SD0, FW0 (in / mm), SNN0 (fraction) | the plowable part of the impervious area: melt coefficients on Dec 21 and Jun 21, base melt temperature, free-water holding fraction, initial snow depth and free water, and the fraction of the impervious area that is plowable |
| IMPERVIOUS | Cmin, Cmax, Tbase, FWF, SD0, FW0, SD100 (in / mm) | the rest of the impervious area; SD100 is the depth above which the area is fully snow-covered (the areal depletion curve applies below it) |
| PERVIOUS | the same | the pervious area |
| REMOVAL | Dplow (in / mm), Fout, Fimp, Fperv, Fimelt, Fsub (fractions), Scatch | plowing: the depth at which it starts, and the fractions sent out of the system, to the impervious area, to the pervious area, melted at once, and moved to the subcatchment `Scatch` |

`+` adds `SnowPack1`, … with all four rows at the EPA GUI's defaults
(0.001 / 0.001 / 32 / 0.10 / 0 / 0 / 0; removal 1.0 and zeros); the
**Name** field renames the pack in every subcatchment's `SnowPack`
column. A subcatchment takes a pack through the **SnowPack** field on its
property sheet, a combo over the packs. The **Temperature and
adjustments** fold holds the `[TEMPERATURE]` and `[ADJUSTMENTS]` sections
as text, written with the same OK: `TIMESERIES name`, `FILE "name"
[start] [C|F]`, `WINDSPEED MONTHLY v1…v12`, `WINDSPEED FILE`, `SNOWMELT
Stemp ATIwt RNM Elev Lat DTLong`, `ADC IMPERVIOUS f0…f9`, `ADC PERVIOUS
f0…f9`; and `TEMPERATURE | EVAPORATION | RAINFALL | CONDUCTIVITY` followed
by twelve monthly values. Checks (`snow.c`): the keyword is one of the
four; a PLOWABLE, IMPERVIOUS or PERVIOUS row has its seven numbers (nine
fields), a REMOVAL row its six; a subcatchment's `SnowPack` names a pack
that exists; `Scatch` names a subcatchment.

## 21.7 Buildup / Washoff

`[BUILDUP]` and `[WASHOFF]` (UM Appendix D; RM III §2, *Pollutant
Buildup* and §3, *Pollutant Washoff*). The left list is the land uses;
each has one buildup and one washoff row per pollutant. Land uses and
pollutants themselves are added with Project → Land Uses… and Project →
Pollutants… (chapter 8).

`[BUILDUP]`: `LandUse Pollutant Function Coeff1 Coeff2 Coeff3 PerUnit`.

| Function | Coeff1 | Coeff2 | Coeff3 |
| --- | --- | --- | --- |
| NONE | — | — | — |
| POW | maximum buildup (mass per unit) | rate constant (mass per unit per day) | exponent (0.01–10) |
| EXP | maximum buildup | rate constant (1/days) | — |
| SAT | maximum buildup | half-saturation constant (days) | — |
| EXT | maximum buildup | scaling factor | a `[TIMESERIES]` name |

Buildup is per **PerUnit**: AREA (per acre, or per hectare in SI) or CURB
(per unit of curb length, in whatever unit the subcatchment's `CurbLen`
was entered). Mass is in the pollutant's units (lb or kg; counts for a
`#/L` pollutant).

`[WASHOFF]`: `LandUse Pollutant Function Coeff1 Coeff2 SweepRmvl BmpRmvl`.

| Function | Coeff1 | Coeff2 |
| --- | --- | --- |
| NONE | — | — |
| EXP | washoff coefficient, for runoff in in/hr (mm/hr) per unit area | washoff exponent |
| RC | rating-curve coefficient, mass/s per (flow unit)^Coeff2 | exponent |
| EMC | event mean concentration, mass/L | — |

SweepRmvl and BmpRmvl are the percentages removed by street sweeping and
by BMPs. **Add buildup row** / **Add washoff row** add a NONE row for the
first pollutant. Checks (`landuse.c`): the land use and pollutant exist;
the function keyword is one of the list; a buildup row other than NONE
has all seven fields and a PerUnit of AREA or CURB; an EXT row names an
existing time series; a washoff row other than NONE has at least five;
coefficients are numbers; the percentages are 0–100.

## 21.8 Land Use Coverages

`[COVERAGES]` (UM Appendix D; RM III §2). Pairs of land use and the
percent of the subcatchment's area under it. **Add land use** adds a
pair; the total is printed under the table and turns red above 100 %.
The engine accepts a total below 100 % (the rest generates no buildup);
above it, buildup and washoff are over-counted, which the validator
reports as a warning. A file row may carry several pairs; the dialog
unrolls them and, only when you change something, writes one pair per
row. Checks: each land use exists; each percent is a number.

## 21.9 Initial Loadings

`[LOADINGS]` (UM Appendix D; RM III §2). Pairs of pollutant and the
buildup present on the subcatchment when the run starts, in mass per unit
area (lb/acre or kg/ha), overriding what `[OPTIONS] DRY_DAYS` would
build up. **Add pollutant** adds a pair. Checks: the pollutant exists; the
buildup is a number.

## 21.10 Treatment

`[TREATMENT]` (UM Appendix D; RM III §5, *Treatment*). One expression per
pollutant at the node: `Node Pollutant Expression`, where the expression
is either `R = …`, the fraction removed, or `C = …`, the outlet
concentration. **Add treatment** adds `R = 0` for the first pollutant.
The engine (`treatmnt.c`) joins the tokens after the pollutant into one
string, so an expression may contain spaces; the dialog writes it back
token by token and reads it the same way. Variables: `HRT` hydraulic
residence time (hours), `DT` the routing time step (seconds), `FLOW` the
inflow (in the model's flow units), `DEPTH` the node's water depth,
`AREA` its surface area, any pollutant's inflow concentration by name,
and `R_<pollutant>` for another pollutant's removal fraction; plus the
parser's functions listed under Groundwater. A line under a row says
what is wrong with it (must start with R or C, needs an `=`, unknown
variable) and the validator reports the same as an error (ERROR 233).
Checks: the pollutant exists; the expression passes.

## 21.11 Unit Hydrographs

`[HYDROGRAPHS]` (UM Appendix D; RM I §6, *RDII*). A set is a `Name
RainGage` row and any number of `Name Month Response R T K [Dmax Drec
D0]` rows. **Rain gage** edits the first; the table holds the rest, one
per month and response, with **Add hydrograph row**:

| Field | Unit | Meaning |
| --- | --- | --- |
| Month | — | ALL, or JAN … DEC; a month's rows override ALL's |
| Response | — | SHORT, MEDIUM or LONG term hydrograph |
| R | fraction | the fraction of the rainfall that becomes RDII through this hydrograph; the three responses of a month sum to at most 1 |
| T | hr | time to peak |
| K | — | ratio of the recession limb's duration to the time to peak |
| Dmax, Drec, D0 | in / mm, in/day / mm/day, in / mm | initial abstraction: maximum depth, recovery rate, initial depth (optional) |

`+` adds `UH1`, … on the model's first rain gage with one SHORT, MEDIUM
and LONG row for ALL months, R = 0 (no RDII until you set it) and times
to peak of 1, 4 and 24 hours; the **Name** field renames the set in every
`[RDII]` row. Checks (`rdii.c`): the set has a gage row and the gage
exists; the month and response keywords are known; R, T, K are numbers,
R between 0 and 1, T not negative (ERROR 151); the responses of a month
sum to at most 1.01 (ERROR 153), which the dialog also prints.

## 21.12 RDII Inflow

`[RDII]` (UM Appendix D; RM I §6). Tick **RDII inflow at this node** to
give the node a row: **UnitHydrograph**, the `[HYDROGRAPHS]` set whose
gage's rainfall drives the inflow, and **SewerArea**, the sewershed area
(acres or hectares) contributing rainfall-derived infiltration and inflow
at the node. Checks: the set exists; the area is a number and not
negative (ERROR 155).

## 21.13 Property sheet

A subcatchment's sheet ends with four buttons and what the subcatchment
has behind each: **LID Usage…** (*no LID units*, *1 LID unit*, *2 LID
units*), **Groundwater…** (*none*, *aquifer flow*, *aquifer flow, 1 GWF
expression(s)*), **Land Use Coverages…** (*2 land use(s), 100 %*),
**Initial Loadings…** (*1 pollutant(s)*). Its **SnowPack** field is a
combo over the `[SNOWPACKS]` names. A node's sheet has **Treatment…** (*1
expression(s)*) and **RDII…** (*set UH1*). Pollutant inflows and
dry-weather pollutant rows are the node sheet's *Inflow* and *Dry Weather
Flow* sub-sheets (chapter 5): their **Constituent** field lists FLOW and
every pollutant.

## 21.14 Worked example: LID_Model.inp

Open `swmm/tests/fixtures/epa-samples/LID_Model.inp` (the EPA's LID
sample: six LID processes on a nine-subcatchment site) and Project → LID
Controls…. The list shows GreenRoof (BC), PorousPave (PP), Planters (BC),
InfilTrench (IT), RainBarrels (RB), Swale (VS). Choose *Planters*: a
bio-retention cell with SURFACE (StorHt 6, VegFrac 0.0, Rough 0.0, Slope
0.0, Xslope 5), SOIL (Thick 12, Por 0.5, FC 0.2, WP 0.1, Ksat 0.5, Kslope
10.0, Suct 3.5), STORAGE (Height 12, Vratio 0.5, Seepage 0.2, Vclog 0,
Covrd NO) and DRAIN (Coeff 0, Expon 1, Offset 0.5, Delay 6, Hopen 0,
Hclose 0). REMOVALS is unticked. Set Thick to 18 and OK: one undo step,
`Planters SOIL 18 0.5 0.2 0.1 0.5 10.0 3.5` in the file, every other row
of the section as it was. Ctrl+Z puts 12 back.

Select subcatchment *S4* on the map and click **LID Usage…** on its
sheet: one row, Planters × 30 at 500 ft² each, Width 0, FromImp 80 (80 %
of S4's impervious runoff is routed into the planters), ToPerv 0, RptFile
and DrainTo `*`, FromPerv 0. The line under the table reads *1 LID unit(s)
cover 15000 of 297079.2 (Number × Area)* — S4 is 6.82 acres. Type
300000 into Area and the line turns red with ERROR 187; Cancel.

Project → Validate Model on the unedited sample reports no errors.

## 21.15 Limitations

- The dialogs write values as text and check what the engine's readers
  check at input; they do not run the process models, so a physically
  odd but syntactically valid layer (a soil with Ksat 0) passes.
- `[LID_CONTROLS]` rows for a type the engine does not know are shown
  and kept but not interpreted; a layer row with more fields than the
  layout shows its extra fields read-only.
- `[COVERAGES]` and `[LOADINGS]` rows carrying several pairs are unrolled
  for editing and written one pair per row *only* when you change the
  pairs; otherwise they are untouched.
- `[GWF]` and `[TREATMENT]` expressions are checked for their identifiers,
  not parsed for syntax: an unbalanced parenthesis is the engine's
  ERROR 233 at run time.
- `[TEMPERATURE]` and `[ADJUSTMENTS]` are edited as text; the validator
  does not check them.
- `[EVAPORATION]`, `[TRANSECTS]`, `[STREETS]`, `[INLETS]`, `[EVENTS]`
  still have no dialog (chapter 8, *What has no dialog*).
- The engine's ERROR 188 (LID capture area larger than the impervious
  area) is not pre-checked; the LID area total (ERROR 187) is.
