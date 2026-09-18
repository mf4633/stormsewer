// SPDX-License-Identifier: GPL-3.0-or-later

//! What the map editor needs from the document beyond raw commands: unique
//! default names, complete default objects, copy/paste with renaming, and
//! the small structural edits (reverse a link, convert a node) that are
//! several rows at once.
//!
//! Everything here builds a [`Command`] and returns it; nothing applies.
//! The editor applies the command inside a gesture, so each function is
//! exactly one undo step however many rows it touches.
//!
//! Defaults follow the EPA SWMM GUI's own defaults for a new object: a
//! junction at invert 0 with a 0 maximum depth (the engine then uses the
//! highest connecting conduit crown), a circular 1-unit conduit with
//! Manning's n 0.01, a 5-unit 25%-impervious subcatchment 500 wide at 0.5%
//! slope, and Horton / Green-Ampt / Curve Number infiltration rows chosen
//! by the model's `[OPTIONS] INFILTRATION`.

use super::schema::{self, LINK_SECTIONS};
use super::{format_number, unquote, Command, InpDoc, ObjectKind, Row};

/// The kinds of node the editor can place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeType {
    Junction,
    Outfall,
    Divider,
    Storage,
}

impl NodeType {
    pub const ALL: [NodeType; 4] = [
        NodeType::Junction,
        NodeType::Outfall,
        NodeType::Divider,
        NodeType::Storage,
    ];

    pub fn section(self) -> &'static str {
        match self {
            Self::Junction => "JUNCTIONS",
            Self::Outfall => "OUTFALLS",
            Self::Divider => "DIVIDERS",
            Self::Storage => "STORAGE",
        }
    }

    /// Default-name prefix, as the EPA GUI uses.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Junction => "J",
            Self::Outfall => "O",
            Self::Divider => "D",
            Self::Storage => "ST",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Junction => "Junction",
            Self::Outfall => "Outfall",
            Self::Divider => "Divider",
            Self::Storage => "Storage Unit",
        }
    }

    pub fn from_section(section: &str) -> Option<Self> {
        match section.to_ascii_uppercase().as_str() {
            "JUNCTIONS" => Some(Self::Junction),
            "OUTFALLS" => Some(Self::Outfall),
            "DIVIDERS" => Some(Self::Divider),
            "STORAGE" => Some(Self::Storage),
            _ => None,
        }
    }

    /// The defining row for a node of this type at `elevation`, after the
    /// name. Every other column takes the GUI default.
    fn default_fields(self, elevation: f64) -> Vec<String> {
        let e = format_number(elevation);
        let s = |v: &str| v.to_string();
        match self {
            // Elevation MaxDepth InitDepth SurDepth Aponded
            Self::Junction => vec![e, s("0"), s("0"), s("0"), s("0")],
            // Elevation Type Gated
            Self::Outfall => vec![e, s("FREE"), s("NO")],
            // Elevation DivertLink Type MaxDepth InitDepth SurDepth Aponded
            Self::Divider => vec![e, s("*"), s("OVERFLOW"), s("0"), s("0"), s("0"), s("0")],
            // Elevation MaxDepth InitDepth FUNCTIONAL Coeff Exponent Constant SurDepth Fevap
            Self::Storage => vec![
                e,
                s("0"),
                s("0"),
                s("FUNCTIONAL"),
                s("1000"),
                s("0"),
                s("0"),
                s("0"),
                s("0"),
            ],
        }
    }
}

/// The kinds of link the editor can draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkType {
    Conduit,
    Pump,
    Orifice,
    Weir,
    Outlet,
}

impl LinkType {
    pub const ALL: [LinkType; 5] = [
        LinkType::Conduit,
        LinkType::Pump,
        LinkType::Orifice,
        LinkType::Weir,
        LinkType::Outlet,
    ];

    pub fn section(self) -> &'static str {
        match self {
            Self::Conduit => "CONDUITS",
            Self::Pump => "PUMPS",
            Self::Orifice => "ORIFICES",
            Self::Weir => "WEIRS",
            Self::Outlet => "OUTLETS",
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Conduit => "C",
            Self::Pump => "P",
            Self::Orifice => "OR",
            Self::Weir => "W",
            Self::Outlet => "OL",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Conduit => "Conduit",
            Self::Pump => "Pump",
            Self::Orifice => "Orifice",
            Self::Weir => "Weir",
            Self::Outlet => "Outlet",
        }
    }

    pub fn from_section(section: &str) -> Option<Self> {
        match section.to_ascii_uppercase().as_str() {
            "CONDUITS" => Some(Self::Conduit),
            "PUMPS" => Some(Self::Pump),
            "ORIFICES" => Some(Self::Orifice),
            "WEIRS" => Some(Self::Weir),
            "OUTLETS" => Some(Self::Outlet),
            _ => None,
        }
    }

    /// The defining row after `Name FromNode ToNode`.
    fn default_fields(self, length: f64) -> Vec<String> {
        let s = |v: &str| v.to_string();
        match self {
            // Length Roughness InOffset OutOffset InitFlow MaxFlow
            Self::Conduit => vec![
                format_number(length.max(1.0)),
                s("0.01"),
                s("0"),
                s("0"),
                s("0"),
                s("0"),
            ],
            // Curve Status Startup Shutoff  ("*" is an ideal pump)
            Self::Pump => vec![s("*"), s("ON"), s("0"), s("0")],
            // Type Offset Qcoeff Gated CloseTime
            Self::Orifice => vec![s("SIDE"), s("0"), s("0.65"), s("NO"), s("0")],
            // Type CrestHt Qcoeff Gated EndCon EndCoeff
            Self::Weir => vec![s("TRANSVERSE"), s("0"), s("3.33"), s("NO"), s("0"), s("0")],
            // Offset Type Qcoeff Qexpon Gated
            Self::Outlet => vec![s("0"), s("FUNCTIONAL/DEPTH"), s("10"), s("0.5"), s("NO")],
        }
    }

    /// The `[XSECTIONS]` row a new link needs, if the type has one.
    fn xsection_fields(self) -> Option<Vec<String>> {
        let s = |v: &str| v.to_string();
        match self {
            // Shape Geom1 Geom2 Geom3 Geom4 Barrels
            Self::Conduit => Some(vec![s("CIRCULAR"), s("1"), s("0"), s("0"), s("0"), s("1")]),
            Self::Orifice => Some(vec![s("CIRCULAR"), s("1"), s("0"), s("0"), s("0")]),
            // Geom1 is the crest height, Geom2 the crest length.
            Self::Weir => Some(vec![s("RECT_OPEN"), s("1"), s("1"), s("0"), s("0")]),
            Self::Pump | Self::Outlet => None,
        }
    }
}

/// A newly built object: the name it was given and the command that adds it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewObject {
    pub name: String,
    pub command: Command,
}

fn taken(doc: &InpDoc, kind: ObjectKind, name: &str, reserved: &[String]) -> bool {
    doc.defining_section(kind, name).is_some()
        || reserved.iter().any(|r| r.eq_ignore_ascii_case(name))
}

/// The first `prefix1`, `prefix2`, ... not defined for `kind` (matching
/// case-insensitively, as the engine does) and not in `reserved`.
pub fn unique_name(doc: &InpDoc, kind: ObjectKind, prefix: &str) -> String {
    unique_name_reserving(doc, kind, prefix, &[])
}

/// [`unique_name`] that also avoids `reserved` — names handed out earlier
/// in the same batch, before the document has them.
pub fn unique_name_reserving(
    doc: &InpDoc,
    kind: ObjectKind,
    prefix: &str,
    reserved: &[String],
) -> String {
    (1u64..)
        .map(|n| format!("{prefix}{n}"))
        .find(|c| !taken(doc, kind, c, reserved))
        .expect("an unused name exists")
}

/// A name based on `base` that is free: `base` itself if unused, else
/// `base` with its trailing number (or a new one) counted up: `J3` becomes
/// `J4`, `Pond` becomes `Pond1`.
pub fn unique_name_from(doc: &InpDoc, kind: ObjectKind, base: &str, reserved: &[String]) -> String {
    let base = unquote(base);
    if !taken(doc, kind, base, reserved) {
        return base.to_string();
    }
    let digits = base.chars().rev().take_while(char::is_ascii_digit).count();
    let (stem, num) = base.split_at(base.len() - digits);
    let start: u64 = num.parse().unwrap_or(0) + 1;
    (start..)
        .map(|n| format!("{stem}{n}"))
        .find(|c| !taken(doc, kind, c, reserved))
        .expect("an unused name exists")
}

/// A node of `kind` at `(x, y)` with its `[COORDINATES]` row.
pub fn new_node(doc: &InpDoc, kind: NodeType, x: f64, y: f64) -> NewObject {
    let name = unique_name(doc, ObjectKind::Node, kind.prefix());
    let mut fields = vec![name.clone()];
    fields.extend(kind.default_fields(0.0));
    NewObject {
        command: Command::Batch(vec![
            Command::AddRow {
                section: kind.section().into(),
                fields,
                comment: None,
            },
            Command::MoveNode {
                name: name.clone(),
                x,
                y,
            },
        ]),
        name,
    }
}

/// A rain gage at `(x, y)` with its `[SYMBOLS]` row. The series is the
/// model's first `[TIMESERIES]` name, or `*` when it has none.
pub fn new_gage(doc: &InpDoc, x: f64, y: f64) -> NewObject {
    let name = unique_name(doc, ObjectKind::Gage, "R");
    let series = doc
        .names("TIMESERIES")
        .first()
        .cloned()
        .unwrap_or_else(|| "*".to_string());
    NewObject {
        command: Command::Batch(vec![
            Command::AddRow {
                section: "RAINGAGES".into(),
                fields: vec![
                    name.clone(),
                    "INTENSITY".into(),
                    "1:00".into(),
                    "1.0".into(),
                    "TIMESERIES".into(),
                    series,
                ],
                comment: None,
            },
            Command::MoveGage {
                name: name.clone(),
                x,
                y,
            },
        ]),
        name,
    }
}

/// A link of `kind` from `from` to `to` with its `[XSECTIONS]` row where
/// the type takes one and its `[VERTICES]` when `vertices` is not empty.
/// `length` is the drawn length, used for conduits.
pub fn new_link(
    doc: &InpDoc,
    kind: LinkType,
    from: &str,
    to: &str,
    vertices: &[(f64, f64)],
    length: f64,
) -> NewObject {
    let name = unique_name(doc, ObjectKind::Link, kind.prefix());
    let mut fields = vec![name.clone(), from.to_string(), to.to_string()];
    fields.extend(kind.default_fields(length));
    let mut cmds = vec![Command::AddRow {
        section: kind.section().into(),
        fields,
        comment: None,
    }];
    if let Some(xs) = kind.xsection_fields() {
        let mut fields = vec![name.clone()];
        fields.extend(xs);
        cmds.push(Command::AddRow {
            section: "XSECTIONS".into(),
            fields,
            comment: None,
        });
    }
    if !vertices.is_empty() {
        cmds.push(Command::SetVertices {
            link: name.clone(),
            points: vertices.to_vec(),
        });
    }
    NewObject {
        command: Command::Batch(cmds),
        name,
    }
}

/// The `[INFILTRATION]` values for a new subcatchment under the model's
/// `[OPTIONS] INFILTRATION` method (Horton when unset).
pub fn infiltration_defaults(doc: &InpDoc) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let method = doc
        .option("INFILTRATION")
        .map(|m| m.to_ascii_uppercase())
        .unwrap_or_else(|| "HORTON".into());
    match method.as_str() {
        // Suction Ksat IMD
        "GREEN_AMPT" | "MODIFIED_GREEN_AMPT" => vec![s("3.5"), s("0.5"), s("0.26")],
        // CurveNum Conductivity DryTime
        "CURVE_NUMBER" => vec![s("80"), s("0.5"), s("7")],
        // MaxRate MinRate Decay DryTime MaxInfil
        _ => vec![s("3.0"), s("0.5"), s("4"), s("7"), s("0")],
    }
}

/// A subcatchment outlined by `polygon`, draining to `outlet` (a node or
/// subcatchment name; `*` for none yet), with `[SUBAREAS]`,
/// `[INFILTRATION]` and `[POLYGONS]` rows. The gage is the model's first
/// rain gage, or `*`.
pub fn new_subcatchment(doc: &InpDoc, polygon: &[(f64, f64)], outlet: &str) -> NewObject {
    let name = unique_name(doc, ObjectKind::Subcatchment, "S");
    let gage = doc
        .names("RAINGAGES")
        .first()
        .cloned()
        .unwrap_or_else(|| "*".to_string());
    let s = |v: &str| v.to_string();
    let mut infil = vec![name.clone()];
    infil.extend(infiltration_defaults(doc));
    let mut cmds = vec![
        Command::AddRow {
            section: "SUBCATCHMENTS".into(),
            // RainGage Outlet Area PctImperv Width PctSlope CurbLen
            fields: vec![
                name.clone(),
                gage,
                outlet.to_string(),
                s("5"),
                s("25"),
                s("500"),
                s("0.5"),
                s("0"),
            ],
            comment: None,
        },
        Command::AddRow {
            section: "SUBAREAS".into(),
            // NImperv NPerv SImperv SPerv PctZero RouteTo
            fields: vec![
                name.clone(),
                s("0.01"),
                s("0.1"),
                s("0.05"),
                s("0.05"),
                s("25"),
                s("OUTLET"),
            ],
            comment: None,
        },
        Command::AddRow {
            section: "INFILTRATION".into(),
            fields: infil,
            comment: None,
        },
    ];
    if !polygon.is_empty() {
        cmds.push(Command::SetPolygon {
            subcatchment: name.clone(),
            points: polygon.to_vec(),
        });
    }
    NewObject {
        command: Command::Batch(cmds),
        name,
    }
}

/// A `[LABELS]` row: `X Y "Text" "" "Arial" 10 0 0` (no anchor node).
pub fn new_label(x: f64, y: f64, text: &str) -> Command {
    Command::AddRow {
        section: "LABELS".into(),
        fields: vec![
            format_number(x),
            format_number(y),
            format!("\"{}\"", unquote(text)),
            "\"\"".into(),
            "\"Arial\"".into(),
            "10".into(),
            "0".into(),
            "0".into(),
        ],
        comment: None,
    }
}

/// Which node section defines `name`, as a type.
pub fn node_type_of(doc: &InpDoc, name: &str) -> Option<NodeType> {
    doc.defining_section(ObjectKind::Node, name)
        .and_then(NodeType::from_section)
}

/// Which link section defines `name`, as a type.
pub fn link_type_of(doc: &InpDoc, name: &str) -> Option<LinkType> {
    doc.defining_section(ObjectKind::Link, name)
        .and_then(LinkType::from_section)
}

/// The link's `FromNode` and `ToNode`.
pub fn link_ends(doc: &InpDoc, name: &str) -> Option<(String, String)> {
    let sec = doc.defining_section(ObjectKind::Link, name)?;
    let (_, row) = doc.find(sec, name)?;
    Some((row.value(1)?.to_string(), row.value(2)?.to_string()))
}

/// Swap a link's endpoints and reverse its vertices, so the drawn line is
/// unchanged and the flow direction flips.
pub fn reverse_link(doc: &InpDoc, name: &str) -> Option<Command> {
    let sec = doc.defining_section(ObjectKind::Link, name)?;
    let (_, row) = doc.find(sec, name)?;
    let mut fields = row.fields.clone();
    if fields.len() < 3 {
        return None;
    }
    fields.swap(1, 2);
    let mut cmds = vec![Command::SetFields {
        section: sec.into(),
        name: name.into(),
        fields,
    }];
    let mut verts = doc.vertices(name);
    if verts.len() > 1 {
        verts.reverse();
        cmds.push(Command::SetVertices {
            link: name.into(),
            points: verts,
        });
    }
    Some(Command::Batch(cmds))
}

/// Move a node to another node section, keeping its name and elevation and
/// taking the new type's defaults for everything else. Links, coordinates
/// and tags address the node by name, so they follow without change.
pub fn convert_node(doc: &InpDoc, name: &str, to: NodeType) -> Option<Command> {
    let sec = doc.defining_section(ObjectKind::Node, name)?;
    if sec.eq_ignore_ascii_case(to.section()) {
        return None;
    }
    let (_, row) = doc.find(sec, name)?;
    let elevation: f64 = row.value(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let spelled = row.fields[0].clone();
    let mut fields = vec![spelled];
    fields.extend(to.default_fields(elevation));
    Some(Command::Batch(vec![
        Command::DeleteObject {
            section: sec.into(),
            name: name.into(),
        },
        Command::AddRow {
            section: to.section().into(),
            fields,
            comment: None,
        },
    ]))
}

/// The batch that removes a rain gage with its `[SYMBOLS]` and `[TAGS]`
/// rows. Subcatchments that used it keep the name; validation flags them.
pub fn cascade_delete_gage(name: &str) -> Command {
    Command::Batch(vec![
        Command::DeleteObject {
            section: "RAINGAGES".into(),
            name: name.into(),
        },
        Command::DeleteObject {
            section: "SYMBOLS".into(),
            name: name.into(),
        },
        Command::DeleteTag {
            kind: "Gage".into(),
            name: name.into(),
        },
    ])
}

/// One object on the map, as the editor addresses it. Labels have no name
/// and are addressed by their `[LABELS]` line index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObjRef {
    Node(String),
    Link(String),
    Subcatchment(String),
    Gage(String),
    Label(usize),
}

impl ObjRef {
    pub fn kind(&self) -> Option<ObjectKind> {
        match self {
            Self::Node(_) => Some(ObjectKind::Node),
            Self::Link(_) => Some(ObjectKind::Link),
            Self::Subcatchment(_) => Some(ObjectKind::Subcatchment),
            Self::Gage(_) => Some(ObjectKind::Gage),
            Self::Label(_) => None,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Node(n) | Self::Link(n) | Self::Subcatchment(n) | Self::Gage(n) => Some(n),
            Self::Label(_) => None,
        }
    }

    pub fn is(&self, kind: ObjectKind, name: &str) -> bool {
        self.kind() == Some(kind) && self.name().is_some_and(|n| n.eq_ignore_ascii_case(name))
    }
}

/// What deleting `items` takes with it beyond the items themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteImpact {
    /// Links attached to a deleted node that were not selected.
    pub links: Vec<String>,
    /// Subcatchments whose outlet is a deleted node (they stay, with a
    /// dangling outlet the validator reports).
    pub orphaned_subcatchments: Vec<String>,
}

impl DeleteImpact {
    pub fn is_empty(&self) -> bool {
        self.links.is_empty() && self.orphaned_subcatchments.is_empty()
    }
}

fn selected(items: &[ObjRef], kind: ObjectKind, name: &str) -> bool {
    items.iter().any(|i| i.is(kind, name))
}

pub fn delete_impact(doc: &InpDoc, items: &[ObjRef]) -> DeleteImpact {
    let mut out = DeleteImpact::default();
    for item in items {
        let ObjRef::Node(node) = item else { continue };
        for sec in LINK_SECTIONS {
            for (_, r) in doc.rows(sec) {
                let attached = [1, 2]
                    .iter()
                    .any(|&i| r.value(i).is_some_and(|f| f.eq_ignore_ascii_case(node)));
                if let (true, Some(link)) = (attached, r.value(0)) {
                    if !selected(items, ObjectKind::Link, link)
                        && !out.links.iter().any(|l| l.eq_ignore_ascii_case(link))
                    {
                        out.links.push(link.to_string());
                    }
                }
            }
        }
        for (_, r) in doc.rows("SUBCATCHMENTS") {
            if let (Some(sub), Some(outlet)) = (r.value(0), r.value(2)) {
                if outlet.eq_ignore_ascii_case(node)
                    && !selected(items, ObjectKind::Subcatchment, sub)
                {
                    out.orphaned_subcatchments.push(sub.to_string());
                }
            }
        }
    }
    out
}

/// The batch that deletes every item with its dependent rows, via the
/// document's cascade builders. Labels are deleted last, highest line
/// first, so earlier indices stay valid.
pub fn cascade_delete(doc: &InpDoc, items: &[ObjRef]) -> Command {
    let mut cmds = Vec::new();
    let mut labels: Vec<usize> = Vec::new();
    for item in items {
        match item {
            ObjRef::Node(n) => cmds.push(doc.cascade_delete_node(n)),
            ObjRef::Link(n) => cmds.push(doc.cascade_delete_link(n)),
            ObjRef::Subcatchment(n) => cmds.push(doc.cascade_delete_subcatchment(n)),
            ObjRef::Gage(n) => cmds.push(cascade_delete_gage(n)),
            ObjRef::Label(li) => labels.push(*li),
        }
    }
    labels.sort_unstable();
    labels.dedup();
    for li in labels.into_iter().rev() {
        cmds.push(Command::DeleteLine {
            section: "LABELS".into(),
            line: li,
        });
    }
    Command::Batch(cmds)
}

/// The rows that make up one copied object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipObject {
    pub kind: ObjectKind,
    pub name: String,
    /// `(section, fields)` in file order.
    pub rows: Vec<(String, Vec<String>)>,
}

/// A copied selection. Rows are held as fields, so a paste into another
/// document (or the same one after edits) re-renders them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clipboard {
    pub objects: Vec<ClipObject>,
    pub labels: Vec<Vec<String>>,
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.labels.is_empty()
    }

    pub fn len(&self) -> usize {
        self.objects.len() + self.labels.len()
    }
}

/// The sections that carry rows for an object of `kind`, after its defining
/// sections. `[TAGS]` is handled by keyword.
fn dependent_sections(kind: ObjectKind) -> &'static [&'static str] {
    match kind {
        ObjectKind::Node => &["COORDINATES", "INFLOWS", "DWF", "RDII", "TREATMENT"],
        ObjectKind::Link => &["XSECTIONS", "LOSSES", "VERTICES", "INLET_USAGE"],
        ObjectKind::Subcatchment => &[
            "SUBAREAS",
            "INFILTRATION",
            "GROUNDWATER",
            "LID_USAGE",
            "COVERAGES",
            "LOADINGS",
            "POLYGONS",
        ],
        ObjectKind::Gage => &["SYMBOLS"],
        _ => &[],
    }
}

fn rows_named<'a>(doc: &'a InpDoc, section: &str, name: &str) -> Vec<&'a Row> {
    doc.find_all(section, name)
}

/// Copy `items` out of the document. Objects come out in the order given;
/// nodes are pasted before links so endpoints exist first.
pub fn copy(doc: &InpDoc, items: &[ObjRef]) -> Clipboard {
    let mut clip = Clipboard::default();
    for item in items {
        let (Some(kind), Some(name)) = (item.kind(), item.name()) else {
            if let ObjRef::Label(li) = item {
                if let Some(row) = doc
                    .section("LABELS")
                    .and_then(|s| s.lines.get(*li))
                    .and_then(|l| l.row.as_ref())
                {
                    clip.labels.push(row.fields.clone());
                }
            }
            continue;
        };
        let mut rows = Vec::new();
        for sec in kind
            .defining_sections()
            .iter()
            .chain(dependent_sections(kind).iter())
        {
            for r in rows_named(doc, sec, name) {
                rows.push((sec.to_string(), r.fields.clone()));
            }
        }
        if let Some(kw) = kind.tag_keyword() {
            for (_, r) in doc.rows("TAGS") {
                if r.fields.first().is_some_and(|k| k.eq_ignore_ascii_case(kw))
                    && r.fields
                        .get(1)
                        .is_some_and(|n| unquote(n).eq_ignore_ascii_case(name))
                {
                    rows.push(("TAGS".to_string(), r.fields.clone()));
                }
            }
        }
        clip.objects.push(ClipObject {
            kind,
            name: name.to_string(),
            rows,
        });
    }
    clip
}

fn shift_number(field: &str, delta: f64) -> String {
    match field.parse::<f64>() {
        Ok(v) => format_number(v + delta),
        Err(_) => field.to_string(),
    }
}

/// Paste a clipboard with every object renamed to a free name, references
/// between pasted objects remapped, and every position offset by
/// `(dx, dy)`. Returns the pasted objects (by their new names) and the
/// command that adds them.
pub fn paste(doc: &InpDoc, clip: &Clipboard, dx: f64, dy: f64) -> (Vec<ObjRef>, Command) {
    let mut reserved: Vec<String> = Vec::new();
    // (kind, old, new) — the map every reference is passed through.
    let mut renames: Vec<(ObjectKind, String, String)> = Vec::new();
    for obj in &clip.objects {
        let new = unique_name_from(doc, obj.kind, &obj.name, &reserved);
        reserved.push(new.clone());
        renames.push((obj.kind, obj.name.clone(), new));
    }
    let mapped = |kind: ObjectKind, name: &str| -> String {
        renames
            .iter()
            .find(|(k, old, _)| *k == kind && unquote(old).eq_ignore_ascii_case(unquote(name)))
            .map(|(_, _, new)| {
                if name.starts_with('"') {
                    format!("\"{new}\"")
                } else {
                    new.clone()
                }
            })
            .unwrap_or_else(|| name.to_string())
    };

    let mut cmds = Vec::new();
    let mut out = Vec::new();
    let infil = doc.option("INFILTRATION").map(|s| s.to_string());
    // Nodes and gages first, then subcatchments, then links.
    let order = |k: ObjectKind| match k {
        ObjectKind::Node | ObjectKind::Gage => 0,
        ObjectKind::Subcatchment => 1,
        _ => 2,
    };
    let mut objects: Vec<&ClipObject> = clip.objects.iter().collect();
    objects.sort_by_key(|o| order(o.kind));
    for obj in objects {
        let new_name = mapped(obj.kind, &obj.name);
        for (section, fields) in &obj.rows {
            let mut fields = fields.clone();
            let upper = section.to_ascii_uppercase();
            let cols = schema::columns(&upper, &fields, infil.as_deref());
            // The object's own name.
            if let Some(ni) = schema::name_index(&upper) {
                if let Some(f) = fields.get_mut(ni) {
                    *f = mapped(obj.kind, f);
                }
            }
            // References to other pasted objects.
            for (sec, col, kind) in schema::REFERENCES {
                if *sec != upper {
                    continue;
                }
                if let Some(i) = schema::field_index(cols, col) {
                    if let Some(f) = fields.get_mut(i) {
                        *f = mapped(*kind, f);
                    }
                }
            }
            // Positions.
            if matches!(
                upper.as_str(),
                "COORDINATES" | "VERTICES" | "POLYGONS" | "SYMBOLS"
            ) {
                if let Some(f) = fields.get_mut(1) {
                    *f = shift_number(f, dx);
                }
                if let Some(f) = fields.get_mut(2) {
                    *f = shift_number(f, dy);
                }
            }
            cmds.push(Command::AddRow {
                section: section.clone(),
                fields,
                comment: None,
            });
        }
        out.push(match obj.kind {
            ObjectKind::Node => ObjRef::Node(new_name),
            ObjectKind::Link => ObjRef::Link(new_name),
            ObjectKind::Subcatchment => ObjRef::Subcatchment(new_name),
            ObjectKind::Gage => ObjRef::Gage(new_name),
            _ => continue,
        });
    }
    for fields in &clip.labels {
        let mut fields = fields.clone();
        if let Some(f) = fields.get_mut(0) {
            *f = shift_number(f, dx);
        }
        if let Some(f) = fields.get_mut(1) {
            *f = shift_number(f, dy);
        }
        // Pasted labels lose their anchor: it may not be pasted with them.
        if let Some(f) = fields.get_mut(3) {
            *f = "\"\"".into();
        }
        cmds.push(Command::AddRow {
            section: "LABELS".into(),
            fields,
            comment: None,
        });
    }
    (out, Command::Batch(cmds))
}

/// The text of a new, empty, runnable model: the `[OPTIONS]` the EPA GUI
/// writes for a new project, in US units with Horton infiltration.
pub fn new_model_text() -> String {
    [
        "[TITLE]",
        ";;Project Title/Notes",
        "",
        "[OPTIONS]",
        ";;Option             Value",
        "FLOW_UNITS           CFS",
        "INFILTRATION         HORTON",
        "FLOW_ROUTING         KINWAVE",
        "LINK_OFFSETS         DEPTH",
        "MIN_SLOPE            0",
        "ALLOW_PONDING        NO",
        "SKIP_STEADY_STATE    NO",
        "",
        "START_DATE           01/01/2026",
        "START_TIME           00:00:00",
        "REPORT_START_DATE    01/01/2026",
        "REPORT_START_TIME    00:00:00",
        "END_DATE             01/01/2026",
        "END_TIME             06:00:00",
        "SWEEP_START          01/01",
        "SWEEP_END            12/31",
        "DRY_DAYS             0",
        "REPORT_STEP          00:15:00",
        "WET_STEP             00:05:00",
        "DRY_STEP             01:00:00",
        "ROUTING_STEP         0:00:30",
        "RULE_STEP            00:00:00",
        "",
        "INERTIAL_DAMPING     PARTIAL",
        "NORMAL_FLOW_LIMITED  BOTH",
        "FORCE_MAIN_EQUATION  H-W",
        "VARIABLE_STEP        0.75",
        "LENGTHENING_STEP     0",
        "MIN_SURFAREA         12.557",
        "MAX_TRIALS           8",
        "HEAD_TOLERANCE       0.005",
        "SYS_FLOW_TOL         5",
        "LAT_FLOW_TOL         5",
        "MINIMUM_STEP         0.5",
        "THREADS              1",
        "",
        "[EVAPORATION]",
        ";;Data Source    Parameters",
        ";;-------------- ----------------",
        "CONSTANT         0.0",
        "DRY_ONLY         NO",
        "",
        "[REPORT]",
        ";;Reporting Options",
        "SUBCATCHMENTS ALL",
        "NODES ALL",
        "LINKS ALL",
        "",
        "[MAP]",
        "DIMENSIONS 0.000 0.000 10000.000 10000.000",
        "Units      None",
        "",
    ]
    .join("\r\n")
}

// --- row-set replacement and the hydrology / quality sections ------------------
//
// The dialogs for `[LID_CONTROLS]`, `[LID_USAGE]`, `[AQUIFERS]`,
// `[GROUNDWATER]`, `[GWF]`, `[SNOWPACKS]`, `[BUILDUP]`, `[WASHOFF]`,
// `[COVERAGES]`, `[LOADINGS]`, `[TREATMENT]`, `[HYDROGRAPHS]` and `[RDII]`
// edit a draft of every row that belongs to one name and write the draft
// back with [`replace_rows`]. Column meanings come from the SWMM 5.2 User's
// Manual, Appendix D; the parsing rules (how many fields a row needs, which
// trailing fields are optional) from the engine's readers in `lid.c`,
// `gwater.c`, `snow.c`, `landuse.c`, `treatmnt.c` and `rdii.c`.

/// A row as text: fields joined by two spaces, empty or blank-containing
/// fields quoted.
fn render_fields(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| {
            if f.is_empty() || f.chars().any(char::is_whitespace) {
                format!("\"{f}\"")
            } else {
                f.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Whether `field` names `name` (quotes ignored, case folded as the engine
/// folds it).
fn names(field: &str, name: &str) -> bool {
    unquote(field).eq_ignore_ascii_case(unquote(name))
}

/// The batch that makes the rows of `section` whose first field is `name`
/// read `rows` (each with the name in its first field). A row whose fields
/// already equal the draft's, quote for quote, is left alone, so a draft
/// written back unchanged is an empty batch and the file does not change.
/// Edited rows keep their line and trailing comment; extra rows go in after
/// the last existing one (or at the section's end when there is none);
/// surplus rows are deleted.
pub fn replace_rows(doc: &InpDoc, section: &str, name: &str, rows: &[Vec<String>]) -> Command {
    let sec = section.trim().to_ascii_uppercase();
    let existing: Vec<(usize, Row)> = doc
        .rows(&sec)
        .into_iter()
        .filter(|(_, r)| r.fields.first().is_some_and(|f| names(f, name)))
        .map(|(li, r)| (li, r.clone()))
        .collect();
    let mut cmds = Vec::new();
    let n = existing.len().min(rows.len());
    for (i, (li, row)) in existing.iter().enumerate().take(n) {
        if row.fields != rows[i] {
            cmds.push(Command::SetLine {
                section: sec.clone(),
                line: *li,
                fields: rows[i].clone(),
                comment: row.comment.clone(),
            });
        }
    }
    if rows.len() > existing.len() {
        match existing.last() {
            Some((last, _)) => {
                for (k, row) in rows[existing.len()..].iter().enumerate() {
                    cmds.push(Command::InsertText {
                        section: sec.clone(),
                        line: Some(last + 1 + k),
                        text: render_fields(row),
                    });
                }
            }
            None => {
                for row in &rows[existing.len()..] {
                    cmds.push(Command::AddRow {
                        section: sec.clone(),
                        fields: row.clone(),
                        comment: None,
                    });
                }
            }
        }
    }
    for (li, _) in existing.iter().skip(rows.len()).rev() {
        cmds.push(Command::DeleteLine {
            section: sec.clone(),
            line: *li,
        });
    }
    Command::Batch(cmds)
}

/// The rows of `section` whose first field is `name`, as fields (quotes
/// kept), in file order: the draft a dialog starts from.
pub fn rows_of(doc: &InpDoc, section: &str, name: &str) -> Vec<Vec<String>> {
    doc.rows(section)
        .into_iter()
        .filter(|(_, r)| r.fields.first().is_some_and(|f| names(f, name)))
        .map(|(_, r)| r.fields.clone())
        .collect()
}

/// The batch that renames an object the engine addresses only by name in
/// these sections (an LID process, aquifer, snow pack, unit hydrograph set
/// or land use): every field at column `idx` of `section` that names `old`
/// becomes `new`, quotes kept. `targets` lists `(section, column index)`;
/// the object's own defining section belongs in the list too.
pub fn rename_in_columns(doc: &InpDoc, targets: &[(&str, usize)], old: &str, new: &str) -> Command {
    let mut cmds = Vec::new();
    for (section, idx) in targets {
        let sec = section.to_ascii_uppercase();
        for (li, r) in doc.rows(&sec) {
            let Some(f) = r.fields.get(*idx) else { continue };
            if !names(f, old) {
                continue;
            }
            let mut fields = r.fields.clone();
            fields[*idx] = if f.starts_with('"') {
                format!("\"{new}\"")
            } else {
                new.to_string()
            };
            cmds.push(Command::SetLine {
                section: sec.clone(),
                line: li,
                fields,
                comment: r.comment.clone(),
            });
        }
    }
    Command::Batch(cmds)
}

/// The first `prefix1`, `prefix2`, ... that no row of `section` is named.
pub fn unique_row_name(doc: &InpDoc, section: &str, prefix: &str) -> String {
    (1u64..)
        .map(|n| format!("{prefix}{n}"))
        .find(|c| !doc.contains(section, c))
        .expect("an unused name exists")
}

// --- LID controls ---------------------------------------------------------------

/// The layers an LID process type uses, with whether the engine insists on
/// the layer (`lid.c` `validateLidProc`: a bio-retention cell or rain
/// garden needs a soil layer, a green roof a soil layer and a drainage mat,
/// permeable pavement a pavement layer, an infiltration trench a storage
/// layer). Other listed layers are read for the type; `REMOVALS` is always
/// optional. Layers not listed are ignored by the engine for that type.
pub fn lid_layers_for(kind: &str) -> &'static [(&'static str, bool)] {
    match kind.trim().to_ascii_uppercase().as_str() {
        "BC" => &[
            ("SURFACE", false),
            ("SOIL", true),
            ("STORAGE", false),
            ("DRAIN", false),
            ("REMOVALS", false),
        ],
        "RG" => &[
            ("SURFACE", false),
            ("SOIL", true),
            ("STORAGE", false),
            ("REMOVALS", false),
        ],
        "GR" => &[
            ("SURFACE", false),
            ("SOIL", true),
            ("DRAINMAT", true),
            ("REMOVALS", false),
        ],
        "IT" => &[
            ("SURFACE", false),
            ("STORAGE", true),
            ("DRAIN", false),
            ("REMOVALS", false),
        ],
        "PP" => &[
            ("SURFACE", false),
            ("PAVEMENT", true),
            ("SOIL", false),
            ("STORAGE", false),
            ("DRAIN", false),
            ("REMOVALS", false),
        ],
        "RB" => &[("STORAGE", false), ("DRAIN", false), ("REMOVALS", false)],
        "VS" => &[("SURFACE", false), ("REMOVALS", false)],
        "RD" => &[("SURFACE", false), ("DRAIN", false), ("REMOVALS", false)],
        _ => &[],
    }
}

/// A readable name for an LID type keyword.
pub fn lid_type_label(kind: &str) -> &'static str {
    match kind.trim().to_ascii_uppercase().as_str() {
        "BC" => "Bio-Retention Cell",
        "RG" => "Rain Garden",
        "GR" => "Green Roof",
        "IT" => "Infiltration Trench",
        "PP" => "Permeable Pavement",
        "RB" => "Rain Barrel",
        "VS" => "Vegetative Swale",
        "RD" => "Rooftop Disconnection",
        _ => "(unknown type)",
    }
}

/// The fewest fields the engine's reader accepts for a layer row (name and
/// layer keyword included): `lid.c` `readSurfaceData` 7, `readSoilData` 9,
/// `readPavementData` 7, `readStorageData` 6, `readDrainData` 6,
/// `readDrainMatData` 5, `readRemovalsData` 4.
pub fn lid_layer_min_fields(layer: &str) -> usize {
    match layer.trim().to_ascii_uppercase().as_str() {
        "SURFACE" | "PAVEMENT" => 7,
        "SOIL" => 9,
        "STORAGE" | "DRAIN" => 6,
        "DRAINMAT" => 5,
        "REMOVALS" => 4,
        _ => 2,
    }
}

/// The parameter fields a new layer row starts with (after the name and
/// the layer keyword): the values the EPA sample `LID_Model.inp` uses for
/// its planters, which are the EPA GUI's own defaults. `REMOVALS` starts
/// empty.
pub fn lid_layer_defaults(layer: &str) -> Vec<String> {
    let v: &[&str] = match layer.trim().to_ascii_uppercase().as_str() {
        // StorHt VegFrac Rough Slope Xslope
        "SURFACE" => &["0", "0.0", "0.1", "1.0", "5"],
        // Thick Por FC WP Ksat Kslope Suct
        "SOIL" => &["12", "0.5", "0.2", "0.1", "0.5", "10.0", "3.5"],
        // Thick Vratio FracImp Perm Vclog
        "PAVEMENT" => &["6", "0.15", "0", "100", "0"],
        // Height Vratio Seepage Vclog Covrd
        "STORAGE" => &["12", "0.75", "0.5", "0", "NO"],
        // Coeff Expon Offset Delay
        "DRAIN" => &["0", "0.5", "0", "6"],
        // Thick Vratio Rough
        "DRAINMAT" => &["3", "0.5", "0.1"],
        _ => &[],
    };
    v.iter().map(|s| s.to_string()).collect()
}

/// The `[LID_CONTROLS]` type of `name`: the second field of its two-field
/// row.
pub fn lid_type_of(doc: &InpDoc, name: &str) -> Option<String> {
    doc.find_all("LID_CONTROLS", name)
        .into_iter()
        .find(|r| r.fields.len() == 2)
        .and_then(|r| r.value(1).map(|s| s.to_ascii_uppercase()))
}

/// The rows of a new LID process of type `kind`: the type row and one row
/// per layer the type uses (`REMOVALS` excepted), in the order the manual
/// lists them.
pub fn lid_control_rows(name: &str, kind: &str) -> Vec<Vec<String>> {
    let kind = kind.trim().to_ascii_uppercase();
    let mut rows = vec![vec![name.to_string(), kind.clone()]];
    for (layer, _) in lid_layers_for(&kind) {
        if *layer == "REMOVALS" {
            continue;
        }
        let mut r = vec![name.to_string(), layer.to_string()];
        r.extend(lid_layer_defaults(layer));
        rows.push(r);
    }
    rows
}

/// A new LID process named `LID1`, `LID2`, ... of type `kind`.
pub fn new_lid_control(doc: &InpDoc, kind: &str) -> NewObject {
    let name = unique_row_name(doc, "LID_CONTROLS", "LID");
    NewObject {
        command: replace_rows(doc, "LID_CONTROLS", &name, &lid_control_rows(&name, kind)),
        name,
    }
}

/// The number of square feet in an acre and of square metres in a hectare:
/// `[SUBCATCHMENTS] Area` is in acres or hectares, `[LID_USAGE] Area` in
/// square feet or square metres.
pub fn subcatchment_area_factor(metric: bool) -> f64 {
    if metric {
        10_000.0
    } else {
        43_560.0
    }
}

/// Whether the model's `[OPTIONS] FLOW_UNITS` is an SI unit.
pub fn is_metric(doc: &InpDoc) -> bool {
    doc.option("FLOW_UNITS")
        .is_some_and(|u| matches!(u.to_ascii_uppercase().as_str(), "CMS" | "LPS" | "MLD"))
}

/// The `[SUBCATCHMENTS] Area` of `name` converted to the `[LID_USAGE]`
/// area unit, and the total LID area (`Number` × `Area`) the subcatchment
/// carries.
pub fn lid_area_check(doc: &InpDoc, name: &str) -> Option<(f64, f64)> {
    let area: f64 = doc
        .field("SUBCATCHMENTS", name, "Area")?
        .trim()
        .parse()
        .ok()?;
    let total: f64 = doc
        .find_all("LID_USAGE", name)
        .iter()
        .map(|r| {
            let n: f64 = r.value(2).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
            let a: f64 = r.value(3).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
            n * a
        })
        .sum();
    Some((area * subcatchment_area_factor(is_metric(doc)), total))
}

/// The fields of a new `[LID_USAGE]` row after the subcatchment: the LID
/// name, one unit, zero area and width, dry, no impervious runoff routed
/// to it, outflow to the outlet, no report file, no drain target.
pub fn lid_usage_defaults(lid: &str) -> Vec<String> {
    let s = |v: &str| v.to_string();
    vec![
        lid.to_string(),
        s("1"),
        s("0"),
        s("0"),
        s("0"),
        s("0"),
        s("0"),
        s("*"),
        s("*"),
        s("0"),
    ]
}

// --- aquifers, groundwater, snow packs ---------------------------------------------

/// The fields of a new `[AQUIFERS]` row after the name: the values of the
/// EPA sample `Groundwater_Model.inp` (porosity 0.5, wilting point 0.15,
/// field capacity 0.30, conductivity 0.1, ...).
pub fn aquifer_defaults() -> Vec<String> {
    [
        "0.5", "0.15", "0.30", "0.1", "12", "15.0", "0.35", "14.0", "0.002", "0.0", "3.5",
        "0.40",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// A new aquifer named `Aquifer1`, `Aquifer2`, ...
pub fn new_aquifer(doc: &InpDoc) -> NewObject {
    let name = unique_row_name(doc, "AQUIFERS", "Aquifer");
    let mut fields = vec![name.clone()];
    fields.extend(aquifer_defaults());
    NewObject {
        command: Command::AddRow {
            section: "AQUIFERS".into(),
            fields,
            comment: None,
        },
        name,
    }
}

/// The fields of a new `[GROUNDWATER]` row after the subcatchment: the
/// aquifer, the receiving node, surface elevation 0, A1 0.001, B1 1, A2 0,
/// B2 0, A3 0, fixed surface-water depth 0.
pub fn groundwater_defaults(aquifer: &str, node: &str) -> Vec<String> {
    let s = |v: &str| v.to_string();
    vec![
        aquifer.to_string(),
        node.to_string(),
        s("0"),
        s("0.001"),
        s("1"),
        s("0"),
        s("0"),
        s("0"),
        s("0"),
    ]
}

/// The parameter fields of a new `[SNOWPACKS]` row (after the name and the
/// keyword): the EPA GUI's defaults: melt coefficients 0.001 in/hr-°F,
/// base temperature 32 °F, free water fraction 0.10, nothing on the ground
/// at the start; plowing at 1 in with nothing removed.
pub fn snowpack_defaults(layer: &str) -> Vec<String> {
    let v: &[&str] = match layer.trim().to_ascii_uppercase().as_str() {
        "PLOWABLE" | "IMPERVIOUS" | "PERVIOUS" => {
            &["0.001", "0.001", "32.0", "0.10", "0.0", "0.0", "0.0"]
        }
        "REMOVAL" => &["1.0", "0.0", "0.0", "0.0", "0.0", "0.0"],
        _ => &[],
    };
    v.iter().map(|s| s.to_string()).collect()
}

/// The rows of a new snow pack: all four keyword rows.
pub fn snowpack_rows(name: &str) -> Vec<Vec<String>> {
    schema::SNOWPACK_LAYERS
        .iter()
        .map(|layer| {
            let mut r = vec![name.to_string(), layer.to_string()];
            r.extend(snowpack_defaults(layer));
            r
        })
        .collect()
}

/// A new snow pack named `SnowPack1`, ...
pub fn new_snowpack(doc: &InpDoc) -> NewObject {
    let name = unique_row_name(doc, "SNOWPACKS", "SnowPack");
    NewObject {
        command: replace_rows(doc, "SNOWPACKS", &name, &snowpack_rows(&name)),
        name,
    }
}

// --- water quality ------------------------------------------------------------------

/// The fields of a new `[BUILDUP]` row after the land use and pollutant:
/// no buildup, coefficients 0, per unit area.
pub fn buildup_defaults() -> Vec<String> {
    ["NONE", "0", "0", "0", "AREA"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The fields of a new `[WASHOFF]` row after the land use and pollutant:
/// no washoff, coefficients 0, no sweeping or BMP removal.
pub fn washoff_defaults() -> Vec<String> {
    ["NONE", "0", "0", "0", "0"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The `[COVERAGES]` (or `[LOADINGS]`) pairs of `name` unrolled: rows may
/// carry several `LandUse Percent` (`Pollutant Buildup`) pairs each.
pub fn pairs_of(doc: &InpDoc, section: &str, name: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in doc.find_all(section, name) {
        let vals: Vec<&str> = r.fields.iter().skip(1).map(|s| unquote(s)).collect();
        for pair in vals.chunks(2) {
            if let [a, b] = pair {
                out.push((a.to_string(), b.to_string()));
            }
        }
    }
    out
}

/// The percentage of `name` covered by land uses, from `[COVERAGES]`.
pub fn coverage_total(doc: &InpDoc, name: &str) -> f64 {
    pairs_of(doc, "COVERAGES", name)
        .iter()
        .filter_map(|(_, p)| p.trim().parse::<f64>().ok())
        .sum()
}

/// A `[TREATMENT]` or `[GWF]` expression as the engine sees it: every field
/// after the first two, joined with single spaces (`treatmnt.c` and
/// `gwater.c` concatenate the tokens before parsing).
pub fn expression_of(fields: &[String]) -> String {
    fields
        .iter()
        .skip(2)
        .map(|s| unquote(s))
        .collect::<Vec<_>>()
        .join(" ")
}

/// An expression as row fields: split on whitespace, so it is written back
/// token by token and read back by [`expression_of`].
pub fn expression_fields(expr: &str) -> Vec<String> {
    expr.split_whitespace().map(str::to_string).collect()
}

// --- RDII -------------------------------------------------------------------------------

/// The rows of a new unit hydrograph set: the rain gage row and, for every
/// month, the three responses with no RDII (R = 0) and times to peak of 1,
/// 4 and 24 hours, K = 2.
pub fn hydrograph_rows(name: &str, gage: &str) -> Vec<Vec<String>> {
    let s = |v: &str| v.to_string();
    vec![
        vec![name.to_string(), gage.to_string()],
        vec![name.to_string(), s("ALL"), s("SHORT"), s("0"), s("1"), s("2")],
        vec![name.to_string(), s("ALL"), s("MEDIUM"), s("0"), s("4"), s("2")],
        vec![name.to_string(), s("ALL"), s("LONG"), s("0"), s("24"), s("2")],
    ]
}

/// A new unit hydrograph set named `UH1`, ..., on the model's first rain
/// gage (`*` when it has none).
pub fn new_hydrograph(doc: &InpDoc) -> NewObject {
    let name = unique_row_name(doc, "HYDROGRAPHS", "UH");
    let gage = doc
        .names("RAINGAGES")
        .first()
        .cloned()
        .unwrap_or_else(|| "*".to_string());
    NewObject {
        command: replace_rows(doc, "HYDROGRAPHS", &name, &hydrograph_rows(&name, &gage)),
        name,
    }
}

/// The `[HYDROGRAPHS]` rain gage of set `name`: its two-field row.
pub fn hydrograph_gage(doc: &InpDoc, name: &str) -> Option<String> {
    doc.find_all("HYDROGRAPHS", name)
        .into_iter()
        .find(|r| r.fields.len() == 2)
        .and_then(|r| r.value(1).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str) -> InpDoc {
        InpDoc::parse(text)
    }

    #[test]
    fn unique_names_skip_taken_ones_case_insensitively() {
        let doc = doc_with("[JUNCTIONS]\nj1 0\nJ2 0\n[OUTFALLS]\nJ3 0 FREE NO\n");
        assert_eq!(unique_name(&doc, ObjectKind::Node, "J"), "J4");
        assert_eq!(unique_name(&doc, ObjectKind::Node, "O"), "O1");
        assert_eq!(
            unique_name_reserving(&doc, ObjectKind::Node, "O", &["O1".into()]),
            "O2"
        );
        assert_eq!(unique_name_from(&doc, ObjectKind::Node, "J1", &[]), "J4");
        assert_eq!(
            unique_name_from(&doc, ObjectKind::Node, "Pond", &[]),
            "Pond"
        );
        let doc = doc_with("[STORAGE]\nPond 0 10 0 FUNCTIONAL 1000 0 0\n");
        assert_eq!(
            unique_name_from(&doc, ObjectKind::Node, "Pond", &[]),
            "Pond1"
        );
    }

    #[test]
    fn new_node_adds_row_and_coordinates_as_one_step() {
        let mut doc = doc_with(&new_model_text());
        let obj = new_node(&doc, NodeType::Junction, 100.0, 200.5);
        assert_eq!(obj.name, "J1");
        doc.apply(obj.command).unwrap();
        assert_eq!(doc.field("JUNCTIONS", "J1", "Elevation"), Some("0"));
        assert_eq!(doc.coordinates("J1"), Some((100.0, 200.5)));
        assert_eq!(doc.undo_depth(), 1);
        assert!(doc.undo());
        assert!(!doc.contains("JUNCTIONS", "J1"));
        assert_eq!(doc.coordinates("J1"), None);
        assert_eq!(doc.to_string(), new_model_text());
    }

    #[test]
    fn every_node_type_has_a_well_formed_row() {
        for kind in NodeType::ALL {
            let mut doc = doc_with("");
            let obj = new_node(&doc, kind, 1.0, 2.0);
            assert!(obj.name.starts_with(kind.prefix()));
            doc.apply(obj.command).unwrap();
            let (_, row) = doc.find(kind.section(), &obj.name).unwrap();
            let min = schema::min_fields(kind.section()).unwrap();
            assert!(row.fields.len() >= min, "{kind:?}: {:?}", row.fields);
            assert!(
                doc.validate()
                    .iter()
                    .all(|f| f.severity != crate::doc::Severity::Error),
                "{:?}",
                doc.validate()
            );
            assert_eq!(node_type_of(&doc, &obj.name), Some(kind));
        }
    }

    #[test]
    fn every_link_type_has_its_rows_and_validates() {
        for kind in LinkType::ALL {
            let mut doc = doc_with("");
            doc.apply(new_node(&doc, NodeType::Junction, 0.0, 0.0).command)
                .unwrap();
            doc.apply(new_node(&doc, NodeType::Outfall, 100.0, 0.0).command)
                .unwrap();
            let obj = new_link(&doc, kind, "J1", "O1", &[(50.0, 10.0)], 100.5);
            doc.apply(obj.command).unwrap();
            assert_eq!(link_type_of(&doc, &obj.name), Some(kind));
            assert_eq!(doc.vertices(&obj.name), vec![(50.0, 10.0)]);
            assert_eq!(
                link_ends(&doc, &obj.name),
                Some(("J1".to_string(), "O1".to_string()))
            );
            let errors: Vec<_> = doc
                .validate()
                .into_iter()
                .filter(|f| f.severity == crate::doc::Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{kind:?}: {errors:?}");
            if kind == LinkType::Conduit {
                assert_eq!(doc.field("XSECTIONS", "C1", "Shape"), Some("CIRCULAR"));
                assert_eq!(doc.field("XSECTIONS", "C1", "Geom1"), Some("1"));
                assert_eq!(doc.field("CONDUITS", "C1", "Length"), Some("100.5"));
            }
        }
    }

    #[test]
    fn subcatchment_infiltration_follows_the_options_method() {
        for (method, n) in [("HORTON", 6), ("GREEN_AMPT", 4), ("CURVE_NUMBER", 4)] {
            let mut doc = doc_with(&format!(
                "[OPTIONS]\nINFILTRATION {method}\n[RAINGAGES]\nRG1 INTENSITY 1:00 1.0 TIMESERIES TS\n[JUNCTIONS]\nJ1 0\n"
            ));
            let obj = new_subcatchment(&doc, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], "J1");
            assert_eq!(obj.name, "S1");
            doc.apply(obj.command).unwrap();
            let (_, row) = doc.find("INFILTRATION", "S1").unwrap();
            assert_eq!(row.fields.len(), n, "{method}: {:?}", row.fields);
            assert_eq!(doc.field("SUBCATCHMENTS", "S1", "RainGage"), Some("RG1"));
            assert_eq!(doc.field("SUBCATCHMENTS", "S1", "Outlet"), Some("J1"));
            assert_eq!(doc.polygon("S1").len(), 3);
            assert!(doc.contains("SUBAREAS", "S1"));
            assert_eq!(doc.undo_depth(), 1);
        }
    }

    #[test]
    fn gage_and_label_rows() {
        let mut doc = doc_with("[TIMESERIES]\nTS 0:00 1.0\n");
        let g = new_gage(&doc, 5.0, 6.0);
        doc.apply(g.command).unwrap();
        assert_eq!(doc.field("RAINGAGES", "R1", "Series"), Some("TS"));
        assert_eq!(doc.symbol("R1"), Some((5.0, 6.0)));
        doc.apply(new_label(1.0, 2.0, "Pond A")).unwrap();
        let (_, row) = doc.rows("LABELS")[0];
        assert_eq!(row.value(2), Some("Pond A"));
        assert_eq!(row.fields[2], "\"Pond A\"");
    }

    #[test]
    fn reverse_link_swaps_ends_and_vertices() {
        let mut doc = doc_with("[CONDUITS]\nC1 A B 100 0.01 0 0\n[VERTICES]\nC1 1 1\nC1 2 2\n");
        doc.apply(reverse_link(&doc, "C1").unwrap()).unwrap();
        assert_eq!(link_ends(&doc, "C1"), Some(("B".into(), "A".into())));
        assert_eq!(doc.vertices("C1"), vec![(2.0, 2.0), (1.0, 1.0)]);
        assert_eq!(doc.undo_depth(), 1);
    }

    #[test]
    fn convert_node_keeps_name_elevation_and_coordinates() {
        let mut doc = doc_with("[JUNCTIONS]\nJ1 12.5 0 0 0 0\n[COORDINATES]\nJ1 3 4\n[CONDUITS]\nC1 J1 J1 1 0.01 0 0\n");
        doc.apply(convert_node(&doc, "J1", NodeType::Storage).unwrap())
            .unwrap();
        assert!(!doc.contains("JUNCTIONS", "J1"));
        assert_eq!(doc.field("STORAGE", "J1", "Elevation"), Some("12.5"));
        assert_eq!(doc.field("STORAGE", "J1", "Shape"), Some("FUNCTIONAL"));
        assert_eq!(doc.coordinates("J1"), Some((3.0, 4.0)));
        assert_eq!(node_type_of(&doc, "J1"), Some(NodeType::Storage));
        assert!(convert_node(&doc, "J1", NodeType::Storage).is_none());
    }

    #[test]
    fn delete_impact_names_attached_links_and_orphaned_subcatchments() {
        let doc = doc_with(
            "[JUNCTIONS]\nJ1 0\nJ2 0\n[CONDUITS]\nC1 J1 J2 1 0.01 0 0\nC2 J2 J1 1 0.01 0 0\n[SUBCATCHMENTS]\nS1 * J1 5 25 500 0.5 0\n",
        );
        let impact = delete_impact(&doc, &[ObjRef::Node("J1".into())]);
        assert_eq!(impact.links, vec!["C1", "C2"]);
        assert_eq!(impact.orphaned_subcatchments, vec!["S1"]);
        let impact = delete_impact(
            &doc,
            &[ObjRef::Node("J1".into()), ObjRef::Link("C1".into())],
        );
        assert_eq!(impact.links, vec!["C2"]);
        assert!(delete_impact(&doc, &[ObjRef::Link("C1".into())]).is_empty());
    }

    #[test]
    fn cascade_delete_is_one_step_and_takes_labels_highest_first() {
        let mut doc = doc_with(
            "[JUNCTIONS]\nJ1 0\nJ2 0\n[CONDUITS]\nC1 J1 J2 1 0.01 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0\n[COORDINATES]\nJ1 0 0\nJ2 1 1\n[LABELS]\n0 0 \"a\"\n1 1 \"b\"\n2 2 \"c\"\n",
        );
        let before = doc.to_string();
        let cmd = cascade_delete(
            &doc,
            &[
                ObjRef::Label(0),
                ObjRef::Node("J1".into()),
                ObjRef::Label(2),
            ],
        );
        doc.apply(cmd).unwrap();
        assert!(!doc.contains("JUNCTIONS", "J1"));
        assert!(!doc.contains("CONDUITS", "C1"));
        assert!(!doc.contains("XSECTIONS", "C1"));
        assert!(!doc.contains("COORDINATES", "J1"));
        assert!(doc.contains("JUNCTIONS", "J2"));
        let labels: Vec<_> = doc
            .rows("LABELS")
            .iter()
            .map(|(_, r)| r.fields[2].clone())
            .collect();
        assert_eq!(labels, vec!["\"b\""]);
        assert_eq!(doc.undo_depth(), 1);
        doc.undo();
        assert_eq!(doc.to_string(), before);
    }

    #[test]
    fn paste_renames_remaps_and_offsets() {
        let mut doc = doc_with(
            "[JUNCTIONS]\nJ1 10 0 0 0 0\nJ2 5 0 0 0 0\n[CONDUITS]\nC1 J1 J2 100 0.01 0 0\n[XSECTIONS]\nC1 CIRCULAR 1 0 0 0\n[SUBCATCHMENTS]\nS1 * J1 5 25 500 0.5 0\n[SUBAREAS]\nS1 0.01 0.1 0.05 0.05 25 OUTLET\n[COORDINATES]\nJ1 0 0\nJ2 100 0\n[VERTICES]\nC1 50 5\n[POLYGONS]\nS1 0 10\nS1 10 10\nS1 10 20\n[TAGS]\nNode J1 manhole\n",
        );
        let items = [
            ObjRef::Link("C1".into()),
            ObjRef::Node("J1".into()),
            ObjRef::Node("J2".into()),
            ObjRef::Subcatchment("S1".into()),
        ];
        let clip = copy(&doc, &items);
        assert_eq!(clip.len(), 4);
        let (pasted, cmd) = paste(&doc, &clip, 1000.0, 0.0);
        doc.apply(cmd).unwrap();
        assert_eq!(doc.undo_depth(), 1);
        let names: Vec<&str> = pasted.iter().filter_map(|p| p.name()).collect();
        assert_eq!(names, vec!["J3", "J4", "S2", "C2"]);
        assert_eq!(doc.coordinates("J3"), Some((1000.0, 0.0)));
        assert_eq!(doc.coordinates("J4"), Some((1100.0, 0.0)));
        assert_eq!(link_ends(&doc, "C2"), Some(("J3".into(), "J4".into())));
        assert_eq!(doc.vertices("C2"), vec![(1050.0, 5.0)]);
        assert_eq!(doc.field("XSECTIONS", "C2", "Shape"), Some("CIRCULAR"));
        assert_eq!(doc.field("SUBCATCHMENTS", "S2", "Outlet"), Some("J3"));
        assert_eq!(doc.polygon("S2")[0], (1000.0, 10.0));
        assert!(doc.contains("SUBAREAS", "S2"));
        assert_eq!(doc.tag("Node", "J3"), Some("manhole"));
        // The originals are untouched.
        assert_eq!(doc.coordinates("J1"), Some((0.0, 0.0)));
        assert_eq!(link_ends(&doc, "C1"), Some(("J1".into(), "J2".into())));
        // A second paste of the same clipboard counts on.
        let (pasted, cmd) = paste(&doc, &clip, 0.0, 500.0);
        doc.apply(cmd).unwrap();
        let names: Vec<&str> = pasted.iter().filter_map(|p| p.name()).collect();
        assert_eq!(names, vec!["J5", "J6", "S3", "C3"]);
        let errors: Vec<_> = doc
            .validate()
            .into_iter()
            .filter(|f| f.severity == crate::doc::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn paste_keeps_unmapped_endpoints_and_drops_label_anchors() {
        let mut doc = doc_with(
            "[JUNCTIONS]\nJ1 0\nJ2 0\n[CONDUITS]\nC1 J1 J2 100 0.01 0 0\n[LABELS]\n1 2 \"x\" J1 \"Arial\" 10 0 0\n",
        );
        let clip = copy(&doc, &[ObjRef::Link("C1".into()), ObjRef::Label(0)]);
        let (pasted, cmd) = paste(&doc, &clip, 10.0, 10.0);
        doc.apply(cmd).unwrap();
        assert_eq!(pasted, vec![ObjRef::Link("C2".into())]);
        assert_eq!(link_ends(&doc, "C2"), Some(("J1".into(), "J2".into())));
        let labels = doc.rows("LABELS");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[1].1.fields[..4], ["11", "12", "\"x\"", "\"\""]);
    }

    #[test]
    fn new_model_text_parses_clean_and_round_trips() {
        let text = new_model_text();
        let doc = InpDoc::parse(&text);
        assert_eq!(doc.to_string(), text);
        assert_eq!(doc.option("INFILTRATION"), Some("HORTON"));
        assert!(doc.validate().is_empty());
    }
}

#[cfg(test)]
mod chapter21_tests {
    use super::*;

    const LID: &str = "[LID_CONTROLS]\n;;Name\tType/Layer\tParameters\nGreenRoof \tBC\nGreenRoof \tSURFACE   \t0.0 \t0.0 \t0.1 \t1.0 \t5\nGreenRoof \tSOIL      \t3 \t0.5 \t0.2 \t0.1 \t0.5 \t10.0 \t3.5\n\nSwale \tVS\nSwale \tSURFACE \t36 \t0.0 \t0.24 \t1.0 \t5\n[LID_USAGE]\nS1 \tGreenRoof \t1 \t500 \t0 \t0 \t0 \t0 \t* \t* \t0\n";

    #[test]
    fn replace_rows_writes_nothing_for_an_unchanged_draft() {
        let mut doc = InpDoc::parse(LID);
        let rows = rows_of(&doc, "LID_CONTROLS", "GreenRoof");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][1], "SURFACE");
        let cmd = replace_rows(&doc, "LID_CONTROLS", "GreenRoof", &rows);
        assert_eq!(cmd, Command::Batch(Vec::new()));
        doc.apply(cmd).unwrap();
        assert_eq!(doc.undo_depth(), 0);
        assert_eq!(doc.to_string(), LID);
    }

    #[test]
    fn replace_rows_edits_in_place_inserts_after_the_last_and_deletes_surplus() {
        let mut doc = InpDoc::parse(LID);
        let mut rows = rows_of(&doc, "LID_CONTROLS", "GreenRoof");
        rows[2][2] = "18".into();
        rows.push(vec!["GreenRoof".into(), "DRAINMAT".into(), "3".into(), "0.5".into(), "0.1".into()]);
        doc.apply(replace_rows(&doc, "LID_CONTROLS", "GreenRoof", &rows)).unwrap();
        let text = doc.to_string();
        // The surface row and the swale are untouched; the mat follows the soil.
        assert!(text.contains("GreenRoof \tSURFACE   \t0.0 \t0.0 \t0.1 \t1.0 \t5\n"), "{text}");
        assert!(text.contains("Swale \tSURFACE \t36 \t0.0 \t0.24 \t1.0 \t5\n"), "{text}");
        let lines: Vec<&str> = text.lines().collect();
        let soil = lines.iter().position(|l| l.contains("SOIL")).unwrap();
        assert!(lines[soil].contains("18"), "{}", lines[soil]);
        assert!(lines[soil + 1].starts_with("GreenRoof  DRAINMAT"), "{}", lines[soil + 1]);
        assert_eq!(doc.undo_depth(), 1);
        // Fewer rows: the surplus goes.
        let rows = vec![vec!["GreenRoof".into(), "BC".into()]];
        doc.apply(replace_rows(&doc, "LID_CONTROLS", "GreenRoof", &rows)).unwrap();
        assert_eq!(rows_of(&doc, "LID_CONTROLS", "GreenRoof").len(), 1);
        assert!(doc.undo() && doc.undo());
        assert_eq!(doc.to_string(), LID);
    }

    #[test]
    fn rename_in_columns_follows_every_reference_and_keeps_quotes() {
        let mut doc = InpDoc::parse("[LID_CONTROLS]\n\"Green Roof\" BC\n\"Green Roof\" SOIL 3 0.5 0.2 0.1 0.5 10 3.5\n[LID_USAGE]\nS1 \"Green Roof\" 1 500 0 0 0 0\n");
        let cmd = rename_in_columns(&doc, &[("LID_CONTROLS", 0), ("LID_USAGE", 1)], "green roof", "GR1");
        doc.apply(cmd).unwrap();
        assert_eq!(rows_of(&doc, "LID_CONTROLS", "GR1").len(), 2);
        let (_, r) = doc.find("LID_USAGE", "S1").unwrap();
        assert_eq!(r.fields[1], "\"GR1\"");
        assert_eq!(doc.undo_depth(), 1);
    }

    #[test]
    fn lid_builders_follow_the_types_layers() {
        let rows = lid_control_rows("X", "GR");
        let layers: Vec<&str> = rows.iter().map(|r| r[1].as_str()).collect();
        assert_eq!(layers, vec!["GR", "SURFACE", "SOIL", "DRAINMAT"]);
        assert_eq!(rows[2].len(), lid_layer_min_fields("SOIL"));
        assert_eq!(lid_control_rows("X", "RB").len(), 3);
        assert!(lid_layers_for("PP").iter().any(|(l, req)| *l == "PAVEMENT" && *req));
        assert!(lid_layers_for("ZZ").is_empty());
        let doc = InpDoc::parse(LID);
        assert_eq!(lid_type_of(&doc, "swale").as_deref(), Some("VS"));
        assert_eq!(new_lid_control(&doc, "IT").name, "LID1");
        assert_eq!(lid_usage_defaults("GreenRoof").len(), 10);
    }

    #[test]
    fn lid_area_check_converts_the_subcatchment_area() {
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS CFS\n[SUBCATCHMENTS]\nS1 * O1 2 50 100 1 0\n[LID_USAGE]\nS1 A 4 500 0 0 0 0\nS1 B 1 1000 0 0 0 0\n");
        assert_eq!(lid_area_check(&doc, "S1"), Some((2.0 * 43560.0, 3000.0)));
        let doc = InpDoc::parse("[OPTIONS]\nFLOW_UNITS LPS\n[SUBCATCHMENTS]\nS1 * O1 2 50 100 1 0\n");
        assert!(is_metric(&doc));
        assert_eq!(lid_area_check(&doc, "S1"), Some((20000.0, 0.0)));
        assert_eq!(lid_area_check(&doc, "S9"), None);
    }

    #[test]
    fn snow_hydrograph_and_aquifer_builders_make_complete_rows() {
        let doc = InpDoc::parse("[RAINGAGES]\nRG1 INTENSITY 1:00 1.0 TIMESERIES TS\n");
        let rows = snowpack_rows("P");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].len(), 9);
        assert_eq!(rows[3].len(), 8);
        let h = new_hydrograph(&doc);
        assert_eq!(h.name, "UH1");
        let mut doc = doc;
        doc.apply(h.command).unwrap();
        assert_eq!(hydrograph_gage(&doc, "UH1").as_deref(), Some("RG1"));
        assert_eq!(doc.find_all("HYDROGRAPHS", "UH1").len(), 4);
        let a = new_aquifer(&doc);
        doc.apply(a.command).unwrap();
        let (_, r) = doc.find("AQUIFERS", "Aquifer1").unwrap();
        assert_eq!(r.fields.len(), 13);
        assert_eq!(groundwater_defaults("Aquifer1", "J1").len(), 9);
        assert_eq!(new_snowpack(&doc).name, "SnowPack1");
        assert_eq!(buildup_defaults().len(), 5);
        assert_eq!(washoff_defaults().len(), 5);
    }

    #[test]
    fn expressions_and_pairs_round_trip() {
        let fields: Vec<String> = ["J1", "TSS", "R", "=", "1", "-", "exp(-0.5*HRT)"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let e = expression_of(&fields);
        assert_eq!(e, "R = 1 - exp(-0.5*HRT)");
        assert_eq!(expression_fields(&e), fields[2..].to_vec());
        let doc = InpDoc::parse("[COVERAGES]\nS1 Res 40 Com 30\nS1 Und 10\n");
        assert_eq!(pairs_of(&doc, "COVERAGES", "S1").len(), 3);
        assert_eq!(coverage_total(&doc, "S1"), 80.0);
    }
}
