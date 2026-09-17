# 7. Layers

The **Layers** tab in the left panel controls how the map draws each kind of
object, the underlay, and the results layers. Nothing here changes the
model; the settings are saved in your preferences and the next session opens
the same way.

## 7.1 Objects

One row per kind — rain gages, subcatchments, junctions, outfalls, dividers,
storage units, conduits, pumps, orifices, weirs, outlets, labels — with:

- a visibility checkbox;
- a **labels** checkbox, drawing the object names of that kind;
- **Symbol size**, with **reset**;
- **Map colour**, an override of the theme's colour for that kind, with
  reset.

**All on**, **All off** and **Defaults** act on the whole list.

## 7.2 Map

- **Flow arrows** — a direction arrow on every link (also View → Flow
  Arrows).
- **Grid** — the snap grid (also View → Grid).
- **Object labels (all)** — every kind's labels at once (also View →
  Object Labels).

## 7.3 Underlay

- **PNG** — the loaded image's name, an **opacity** slider, **Remove PNG**,
  **Load PNG…** (also File → Load PNG Background…).
- **DXF** — the loaded drawing's name, **Remove DXF**, **Load DXF…** (also
  File → DXF Underlay…).

Both are the storm-sewer workspace's drawing aids, placed by their own
scaling and not stored in the `.inp`. The model's own georeferenced image —
SWMM's `[BACKDROP]`, placed from a world file — is a separate thing,
controlled from View → Backdrop ([§3.9](03-map-and-tools.md)).

## 7.4 Results

Empty until a run has finished, then:

- **Node results** and **Link results** — turn the coloured overlay on the
  editing map on and off, per kind;
- **Legend** — show or hide the five-class legend and the frame timestamp
  in the map's corner.

Which variable colours the map, the class breaks, playback and the query
tool are in the Results view ([chapter 10](10-results-views.md)); the
layers tab only switches the overlay. With the overlay on, the map you edit
is the map that shows the results, so a node's colour and its Properties are
one click apart.
