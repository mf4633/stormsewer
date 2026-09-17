// SPDX-License-Identifier: GPL-3.0-or-later

//! Project → Design Storm…: build a hyetograph (NRCS Type I/IA/II/III,
//! NRCS NOAA Atlas 14 regional, alternating block from an IDF curve or a
//! pasted NOAA PFDS row, Chicago, uniform), see it plotted and tabled,
//! and write it as a `[TIMESERIES]` plus a `[RAINGAGES]` row in one undo
//! step — assigned to the selected subcatchments when asked. The maths
//! and its sources are in `stormsewer_swmm::storm`.

use eframe::egui::{self, Id, RichText, Ui, Vec2};
use stormsewer_swmm::doc::build::ObjRef;
use stormsewer_swmm::storm::{self, Format, Method, NoaaRegion, ScsType, Storm, StormSpec};

use crate::state::AppState;
use crate::swmm_dialogs::plot;
use crate::swmm_doc::SwmmEditor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Scs,
    NoaaRegion,
    IdfBlock,
    PfdsBlock,
    Chicago,
    Uniform,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Scs,
        Kind::NoaaRegion,
        Kind::PfdsBlock,
        Kind::IdfBlock,
        Kind::Chicago,
        Kind::Uniform,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Scs => "NRCS (SCS) 24-hour Type I / IA / II / III",
            Kind::NoaaRegion => "NRCS NOAA Atlas 14 regional 24-hour (A–D)",
            Kind::PfdsBlock => "Alternating block from a NOAA Atlas 14 PFDS row",
            Kind::IdfBlock => "Alternating block from an IDF curve",
            Kind::Chicago => "Chicago (Keifer–Chu) from an IDF curve",
            Kind::Uniform => "Uniform",
        }
    }

    /// Where the numbers come from, shown under the picker.
    pub fn source(self) -> &'static str {
        match self {
            Kind::Scs => "NRCS TR-20 tabular distributions; NEH 630 Ch. 4 (2019) §630.0403, fig. 4-36",
            Kind::NoaaRegion => "NEH 630 Ch. 4 (2019) §630.0408, fig. 4-72 ratios, nested (no WinTR-20 smoothing)",
            Kind::PfdsBlock => "Depths from NOAA Atlas 14 PFDS csv; alternating block (Chow, Maidment & Mays §14.4)",
            Kind::IdfBlock => "i = a / (t + b)^c, t in minutes; alternating block (Chow, Maidment & Mays §14.4)",
            Kind::Chicago => "Keifer & Chu (1957); i = a / (t + b)^c, peak at fraction r of the duration",
            Kind::Uniform => "Constant intensity",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StormDraft {
    pub kind: Kind,
    pub scs: ScsType,
    pub region: NoaaRegion,
    pub depth: String,
    pub ari: String,
    pub pfds: String,
    pub a: String,
    pub b: String,
    pub c: String,
    pub r: String,
    pub duration_min: String,
    pub step_min: String,
    pub format: Format,
    pub series: String,
    pub gage: String,
    pub place_gage: bool,
    /// Assign the gage to the selected subcatchments (or all, below).
    pub assign_selected: bool,
    pub assign_all: bool,
    pub preview: Option<Storm>,
    pub picked_ari: Option<f64>,
    pub error: String,
    pub dirty: bool,
}

impl Default for StormDraft {
    fn default() -> Self {
        Self {
            kind: Kind::Scs,
            scs: ScsType::II,
            region: NoaaRegion::B,
            depth: "6".into(),
            ari: "10".into(),
            pfds: String::new(),
            a: "60".into(),
            b: "10".into(),
            c: "0.8".into(),
            r: "0.4".into(),
            duration_min: "1440".into(),
            step_min: "5".into(),
            format: Format::Volume,
            series: "TS_TypeII_6in".into(),
            gage: "RG_Design".into(),
            place_gage: true,
            assign_selected: false,
            assign_all: false,
            preview: None,
            picked_ari: None,
            error: String::new(),
            dirty: true,
        }
    }
}

fn num(s: &str, what: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| format!("{what}: not a number"))
}

fn int(s: &str, what: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("{what}: whole minutes, at least 1"))
}

/// The spec the draft describes.
pub fn spec_of(d: &StormDraft) -> Result<(StormSpec, Option<f64>), String> {
    let step_min = int(&d.step_min, "time step")?;
    let mut duration_min = int(&d.duration_min, "duration")?;
    let mut picked = None;
    let method = match d.kind {
        Kind::Scs => Method::Scs {
            kind: d.scs,
            depth: num(&d.depth, "depth")?,
        },
        Kind::NoaaRegion => Method::NoaaRegion {
            region: d.region,
            depth: num(&d.depth, "depth")?,
        },
        Kind::IdfBlock => Method::AlternatingBlockIdf {
            a: num(&d.a, "a")?,
            b: num(&d.b, "b")?,
            c: num(&d.c, "c")?,
        },
        Kind::PfdsBlock => {
            let p = storm::parse_pfds(&d.pfds)?;
            let ari = num(&d.ari, "return period")?;
            let (got, depths) = p
                .column(ari)
                .ok_or("the pasted table has no return-period columns")?;
            picked = Some(got);
            Method::AlternatingBlockDepths { depths }
        }
        Kind::Chicago => Method::Chicago {
            a: num(&d.a, "a")?,
            b: num(&d.b, "b")?,
            c: num(&d.c, "c")?,
            r: num(&d.r, "r")?,
        },
        Kind::Uniform => Method::Uniform {
            depth: num(&d.depth, "depth")?,
        },
    };
    if matches!(d.kind, Kind::Scs | Kind::NoaaRegion) {
        duration_min = 1440;
    }
    Ok((
        StormSpec {
            method,
            duration_min,
            step_min,
            format: d.format,
        },
        picked,
    ))
}

/// Rebuild the preview from the inputs.
pub fn rebuild(d: &mut StormDraft) {
    d.dirty = false;
    match spec_of(d).and_then(|(spec, picked)| storm::build(&spec).map(|s| (s, picked))) {
        Ok((s, picked)) => {
            d.preview = Some(s);
            d.picked_ari = picked;
            d.error.clear();
        }
        Err(e) => {
            d.preview = None;
            d.error = e;
        }
    }
}

pub fn open(ed: &mut SwmmEditor) {
    let mut d = StormDraft {
        assign_selected: ed
            .selection
            .iter()
            .any(|r| matches!(r, ObjRef::Subcatchment(_))),
        ..StormDraft::default()
    };
    rebuild(&mut d);
    ed.dialogs.storm = Some(d);
}

/// The subcatchments the storm is assigned to.
fn assign_list(ed: &SwmmEditor, d: &StormDraft) -> Vec<String> {
    if d.assign_all {
        ed.doc.names("SUBCATCHMENTS")
    } else if d.assign_selected {
        ed.selection
            .iter()
            .filter_map(|r| match r {
                ObjRef::Subcatchment(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Where a new gage symbol goes: outside the model's upper-left corner.
fn gage_spot(ed: &SwmmEditor) -> (f64, f64) {
    match ed.bounds {
        Some((x0, _, x1, y1)) => {
            let m = ((x1 - x0) * 0.05).max(10.0);
            let taken = ed.gages.len() as f64;
            (x0 - m + taken * m * 0.6, y1 + m)
        }
        None => (0.0, 0.0),
    }
}

/// Write the previewed storm: one undo step. Returns the status line.
pub fn commit(ed: &mut SwmmEditor) -> Option<String> {
    let mut d = ed.dialogs.storm.take()?;
    if d.dirty {
        rebuild(&mut d);
    }
    let Some(s) = d.preview.clone() else {
        ed.dialogs.storm = Some(d);
        return None;
    };
    let series = if d.series.trim().is_empty() {
        "TS_Storm"
    } else {
        d.series.trim()
    };
    let gage = if d.gage.trim().is_empty() {
        "RG_Design"
    } else {
        d.gage.trim()
    };
    let assign = assign_list(ed, &d);
    let symbol = d.place_gage.then(|| gage_spot(ed));
    let w = storm::command(&ed.doc, &s, series, gage, symbol, &assign);
    let label = format!("add design storm {}", w.series);
    if ed.apply(w.command.clone(), &label) {
        ed.select_only(storm::gage_ref(&w));
        Some(format!(
            "Added {} ({} {}, {} intervals) and gage {}{}",
            w.series,
            stormsewer_swmm::doc::format_number((s.total_depth() * 1000.0).round() / 1000.0),
            match ed
                .doc
                .option("FLOW_UNITS")
                .map(|u| u.to_ascii_uppercase())
                .as_deref()
            {
                Some("CMS" | "LPS" | "MLD") => "mm",
                _ => "in",
            },
            s.increments.len(),
            w.gage,
            if assign.is_empty() {
                String::new()
            } else {
                format!("; assigned to {} subcatchment(s)", assign.len())
            }
        ))
    } else {
        d.error = ed.last_error.clone().unwrap_or_default();
        ed.dialogs.storm = Some(d);
        None
    }
}

fn text(ui: &mut Ui, label: &str, id: &str, s: &mut String, width: f32) -> bool {
    ui.label(label);
    let r = ui.add(
        egui::TextEdit::singleline(s)
            .id(Id::new(("swmm-storm", id)))
            .desired_width(width),
    );
    r.changed()
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut d) = state.swmm_doc.dialogs.storm.clone() else {
        return;
    };
    let mut open = true;
    let mut action: Option<&str> = None;
    let size = Vec2::new(760.0, 560.0);
    egui::Window::new("Design Storm")
        .id(Id::new(("swmm-dialog", "storm")))
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .default_pos(ctx.screen_rect().center() - size / 2.0)
        .open(&mut open)
        .show(ctx, |ui| {
            let ed = &state.swmm_doc;
            let metric = matches!(
                ed.doc.option("FLOW_UNITS").map(|u| u.to_ascii_uppercase()).as_deref(),
                Some("CMS" | "LPS" | "MLD")
            );
            let unit = if metric { "mm" } else { "in" };
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                let mut changed = false;
                egui::ComboBox::from_id_salt("swmm-storm-kind")
                    .selected_text(d.kind.label())
                    .width(340.0)
                    .show_ui(ui, |ui| {
                        for k in Kind::ALL {
                            changed |= ui.selectable_value(&mut d.kind, k, k.label()).changed();
                        }
                    });
                ui.label(RichText::new(d.kind.source()).small().weak());
                ui.add_space(4.0);
                egui::Grid::new("swmm-storm-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        match d.kind {
                            Kind::Scs => {
                                ui.label("Distribution");
                                ui.horizontal(|ui| {
                                    for k in ScsType::ALL {
                                        changed |= ui.selectable_value(&mut d.scs, k, k.label()).changed();
                                    }
                                });
                                ui.end_row();
                                changed |= text(ui, &format!("24-h depth ({unit})"), "depth", &mut d.depth, 80.0);
                                ui.end_row();
                            }
                            Kind::NoaaRegion => {
                                ui.label("Region");
                                ui.horizontal(|ui| {
                                    for k in NoaaRegion::ALL {
                                        changed |= ui.selectable_value(&mut d.region, k, k.label()).changed();
                                    }
                                });
                                ui.end_row();
                                changed |= text(ui, &format!("24-h depth ({unit})"), "depth", &mut d.depth, 80.0);
                                ui.end_row();
                            }
                            Kind::PfdsBlock => {
                                changed |= text(ui, "Return period (years)", "ari", &mut d.ari, 60.0);
                                ui.end_row();
                                changed |= text(ui, "Duration (min)", "dur", &mut d.duration_min, 60.0);
                                ui.end_row();
                            }
                            Kind::IdfBlock | Kind::Chicago => {
                                changed |= text(ui, &format!("a ({unit}/h)"), "a", &mut d.a, 70.0);
                                ui.end_row();
                                changed |= text(ui, "b (min)", "b", &mut d.b, 70.0);
                                ui.end_row();
                                changed |= text(ui, "c", "c", &mut d.c, 70.0);
                                ui.end_row();
                                if d.kind == Kind::Chicago {
                                    changed |= text(ui, "r (peak position 0–1)", "r", &mut d.r, 70.0);
                                    ui.end_row();
                                }
                                changed |= text(ui, "Duration (min)", "dur", &mut d.duration_min, 60.0);
                                ui.end_row();
                            }
                            Kind::Uniform => {
                                changed |= text(ui, &format!("Depth ({unit})"), "depth", &mut d.depth, 80.0);
                                ui.end_row();
                                changed |= text(ui, "Duration (min)", "dur", &mut d.duration_min, 60.0);
                                ui.end_row();
                            }
                        }
                        changed |= text(ui, "Time step (min)", "step", &mut d.step_min, 60.0);
                        ui.end_row();
                        ui.label("Series format");
                        ui.horizontal(|ui| {
                            changed |= ui.selectable_value(&mut d.format, Format::Volume, "VOLUME").changed();
                            changed |= ui.selectable_value(&mut d.format, Format::Intensity, "INTENSITY").changed();
                        });
                        ui.end_row();
                    });
                if d.kind == Kind::PfdsBlock {
                    ui.label(RichText::new("Paste the PFDS csv (the \"by duration for ARI\" table):").small());
                    changed |= ui
                        .add(
                            egui::TextEdit::multiline(&mut d.pfds)
                                .id(Id::new(("swmm-storm", "pfds")))
                                .desired_rows(5)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        )
                        .changed();
                    if let Some(a) = d.picked_ari {
                        ui.label(RichText::new(format!("Using the {a}-year column")).small());
                    }
                }
                ui.separator();
                egui::Grid::new("swmm-storm-names")
                    .num_columns(2)
                    .show(ui, |ui| {
                        text(ui, "Series name", "series", &mut d.series, 160.0);
                        ui.end_row();
                        text(ui, "Gage name", "gage", &mut d.gage, 160.0);
                        ui.end_row();
                    });
                ui.checkbox(&mut d.place_gage, "Place the gage symbol on the map");
                let n_sel = ed
                    .selection
                    .iter()
                    .filter(|r| matches!(r, ObjRef::Subcatchment(_)))
                    .count();
                ui.add_enabled(
                    n_sel > 0,
                    egui::Checkbox::new(
                        &mut d.assign_selected,
                        format!("Assign to the {n_sel} selected subcatchment(s)"),
                    ),
                );
                ui.checkbox(&mut d.assign_all, "Assign to every subcatchment");
                if changed {
                    d.dirty = true;
                }
                if d.dirty {
                    rebuild(&mut d);
                }

                let ui = &mut cols[1];
                match &d.preview {
                    Some(s) => {
                        let per = if d.format == Format::Intensity {
                            format!("{unit}/h")
                        } else {
                            unit.to_string()
                        };
                        ui.label(RichText::new(format!(
                            "Total {:.3} {unit} over {} min in {} steps of {} min; peak {:.3} {unit}/h",
                            s.total_depth(),
                            s.duration_min,
                            s.increments.len(),
                            s.step_min,
                            s.peak_intensity()
                        ))
                        .small());
                        let pts: Vec<(f64, f64)> = s
                            .values()
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (i as f64, *v))
                            .collect();
                        plot(ui, Id::new("swmm-storm-plot"), &pts, 150.0, true);
                        ui.label(RichText::new(format!("Series values ({per}) per interval")).small());
                        let cum = s.cumulative();
                        plot(ui, Id::new("swmm-storm-cum"), &cum, 110.0, false);
                        ui.label(RichText::new(format!("Cumulative depth ({unit}) vs minutes")).small());
                        egui::ScrollArea::vertical()
                            .id_salt("swmm-storm-table")
                            .max_height(150.0)
                            .show(ui, |ui| {
                                egui::Grid::new("swmm-storm-table-grid")
                                    .num_columns(3)
                                    .striped(true)
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Time").strong());
                                        ui.label(RichText::new(format!("Value ({per})")).strong());
                                        ui.label(RichText::new(format!("Cum ({unit})")).strong());
                                        ui.end_row();
                                        let vals = s.values();
                                        for (i, (row, v)) in s.rows("x").iter().zip(vals.iter()).enumerate() {
                                            ui.label(&row[1]);
                                            ui.label(format!("{v:.4}"));
                                            ui.label(format!("{:.4}", cum.get(i + 1).map(|c| c.1).unwrap_or(0.0)));
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                    None => {
                        ui.label(
                            RichText::new(if d.error.is_empty() { "no preview" } else { d.error.as_str() })
                                .color(crate::theme::palette::error_text(ui.visuals().dark_mode)),
                        );
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(d.preview.is_some(), egui::Button::new("Add to Model"))
                    .clicked()
                {
                    action = Some("ok");
                }
                if ui.button("Cancel").clicked() {
                    action = Some("cancel");
                }
                ui.label(RichText::new("Writes [TIMESERIES] and [RAINGAGES] as one undo step.").small());
            });
        });
    let ed = &mut state.swmm_doc;
    match action {
        Some("ok") => {
            ed.dialogs.storm = Some(d);
            if let Some(s) = commit(ed) {
                state.status = s;
            }
        }
        Some("cancel") => ed.dialogs.storm = None,
        _ => ed.dialogs.storm = if open { Some(d) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pond() -> SwmmEditor {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../swmm/tests/fixtures/epa-samples/Detention_Pond_Model.inp");
        let mut ed = SwmmEditor::default();
        ed.open_path(&path).unwrap();
        ed.refresh();
        ed
    }

    #[test]
    fn type_ii_storm_is_added_with_its_gage_and_assigned_to_the_selection() {
        let mut ed = pond();
        ed.select_only(ObjRef::Subcatchment("S1".into()));
        open(&mut ed);
        let d = ed.dialogs.storm.as_mut().unwrap();
        assert!(d.assign_selected);
        assert!(d.preview.is_some(), "{}", d.error);
        d.depth = "6".into();
        d.step_min = "5".into();
        d.dirty = true;
        rebuild(d);
        let s = d.preview.clone().unwrap();
        assert_eq!(s.increments.len(), 288);
        assert!((s.total_depth() - 6.0).abs() < 1e-9);
        let depth = ed.undo_depth();
        let status = commit(&mut ed).unwrap();
        assert!(
            status.contains("TS_TypeII_6in") && status.contains("RG_Design"),
            "{status}"
        );
        assert_eq!(ed.undo_depth(), depth + 1, "one step");
        assert_eq!(ed.doc.timeseries("TS_TypeII_6in").len(), 289);
        assert_eq!(
            ed.doc.field("RAINGAGES", "RG_Design", "Format"),
            Some("VOLUME")
        );
        assert_eq!(
            ed.doc.field("RAINGAGES", "RG_Design", "Interval"),
            Some("0:05")
        );
        assert_eq!(
            ed.doc.field("SUBCATCHMENTS", "S1", "RainGage"),
            Some("RG_Design")
        );
        assert_eq!(
            ed.doc.field("SUBCATCHMENTS", "S2", "RainGage"),
            Some("RainGage")
        );
        assert!(ed.doc.symbol("RG_Design").is_some(), "placed on the map");
        assert_eq!(ed.selection, vec![ObjRef::Gage("RG_Design".into())]);
        assert!(ed.dialogs.storm.is_none());
        assert!(ed.undo().is_some());
        assert!(!ed.doc.contains("RAINGAGES", "RG_Design"));
    }

    #[test]
    fn every_method_previews_and_bad_input_is_named() {
        let mut d = StormDraft::default();
        for k in Kind::ALL {
            d.kind = k;
            d.pfds = "by duration for ARI (years):,1,2,5,10\n5-min:,0.3,0.4,0.5,0.6\n60-min:,1.0,1.3,1.6,2.0\n24-hr:,2.5,3.0,4.0,5.0\n".into();
            rebuild(&mut d);
            assert!(d.preview.is_some(), "{k:?}: {}", d.error);
        }
        assert_eq!(d.picked_ari, None, "uniform picks nothing");
        d.kind = Kind::PfdsBlock;
        rebuild(&mut d);
        assert_eq!(d.picked_ari, Some(10.0));
        assert!((d.preview.as_ref().unwrap().total_depth() - 5.0).abs() < 1e-9);
        d.step_min = "0".into();
        rebuild(&mut d);
        assert!(d.preview.is_none());
        assert!(d.error.contains("time step"), "{}", d.error);
        d.step_min = "15".into();
        d.kind = Kind::Chicago;
        d.r = "x".into();
        rebuild(&mut d);
        assert!(d.error.starts_with("r:"), "{}", d.error);
    }
}
