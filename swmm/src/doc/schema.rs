// SPDX-License-Identifier: GPL-3.0-or-later

//! Column definitions for the EPA SWMM 5.2 input sections the editor needs,
//! and the table of which columns refer to which kind of object.
//!
//! Everything here is a table lookup over a row's fields. The variable-column
//! sections (`[OUTFALLS]` by type, `[STORAGE]` by shape, `[DIVIDERS]` by type,
//! `[OUTLETS]` by type, `[XSECTIONS]` by shape, `[RAINGAGES]` by source,
//! `[INFILTRATION]` by method, `[TIMESERIES]` and `[CURVES]` by field count)
//! pick a layout from the row itself, plus the `[OPTIONS] INFILTRATION`
//! value for `[INFILTRATION]`. Column names follow the SWMM 5.2 user manual,
//! Appendix D, spelled CamelCase without spaces; lookup is case-insensitive.

/// What a name refers to. Drives [`crate::doc::Command::Rename`] and the
/// cascade-delete builders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Node,
    Link,
    Subcatchment,
    Gage,
    Curve,
    Timeseries,
    Pattern,
}

impl ObjectKind {
    /// The sections that *define* objects of this kind, in the name column.
    pub fn defining_sections(self) -> &'static [&'static str] {
        match self {
            Self::Node => NODE_SECTIONS,
            Self::Link => LINK_SECTIONS,
            Self::Subcatchment => &["SUBCATCHMENTS"],
            Self::Gage => &["RAINGAGES"],
            Self::Curve => &["CURVES"],
            Self::Timeseries => &["TIMESERIES"],
            Self::Pattern => &["PATTERNS"],
        }
    }

    /// The `[TAGS]` kind keyword for this kind, where SWMM has one.
    pub fn tag_keyword(self) -> Option<&'static str> {
        match self {
            Self::Node => Some("Node"),
            Self::Link => Some("Link"),
            Self::Subcatchment => Some("Subcatch"),
            Self::Gage => Some("Gage"),
            _ => None,
        }
    }

    /// The `[CONTROLS]` keywords after which an object of this kind is named.
    pub fn control_keywords(self) -> &'static [&'static str] {
        match self {
            Self::Node => &["NODE"],
            Self::Link => &["LINK", "CONDUIT", "PUMP", "ORIFICE", "WEIR", "OUTLET"],
            Self::Gage => &["GAGE"],
            _ => &[],
        }
    }
}

pub const NODE_SECTIONS: &[&str] = &["JUNCTIONS", "OUTFALLS", "STORAGE", "DIVIDERS"];
pub const LINK_SECTIONS: &[&str] = &["CONDUITS", "PUMPS", "ORIFICES", "WEIRS", "OUTLETS"];

/// Sections whose rows are `KEY value...` rather than named objects.
pub const KEY_VALUE_SECTIONS: &[&str] = &[
    "OPTIONS",
    "REPORT",
    "MAP",
    "BACKDROP",
    "EVAPORATION",
    "TEMPERATURE",
    "ADJUSTMENTS",
];

/// Sections where one object legitimately spans several rows, so a repeated
/// name is not a duplicate.
pub const MULTI_ROW_SECTIONS: &[&str] = &[
    "VERTICES",
    "POLYGONS",
    "TIMESERIES",
    "CURVES",
    "PATTERNS",
    "INFLOWS",
    "DWF",
    "TAGS",
    "LID_CONTROLS",
    "LID_USAGE",
    "COVERAGES",
    "LOADINGS",
    "TREATMENT",
    "TRANSECTS",
    "HYDROGRAPHS",
    "BUILDUP",
    "WASHOFF",
    "SNOWPACKS",
    "GWF",
    "INLETS",
    "STREETS",
    "REPORT",
    "LABELS",
    "TITLE",
    "CONTROLS",
    "FILES",
];

/// Which field carries the object name for addressing, or `None` for free
/// text sections. `[TAGS]` rows are `Kind Name Tag`, so the name is second.
pub fn name_index(section: &str) -> Option<usize> {
    match section {
        "TAGS" => Some(1),
        "TITLE" | "CONTROLS" | "LABELS" | "FILES" | "PROFILES" => None,
        _ => Some(0),
    }
}

const RAINGAGES_SERIES: &[&str] = &["Name", "Format", "Interval", "SCF", "Source", "Series"];
const RAINGAGES_FILE: &[&str] = &[
    "Name", "Format", "Interval", "SCF", "Source", "File", "Station", "Units",
];
const SUBCATCHMENTS: &[&str] = &[
    "Name",
    "RainGage",
    "Outlet",
    "Area",
    "PctImperv",
    "Width",
    "PctSlope",
    "CurbLen",
    "SnowPack",
];
const SUBAREAS: &[&str] = &[
    "Subcatchment",
    "NImperv",
    "NPerv",
    "SImperv",
    "SPerv",
    "PctZero",
    "RouteTo",
    "PctRouted",
];
const INFIL_HORTON: &[&str] = &[
    "Subcatchment",
    "MaxRate",
    "MinRate",
    "Decay",
    "DryTime",
    "MaxInfil",
    "Method",
];
const INFIL_GREEN_AMPT: &[&str] = &["Subcatchment", "Suction", "Ksat", "IMD", "Method"];
const INFIL_CURVE_NUMBER: &[&str] = &[
    "Subcatchment",
    "CurveNum",
    "Conductivity",
    "DryTime",
    "Method",
];
const JUNCTIONS: &[&str] = &[
    "Name",
    "Elevation",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
];
const OUTFALLS_FREE: &[&str] = &["Name", "Elevation", "Type", "Gated", "RouteTo"];
const OUTFALLS_FIXED: &[&str] = &["Name", "Elevation", "Type", "Stage", "Gated", "RouteTo"];
const OUTFALLS_TIDAL: &[&str] = &["Name", "Elevation", "Type", "Curve", "Gated", "RouteTo"];
const OUTFALLS_SERIES: &[&str] = &["Name", "Elevation", "Type", "Series", "Gated", "RouteTo"];
const STORAGE_TABULAR: &[&str] = &[
    "Name",
    "Elevation",
    "MaxDepth",
    "InitDepth",
    "Shape",
    "Curve",
    "SurDepth",
    "Fevap",
    "Psi",
    "Ksat",
    "IMD",
];
const STORAGE_FUNCTIONAL: &[&str] = &[
    "Name",
    "Elevation",
    "MaxDepth",
    "InitDepth",
    "Shape",
    "Coeff",
    "Exponent",
    "Constant",
    "SurDepth",
    "Fevap",
    "Psi",
    "Ksat",
    "IMD",
];
const STORAGE_GEOMETRIC: &[&str] = &[
    "Name",
    "Elevation",
    "MaxDepth",
    "InitDepth",
    "Shape",
    "Length",
    "Width",
    "Z",
    "SurDepth",
    "Fevap",
    "Psi",
    "Ksat",
    "IMD",
];
const DIVIDERS_OVERFLOW: &[&str] = &[
    "Name",
    "Elevation",
    "DivertLink",
    "Type",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
];
const DIVIDERS_CUTOFF: &[&str] = &[
    "Name",
    "Elevation",
    "DivertLink",
    "Type",
    "Qmin",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
];
const DIVIDERS_TABULAR: &[&str] = &[
    "Name",
    "Elevation",
    "DivertLink",
    "Type",
    "Curve",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
];
const DIVIDERS_WEIR: &[&str] = &[
    "Name",
    "Elevation",
    "DivertLink",
    "Type",
    "Qmin",
    "Height",
    "Cd",
    "MaxDepth",
    "InitDepth",
    "SurDepth",
    "Aponded",
];
const CONDUITS: &[&str] = &[
    "Name",
    "FromNode",
    "ToNode",
    "Length",
    "Roughness",
    "InOffset",
    "OutOffset",
    "InitFlow",
    "MaxFlow",
];
const PUMPS: &[&str] = &[
    "Name", "FromNode", "ToNode", "Curve", "Status", "Startup", "Shutoff",
];
const ORIFICES: &[&str] = &[
    "Name",
    "FromNode",
    "ToNode",
    "Type",
    "Offset",
    "Qcoeff",
    "Gated",
    "CloseTime",
];
const WEIRS: &[&str] = &[
    "Name",
    "FromNode",
    "ToNode",
    "Type",
    "CrestHt",
    "Qcoeff",
    "Gated",
    "EndCon",
    "EndCoeff",
    "Surcharge",
    "RoadWidth",
    "RoadSurf",
    "CoeffCurve",
];
const OUTLETS_TABULAR: &[&str] = &[
    "Name", "FromNode", "ToNode", "Offset", "Type", "Curve", "Gated",
];
const OUTLETS_FUNCTIONAL: &[&str] = &[
    "Name", "FromNode", "ToNode", "Offset", "Type", "Qcoeff", "Qexpon", "Gated",
];
const XSECTIONS: &[&str] = &[
    "Link", "Shape", "Geom1", "Geom2", "Geom3", "Geom4", "Barrels", "Culvert",
];
const XSECTIONS_CUSTOM: &[&str] = &["Link", "Shape", "Geom1", "Curve", "Barrels"];
const XSECTIONS_IRREGULAR: &[&str] = &["Link", "Shape", "Transect"];
const XSECTIONS_STREET: &[&str] = &["Link", "Shape", "Street"];
const LOSSES: &[&str] = &["Link", "Kentry", "Kexit", "Kavg", "FlapGate", "Seepage"];
const INFLOWS: &[&str] = &[
    "Node",
    "Constituent",
    "Series",
    "Type",
    "Mfactor",
    "Sfactor",
    "Baseline",
    "Pattern",
];
const DWF: &[&str] = &[
    "Node",
    "Constituent",
    "Baseline",
    "Pattern1",
    "Pattern2",
    "Pattern3",
    "Pattern4",
];
const RDII: &[&str] = &["Node", "UnitHydrograph", "SewerArea"];
const TREATMENT: &[&str] = &["Node", "Pollutant", "Expression"];
const TIMESERIES_DATED: &[&str] = &["Name", "Date", "Time", "Value"];
const TIMESERIES_PLAIN: &[&str] = &["Name", "Time", "Value"];
const TIMESERIES_FILE: &[&str] = &["Name", "Source", "File"];
const CURVES_TYPED: &[&str] = &["Name", "Type", "X", "Y"];
const CURVES_PLAIN: &[&str] = &["Name", "X", "Y"];
const PATTERNS_TYPED: &[&str] = &["Name", "Type", "Multiplier"];
const PATTERNS_PLAIN: &[&str] = &["Name", "Multiplier"];
const COORDINATES: &[&str] = &["Node", "X", "Y"];
const VERTICES: &[&str] = &["Link", "X", "Y"];
const POLYGONS: &[&str] = &["Subcatchment", "X", "Y"];
const SYMBOLS: &[&str] = &["Gage", "X", "Y"];
const LABELS: &[&str] = &[
    "X", "Y", "Label", "Anchor", "Font", "Size", "Bold", "Italic",
];
const TAGS: &[&str] = &["Kind", "Name", "Tag"];
const KEY_VALUE: &[&str] = &["Option", "Value"];
const MAP_DIMENSIONS: &[&str] = &["Option", "X1", "Y1", "X2", "Y2"];
const GROUNDWATER: &[&str] = &[
    "Subcatchment",
    "Aquifer",
    "Node",
    "Esurf",
    "A1",
    "B1",
    "A2",
    "B2",
    "A3",
    "Dsw",
    "Egwt",
    "Ebot",
    "Wgr",
    "Umc",
];
const LID_USAGE: &[&str] = &[
    "Subcatchment",
    "LID",
    "Number",
    "Area",
    "Width",
    "InitSat",
    "FromImp",
    "ToPerv",
    "RptFile",
    "DrainTo",
    "FromPerv",
];
const COVERAGES: &[&str] = &["Subcatchment", "LandUse", "Percent"];
const LOADINGS: &[&str] = &["Subcatchment", "Pollutant", "Buildup"];
const INLET_USAGE: &[&str] = &[
    "Link",
    "Inlet",
    "Node",
    "Number",
    "PctClogged",
    "Qmax",
    "aLocal",
    "wLocal",
    "Placement",
];
const POLLUTANTS: &[&str] = &[
    "Name",
    "Units",
    "Crain",
    "Cgw",
    "Crdii",
    "Kdecay",
    "SnowOnly",
    "CoPollutant",
    "CoFrac",
    "Cdwf",
    "Cinit",
];
const LANDUSES: &[&str] = &["Name", "SweepInterval", "Availability", "LastSweep"];
const AQUIFERS: &[&str] = &[
    "Name", "Por", "WP", "FC", "Ksat", "Kslope", "Tslope", "ETu", "ETs", "Seep", "Ebot", "Egw",
    "Umc", "ETupat",
];
const GWF: &[&str] = &["Subcatchment", "Type", "Expression"];
const SNOWPACKS_PLOWABLE: &[&str] = &[
    "Name", "Layer", "Cmin", "Cmax", "Tbase", "FWF", "SD0", "FW0", "SNN0",
];
const SNOWPACKS_AREA: &[&str] = &[
    "Name", "Layer", "Cmin", "Cmax", "Tbase", "FWF", "SD0", "FW0", "SD100",
];
const SNOWPACKS_REMOVAL: &[&str] = &[
    "Name", "Layer", "Dplow", "Fout", "Fimp", "Fperv", "Fimelt", "Fsub", "Scatch",
];
const SNOWPACKS_OTHER: &[&str] = &["Name", "Layer"];
const HYDROGRAPHS_GAGE: &[&str] = &["Name", "RainGage"];
const HYDROGRAPHS_PARAMS: &[&str] = &[
    "Name", "Month", "Response", "R", "T", "K", "Dmax", "Drec", "D0",
];
const LID_TYPE_ROW: &[&str] = &["Name", "Type"];
const LID_SURFACE: &[&str] = &[
    "Name", "Layer", "StorHt", "VegFrac", "Rough", "Slope", "Xslope",
];
const LID_SOIL: &[&str] = &[
    "Name", "Layer", "Thick", "Por", "FC", "WP", "Ksat", "Kslope", "Suct",
];
const LID_PAVEMENT: &[&str] = &[
    "Name", "Layer", "Thick", "Vratio", "FracImp", "Perm", "Vclog", "Treg", "Freg",
];
const LID_STORAGE: &[&str] = &[
    "Name", "Layer", "Height", "Vratio", "Seepage", "Vclog", "Covrd",
];
const LID_DRAIN: &[&str] = &[
    "Name", "Layer", "Coeff", "Expon", "Offset", "Delay", "Hopen", "Hclose", "Qcurve",
];
const LID_DRAINMAT: &[&str] = &["Name", "Layer", "Thick", "Vratio", "Rough"];
const LID_REMOVALS: &[&str] = &["Name", "Layer", "Pollutant", "Removal"];
const BUILDUP: &[&str] = &[
    "LandUse", "Pollutant", "Function", "Coeff1", "Coeff2", "Coeff3", "PerUnit",
];
const WASHOFF: &[&str] = &[
    "LandUse", "Pollutant", "Function", "Coeff1", "Coeff2", "SweepRmvl", "BmpRmvl",
];
const EMPTY: &[&str] = &[];

/// The `[LID_CONTROLS]` process types (lid.c `LidTypeWords`): bio-retention
/// cell, rain garden, green roof, infiltration trench, permeable pavement,
/// rain barrel, vegetative swale, rooftop disconnection.
pub const LID_TYPES: &[&str] = &["BC", "RG", "GR", "IT", "PP", "RB", "VS", "RD"];

/// The `[LID_CONTROLS]` layer keywords (lid.c `LidLayerWords`).
pub const LID_LAYERS: &[&str] = &[
    "SURFACE", "SOIL", "PAVEMENT", "STORAGE", "DRAIN", "DRAINMAT", "REMOVALS",
];

/// The `[SNOWPACKS]` row keywords (snow.c `SnowmeltWords`).
pub const SNOWPACK_LAYERS: &[&str] = &["PLOWABLE", "IMPERVIOUS", "PERVIOUS", "REMOVAL"];

/// `[BUILDUP]` function keywords (landuse.c `BuildupTypeWords`).
pub const BUILDUP_FUNCTIONS: &[&str] = &["NONE", "POW", "EXP", "SAT", "EXT"];

/// `[WASHOFF]` function keywords (landuse.c `WashoffTypeWords`).
pub const WASHOFF_FUNCTIONS: &[&str] = &["NONE", "EXP", "RC", "EMC"];

/// `[BUILDUP]` normalizer keywords (landuse.c `NormalizerWords`).
pub const BUILDUP_NORMALIZERS: &[&str] = &["AREA", "CURB"];

/// `[HYDROGRAPHS]` response keywords (rdii.c `UHTypeWords`).
pub const UH_RESPONSES: &[&str] = &["SHORT", "MEDIUM", "LONG"];

/// `[HYDROGRAPHS]` month keywords: `ALL` or a three-letter month.
pub const UH_MONTHS: &[&str] = &[
    "ALL", "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// `[GWF]` flow types.
pub const GWF_TYPES: &[&str] = &["LATERAL", "DEEP"];

/// Infiltration method keywords accepted by `[OPTIONS] INFILTRATION` and, in
/// SWMM 5.2, as a trailing per-row override in `[INFILTRATION]`.
pub const INFILTRATION_METHODS: &[&str] = &[
    "HORTON",
    "MODIFIED_HORTON",
    "GREEN_AMPT",
    "MODIFIED_GREEN_AMPT",
    "CURVE_NUMBER",
];

fn upper(s: &str) -> String {
    s.trim_matches('"').to_ascii_uppercase()
}

fn is_number(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

/// The column names for one row of `section`. `infiltration` is the
/// `[OPTIONS] INFILTRATION` value (any case), used only for `[INFILTRATION]`.
/// Sections without a table return an empty slice; their fields are still
/// addressable by index.
pub fn columns(
    section: &str,
    fields: &[String],
    infiltration: Option<&str>,
) -> &'static [&'static str] {
    let f = |i: usize| fields.get(i).map(|s| upper(s)).unwrap_or_default();
    match section {
        "RAINGAGES" => {
            if f(4) == "FILE" {
                RAINGAGES_FILE
            } else {
                RAINGAGES_SERIES
            }
        }
        "SUBCATCHMENTS" => SUBCATCHMENTS,
        "SUBAREAS" => SUBAREAS,
        "INFILTRATION" => {
            // A trailing method keyword on the row wins over the global option.
            let last = fields.last().map(|s| upper(s)).unwrap_or_default();
            let method = if INFILTRATION_METHODS.contains(&last.as_str()) {
                last
            } else {
                infiltration
                    .map(upper)
                    .unwrap_or_else(|| "HORTON".to_string())
            };
            match method.as_str() {
                "GREEN_AMPT" | "MODIFIED_GREEN_AMPT" => INFIL_GREEN_AMPT,
                "CURVE_NUMBER" => INFIL_CURVE_NUMBER,
                _ => INFIL_HORTON,
            }
        }
        "JUNCTIONS" => JUNCTIONS,
        "OUTFALLS" => match f(2).as_str() {
            "FIXED" => OUTFALLS_FIXED,
            "TIDAL" => OUTFALLS_TIDAL,
            "TIMESERIES" => OUTFALLS_SERIES,
            _ => OUTFALLS_FREE,
        },
        "STORAGE" => match f(4).as_str() {
            "FUNCTIONAL" => STORAGE_FUNCTIONAL,
            "CYLINDRICAL" | "CONICAL" | "PARABOLOID" | "PYRAMIDAL" => STORAGE_GEOMETRIC,
            _ => STORAGE_TABULAR,
        },
        "DIVIDERS" => match f(3).as_str() {
            "CUTOFF" => DIVIDERS_CUTOFF,
            "TABULAR" => DIVIDERS_TABULAR,
            "WEIR" => DIVIDERS_WEIR,
            _ => DIVIDERS_OVERFLOW,
        },
        "CONDUITS" => CONDUITS,
        "PUMPS" => PUMPS,
        "ORIFICES" => ORIFICES,
        "WEIRS" => WEIRS,
        "OUTLETS" => {
            if f(4).starts_with("FUNCTIONAL") {
                OUTLETS_FUNCTIONAL
            } else {
                OUTLETS_TABULAR
            }
        }
        "XSECTIONS" => match f(1).as_str() {
            "CUSTOM" => XSECTIONS_CUSTOM,
            "IRREGULAR" => XSECTIONS_IRREGULAR,
            "STREET" => XSECTIONS_STREET,
            _ => XSECTIONS,
        },
        "LOSSES" => LOSSES,
        "INFLOWS" => INFLOWS,
        "DWF" => DWF,
        "RDII" => RDII,
        "TREATMENT" => TREATMENT,
        "TIMESERIES" => {
            if f(1) == "FILE" {
                TIMESERIES_FILE
            } else if fields.len() >= 4
                && fields
                    .get(1)
                    .is_some_and(|d| d.contains('/') || d.contains('-'))
            {
                TIMESERIES_DATED
            } else {
                TIMESERIES_PLAIN
            }
        }
        "CURVES" => {
            if fields.len() >= 4 && !fields.get(1).is_some_and(|s| is_number(s)) {
                CURVES_TYPED
            } else {
                CURVES_PLAIN
            }
        }
        "PATTERNS" => {
            if fields.len() >= 2 && !fields.get(1).is_some_and(|s| is_number(s)) {
                PATTERNS_TYPED
            } else {
                PATTERNS_PLAIN
            }
        }
        "COORDINATES" => COORDINATES,
        "VERTICES" => VERTICES,
        "POLYGONS" => POLYGONS,
        "SYMBOLS" => SYMBOLS,
        "LABELS" => LABELS,
        "TAGS" => TAGS,
        "MAP" => {
            if f(0) == "DIMENSIONS" {
                MAP_DIMENSIONS
            } else {
                KEY_VALUE
            }
        }
        "OPTIONS" | "REPORT" | "BACKDROP" | "EVAPORATION" | "TEMPERATURE" | "ADJUSTMENTS" => {
            KEY_VALUE
        }
        "GROUNDWATER" => GROUNDWATER,
        "LID_USAGE" => LID_USAGE,
        "COVERAGES" => COVERAGES,
        "LOADINGS" => LOADINGS,
        "INLET_USAGE" => INLET_USAGE,
        "POLLUTANTS" => POLLUTANTS,
        "LANDUSES" => LANDUSES,
        "AQUIFERS" => AQUIFERS,
        "GWF" => GWF,
        "SNOWPACKS" => match f(1).as_str() {
            "PLOWABLE" => SNOWPACKS_PLOWABLE,
            "IMPERVIOUS" | "PERVIOUS" => SNOWPACKS_AREA,
            "REMOVAL" => SNOWPACKS_REMOVAL,
            _ => SNOWPACKS_OTHER,
        },
        "HYDROGRAPHS" => {
            if fields.len() <= 2 {
                HYDROGRAPHS_GAGE
            } else {
                HYDROGRAPHS_PARAMS
            }
        }
        "LID_CONTROLS" => match f(1).as_str() {
            "SURFACE" => LID_SURFACE,
            "SOIL" => LID_SOIL,
            "PAVEMENT" => LID_PAVEMENT,
            "STORAGE" => LID_STORAGE,
            "DRAIN" => LID_DRAIN,
            "DRAINMAT" => LID_DRAINMAT,
            "REMOVALS" => LID_REMOVALS,
            _ => LID_TYPE_ROW,
        },
        "BUILDUP" => BUILDUP,
        "WASHOFF" => WASHOFF,
        _ => EMPTY,
    }
}

/// The fewest fields a well-formed row of `section` can have, for the light
/// validation pass. `None` means no opinion.
pub fn min_fields(section: &str) -> Option<usize> {
    Some(match section {
        "RAINGAGES" => 6,
        "SUBCATCHMENTS" => 8,
        "SUBAREAS" => 7,
        "INFILTRATION" => 2,
        "JUNCTIONS" => 2,
        "OUTFALLS" => 3,
        "STORAGE" => 6,
        "DIVIDERS" => 4,
        "CONDUITS" => 5,
        "PUMPS" => 4,
        "ORIFICES" => 6,
        "WEIRS" => 6,
        "OUTLETS" => 6,
        "XSECTIONS" => 3,
        "LOSSES" => 4,
        "INFLOWS" => 3,
        "TIMESERIES" | "CURVES" | "COORDINATES" | "VERTICES" | "POLYGONS" | "SYMBOLS" | "TAGS" => 3,
        // gwater.c: 13 values before the optional pattern; 11 before the
        // optional elevations.
        "AQUIFERS" => 13,
        "GROUNDWATER" => 11,
        // lid.c: a type row is 2 fields, every layer row at least 5.
        "LID_CONTROLS" => 2,
        "LID_USAGE" => 8,
        "RDII" | "TREATMENT" | "GWF" | "COVERAGES" | "LOADINGS" | "BUILDUP" | "WASHOFF" => 3,
        "HYDROGRAPHS" => 2,
        "SNOWPACKS" => 8,
        _ => return None,
    })
}

/// A column that names another object: `(section, column, kind)`.
///
/// `[TAGS]` and `[CONTROLS]` are handled in code, not here — the kind of a
/// tag row depends on its first field, and control rules are free text.
pub const REFERENCES: &[(&str, &str, ObjectKind)] = &[
    ("CONDUITS", "FromNode", ObjectKind::Node),
    ("CONDUITS", "ToNode", ObjectKind::Node),
    ("PUMPS", "FromNode", ObjectKind::Node),
    ("PUMPS", "ToNode", ObjectKind::Node),
    ("ORIFICES", "FromNode", ObjectKind::Node),
    ("ORIFICES", "ToNode", ObjectKind::Node),
    ("WEIRS", "FromNode", ObjectKind::Node),
    ("WEIRS", "ToNode", ObjectKind::Node),
    ("OUTLETS", "FromNode", ObjectKind::Node),
    ("OUTLETS", "ToNode", ObjectKind::Node),
    ("SUBCATCHMENTS", "Outlet", ObjectKind::Node),
    ("SUBCATCHMENTS", "Outlet", ObjectKind::Subcatchment),
    ("SUBCATCHMENTS", "RainGage", ObjectKind::Gage),
    ("INFLOWS", "Node", ObjectKind::Node),
    ("INFLOWS", "Series", ObjectKind::Timeseries),
    ("INFLOWS", "Pattern", ObjectKind::Pattern),
    ("DWF", "Node", ObjectKind::Node),
    ("DWF", "Pattern1", ObjectKind::Pattern),
    ("DWF", "Pattern2", ObjectKind::Pattern),
    ("DWF", "Pattern3", ObjectKind::Pattern),
    ("DWF", "Pattern4", ObjectKind::Pattern),
    ("RDII", "Node", ObjectKind::Node),
    ("TREATMENT", "Node", ObjectKind::Node),
    ("COORDINATES", "Node", ObjectKind::Node),
    ("GROUNDWATER", "Node", ObjectKind::Node),
    ("GROUNDWATER", "Subcatchment", ObjectKind::Subcatchment),
    ("INLET_USAGE", "Node", ObjectKind::Node),
    ("INLET_USAGE", "Link", ObjectKind::Link),
    ("LABELS", "Anchor", ObjectKind::Node),
    ("XSECTIONS", "Link", ObjectKind::Link),
    ("XSECTIONS", "Curve", ObjectKind::Curve),
    ("LOSSES", "Link", ObjectKind::Link),
    ("VERTICES", "Link", ObjectKind::Link),
    ("DIVIDERS", "DivertLink", ObjectKind::Link),
    ("DIVIDERS", "Curve", ObjectKind::Curve),
    ("SUBAREAS", "Subcatchment", ObjectKind::Subcatchment),
    ("INFILTRATION", "Subcatchment", ObjectKind::Subcatchment),
    ("LID_USAGE", "Subcatchment", ObjectKind::Subcatchment),
    ("COVERAGES", "Subcatchment", ObjectKind::Subcatchment),
    ("LOADINGS", "Subcatchment", ObjectKind::Subcatchment),
    ("POLYGONS", "Subcatchment", ObjectKind::Subcatchment),
    ("OUTFALLS", "RouteTo", ObjectKind::Subcatchment),
    ("OUTFALLS", "Curve", ObjectKind::Curve),
    ("OUTFALLS", "Series", ObjectKind::Timeseries),
    ("SYMBOLS", "Gage", ObjectKind::Gage),
    ("RAINGAGES", "Series", ObjectKind::Timeseries),
    ("STORAGE", "Curve", ObjectKind::Curve),
    ("PUMPS", "Curve", ObjectKind::Curve),
    ("OUTLETS", "Curve", ObjectKind::Curve),
    ("WEIRS", "CoeffCurve", ObjectKind::Curve),
    ("HYDROGRAPHS", "RainGage", ObjectKind::Gage),
    ("AQUIFERS", "ETupat", ObjectKind::Pattern),
    ("GWF", "Subcatchment", ObjectKind::Subcatchment),
    ("SNOWPACKS", "Scatch", ObjectKind::Subcatchment),
    ("LID_CONTROLS", "Qcurve", ObjectKind::Curve),
];

/// Index of the named column in a row's layout, case-insensitively.
pub fn field_index(columns: &[&str], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.eq_ignore_ascii_case(name))
}
