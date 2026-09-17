# 6. Attribute tables

An attribute table is one section of the file as a grid, with the section's
typed columns, editable in place. Open one with View → Attribute Table…
(for the selected object's section, or the last section shown), the
**Table** button in the Project tab, or the map's context menu.

The window title is `<SECTION> (n)`; a grid exists for every section with a
typed column layout (the list is in [chapter 17](17-file-formats.md)).

## 6.1 Reading

- **Sort** by clicking a column heading; click again to reverse. The arrow
  marks the sort column. Numbers sort as numbers.
- **Filter** with the box above the grid; rows whose name contains the text
  stay. `×` clears. The count reads `n of m rows · k selected`.
- **Columns…** chooses which columns show.
- Units are in the headings (`Elevation (ft)`) and follow `FLOW_UNITS` like
  the sheet.
- Rows are addressed by line, so a multi-row section — `[VERTICES]`,
  `[TIMESERIES]`, `[CURVES]`, `[POLYGONS]` — shows every row, one per line
  of the file, and editing one edits that line.

## 6.2 Selection

Row selection and map selection are the same selection, both ways: click a
row and the object highlights on the map; select on the map and the row
highlights. **Zoom to** centres the map on the selected row; **Properties**
shows the sheet for it.

## 6.3 Editing

Click a cell and type. The commit is the same command the sheet issues, one
undo step per cell (`Set Roughness of C3`), with the same widgets: dropdowns
for keywords and references, checkboxes for flags. A name cell renames with
every reference following.

## 6.4 Replace in column

The bulk edit people otherwise script. Choose a column, enter a value, and
click **Set all** to write it to *every filtered row*, or to the selected
rows when there is a selection. One undo step, labelled
`Set <column> on <n> rows`.

**or scale by** multiplies instead: choose a column, enter a factor, click
**Scale**. Non-numeric cells are left alone; a factor that is not a number
is refused. `Scaled <column> on <n> rows` is one undo step.

Filter first, then replace. To set Manning's n on every conduit whose name
starts with `C1`: filter `C1`, column `Roughness`, value `0.013`, Set all.

## 6.5 Copy, export, import

- **Copy as TSV** puts the visible rows (headings first, tabs between
  cells) on the clipboard, ready for a spreadsheet.
- **Export CSV…** writes the same as a file, quoting cells that need it.
- **Import CSV…** reads a CSV or TSV whose first line names the columns.
  The `Name` column (or the first column) picks the row; every other named
  column that the row's layout has is set. Rows the file names but the
  section does not have are ignored, and so are columns the section does not
  have. One undo step: `Updated n rows from <file>`.

Round trip: Export CSV, edit in a spreadsheet, Import CSV. Keep the `Name`
column and do not rename objects in the spreadsheet — rename in the table
so references follow.
