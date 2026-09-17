// SPDX-License-Identifier: GPL-3.0-or-later

//! PNG and CSV export for the SWMM results views.
//!
//! egui has no offscreen rasteriser, so a PNG of a view is taken the way the
//! window itself is drawn: a screenshot is requested from the viewport, the
//! reply arrives as an input event on a later frame, and the view's rectangle
//! is cropped out of it. Both renderers (glow and wgpu) answer the request.
//! The view that asked keeps polling for the reply while it is on screen,
//! which it is, because the user just pressed its button.

use std::path::PathBuf;

use eframe::egui::{self, Rect};

/// A screenshot request in flight, and the last export message for the view.
#[derive(Default)]
pub struct ExportState {
    /// The view rectangle (in points) and destination for a requested PNG.
    pending: Option<(Rect, PathBuf)>,
    /// What the last export did, for the view to show.
    pub message: String,
}

impl ExportState {
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Ask for a PNG of `rect`, choosing the file through a dialog.
    pub fn request_png(&mut self, ctx: &egui::Context, rect: Rect, default_name: &str) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };
        self.request_png_to(ctx, rect, path);
    }

    /// Ask for a PNG of `rect` written to `path`, without a dialog.
    pub fn request_png_to(&mut self, ctx: &egui::Context, rect: Rect, path: PathBuf) {
        self.pending = Some((rect, path));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        ctx.request_repaint();
    }

    /// Collect a screenshot reply if one arrived this frame and write the
    /// cropped PNG. Call once per frame from the view that requested it.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some((rect, path)) = self.pending.as_ref() else {
            return;
        };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        let result = crop_to_png(&image, *rect, ppp, path);
        self.message = match result {
            Ok(()) => format!("PNG saved: {}", path.display()),
            Err(e) => format!("PNG export failed: {e}"),
        };
        self.pending = None;
    }
}

/// Crop `rect` (points) out of a full-window capture and write it as PNG.
fn crop_to_png(
    image: &egui::ColorImage,
    rect: Rect,
    pixels_per_point: f32,
    path: &std::path::Path,
) -> Result<(), String> {
    let [w, h] = image.size;
    let x0 = ((rect.left() * pixels_per_point).floor().max(0.0) as usize).min(w);
    let y0 = ((rect.top() * pixels_per_point).floor().max(0.0) as usize).min(h);
    let x1 = ((rect.right() * pixels_per_point).ceil().max(0.0) as usize).min(w);
    let y1 = ((rect.bottom() * pixels_per_point).ceil().max(0.0) as usize).min(h);
    if x1 <= x0 || y1 <= y0 {
        return Err("the view has no size to capture".to_string());
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut buf = image::RgbaImage::new(cw as u32, ch as u32);
    for y in 0..ch {
        for x in 0..cw {
            let c = image.pixels[(y0 + y) * w + (x0 + x)];
            buf.put_pixel(x as u32, y as u32, image::Rgba([c.r(), c.g(), c.b(), 255]));
        }
    }
    buf.save(path).map_err(|e| e.to_string())
}

/// Write `text` to a file chosen through a save dialog. Returns a status line.
pub fn save_csv(default_name: &str, text: &str) -> Option<String> {
    let path = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .set_file_name(default_name)
        .save_file()?;
    Some(match std::fs::write(&path, text) {
        Ok(()) => format!("CSV saved: {}", path.display()),
        Err(e) => format!("CSV export failed: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_the_view_out_of_a_window_capture() {
        // A 10×10 capture at 2 px/pt; the view is the 2×2-point square at (1,1).
        let mut image = egui::ColorImage::new([10, 10], egui::Color32::BLACK);
        for y in 2..6 {
            for x in 2..6 {
                image.pixels[y * 10 + x] = egui::Color32::WHITE;
            }
        }
        let dir = std::env::temp_dir().join("stormsewer-app-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crop.png");
        let rect = Rect::from_min_size(egui::pos2(1.0, 1.0), egui::vec2(2.0, 2.0));
        crop_to_png(&image, rect, 2.0, &path).unwrap();
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (4, 4));
        assert!(back.pixels().all(|p| p.0 == [255, 255, 255, 255]));

        let empty = Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(2.0, 2.0));
        assert!(crop_to_png(&image, empty, 2.0, &path).is_err());
    }

    #[test]
    fn a_request_is_pending_until_a_reply_arrives() {
        let ctx = egui::Context::default();
        let mut s = ExportState::default();
        assert!(!s.is_pending());
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(4.0, 4.0));
        let path = std::env::temp_dir().join("stormsewer-app-tests/pending.png");
        let _ = ctx.run(Default::default(), |ctx| {
            s.request_png_to(ctx, rect, path.clone());
            s.poll(ctx);
        });
        assert!(s.is_pending(), "no screenshot event was delivered");
        // Deliver one.
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            image: std::sync::Arc::new(egui::ColorImage::new([8, 8], egui::Color32::RED)),
        });
        let _ = ctx.run(input, |ctx| s.poll(ctx));
        assert!(!s.is_pending());
        assert!(s.message.starts_with("PNG saved"), "{}", s.message);
    }
}
