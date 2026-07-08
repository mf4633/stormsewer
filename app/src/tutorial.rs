// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive first-run tutorial. Walks the user through a full analysis on the
//! live demo network, driving the real app (Analyze, select, review, auto-size,
//! profile) rather than showing static screenshots. It reopens on every launch
//! until the user ticks "Don't show on startup" (persisted in prefs).

use eframe::egui::{self, RichText};

use crate::panels::SideTab;
use crate::state::{AppState, ViewTab};

/// An action a tutorial step can perform on the real app, so the user can follow
/// along even before they know where the control lives.
#[derive(Clone, Copy)]
enum Act {
    Analyze,
    SelectPipe(&'static str),
    ShowReview,
    AutoSize,
    ShowProfile,
    ShowPlan,
}

struct Step {
    title: &'static str,
    body: &'static str,
    action: Option<(&'static str, Act)>,
}

const STEPS: &[Step] = &[
    Step {
        title: "Welcome to StormSewer",
        body: "You're looking at a real, fully analyzed storm-sewer network: three \
               pipes (P1–P3) carrying runoff from two inlets and a junction down to an \
               outfall. This short tutorial walks the whole workflow using this live \
               data. It reopens each launch until you tick \"Don't show on startup\" below.",
        action: None,
    },
    Step {
        title: "1 · The plan view",
        body: "The canvas shows the network in plan. Pipes within capacity are blue; \
               nodes are colored by type — green inlets, a purple junction, an orange \
               outfall. Drag to pan, scroll to zoom, press F to fit. The legend in the \
               top-right corner is your color key.",
        action: Some(("Show plan view", Act::ShowPlan)),
    },
    Step {
        title: "2 · Run the analysis",
        body: "Click Analyze in the toolbar (or press F5) to compute design flows \
               (Rational method), pipe hydraulics (Manning), and the hydraulic grade \
               line. Run it now:",
        action: Some(("Run analysis now", Act::Analyze)),
    },
    Step {
        title: "3 · Read the report",
        body: "The right-hand panel is the hydraulic report — each pipe's flow Q, \
               capacity, percent full, velocity, and HGL, followed by a design review. \
               Notice that pipe P2 is running close to capacity.",
        action: None,
    },
    Step {
        title: "4 · Inspect a pipe",
        body: "Select pipe P2 to see its live hydraulics in the Inspector along the \
               bottom: it carries about 92% of full capacity — the tightest reach in \
               this run. Click to select it:",
        action: Some(("Select pipe P2", Act::SelectPipe("P2"))),
    },
    Step {
        title: "5 · Design review",
        body: "StormSewer checks every pipe against your criteria. P2 exceeds the \
               85%-full guideline, so it's flagged. Open the Review tab to see all \
               findings — click one to jump to it on the plan.",
        action: Some(("Show design review", Act::ShowReview)),
    },
    Step {
        title: "6 · Auto-size the pipes",
        body: "Let StormSewer pick standard diameters that satisfy the velocity and \
               percent-full criteria. This updates the network in place — undo any time \
               with Ctrl+Z.",
        action: Some(("Auto-size now", Act::AutoSize)),
    },
    Step {
        title: "7 · The HGL profile",
        body: "Switch to the Profile view to see the hydraulic grade line as a \
               long-section against ground and pipe invert — the classic storm-sewer \
               profile. Elevation runs up the left axis, station along the bottom.",
        action: Some(("Show profile", Act::ShowProfile)),
    },
    Step {
        title: "You're ready",
        body: "That's the full loop: build or import a network, Analyze, review, \
               Auto-size, and read the profile and report. To use your own data, use \
               File → Import (Hydraflow .STM, LandXML, or DXF) or File → New Project. \
               You can reopen this any time from Help → Interactive Tutorial.",
        action: Some(("Back to plan view", Act::ShowPlan)),
    },
];

/// Per-session tutorial window state.
#[derive(Clone, Debug, Default)]
pub struct TutorialState {
    pub open: bool,
    pub step: usize,
}

/// Draw the floating tutorial (non-modal, so the app stays usable alongside it).
pub fn draw_tutorial(ctx: &egui::Context, state: &mut AppState) {
    if !state.tutorial.open {
        return;
    }
    let n = STEPS.len();
    let step = state.tutorial.step.min(n - 1);
    let s = &STEPS[step];

    let mut window_open = true;
    let (mut go_back, mut go_next, mut finish, mut do_action) = (false, false, false, false);
    let mut dont_show = state.prefs.tutorial_done;

    egui::Window::new(format!("Interactive Tutorial — step {} of {}", step + 1, n))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -48.0])
        .open(&mut window_open)
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(s.title);
            ui.add_space(6.0);
            ui.label(RichText::new(s.body).size(14.0));

            if let Some((label, _)) = s.action {
                ui.add_space(10.0);
                if ui.button(RichText::new(label).strong()).clicked() {
                    do_action = true;
                }
            }

            ui.add_space(12.0);
            ui.add(
                egui::ProgressBar::new((step + 1) as f32 / n as f32)
                    .desired_width(ui.available_width())
                    .text(format!("Step {} of {}", step + 1, n)),
            );
            ui.separator();

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(step > 0, egui::Button::new("< Back"))
                    .clicked()
                {
                    go_back = true;
                }
                if step + 1 < n {
                    if ui.button("Next >").clicked() {
                        go_next = true;
                    }
                } else if ui.button("Finish").clicked() {
                    finish = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut dont_show, "Don't show on startup");
                });
            });
        });

    // Persist the "don't show again" choice as soon as it changes.
    if dont_show != state.prefs.tutorial_done {
        state.prefs.tutorial_done = dont_show;
        state.prefs.save();
    }
    if do_action {
        if let Some((_, act)) = s.action {
            run_action(act, state);
        }
    }
    if go_back && step > 0 {
        state.tutorial.step = step - 1;
    }
    if go_next && step + 1 < n {
        state.tutorial.step = step + 1;
    }
    if finish || !window_open {
        state.tutorial.open = false;
    }
}

fn run_action(act: Act, state: &mut AppState) {
    match act {
        Act::Analyze => state.run_analysis(),
        Act::SelectPipe(id) => {
            state.view_tab = ViewTab::Plan;
            state.select_by_id(id);
        }
        Act::ShowReview => {
            state.update_diagnostics();
            state.side_tab = SideTab::Review;
        }
        Act::AutoSize => state.apply_sizing(),
        Act::ShowProfile => state.view_tab = ViewTab::Profile,
        Act::ShowPlan => state.view_tab = ViewTab::Plan,
    }
}
