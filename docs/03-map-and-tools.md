# 3. Map and tools

The map draws the model from the document and edits it with the toolbar's
tools. It is drawn from caches rebuilt whenever the document's text changes,
and those caches come only from the document, so the map and the file can
never disagree.

## 3.1 Symbols

The symbols follow the EPA SWMM GUI: a **junction** is a circle, an
**outfall** a triangle, a **storage unit** a rectangle, a **divider** a
diamond, a **rain gage** a drop. A **conduit** is a line with a direction
arrow when View → Flow Arrows is on; a **pump, orifice, weir or outlet** is a
line marked with its symbol at the midpoint. A **subcatchment** is a filled
outline with a dashed line from its centroid to its outlet. **Labels** are
text at a position, optionally anchored to a node.

Objects with no `[COORDINATES]` row cannot be drawn and are reported by the
validator (`no [COORDINATES] row, so it cannot be drawn`). See
[chapter 16](16-troubleshooting.md).

## 3.2 Tools

| Tool | Key | What it does |
| --- | --- | --- |
| Select | `S` | Click selects; Shift-click adds; Ctrl-click toggles; drag moves the selection or, from empty space, rubber-bands a box. |
| Pan | `H` | Drag to pan. The wheel zooms in every tool. |
| Zoom + | `+` | Click to zoom in on a point. |
| Zoom − | `-` | Click to zoom out from a point. |
| Zoom Win | `Z` | Drag a rectangle to zoom to it. |
| Gage | `R` | Click to place a rain gage. |
| Subcatch | `A` | Click each corner; double-click, Enter, or click the first corner to close; Esc cancels. |
| Junction | `J` | Click to place. |
| Outfall | `O` | Click to place. |
| Divider | `D` | Click to place. |
| Storage | `T` | Click to place. |
| Conduit | `C` | Click the start node, click to add vertices, click the end node; Esc cancels. |
| Pump | `P` | As Conduit. |
| Orifice | `I` | As Conduit. |
| Weir | `W` | As Conduit. |
| Outlet | `U` | As Conduit. |
| Label | `L` | Click where the label goes; a *Label text* prompt asks for the text. |

The letter keys work only when no text field has the keyboard. `Esc` in
order: cancels a profile pick, then an in-progress gesture, then clears the
selection, then returns to Select. `F` fits the whole model; `Delete`
removes the selection.

## 3.3 What a new object gets

A new object is one undo step and follows the EPA GUI's defaults for a new
object:

- **Junction**: next free name with prefix `J`, invert 0, MaxDepth 0 (the
  engine then uses the highest connecting conduit crown), InitDepth 0,
  SurDepth 0, Aponded 0. Outfall prefix `O`, divider `D`, storage `ST`.
- **Conduit**: prefix `C`; the length you drew in map units (minimum 1),
  Manning's n 0.01, offsets 0, InitFlow 0, MaxFlow 0; an `[XSECTIONS]` row
  `CIRCULAR 1`. Pump prefix `P` (ideal pump, curve `*`, ON); orifice `OR`
  (SIDE, 0.65); weir `W`; outlet `OL`.
- **Subcatchment**: prefix `S`; the model's first rain gage; the node nearest
  the polygon's centroid as outlet; area 5, 25 % impervious, width 500, slope
  0.5 %; a `[SUBAREAS]` row (n 0.01/0.1, storage 0.05/0.05, 25 % zero,
  OUTLET) and an `[INFILTRATION]` row whose columns follow `[OPTIONS]
  INFILTRATION` (Horton, Green-Ampt or Curve Number); a `[POLYGONS]` row per
  corner.
- **Rain gage**: prefix `R`; INTENSITY, 1:00, SCF 1.0, TIMESERIES with the
  model's first series or `*`.

Numbers are in the model's units, whatever `FLOW_UNITS` says. Nothing is
converted.

## 3.4 Selecting, moving, vertices

- A drag of the selection moves every selected node, gage, label, link
  vertex and polygon corner together. Links attached to a moved node follow
  it. One undo step per drag.
- Click a selected link or polygon near a vertex to make that vertex active
  (`Vertex chosen — drag it, or press Delete`); drag it, or `Delete` removes
  that vertex rather than the object.
- Double-click a selected link or polygon away from a vertex to insert a
  vertex there. Double-click a node or link to open its Properties.
- Double-clicking while drawing a subcatchment closes it; the second click of
  the pair is not taken as a corner.

## 3.5 Snapping and the grid

**Snap to Objects** (View → Snap to Objects) snaps a placed point, a link
end or a polygon corner to a nearby node. **Snap to Grid** snaps to
multiples of **Grid spacing**, which appears beneath it while it is on (in
map units). View → Grid draws the grid. Neither changes existing
coordinates.

## 3.6 Context menu

Right-click on the map:

| Item | On |
| --- | --- |
| Zoom Extents | anywhere |
| Zoom To | an object |
| Edit Properties… | an object — shows the Properties sheet |
| Attribute Table… | an object — opens its section's table |
| Profile from Here… | a node — then click the end node; Esc cancels |
| Reverse Link | a link — swaps From and To and reverses its vertices, so the drawn line is unchanged and the flow direction flips. The in/out offsets are *not* swapped; check them afterwards |
| Convert Node Type | a node — rewrites its row as a junction, outfall, divider or storage unit with that layout's defaults, keeping the name, invert and coordinates; links and subcatchments still point at it |
| Delete | the selection |

## 3.7 Copy, cut, paste

`Ctrl+C`, `Ctrl+X`, `Ctrl+V` and the Edit menu. Pasted objects get new
unique names; links between two pasted nodes are pasted with them; the paste
is offset by a fraction of the model's extent so it does not land on the
original. One undo step.

## 3.8 Delete

`Delete` removes the selection. When a node goes, so do its links, and a
subcatchment draining to it is left with a dangling outlet the validator
reports. Before doing that the *Delete takes more with it* dialog names the
links and the subcatchments it would orphan; *Delete all of it* or *Keep
them*. One undo step either way.

## 3.9 Backdrop and underlays

Two different things sit under the map.

**The backdrop** (View → Backdrop) is SWMM's own: a PNG or JPEG placed in
model coordinates and kept in the `.inp`'s `[BACKDROP]` section, so it
travels with the model and the EPA GUI shows it too.

- **Load Image…** — pick the image. If a world file (`.pgw` for PNG,
  `.jgw` for JPEG, or `.wld`) sits beside it, the image is georeferenced
  from it at once (`Backdrop site.png placed from site.pgw`); otherwise
  the status bar says to place it by hand.
- **Georeference…** — the *Georeference Backdrop* dialog: either the
  **Extent (map corners)** — lower-left and upper-right X and Y — or
  **Scale and offset** — map units per pixel in X and Y (or *same as X*)
  and the offset. **Fit to model** sets the extent to the model's, keeping
  the image's aspect. **Write world file** saves a `.pgw`/`.jgw` beside the
  image so the next load is automatic. OK writes `FILE`, `DIMENSIONS`,
  `UNITS`, `OFFSET` and `SCALING` to `[BACKDROP]` as one undo step.
- **Show** toggles it; **Remove** deletes the section (undoable).

A world file's rotation terms are ignored with a note: SWMM cannot draw a
rotated backdrop. Lengths are never measured from the picture; see
*Compute Conduit Lengths* below.

**Map Dimensions…** (View menu) edits `[MAP] DIMENSIONS` — the map's
extent as the EPA GUI records it — with **Set from model** (the drawn
objects' extent with 6 % room) and **Set from backdrop**, and `[MAP]
Units` (None, Feet, Meters, Degrees). StormSewer's own view always fits to
the objects; the dimensions matter to the EPA GUI and to the length
factor.

**Underlays** (File → Load PNG Background…, File → DXF Underlay…) are the
storm-sewer workspace's drawing aids, placed by their own width scaling
and offset, not stored in the `.inp`, with their opacity and Remove
buttons in the Layers tab. Use the backdrop for anything that should
survive in the model file.

## 3.10 Conduit lengths

Drawing a conduit writes its drawn length once ([§3.3](#33-what-a-new-object-gets)).
Moving a node afterwards does not change `Length`; nothing recomputes on
its own. Project → Compute Conduit Lengths… does it on request:

- a table of every conduit with both ends on the map: **Stored** length,
  **Drawn (map)** length through its vertices, **Geometric** = drawn × the
  factor, **Diff %**;
- the factor comes from `[MAP] Units` and `FLOW_UNITS` (map feet to model
  feet is 1; feet to metres 0.3048; metres to feet 3.28084; `DEGREES` is
  not a length and you must set the factor yourself; no `Units` line
  assumes map units are model units), and can be typed over;
- **Tick where |diff| >** a percentage, or **Selected conduits only**;
  **Apply to n ticked** writes the geometric lengths to `[CONDUITS]` as
  one undo step.

## 3.11 The status bar and the validation line

The status bar shows, left to right: the active tool and its key
(`Tool: Select (S)`); the pointer's model coordinates; `undo N`, the number
of steps that can be undone; the tool's hint; and the last file message.

The line under the map, `Validation: …`, is the live referential check over
the document ([chapter 16](16-troubleshooting.md) lists what it checks). It
reads `no findings`, or counts errors and warnings; click it to list them,
click a finding to select and zoom to its object. Project → Validate Model
does the same from the menu. Errors refuse a run; warnings do not.

## 3.12 What the map does not do

- No coordinate reference system. Coordinates are the model's map units and
  nothing converts them; a world file places an image in those units and
  that is all. GeoJSON export says so in a `note`.
- No automatic length: `Length` changes only when you draw a conduit or
  run Compute Conduit Lengths ([§3.10](#310-conduit-lengths)).
- No topology tools beyond snapping: no splitting a conduit at a node, no
  merging.
