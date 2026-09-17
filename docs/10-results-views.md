# 10. Results views

Four views read a finished run: **Results** (the map coloured by a
variable), **Plots** (time series and scatter), **Tables** (the report's
summary tables), and **Profile** ([chapter 11](11-profile.md)). **Chart**
is the older single-series plot kept for the panel; Plots supersedes it.
The view buttons are in the SWMM panel and the View menu.

Everything below comes from two files the engine wrote: the binary `.out`
for every reported variable at every reporting period, and the text `.rpt`
for the summary tables. StormSewer reads both directly
([chapter 17](17-file-formats.md)); nothing is recomputed.

## 10.1 Results on the map

The map is repainted with nodes and links coloured by one variable each.

- **Nodes**: Depth, Head, Volume, Lateral inflow, Total inflow, Flooding,
  then each pollutant's concentration.
- **Links**: Flow, Depth, Velocity, Volume, Capacity (fraction of full),
  then each pollutant.

These are the variables the engine writes to the `.out` (UM Appendix E), in
the units of the run (`FLOW_UNITS` and its system).

**Classes.** Five colour classes. With **Auto breaks** on, the four break
values are recomputed from the data each frame, by **Equal** intervals or
by **Quantile**. Turn it off to type **Node breaks** and **Link breaks** by
hand; hand breaks persist while you scrub time.

**Width by flow** scales each link's stroke by its flow at the frame.
**Arrows** draws the flow direction, reversed where the flow is negative.

**Time.** The slider chooses the reporting period; **Play** animates at the
**speed** factor, `◀` and `▶` step, **Peaks** colours every object by its
maximum over the run and the legend says `showing run maxima`. The legend,
bottom-left, shows the five classes and the frame's date and time.

**Query.** Choose **nodes** or **links**, a variable, `>` or `<`, and a
value; the objects that satisfy it at the current frame (or at their peak,
in Peaks mode) are highlighted and counted (`3 match`). Use it to find
every node with flooding above 0, or every conduit above 0.9 capacity.

The colouring stays on the editing map as a layer; the Layers tab switches
it off ([chapter 7](07-layers.md)).

## 10.2 Plots

Several series on one plot, or a scatter of one against another.

- The strip above the plot picks **Node** or **Link**, the object, and the
  variable; **Add** adds the series. **Add selected** takes the map or
  table selection and the variable picked.
- **Clear** empties the plot. Click a series in the list to focus it.
- **Scatter** takes two series and plots the second against the first,
  sample by sample — a storage unit's depth against its outlet's flow gives
  the outlet's rating as the engine computed it.
- The cursor readout gives the time and each series' value under the
  pointer. The Time slider's frame is marked.
- **Statistics** for the focused series: samples, min, max, mean, total
  (Σ), and **Top peaks** — the N largest local maxima with their times.
- **Export CSV** writes every plotted series with shared `time_s` and
  `time_h` columns and one column per series (`J11 · Depth (ft)`).
  **Export PNG** writes the view as drawn.

The time axis is hours from the simulation start. Series are read from the
`.out` on demand, one object at a time, so a large model's plot opens as
fast as a small one's.

## 10.3 Tables

Every summary table the engine printed in the `.rpt`, in report order, as
a grid: the Subcatchment Runoff Summary, Node Depth, Node Inflow, Node
Surcharge, Node Flooding, Storage Volume, Outfall Loading, Link Flow, Flow
Classification, Conduit Surcharge and Pumping summaries, the quality
summaries when the model has pollutants, and the Routing Time Step Summary
as `Item` / `Value` rows. A table the engine replaced with a sentence (`No
nodes were flooded.`) shows that sentence.

The column headings are the engine's own words stacked (`Maximum / Depth /
Feet`), so the units are the run's. Sort by any column; filter by name;
click a row to select the object on the map and in the other views. **CSV**
writes the table.

The engine prints only the top five in the continuity and instability
lists; those are in the report text, which the Model Report includes in
full ([chapter 12](12-design-panel.md) for the design report,
[§2.10](02-tutorials.md) for the model report).

## 10.4 Export of a view as an image

PNG export of the Results map, Plots and Profile is a screenshot of the
view's rectangle taken from the window itself, because the drawing
toolkit has no offscreen renderer. The image is at the window's pixel
size and scale; enlarge the window for a larger image.
