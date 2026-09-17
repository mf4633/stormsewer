# 4. Project browser

The **Project** tab in the left panel shows the model as a tree, in the
order the EPA GUI uses, with a count per kind.

```
Title/Notes (2)
Options (33)
Climatology
  Evaporation (2)
  Temperature
  Adjustments
Hydrology
  Rain Gages (1)
  Subcatchments (8)
  Aquifers
  Snow Packs
  Unit Hydrographs
  LID Controls
Hydraulics
  Nodes: Junctions (12), Outfalls (1), Dividers (0), Storage Units (1)
  Links: Conduits (12), Pumps (0), Orifices (1), Weirs (1), Outlets (0)
Quality
  Pollutants (0)
  Land Uses (0)
Curves (1)
Time Series (3)
Patterns (0)
Controls
Map
  Labels, Backdrop
```

The counts are rows in the corresponding `[SECTION]`: Title/Notes counts
data lines in `[TITLE]`, Options counts keys in `[OPTIONS]`.

## 4.1 Using it

- **Expand a kind** to list its objects by name. **Click** a name to select
  it and zoom the map to it. **Double-click** a map object to show its
  Properties; double-click a kind that has a dialog (Rain Gages, Curves, Time
  Series, Patterns, Controls, Pollutants, Land Uses, Options, Title/Notes) to
  open that dialog on it.
- **Search** filters the tree to names containing the text; `×` clears.
- **`+`** adds one object of the chosen kind. A node, gage or subcatchment
  is placed at the model's centre (the origin in an empty model); a link
  needs two nodes, so `+` on a link kind switches to that link's map tool
  and the status bar says `click the start node on the map`. A curve, time
  series, pattern, pollutant or land use is added by opening its dialog. The
  new object gets the defaults in [§3.3](03-map-and-tools.md).
- **`−`** deletes the chosen object(s) of that kind, through the same
  *Delete takes more with it* check as the map.
- **Table** opens the kind's attribute table ([chapter 6](06-attribute-tables.md)).
- **Edit…** opens the kind's dialog ([chapter 8](08-project-dialogs.md)).

Selection is shared: what is selected in the tree is selected on the map,
in the open attribute table and in the Properties sheet.

## 4.2 Kinds without an editor

Aquifers, Snow Packs, Unit Hydrographs, LID Controls, Adjustments,
Temperature, Evaporation and the map Backdrop are listed and counted, and
their rows are preserved exactly in the file, but this build has no dialog
or sheet for them. `[LID_USAGE]` and `[GROUNDWATER]` have typed columns and
can be edited in the attribute table; `[AQUIFERS]`, `[SNOWPACKS]`,
`[HYDROGRAPHS]`, `[LID_CONTROLS]`, `[EVAPORATION]`, `[TEMPERATURE]` and
`[ADJUSTMENTS]` do not, and are edited in a text editor. See
[chapter 17](17-file-formats.md) for the full list of typed sections.
