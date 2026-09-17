// SPDX-License-Identifier: GPL-3.0-or-later

//! An index of the EPA SWMM 5.2 engine's `ERROR nnn` and `WARNING nn`
//! codes, each with what it means in plain language, what usually causes
//! it, and what people needed to do about it.
//!
//! The codes and their subjects come from the public-domain SWMM 5.2
//! User's Manual (Appendix E) and the engine's own `error.c`; the wording
//! here is ours. The remedies are the ones that recur in the OpenSWMM and
//! GitHub threads: ERROR 209 is nearly always a deleted object that a
//! `[SUBCATCHMENTS]`, `[INFLOWS]` or `[REPORT]` row still names; ERROR 363
//! is a rain or inflow file with a header, blank line, or date the reader
//! cannot take; ERROR 335 is a hotstart file made by a different model.
//!
//! [`parse_line`] pulls the code and, where the message names one, the
//! object out of a report line, so the run panel can make the line
//! clickable.

/// Whether a code is fatal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Error,
    Warning,
}

/// One engine message code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    pub kind: Kind,
    pub code: u16,
    /// What the engine is saying, in a few words.
    pub meaning: &'static str,
    /// The usual reason.
    pub cause: &'static str,
    /// What to do, including which `.inp` section to look in.
    pub fix: &'static str,
}

impl Code {
    /// `ERROR 209` / `WARNING 03`.
    pub fn label(&self) -> String {
        match self.kind {
            Kind::Error => format!("ERROR {}", self.code),
            Kind::Warning => format!("WARNING {:02}", self.code),
        }
    }
}

macro_rules! codes {
    ($( $kind:ident $code:literal : $meaning:literal ; $cause:literal ; $fix:literal ),* $(,)?) => {
        &[ $( Code { kind: Kind::$kind, code: $code, meaning: $meaning, cause: $cause, fix: $fix } ),* ]
    };
}

/// Every code in the index, warnings first, then errors by number.
pub const ALL: &[Code] = codes![
    Warning 1: "Wet-weather time step reduced to the rain gage's recording interval";
        "WET_STEP in [OPTIONS] is longer than the gage's rainfall interval, so runoff would skip rain.";
        "Nothing is wrong; set WET_STEP no longer than the shortest gage interval to silence it.",
    Warning 2: "A node's maximum depth was raised";
        "A conduit's crown (invert + offset + height) is above the node's rim (invert + MaxDepth), so the engine raised the rim to fit the pipe.";
        "Check that node's MaxDepth in [JUNCTIONS] / [STORAGE] and the link's offsets in [CONDUITS] / [XSECTIONS]; the reported rim is no longer what the file says.",
    Warning 3: "A negative link offset was set to zero";
        "An InOffset/OutOffset (or ELEVATION-mode offset below the node invert) was negative; the engine silently uses 0 and the pipe invert moves up to the node invert.";
        "Fix the offset in [CONDUITS] / [ORIFICES] / [WEIRS] / [OUTLETS], or lower the node invert in [JUNCTIONS]; otherwise the pipe is not where the drawing says.",
    Warning 4: "Minimum elevation drop used for a conduit";
        "The conduit's two invert elevations are so close that the slope would be below the engine's floor.";
        "Give the conduit a real drop, or set MIN_SLOPE in [OPTIONS] deliberately.",
    Warning 5: "Minimum slope used for a conduit";
        "The conduit's slope is below MIN_SLOPE in [OPTIONS], so that value is used instead.";
        "Check the two node inverts and offsets in [CONDUITS]; a flat pipe here is usually a data-entry slip.",
    Warning 6: "Dry-weather time step increased to the wet-weather step";
        "DRY_STEP was shorter than WET_STEP in [OPTIONS], which makes no sense, so DRY_STEP was raised.";
        "Set DRY_STEP >= WET_STEP.",
    Warning 7: "Routing time step reduced to the wet-weather step";
        "ROUTING_STEP was longer than WET_STEP in [OPTIONS].";
        "Set ROUTING_STEP no longer than WET_STEP (it is normally much shorter).",
    Warning 8: "Elevation drop exceeds a conduit's length";
        "The invert drop between the conduit's ends is greater than its Length, so the slope is over 100%; the engine keeps going but the geometry is suspect.";
        "Check Length and the node inverts in [CONDUITS] / [JUNCTIONS]; a drop that big is usually a wrong invert or a length entered in the wrong units.",
    Warning 9: "Time series interval greater than the gage's recording interval";
        "The rain gage's Interval in [RAINGAGES] is shorter than the spacing of its time series values.";
        "Set the gage Interval to the series' actual spacing, or the rain is spread over the wrong duration.",
    Warning 10: "A regulator's crest is below the downstream invert";
        "A weir crest or orifice/outlet offset is lower than the invert of the node it discharges to, so it may flow backwards.";
        "Check CrestHt/Offset in [WEIRS] / [ORIFICES] / [OUTLETS] against the downstream node's Elevation.",
    Warning 11: "A control rule compares attributes that do not go together";
        "A [CONTROLS] clause names an object attribute the object kind does not have (a conduit's SETTING, a node's FLOW, and so on).";
        "Rewrite the clause in [CONTROLS] with an attribute the object supports.",
    Error 101: "Out of memory";
        "The model, or its output, needs more memory than the process could get.";
        "Shorten the run, lengthen REPORT_STEP, or reduce the number of reported objects in [REPORT].",
    Error 103: "Kinematic-wave equations could not be solved for a link";
        "Under FLOW_ROUTING KINWAVE the named conduit's geometry (usually a zero or adverse slope, or an odd cross-section) defeats the solver.";
        "Fix the slope in [CONDUITS] / node inverts, or switch to DYNWAVE.",
    Error 105: "The ODE solver could not be opened";
        "Internal: the runoff/groundwater integrator failed to initialise.";
        "Rerun; if it repeats, the engine binary is broken.",
    Error 107: "No valid time step could be computed";
        "Routing could not find a usable time step, typically because a conduit has zero length or the routing step is zero.";
        "Check ROUTING_STEP in [OPTIONS] and every Length in [CONDUITS].",
    Error 108: "A subcatchment's outlet name is ambiguous";
        "The Outlet in [SUBCATCHMENTS] matches both a node and a subcatchment name.";
        "Rename one of them; the engine cannot tell which you meant.",
    Error 109: "Invalid aquifer parameters";
        "A row in [AQUIFERS] has a porosity, field capacity, wilting point or elevation out of range.";
        "Check that row: 0 < wilting point < field capacity < porosity, and the bottom elevation below the water table.",
    Error 110: "Ground elevation below the water table";
        "A [GROUNDWATER] row puts the surface below the initial groundwater elevation.";
        "Check Esurf against Egwt on that row.",
    Error 111: "Invalid conduit length";
        "Length in [CONDUITS] is zero or negative.";
        "Enter the real length; auto-length from coordinates needs the nodes to have coordinates.",
    Error 112: "Elevation drop exceeds a conduit's length";
        "The invert drop between the ends is greater than the Length, and this build treats it as fatal.";
        "Check Length and inverts in [CONDUITS] / [JUNCTIONS].",
    Error 113: "Invalid conduit roughness";
        "Roughness in [CONDUITS] is zero or negative.";
        "Enter Manning's n (0.01-0.03 for pipes).",
    Error 114: "Invalid number of barrels";
        "Barrels in [XSECTIONS] is zero or negative.";
        "Use 1 or more.",
    Error 115: "Adverse slope under kinematic-wave routing";
        "KINWAVE cannot route uphill; the conduit's downstream invert is above its upstream invert.";
        "Fix inverts/offsets in [CONDUITS], reverse the link, or use DYNWAVE.",
    Error 117: "A link has no cross-section";
        "There is no [XSECTIONS] row for the named conduit, orifice or weir.";
        "Add one; every conduit, orifice and weir needs a shape.",
    Error 119: "Invalid cross-section";
        "An [XSECTIONS] row has a zero dimension, an unknown shape, or a shape that the link type cannot use (a weir is RECT_OPEN/TRAPEZOIDAL/TRIANGULAR; an orifice is CIRCULAR/RECT_CLOSED).";
        "Check Shape and Geom1..Geom4 on that row.",
    Error 121: "A pump has no usable curve";
        "The Curve named in [PUMPS] is missing, or is not a PUMP1-PUMP5 type in [CURVES].";
        "Define the pump curve in [CURVES] with a PUMPn type, or use * for an ideal pump.",
    Error 122: "Pump start-up depth not above shut-off depth";
        "Startup <= Shutoff in [PUMPS].";
        "Set Startup higher than Shutoff, or leave both 0 and drive the pump from [CONTROLS].",
    Error 131: "Cyclic loop in the drainage network";
        "Under STEADY or KINWAVE routing the links listed form a loop; those methods need a tree.";
        "Break the loop, reverse a link, or switch to DYNWAVE.",
    Error 133: "A node has more than one outlet link";
        "Under STEADY or KINWAVE routing each non-divider node may have one outgoing link.";
        "Make the node a divider, remove a link, or use DYNWAVE.",
    Error 134: "Illegal dummy link connections";
        "A DUMMY cross-section link is connected in a way the routing cannot handle.";
        "Check the DUMMY links' end nodes in [CONDUITS] / [XSECTIONS].",
    Error 135: "A divider does not have two outlet links";
        "A node in [DIVIDERS] needs exactly two outgoing links.";
        "Add or remove links at that divider.",
    Error 136: "A divider's diversion link is invalid";
        "DivertLink in [DIVIDERS] is not one of the divider's outgoing links.";
        "Name one of the two outgoing links.",
    Error 137: "Invalid weir-divider parameters";
        "A WEIR-type row in [DIVIDERS] has a bad Qmin, Height or Cd.";
        "Check those three values.",
    Error 138: "A node's initial depth exceeds its maximum depth";
        "InitDepth > MaxDepth in [JUNCTIONS] / [STORAGE] / [DIVIDERS].";
        "Lower InitDepth or raise MaxDepth.",
    Error 139: "A regulator is the outlet of a non-storage node under this routing";
        "Under STEADY or KINWAVE an orifice, weir or outlet may only leave a storage node.";
        "Make the upstream node a storage unit, or use DYNWAVE.",
    Error 141: "An outfall has more than one inlet link, or an outlet link";
        "Outfalls take exactly one incoming link and nothing leaving.";
        "Check the links at that node in [CONDUITS] and the regulator sections; add a junction upstream if two pipes must join.",
    Error 143: "A regulator has an invalid cross-section shape";
        "An orifice or weir in [XSECTIONS] uses a shape that type cannot take.";
        "Orifices: CIRCULAR or RECT_CLOSED. Weirs: RECT_OPEN, TRAPEZOIDAL or TRIANGULAR.",
    Error 145: "The network has no acceptable outlet";
        "Under STEADY or KINWAVE no outfall is reachable, or there is none.";
        "Add an [OUTFALLS] node at the downstream end.",
    Error 151: "A unit hydrograph has an invalid time base";
        "A [HYDROGRAPHS] row has T or K parameters that give a non-positive time base.";
        "Check R, T and K on that row.",
    Error 153: "A unit hydrograph set's response ratios are invalid";
        "The R values in a [HYDROGRAPHS] set sum to more than 1, or are negative.";
        "Keep each R in 0-1 and the three together at most 1.",
    Error 155: "Invalid sewer area for RDII";
        "SewerArea in [RDII] is zero or negative.";
        "Enter the sewershed area feeding that node.",
    Error 156: "Ambiguous station id for a rain gage";
        "A FILE-source [RAINGAGES] row names a station that matches more than one in the file.";
        "Use the full station id as written in the rain file.",
    Error 157: "Inconsistent rainfall format for a gage";
        "Two gages read the same time series or file with different Format / Interval / Units.";
        "Give each gage its own series, or make the settings agree.",
    Error 158: "A rain gage's time series is also used by another object";
        "The series in [RAINGAGES] is also named in [INFLOWS], [OUTFALLS] or elsewhere.";
        "Copy the series under another name for the other use.",
    Error 159: "Recording interval greater than the time series interval";
        "Interval in [RAINGAGES] is longer than the spacing of the series values, so values would be skipped.";
        "Set the gage Interval to the series' spacing.",
    Error 161: "Cyclic dependency in treatment functions";
        "A [TREATMENT] expression at a node refers to a pollutant whose own expression refers back.";
        "Rewrite the expressions so no pollutant depends on itself.",
    Error 171: "A curve has invalid or out-of-sequence data";
        "X values in a [CURVES] curve must increase; a repeated or decreasing X, or a bad number, stops the read.";
        "Sort the points by X and remove duplicates.",
    Error 173: "A time series has its data out of sequence";
        "Dates/times in a [TIMESERIES] series (or its file) must increase.";
        "Sort the series; watch for a missing date on a row that crosses midnight.",
    Error 181: "Invalid snowmelt climatology parameters";
        "The [TEMPERATURE] SNOWMELT line has values out of range.";
        "Check dividing temperature, ATI weight, negative melt ratio, elevation, latitude and longitude correction.",
    Error 182: "Invalid snow pack parameters";
        "A [SNOWPACKS] row has a melt coefficient, base temperature or fraction out of range.";
        "Check that pack's PLOWABLE / IMPERVIOUS / PERVIOUS rows.",
    Error 183: "An LID control has no type";
        "The first row for an LID in [LID_CONTROLS] must name its type (BC, RG, GR, IT, PP, RB, RD, VS).";
        "Add the type row.",
    Error 184: "An LID control is missing a layer";
        "The LID type needs a layer (SURFACE, SOIL, STORAGE, PAVEMENT, DRAIN, DRAINMAT) that [LID_CONTROLS] does not give.";
        "Add the missing layer row.",
    Error 185: "Invalid LID parameter value";
        "A layer row in [LID_CONTROLS] has a value out of range (a porosity above 1, a negative thickness).";
        "Check that LID's rows.",
    Error 186: "Invalid LID placement parameters";
        "A [LID_USAGE] row has a bad Number, Area, Width, InitSat, FromImp or ToPerv.";
        "Check that row; percentages are 0-100 and Area is per unit.",
    Error 187: "LID area exceeds the subcatchment area";
        "Number x Area in [LID_USAGE] is more than the subcatchment's Area.";
        "Reduce the LID count or unit area.",
    Error 188: "LID capture area exceeds the impervious area";
        "The FromImp percentages in [LID_USAGE] capture more impervious runoff than the subcatchment has.";
        "Reduce FromImp so the sum over the subcatchment's LIDs is at most 100%.",
    Error 191: "Simulation start date is after the end date";
        "START_DATE/START_TIME is later than END_DATE/END_TIME in [OPTIONS].";
        "Fix the dates; a missing year or an mm/dd swap is the usual cause.",
    Error 193: "Report start date is after the end date";
        "REPORT_START_DATE/TIME is later than END_DATE/TIME in [OPTIONS].";
        "Set the report start within the simulation.",
    Error 195: "Reporting step or duration is less than the routing step";
        "REPORT_STEP is shorter than ROUTING_STEP, or the whole run is shorter than one routing step.";
        "Lengthen REPORT_STEP or shorten ROUTING_STEP in [OPTIONS].",
    Error 200: "One or more errors in the input file";
        "The summary line after the input-read errors listed above it.";
        "Fix the numbered errors that precede this line in the report.",
    Error 201: "An input line is too long";
        "A line in the .inp exceeds the engine's line buffer (1024 characters).";
        "Split the line; time series and curves may carry several pairs per row, but not that many.",
    Error 203: "Too few items on a line";
        "A row in the named section is missing required columns.";
        "Compare the row with the section's column list; a tab lost to a text editor is a common cause.",
    Error 205: "Invalid keyword";
        "A section header or a keyword token (option name, shape, type) is not one the engine knows.";
        "Check spelling against Appendix D of the User's Manual; a newer section in an older engine also shows as this.",
    Error 207: "Duplicate id name";
        "Two objects in the same class share a name (names are case-insensitive: j1 and J1 collide).";
        "Rename one; nodes share a namespace across [JUNCTIONS], [OUTFALLS], [STORAGE], [DIVIDERS], as do all link sections.",
    Error 209: "Undefined object";
        "A row names an object that does not exist: a rain gage or outlet in [SUBCATCHMENTS], an end node in a link section, a series or pattern in [INFLOWS] / [DWF], a curve in [PUMPS] / [STORAGE] / [OUTLETS], or a node/link left in [REPORT] after a delete.";
        "Read the line number in the message, open that section, and define or rename the object it names. Deleted objects still listed under [REPORT] NODES/LINKS are the classic case.",
    Error 211: "Invalid number";
        "A field that must be numeric holds text, or a number is out of range (a negative area, a zero conduit length).";
        "Fix the value on the reported line; a decimal comma or a stray unit label is often the culprit.",
    Error 213: "Invalid date or time";
        "A date is not mm/dd/yyyy or a time is not hh:mm:ss (or decimal hours) on the reported line.";
        "Rewrite the date/time; days over 31 and yyyy/mm/dd both fail.",
    Error 217: "Control rule clause invalid or out of sequence";
        "A [CONTROLS] rule breaks the RULE / IF / AND-OR / THEN / ELSE / PRIORITY order, or a clause has the wrong shape.";
        "Every rule starts with RULE name, then IF, then THEN; each IF needs its own RULE line. Check object and attribute names.",
    Error 219: "Data for an unidentified transect";
        "A GR (station) row in [TRANSECTS] appears before any NC/X1 row names the transect.";
        "Put an X1 row with the transect name before its GR rows.",
    Error 221: "Transect station out of sequence";
        "Station values in [TRANSECTS] GR rows must increase left to right.";
        "Sort the station/elevation pairs.",
    Error 223: "A transect has no Manning's n";
        "No NC row gives roughness for the named transect.";
        "Add an NC row (left, right, channel n) before X1.",
    Error 225: "A transect has invalid overbank locations";
        "The left/right bank stations on the X1 row are not among the GR stations, or are out of order.";
        "Set the bank stations to values that appear in the GR rows, left before right.",
    Error 227: "A transect has no stations";
        "An X1 row names a transect with no GR rows.";
        "Add GR station/elevation rows.",
    Error 229: "A transect has too many stations";
        "More GR points than the engine allows (1500).";
        "Thin the cross-section.",
    Error 233: "Invalid math expression";
        "A [TREATMENT] or control-rule expression could not be parsed.";
        "Check operators, parentheses and variable names (C, R, HRT, DT, FLOW, DEPTH, AREA).",
    Error 301: "Files share the same name";
        "The .inp, .rpt and .out paths are the same file.";
        "Give the report and output distinct names (StormSewer does this for you; check [FILES]).",
    Error 303: "Cannot open the input file";
        "The .inp path does not exist, is locked, or (with stock engines) contains characters outside ASCII.";
        "Check the path; StormSewer copies models with non-ASCII paths to a scratch folder before running.",
    Error 305: "Cannot open the report file";
        "The .rpt cannot be created: the folder is read-only, or too many files are open in one process.";
        "Run from a writable folder; close other copies of the report.",
    Error 307: "Cannot open the binary results file";
        "The .out cannot be created (read-only folder, or the file is open in another program).";
        "Close whatever has the .out open and rerun.",
    Error 309: "Error writing the binary results file";
        "The disk filled up, or the file was removed mid-run.";
        "Free space; lengthen REPORT_STEP or report fewer objects in [REPORT].",
    Error 311: "Error reading the binary results file";
        "The .out is truncated or from a different run.";
        "Rerun the model.",
    Error 313: "Cannot open the scratch rainfall interface file";
        "The temporary folder is not writable.";
        "Set TEMPDIR in [OPTIONS] to a writable folder, or free the system temp.",
    Error 315: "Cannot open the rainfall interface file";
        "The USE RAINFALL file in [FILES] is missing or unreadable.";
        "Check the path in [FILES]; relative paths resolve against the .inp's folder.",
    Error 317: "Cannot open a rainfall data file";
        "A FILE-source gage in [RAINGAGES] names a file that is not there.";
        "Check the path on that gage row; quote it if it has spaces.",
    Error 318: "A line in a rainfall data file could not be read";
        "A row of the gage's rain file has an unexpected format, or a date out of sequence.";
        "Match the file to the format the gage row declares (STD, NWS, AES, ...), or use a user-defined time series instead.",
    Error 319: "Invalid format for the rainfall interface file";
        "The USE RAINFALL file is not one SWMM wrote.";
        "Regenerate it with SAVE RAINFALL.",
    Error 320: "Invalid date in the rainfall interface file";
        "The interface file's dates do not cover the simulation, or are corrupt.";
        "Regenerate it with SAVE RAINFALL for this period.",
    Error 321: "No data in the rainfall interface file for a gage";
        "The USE RAINFALL file has no record of the named gage.";
        "Regenerate it from a model that has this gage, or read the gage from its series instead.",
    Error 323: "Cannot open the runoff interface file";
        "The USE RUNOFF file in [FILES] is missing.";
        "Check the path.",
    Error 325: "Incompatible runoff interface file";
        "The USE RUNOFF file was written by a model with different subcatchments or pollutants.";
        "Regenerate it with SAVE RUNOFF from this model.",
    Error 327: "Read past the end of the runoff interface file";
        "The USE RUNOFF file covers a shorter period than this run.";
        "Regenerate it for the full period.",
    Error 329: "Error reading the runoff interface file";
        "The file is corrupt.";
        "Regenerate it.",
    Error 331: "Cannot open the hotstart file";
        "The USE HOTSTART path in [FILES] is missing or unreadable.";
        "Check the path; it resolves relative to the .inp. StormSewer copies it beside a scratch model when it runs from one.",
    Error 333: "Incompatible hotstart file";
        "The USE HOTSTART file was saved by a model with a different set of nodes, links, pollutants or land uses, or by another engine version.";
        "Regenerate it with SAVE HOTSTART from this model on this engine.",
    Error 335: "Error reading the hotstart file";
        "The hotstart file is truncated or from a different model, so its records run out or mismatch part-way through.";
        "Regenerate it with SAVE HOTSTART from the current model; if you edited objects since it was saved, it is stale.",
    Error 336: "No climate file specified for evaporation or wind";
        "[EVAPORATION] or [TEMPERATURE] says FILE, but no climate FILE line names one.";
        "Add the FILE line under [TEMPERATURE] (it serves evaporation and wind too).",
    Error 337: "Cannot open the climate file";
        "The [TEMPERATURE] FILE path is missing or unreadable.";
        "Check the path.",
    Error 338: "Error reading the climate file";
        "A row of the climate file has an unexpected format.";
        "Use the NCDC/GHCN daily formats the manual lists, or a user-prepared file with date, tmax, tmin, evap, wind.",
    Error 339: "Read past the end of the climate file";
        "The climate file stops before the simulation ends.";
        "Extend the file to cover END_DATE.",
    Error 341: "Cannot open the scratch RDII interface file";
        "The temporary folder is not writable.";
        "Set TEMPDIR in [OPTIONS] to a writable folder.",
    Error 343: "Cannot open the RDII interface file";
        "The USE RDII file in [FILES] is missing.";
        "Check the path.",
    Error 345: "Invalid format for the RDII interface file";
        "The USE RDII file is not one SWMM wrote.";
        "Regenerate it with SAVE RDII.",
    Error 351: "Cannot open a routing interface file";
        "The USE INFLOWS / SAVE OUTFLOWS file in [FILES] cannot be opened.";
        "Check the path and that the folder is writable.",
    Error 353: "Invalid format for a routing interface file";
        "The USE INFLOWS file is not in the outflows-file format.";
        "Use a file written by SAVE OUTFLOWS, or follow the routing interface format in the manual.",
    Error 355: "Mismatched names in a routing interface file";
        "The USE INFLOWS file names nodes or pollutants this model does not have.";
        "Match the node names, or regenerate the file from the upstream model.",
    Error 357: "Inflows and outflows interface files have the same name";
        "USE INFLOWS and SAVE OUTFLOWS in [FILES] point at one file.";
        "Give them different names.",
    Error 361: "Could not open an external time series file";
        "A FILE-source series in [TIMESERIES] names a file that is not there.";
        "Check the path on that row; it resolves relative to the .inp.",
    Error 363: "Invalid data in an external time series file";
        "A row of the series file is not `date time value` or `time value`: a header line, a blank line, a comma-separated row, a date in the wrong order, or a station name longer than the reader takes.";
        "Strip headers (or start them with ;), use whitespace between columns, mm/dd/yyyy dates, and one value per row; the file's rows must be in time order.",
    Error 401: "General system error";
        "The engine hit an internal failure it cannot name.";
        "Rerun; if it repeats, simplify the model to find the object that triggers it and report it upstream.",
    Error 402: "A new project was opened while one was still open";
        "Toolkit misuse: swmm_open called twice.";
        "Only relevant to API callers; close the first project.",
    Error 403: "Project not open, or the last run not ended";
        "Toolkit misuse: a call out of sequence.";
        "Only relevant to API callers; open, start, end in order.",
    Error 405: "The output would exceed the maximum file size";
        "Reporting periods x reported objects would push the .out past its size limit.";
        "Shorten the run (END_DATE), lengthen REPORT_STEP, or report fewer objects in [REPORT].",
];

/// Look up a code.
pub fn lookup(kind: Kind, code: u16) -> Option<&'static Code> {
    ALL.iter().find(|c| c.kind == kind && c.code == code)
}

/// Codes whose label, meaning, cause or fix contains `query`
/// (case-insensitive); everything when the query is blank.
pub fn search(query: &str) -> Vec<&'static Code> {
    let q = query.trim().to_ascii_lowercase();
    ALL.iter()
        .filter(|c| {
            q.is_empty()
                || c.label().to_ascii_lowercase().contains(&q)
                || c.code.to_string() == q
                || c.meaning.to_ascii_lowercase().contains(&q)
                || c.cause.to_ascii_lowercase().contains(&q)
                || c.fix.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

/// What a report line says, taken apart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLine {
    pub kind: Option<Kind>,
    pub code: Option<u16>,
    /// The text after the code.
    pub message: String,
    /// The object the message names, when it names one, as `(what, name)`
    /// — `("Node", "J1")`, `("Rain Gage", "RG1")`, `("object", "RG2")`.
    pub object: Option<(String, String)>,
    /// `[SECTION]` named by an input-read error.
    pub section: Option<String>,
    /// `at line N` of the input.
    pub line: Option<usize>,
}

impl ParsedLine {
    /// The index entry for this line's code.
    pub fn entry(&self) -> Option<&'static Code> {
        lookup(self.kind?, self.code?)
    }
}

/// Words the engine puts before an object name in its messages, longest
/// first so "Rain Gage" wins over "Gage".
const OBJECT_WORDS: &[&str] = &[
    "Unit Hydrograph",
    "Snow Pack",
    "Time Series",
    "Rain Gage",
    "Subcatchment",
    "Regulator",
    "Conduit",
    "Orifice",
    "Outfall",
    "Storage",
    "Divider",
    "Aquifer",
    "Transect",
    "Pattern",
    "Junction",
    "Outlet",
    "Curve",
    "Pump",
    "Link",
    "Node",
    "Weir",
    "Gage",
    "LID",
    "object",
    "name",
    "keyword",
    "number",
    "date/time",
];

/// Take an `ERROR nnn: ...` / `WARNING nn: ...` line apart.
pub fn parse_line(line: &str) -> ParsedLine {
    let t = line.trim();
    let (kind, rest) = if let Some(r) = t.strip_prefix("ERROR") {
        (Some(Kind::Error), r)
    } else if let Some(r) = t.strip_prefix("WARNING") {
        (Some(Kind::Warning), r)
    } else {
        (None, t)
    };
    let mut out = ParsedLine {
        kind,
        ..Default::default()
    };
    let rest = rest.trim_start();
    let (code, message) = if kind.is_some() {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let after = rest[digits.len()..].trim_start_matches(':').trim();
        (digits.parse::<u16>().ok(), after.to_string())
    } else {
        (None, rest.to_string())
    };
    out.code = code;
    out.message = message.clone();

    // "... at line 22 of [SUBCATCHMENTS]"
    if let Some(i) = message.find("at line ") {
        let tail = &message[i + 8..];
        let n: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        out.line = n.parse().ok();
    }
    if let (Some(a), Some(b)) = (message.find('['), message.find(']')) {
        if b > a + 1 {
            out.section = Some(message[a + 1..b].trim().to_ascii_uppercase());
        }
    }
    // The object. The engine's templates put it in one of three places:
    // at the start ("Outfall O1 has ..."), at the end ("... for Rain Gage
    // RG1"), or after a generic word in the input-read errors ("undefined
    // object RG2 at line 22", "invalid number 4.x at line 88").
    let clean = |s: &str| {
        s.trim_matches(|c: char| c == '.' || c == ',' || c == ':' || c == '"')
            .to_string()
    };
    let words: Vec<&str> = message.split_whitespace().collect();
    // Exact case: the engine capitalises the kind before a name ("Rain
    // Gage RG1") and not the same words in prose ("time series interval").
    let matches_at = |i: usize, parts: &[&str]| {
        i + parts.len() <= words.len()
            && parts.iter().enumerate().all(|(k, p)| words[i + k] == *p)
    };
    let specific: Vec<&&str> = OBJECT_WORDS.iter().filter(|w| !GENERIC_WORDS.contains(w)).collect();
    for w in &specific {
        let parts: Vec<&str> = w.split_whitespace().collect();
        if words.len() > parts.len() && matches_at(0, &parts) {
            let name = clean(words[parts.len()]);
            if !name.is_empty() {
                out.object = Some((w.to_string(), name));
                return out;
            }
        }
    }
    for w in &specific {
        let parts: Vec<&str> = w.split_whitespace().collect();
        if words.len() > parts.len() && matches_at(words.len() - parts.len() - 1, &parts) {
            let name = clean(words[words.len() - 1]);
            if !name.is_empty() {
                out.object = Some((w.to_string(), name));
                return out;
            }
        }
    }
    for w in GENERIC_WORDS {
        for i in 0..words.len().saturating_sub(1) {
            if words[i].eq_ignore_ascii_case(w) {
                let name = clean(words[i + 1]);
                if !name.is_empty() && !name.eq_ignore_ascii_case("at") {
                    out.object = Some((w.to_string(), name));
                    return out;
                }
            }
        }
    }
    out
}

/// Words that introduce a *value* in the input-read errors rather than an
/// object of a kind.
const GENERIC_WORDS: &[&str] = &["object", "name", "keyword", "number", "date/time"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_unique_and_has_all_three_texts() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(!a.meaning.is_empty() && !a.cause.is_empty() && !a.fix.is_empty(), "{}", a.label());
            for b in &ALL[i + 1..] {
                assert!(!(a.kind == b.kind && a.code == b.code), "duplicate {}", a.label());
            }
        }
        assert!(ALL.len() > 90);
        assert_eq!(lookup(Kind::Error, 209).unwrap().label(), "ERROR 209");
        assert_eq!(lookup(Kind::Warning, 3).unwrap().label(), "WARNING 03");
        assert!(lookup(Kind::Error, 999).is_none());
    }

    #[test]
    fn search_matches_text_and_number() {
        assert!(search("hotstart").iter().any(|c| c.code == 335));
        assert_eq!(search("209").len(), 1);
        assert_eq!(search("").len(), ALL.len());
        assert!(search("nothing matches this xyzzy").is_empty());
    }

    #[test]
    fn parses_object_section_and_line_from_report_lines() {
        let p = parse_line("  ERROR 209: undefined object RG2 at line 22 of [SUBCATCHMENTS]");
        assert_eq!(p.kind, Some(Kind::Error));
        assert_eq!(p.code, Some(209));
        assert_eq!(p.object, Some(("object".into(), "RG2".into())));
        assert_eq!(p.section.as_deref(), Some("SUBCATCHMENTS"));
        assert_eq!(p.line, Some(22));
        assert_eq!(p.entry().unwrap().code, 209);

        let p = parse_line("WARNING 09: time series interval greater than recording interval for Rain Gage RG1");
        assert_eq!(p.kind, Some(Kind::Warning));
        assert_eq!(p.code, Some(9));
        assert_eq!(p.object, Some(("Rain Gage".into(), "RG1".into())));

        let p = parse_line("  WARNING 03: negative offset ignored for Link C2");
        assert_eq!(p.object, Some(("Link".into(), "C2".into())));

        let p = parse_line("  ERROR 141: Outfall O1 has more than 1 inlet link or an outlet link.");
        assert_eq!(p.object, Some(("Outfall".into(), "O1".into())));

        let p = parse_line("  ERROR 335: error in reading from hotstart interface file warm.hsf.");
        assert_eq!(p.code, Some(335));
        assert!(p.object.is_none());

        let p = parse_line("plain text");
        assert_eq!(p.kind, None);
        assert!(p.entry().is_none());
    }
}
