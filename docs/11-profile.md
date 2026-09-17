# 11. Profile

A long-section between two nodes: ground, conduit inverts and crowns, node
shafts, the HGL at the animated instant, and the maximum-HGL envelope of
the run. It answers the question the map cannot: where does the water
surface sit against the pipe and the ground along this path?

## 11.1 Choosing a path

Three ways:

- Select exactly two nodes and choose View → Profile from Selection.
- View → Pick Profile Path…, then click the start node and the end node on
  the map (`Esc` cancels).
- Right-click a node → Profile from Here…, then click the end node.

The **From** and **To** dropdowns in the view's strip change the path
without going back to the map; **Swap** reverses it. The path between the
two nodes is found through the model's links, in either direction, so a
profile can run against the flow; it is drawn from the first node
station 0. If no chain of links joins the two nodes the view says so.

## 11.2 What is drawn, and where the elevations come from

The geometry comes from the model file the engine ran, reduced to inverts,
crowns, rims, offsets and lengths, by the engine's own rules rather than the
drawing's convenience:

- A link end's invert is the node invert plus the link's offset. With
  `LINK_OFFSETS ELEVATION` the offset column is an absolute elevation and is
  converted to a depth above the node invert, never below it.
- A conduit's crown is its invert plus the cross-section's rise (`Geom1`
  for the standard closed shapes).
- A node whose `MaxDepth` is zero or absent takes its rim from the highest
  crown among the links that connect to it, which is what the engine does
  when it validates a junction (UM §3.3). Otherwise rim = invert + MaxDepth.
- Stations accumulate conduit `Length`. Pumps, orifices, weirs and outlets
  have no length and are given a short nominal span so the plot can show
  them without stacking two nodes on one station.
- The ground line joins the rims.

A drop structure therefore shows as a step in the invert at the node, and
the shaft runs from the node invert to the rim.

## 11.3 The HGL

Two lines:

- **HGL** — each node's invert plus its reported `Depth` at the reporting
  period on the Time slider, joined node to node. The slider is shared with
  the Results map, so scrubbing time moves both.
- **Max HGL** (checkbox) — each node's invert plus its maximum depth over
  the whole run, read from the `.out`. The engine reports at `REPORT_STEP`,
  so the animated line is a set of snapshots and can miss the true peak
  between steps; the envelope is the maximum of those snapshots at every
  node. Where it reaches the rim the node was surcharged to the surface —
  flooding if `ALLOW_PONDING` is off, ponding if it is on
  ([§16.4](16-troubleshooting.md)).

The elevation shown is the node's own invert plus depth, which is what the
engine reports as the node's head when the two agree; if you have edited a
node's invert since the run, the profile still shows the geometry the
engine ran (see §11.5).

The HGL is drawn between node heads only; no attempt is made to draw a
water surface inside a conduit between nodes, because the `.out` does not
carry one (it carries the link's depth at its midpoint, which is a
different thing).

## 11.4 Controls and export

- **V. exag.** — vertical exaggeration; the axis label states it.
- **Export PNG** — the view as drawn.
- **Export CSV** — the station table: one row per node with `node`,
  `kind`, `station`, `invert`, `rim`, `hgl`, `max_hgl`, `link_in`,
  `link_in_invert`, `link_in_crown`, `link_out`, `link_out_invert`,
  `link_out_crown`, in feet or metres by `FLOW_UNITS`.

The same station table is a block of the Model Report
([§2.10](02-tutorials.md)).

## 11.5 Which file the profile reads

The geometry is read from the model file the engine ran — the file on disk
at the path of the last run — not from the document you are editing. Two
consequences:

- Before the first run of a session the profile has nothing to draw; run
  the model (or Compare Engines) first.
- Edits made after a run do not appear in the profile until you run again.
  The map and the sheet show the edited document; the profile shows what
  the engine saw.

The first path offered is the model's first node to its first outfall, so
the view is never empty once a run exists.
