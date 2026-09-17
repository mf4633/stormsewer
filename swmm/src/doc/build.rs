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
fn infiltration_defaults(doc: &InpDoc) -> Vec<String> {
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
