# 5. Property sheet

The **Properties** sheet on the right edits the selected object. View →
Properties hides and shows it; the map's context menu *Edit Properties…*
and a double-click on the map bring it up.

## 5.1 What it shows

Every field on the sheet is a column of the object's row in its section,
taken from the same column table the document uses (SWMM 5.2 User's Manual
Appendix D, spelled without spaces: `Elevation`, `MaxDepth`, `InitDepth`,
`SurDepth`, `Aponded` for a junction). A column the document knows is a
field the sheet shows, and nothing else is, so a field cannot exist in the
table and not on the sheet.

Objects with rows in more than one section show them all, one block per
section:

| Selected | Sections on the sheet |
| --- | --- |
| Junction, outfall, divider, storage unit | its node section, then its `[INFLOWS]` and `[DWF]` rows (a row can be added from the sheet when there is none) |
| Conduit | `[CONDUITS]`, `[XSECTIONS]`, `[LOSSES]` (addable) |
| Orifice, weir | its link section, `[XSECTIONS]` |
| Pump, outlet | its link section |
| Subcatchment | `[SUBCATCHMENTS]`, `[SUBAREAS]`, `[INFILTRATION]`, then `[LID_USAGE]`, `[GROUNDWATER]`, `[COVERAGES]` rows |
| Rain gage | `[RAINGAGES]` |
| Label | `[LABELS]` |

A tag field writes the object's `[TAGS]` row. Sections not on the sheet
(`[RDII]`, `[TREATMENT]`, `[LOADINGS]`, `[COORDINATES]`, `[VERTICES]`,
`[POLYGONS]`) are edited in the attribute table or on the map.

The unit after a field (`ft`, `in`, `in/hr`, `ft²`, `acres`, or `m`, `mm`,
`mm/hr`, `m²`, `ha`) follows `[OPTIONS] FLOW_UNITS`: CFS, GPM and MGD are
U.S. customary, CMS, LPS and MLD are metric. It is a label only. Changing
`FLOW_UNITS` relabels every field and converts nothing; see
[chapter 16](16-troubleshooting.md) for the checklist.

## 5.2 Editing a field

Each field's widget is chosen by what the column is:

- a **number** — a text field; the value is written as you typed it, so
  `4973` stays `4973` and `0.0100` stays `0.0100`;
- a **keyword** from a fixed list — a dropdown (`FREE`, `NORMAL`, `FIXED`,
  `TIDAL`, `TIMESERIES` for an outfall type; `CIRCULAR`, `RECT_CLOSED`, … for
  a shape; `HORTON`, `GREEN_AMPT`, … for an infiltration row's method);
- a **yes/no** flag — a checkbox writing `YES` or `NO`;
- a **reference** to another object — a dropdown of the objects of that kind
  in the model (a conduit's nodes, a subcatchment's gage and outlet, a
  pump's curve, a gage's time series);
- **free text** otherwise.

Press Enter or leave the field to commit. Each commit is one command and one
undo step, labelled `Set <field> of <name>`. The row is rewritten with the
new value, padded to the section's ruler; every other row is untouched.

**Renaming.** The name field renames through the document's rename command,
which follows every reference: links that connect to a node, subcatchments
that drain to it, `[COORDINATES]`, `[TAGS]`, `[LABELS]` anchors,
`[INFLOWS]`, `[DWF]`, `[REPORT]` lists, and the object names inside
`[CONTROLS]` rules (the token after `NODE`, `LINK`, `PUMP`, `ORIFICE`,
`WEIR`, `OUTLET`, `CONDUIT`, `GAGE`). Names are matched the way the engine
matches them, without regard to case. A rename is one undo step.

**Type columns.** Some keywords change the row's layout: an outfall's
`Type`, a storage unit's `Shape`, a divider's `Type`, an outlet's `Type`, a
cross-section's `Shape`, a gage's `Source`, an infiltration row's method.
Changing one rewrites the row with the new layout's defaults for the columns
that appear, keeping the ones that carry over (name, elevation, endpoints).
See [chapter 17](17-file-formats.md) for the layouts.

## 5.3 Several objects at once

With more than one object selected the sheet shows `N objects selected` and
the **common fields** — the columns every selected row has, in the first
row's order. A value entered there applies to all of them as one undo step
(`Set Roughness on 12 rows`). Objects of different kinds share fewer common
fields; nodes of one kind share all of them.

Group edits over a whole section, with a filter, are quicker in the
attribute table's *Replace in column* ([chapter 6](06-attribute-tables.md)).

## 5.4 Multi-row objects

A curve, time series or pattern is many rows; the sheet does not edit those.
Double-click the object in the Project browser, or use Project → Curves…,
Time Series… or Patterns…, which edit the whole object as a draft
([chapter 8](08-project-dialogs.md)).
