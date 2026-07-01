// SPDX-License-Identifier: GPL-3.0-or-later

//! StormSewer Help browser — topics aligned with Hydraflow Storm Sewers documentation.

use eframe::egui::{self, RichText, ScrollArea};

/// Active help topic (mirrors Hydraflow Help → Contents structure).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HelpTopic {
    #[default]
    GettingStarted,
    QuickStart,
    KeyboardShortcuts,
    DesignWorkflow,
    DesignCodes,
    Hydrology,
    Hydraulics,
    InletsHeC22,
    FileIo,
    Reports,
    HydraflowMigration,
    Troubleshooting,
}

impl HelpTopic {
    pub const ALL: [HelpTopic; 12] = [
        HelpTopic::GettingStarted,
        HelpTopic::QuickStart,
        HelpTopic::KeyboardShortcuts,
        HelpTopic::DesignWorkflow,
        HelpTopic::DesignCodes,
        HelpTopic::Hydrology,
        HelpTopic::Hydraulics,
        HelpTopic::InletsHeC22,
        HelpTopic::FileIo,
        HelpTopic::Reports,
        HelpTopic::HydraflowMigration,
        HelpTopic::Troubleshooting,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::GettingStarted => "Getting Started",
            Self::QuickStart => "Quick Start Tutorial",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
            Self::DesignWorkflow => "Design Workflow",
            Self::DesignCodes => "Design Codes & Criteria",
            Self::Hydrology => "Hydrology (Rational Method)",
            Self::Hydraulics => "Hydraulics (Manning & HGL)",
            Self::InletsHeC22 => "Inlet Analysis (HEC-22)",
            Self::FileIo => "File Import & Export",
            Self::Reports => "Reports & Printing",
            Self::HydraflowMigration => "Hydraflow Migration Guide",
            Self::Troubleshooting => "Troubleshooting",
        }
    }
}

/// Help window state.
#[derive(Clone, Debug, Default)]
pub struct HelpState {
    pub open: bool,
    pub topic: HelpTopic,
}

/// Draw the modal Help browser (F1 or Help menu).
pub fn draw_help_window(ctx: &egui::Context, state: &mut HelpState) {
    if !state.open {
        return;
    }

    let mut close = false;
    egui::Window::new("StormSewer Help")
        .collapsible(false)
        .resizable(true)
        .default_width(680.0)
        .default_height(480.0)
        .min_size([500.0, 300.0])
        .max_size([800.0, 600.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut state.open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(180.0);
                    ui.heading("Contents");
                    ui.separator();
                    for topic in HelpTopic::ALL {
                        if ui
                            .selectable_label(state.topic == topic, topic.title())
                            .clicked()
                        {
                            state.topic = topic;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.heading(state.topic.title());
                    ui.separator();
                    ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            draw_topic(ui, state.topic);
                        });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        });

    if close {
        state.open = false;
    }
}

fn draw_topic(ui: &mut egui::Ui, topic: HelpTopic) {
    match topic {
        HelpTopic::GettingStarted => {
            body(ui, "StormSewer is a standalone storm sewer design application. It replaces the Hydraflow Storm Sewers extension workflow with a native desktop program — no AutoCAD or Civil 3D required.");
            heading(ui, "Main Window");
            bullet(ui, "Left panel — Parameters, Tables, and Design Review tabs");
            bullet(ui, "Center — Plan and Profile views with drawing tools");
            bullet(ui, "Right panel — Hydraulic report, sizing, and multi-RP comparison");
            bullet(ui, "Bottom — Inspector for selected structures and pipes");
            heading(ui, "Units");
            body(ui, "StormSewer uses U.S. customary units: feet, inches, acres, cfs, and minutes. IDF intensity is in inches per hour.");
            heading(ui, "Project Files");
            body(ui, "Save your work as a native .ssproj JSON file. You can reopen, share, and version-control project files independently of CAD drawings.");
        }
        HelpTopic::QuickStart => {
            body(ui, "Follow these steps to model a simple trunk line (similar to the Hydraflow Quick Start Tutorial):");
            numbered(ui, 1, "File → New Demo Project to explore a completed example, or New Project for a blank network.");
            numbered(ui, 2, "Set the IDF curve coefficients (a, b, c) and design return period in Parameters.");
            numbered(ui, 3, "Place inlets, junctions, and an outfall using tools 2–4 on the keyboard.");
            numbered(ui, 4, "Draw pipes (tool 5): click the upstream node, then the downstream node.");
            numbered(ui, 5, "Draw catchments (tool 6): click vertices, then close on the first point.");
            numbered(ui, 6, "Press F5 or click Analyze to compute peak flows and the hydraulic grade line.");
            numbered(ui, 7, "Review findings in the Review tab; use Auto-Size Pipes to apply municipal criteria.");
            numbered(ui, 8, "Export DXF, PDF, or HTML reports for submittal packages.");
        }
        HelpTopic::KeyboardShortcuts => {
            shortcut_grid(ui);
        }
        HelpTopic::DesignWorkflow => {
            body(ui, "StormSewer follows the standard storm sewer design sequence used by Hydraflow:");
            numbered(ui, 1, "Set up — project name, IDF curve, design return period, tailwater, and junction losses.");
            numbered(ui, 2, "Build — place structures, draw pipes, assign catchment areas and runoff coefficients.");
            numbered(ui, 3, "Analyze — Rational-method hydrology, Manning pipe hydraulics, HGL backwater.");
            numbered(ui, 4, "Review — velocity, cover, slope, capacity, and size-progression checks.");
            numbered(ui, 5, "Size — auto-size pipes against the standard RCP catalog.");
            numbered(ui, 6, "Deliver — DXF for CAD, PDF/HTML reports, LandXML for Civil 3D exchange.");
            heading(ui, "Modeling Tips");
            bullet(ui, "Start at the downstream outfall and work upstream.");
            bullet(ui, "Assign tributary area (C × A) at inlets or via catchment polygons.");
            bullet(ui, "Re-run analysis after any geometry or hydrology change.");
        }
        HelpTopic::DesignCodes => {
            body(ui, "Design Codes (Hydraflow: Design Codes dialog) control sizing and review thresholds. Configure them in Parameters → Design Codes:");
            bullet(ui, "Min / max velocity (ft/s) — self-cleansing and scour limits");
            bullet(ui, "Max % full — capacity utilization before warning");
            bullet(ui, "Min cover (ft) — rim to pipe crown");
            bullet(ui, "Min slope — flat-pipe warning threshold");
            bullet(ui, "Size progression — warn when a downstream pipe is smaller than upstream");
            body(ui, "Auto-Size Pipes uses the municipal RCP catalog (8\" through 72\") with the same velocity and capacity limits.");
        }
        HelpTopic::Hydrology => {
            body(ui, "Peak flows use the Rational Method: Q = C × i × A, where i comes from the IDF curve at the time of concentration (Tc).");
            heading(ui, "IDF Curve");
            body(ui, "Intensity: i = a / (t + b)^c  (in/hr or mm/hr in SI mode), with t in minutes. Set coefficients from your municipality's rainfall study.");
            heading(ui, "Time of Concentration");
            bullet(ui, "Tools → Tc Calculator — FAA, TR-55 sheet flow (uses project P2 rainfall), and Kirpich");
            bullet(ui, "Inlet Tc — entered per structure or merged from catchment polygons");
            bullet(ui, "Catchment Tc — computed from flow length and slope (Kirpich)");
            bullet(ui, "Min Tc — project-wide floor applied during analysis");
            heading(ui, "Multi-RP Comparison");
            body(ui, "Enable \"Show multi-RP comparison\" to tabulate peak Q at several return periods using parallel IDF curves.");
        }
        HelpTopic::Hydraulics => {
            body(ui, "Pipe hydraulics use Manning's equation for circular conduits with partial-flow depth iteration.");
            heading(ui, "Hydraulic Grade Line");
            bullet(ui, "Backwater computed from the outfall (or tailwater) upstream");
            bullet(ui, "Junction losses applied at structures (project Junction K)");
            bullet(ui, "Normal depth, critical depth, and full-flow capacity reported per pipe");
            heading(ui, "Pipe Sizing");
            body(ui, "When diameters are blank or undersized, Auto-Size selects the smallest standard RCP that meets velocity and capacity criteria at design flow.");
        }
        HelpTopic::InletsHeC22 => {
            body(ui, "Inlet interception follows FHWA HEC-22 methodology (simplified US customary forms). Configure defaults in Parameters → Inlet Analysis (HEC-22):");
            bullet(ui, "Grate on grade — composite gutter capacity");
            bullet(ui, "Curb opening on grade");
            bullet(ui, "Combination (grate + curb)");
            bullet(ui, "Sag grate — weir-controlled low-point capture");
            body(ui, "Select an inlet structure and compare design Q from analysis against inlet capacity. Undersized inlets are flagged in the inlet check panel.");
        }
        HelpTopic::FileIo => {
            heading(ui, "Native Project");
            bullet(ui, "Open / Save .ssproj — full project state including catchments and background");
            heading(ui, "CAD Exchange");
            bullet(ui, "Import / Export DXF — structures, pipes, and catchments with STORMSEWER XDATA");
            bullet(ui, "Import / Export LandXML — Civil 3D pipe network exchange");
            heading(ui, "Background");
            bullet(ui, "Load PNG site plan underlay with opacity and width scaling");
            heading(ui, "Reports");
            bullet(ui, "Export PDF — plan, profile, and tables with design review findings");
            bullet(ui, "Export HTML — KaTeX-formatted engineering report");
        }
        HelpTopic::Reports => {
            body(ui, "Hydraflow provides Print Reports from the Results tab. StormSewer exports formal reports via File → Export PDF Report or Export HTML Report.");
            heading(ui, "PDF Report Contents");
            bullet(ui, "Page 1 — Plan view with structure labels");
            bullet(ui, "Page 2 — Profile with HGL and invert elevations");
            bullet(ui, "Page 3 — Hydraulic tables and design review findings");
            heading(ui, "After Export");
            body(ui, "Use \"Open report after export\" in the export dialog to launch the PDF or HTML in your default viewer.");
            heading(ui, "Custom Reports (MyReport)");
            bullet(ui, "File → Custom Report — choose Municipal, Hydraflow Pipe Table, or Cost templates");
            bullet(ui, "Edit Columns — visual column picker (add, remove, reorder)");
            bullet(ui, "Export Custom CSV or HTML with 23 Hydraflow-style variables");
            bullet(ui, "Save/load .srpt template files");
        }
        HelpTopic::HydraflowMigration => {
            body(ui, "StormSewer implements the core Hydraflow Storm Sewers calculation methods without requiring Civil 3D:");
            heading(ui, "Feature Parity");
            parity_row(ui, "Rational method + IDF", "Yes");
            parity_row(ui, "Manning hydraulics + HGL", "Yes");
            parity_row(ui, "Design codes / review", "Yes");
            parity_row(ui, "HEC-22 inlet analysis", "Yes");
            parity_row(ui, "DXF import/export", "Yes");
            parity_row(ui, "LandXML import/export", "Yes");
            parity_row(ui, "PNG background underlay", "Yes");
            parity_row(ui, "DXF background underlay", "Yes (STM import)");
            parity_row(ui, "PDF/HTML reports", "Yes");
            parity_row(ui, "Undo / redo", "Yes");
            parity_row(ui, "Box / elliptical pipes", "Yes (equiv. diameter)");
            parity_row(ui, "Cost estimation", "Yes");
            parity_row(ui, "TR-55 / FAA Tc calculator", "Yes");
            parity_row(ui, "SI units", "Yes");
            parity_row(ui, "Global pipe editing", "Yes");
            parity_row(ui, "Network diagnostics", "Yes");
            parity_row(ui, "Print report", "Yes (Ctrl+P)");
            parity_row(ui, "Hydraflow .stm import", "Yes (lines, IDF, inlets, DXF bg)");
            parity_row(ui, "STM embedded IDF curves", "Yes");
            parity_row(ui, "STM inlet HEC-22 geometry", "Yes");
            parity_row(ui, "Custom MyReport templates", "Yes (.srpt + editor)");
            heading(ui, "Data Migration");
            bullet(ui, "Export Civil 3D pipe networks as LandXML or DXF, then import into StormSewer.");
            bullet(ui, "Import legacy .stm projects via File → Import Hydraflow STM — IDF curves, inlet lengths, and background DXF are restored automatically.");
            bullet(ui, "STM IDF return-period index maps to 2/5/10/25/50/100-year storms.");
        }
        HelpTopic::Troubleshooting => {
            heading(ui, "Analysis Fails Validation");
            bullet(ui, "Ensure every pipe connects two existing nodes");
            bullet(ui, "Verify the network has exactly one outfall path");
            bullet(ui, "Check that inlets have tributary area or linked catchments");
            heading(ui, "HGL Surcharge Warnings");
            bullet(ui, "Increase pipe diameter or reduce tributary area");
            bullet(ui, "Lower downstream invert or adjust rim elevations");
            heading(ui, "DXF Import Issues");
            bullet(ui, "Structures and pipes must carry STORMSEWER XDATA or standard layer naming");
            bullet(ui, "Re-export from StormSewer to verify round-trip compatibility");
            heading(ui, "Undo");
            body(ui, "Use Edit → Undo (Ctrl+Z) to reverse the last edit. Undo is available for geometry changes, deletions, sizing, and imports.");
        }
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).strong());
}

fn body(ui: &mut egui::Ui, text: &str) {
    ui.label(text);
    ui.add_space(4.0);
}

fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.label(format!("• {text}"));
}

fn numbered(ui: &mut egui::Ui, n: u32, text: &str) {
    ui.label(format!("{n}. {text}"));
}

fn parity_row(ui: &mut egui::Ui, feature: &str, status: &str) {
    ui.horizontal(|ui| {
        ui.label(feature);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(status).small());
        });
    });
}

fn shortcut_grid(ui: &mut egui::Ui) {
    egui::Grid::new("help_shortcuts")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            shortcut_row(ui, "Ctrl+Z", "Undo");
            shortcut_row(ui, "Ctrl+Y", "Redo");
            shortcut_row(ui, "Ctrl+N", "New project");
            shortcut_row(ui, "Ctrl+O", "Open project");
            shortcut_row(ui, "Ctrl+S", "Save project");
            shortcut_row(ui, "Ctrl+A / F5", "Run analysis");
            shortcut_row(ui, "Ctrl+P", "Print report (PDF)");
            shortcut_row(ui, "Delete", "Delete selected node or pipe");
            shortcut_row(ui, "1", "Select tool");
            shortcut_row(ui, "2", "Place inlet");
            shortcut_row(ui, "3", "Place junction");
            shortcut_row(ui, "4", "Place outfall");
            shortcut_row(ui, "5", "Draw pipe");
            shortcut_row(ui, "6", "Draw catchment");
            shortcut_row(ui, "F", "Zoom to extents");
            shortcut_row(ui, "G", "Zoom to selection");
            shortcut_row(ui, "Esc", "Cancel pipe/catchment drawing");
            shortcut_row(ui, "F1", "Show Help");
        });
}

fn shortcut_row(ui: &mut egui::Ui, key: &str, action: &str) {
    ui.label(RichText::new(key).monospace().strong());
    ui.label(action);
    ui.end_row();
}

/// Open Help to a specific topic (from menu items).
pub fn open_help(state: &mut HelpState, topic: HelpTopic) {
    state.open = true;
    state.topic = topic;
}