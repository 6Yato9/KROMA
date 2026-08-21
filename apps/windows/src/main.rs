//! The Windows shell.
//!
//! **This crate contains no image processing.** Its entire vocabulary is: read
//! the stack, mutate a parameter, ask `pe-render` for a texture, draw it. The
//! day a convenience function that touches pixels appears in here is the day
//! the Mac port silently becomes a rewrite.
//!
//! W1 of the Windows app plan: the things that block using it at all. Open a
//! photo, save the edit, export. The Lightroom-style Basic panel and the
//! Resolve wheels come next, as pinned rows at the head of the same stack the
//! effects list already uses.

mod basic;
mod crop;
mod curve;
mod inspector;
mod mixer;
mod preview;
mod wheels;

use std::path::PathBuf;

use pe_core::{Document, History, RowIdGenerator};
use pe_render::GpuContext;

use crate::preview::{Framing, Preview, View};

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(PathBuf::from);

    let image = match &path {
        Some(p) => match pe_io::load(p) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("could not open {}: {e}", p.display());
                std::process::exit(1);
            }
        },
        None => {
            eprintln!("no image given, showing the built-in test chart");
            pe_io::test_chart(1600, 1200)
        }
    };

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Photo Editor"),
        ..Default::default()
    };

    eframe::run_native(
        "Photo Editor",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, image, path)))),
    )
}

pub struct App {
    image: pe_io::DecodedImage,
    path: Option<PathBuf>,
    history: History,
    ids: RowIdGenerator,
    preview: Option<Preview>,
    gpu_name: String,

    /// Resolve binds this to Shift-D and you reach for it constantly: flatten
    /// the whole stack for an honest before/after.
    bypass_all: bool,
    /// Passes the last frame executed. Displayed because it is the number that
    /// proves the stage cache works — it should read 1 while a slider is being
    /// dragged, not the stack depth.
    last_passes: usize,
    status: String,
    /// The window title can only be set once a Context exists, so it is
    /// deferred to the first frame rather than done in the constructor.
    titled: bool,
    view: View,
    /// Whether the crop tool is open. It changes what the viewer shows — the
    /// whole straightened frame rather than the cropped result — so it lives
    /// here rather than inside the panel.
    cropping: bool,
    /// Last frame's framing. Turning a drag or a scroll into a move in image
    /// space needs the scale and the visible rectangle, and both are outputs
    /// of rendering — so the interaction uses the previous frame's, which is
    /// one frame stale and entirely imperceptible.
    last: Option<(f32, egui::Rect)>,
    /// Pixel size of the frame that was last drawn. A drag is measured against
    /// the picture on screen, and once the photograph is cropped that is no
    /// longer the size of the file it came from.
    last_frame: (u32, u32),
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        image: pe_io::DecodedImage,
        path: Option<PathBuf>,
    ) -> Self {
        let doc = match &path {
            Some(p) => pe_effects::new_document(p.to_string_lossy().to_string()),
            None => pe_effects::new_document("<test chart>"),
        };

        let (preview, gpu_name) = match cc.wgpu_render_state.as_ref() {
            Some(rs) => {
                let gpu =
                    GpuContext::from_parts(rs.adapter.clone(), rs.device.clone(), rs.queue.clone());
                let name = gpu.describe();
                (Some(Preview::new(gpu, rs.renderer.clone(), &image)), name)
            }
            None => (None, "no wgpu render state".to_string()),
        };

        Self {
            image,
            path,
            history: History::new(doc),
            ids: RowIdGenerator::default(),
            preview,
            gpu_name,
            bypass_all: false,
            last_passes: 0,
            status: String::new(),
            titled: false,
            view: View::default(),
            cropping: false,
            last: None,
            last_frame: (1, 1),
        }
    }

    /// Open a photograph, replacing whatever is loaded.
    ///
    /// The edit is reset rather than carried over. Carrying a grade to a new
    /// image is a real feature — Resolve's "apply grade from" — but it should
    /// be an explicit action, not something that happens silently because you
    /// opened a different file.
    fn open_image(&mut self, path: PathBuf, ctx: &egui::Context) {
        let image = match pe_io::load(&path) {
            Ok(img) => img,
            Err(e) => {
                self.status = format!("could not open {}: {e}", path.display());
                return;
            }
        };

        if let Some(preview) = self.preview.as_mut()
            && let Err(e) = preview.set_source(&image)
        {
            self.status = format!("could not upload {}: {e}", path.display());
            return;
        }

        let doc = pe_effects::new_document(path.to_string_lossy().to_string());
        self.ids = RowIdGenerator::resuming(&doc);
        self.history = History::new(doc);
        self.status = format!("opened {}", path.display());
        self.image = image;
        self.path = Some(path);
        self.view.fit();
        self.cropping = false;
        self.last = None;
        self.set_title(ctx);
    }

    /// Zoom so one image pixel is one screen pixel.
    fn zoom_to_actual_pixels(&mut self) {
        // `scale` is screen pixels per image pixel at the current zoom, so the
        // factor that takes it to 1.0 is what we want.
        if let Some((scale, _)) = self.last {
            self.view.zoom = (self.view.zoom / scale.max(1e-4)).clamp(1.0, 32.0);
        }
    }

    /// Scroll to zoom about the cursor, drag to pan, double-click to fit.
    ///
    /// Zooming keeps the point under the cursor fixed, which is the difference
    /// between a viewer that feels direct and one that feels like it is
    /// fighting you.
    fn handle_view_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect) {
        let Some((scale, visible)) = self.last else {
            return;
        };
        let image = egui::vec2(
            self.last_frame.0.max(1) as f32,
            self.last_frame.1.max(1) as f32,
        );

        if response.double_clicked() {
            self.view.fit();
            return;
        }

        if response.dragged() {
            // Screen points -> image pixels -> frame uv.
            let delta = response.drag_delta() / scale.max(1e-4);
            self.view.centre -= egui::vec2(delta.x / image.x, delta.y / image.y);
        }

        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1
            && let Some(pointer) = response.hover_pos()
        {
            let factor = (scroll * 0.004).exp();
            let new_zoom = (self.view.zoom * factor).clamp(1.0, 32.0);
            let applied = new_zoom / self.view.zoom;
            if (applied - 1.0).abs() > 1e-4 {
                // Where the cursor sits within the visible rectangle, 0..1.
                let frac = egui::vec2(
                    ((pointer.x - rect.min.x) / rect.width().max(1e-4)).clamp(0.0, 1.0),
                    ((pointer.y - rect.min.y) / rect.height().max(1e-4)).clamp(0.0, 1.0),
                );
                // The frame point under the cursor, which must not move.
                let anchor =
                    visible.min + egui::vec2(frac.x * visible.width(), frac.y * visible.height());
                let new_size = egui::vec2(visible.width() / applied, visible.height() / applied);
                self.view.centre = egui::vec2(
                    anchor.x - (frac.x - 0.5) * new_size.x,
                    anchor.y - (frac.y - 0.5) * new_size.y,
                );
                self.view.zoom = new_zoom;
            }
        }
    }

    fn set_title(&self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "test chart".to_string());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{name} — {}x{} — Photo Editor",
            self.image.width, self.image.height
        )));
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let start = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png"])
            .set_title("Open photo");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_image(path, ctx);
        }
    }

    /// Save the edit beside the photo as `<name>.peproj`.
    ///
    /// The stack *is* the document, so this is a few kilobytes of JSON and the
    /// original file is never touched.
    fn save_edit(&mut self) {
        let Some(path) = self.edit_path() else {
            self.status = "open a photo first".into();
            return;
        };
        match self
            .history
            .document()
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
        {
            Ok(()) => self.status = format!("saved {}", path.display()),
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    fn load_edit(&mut self) {
        let Some(path) = self.edit_path() else {
            self.status = "open a photo first".into();
            return;
        };
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| Document::from_json(&json).map_err(|e| e.to_string()));
        match loaded {
            Ok(doc) => {
                self.ids = RowIdGenerator::resuming(&doc);
                self.history = History::new(doc);
                self.status = format!("loaded {}", path.display());
            }
            Err(e) => self.status = format!("load failed: {e}"),
        }
    }

    fn edit_path(&self) -> Option<PathBuf> {
        Some(self.path.as_ref()?.with_extension("peproj"))
    }

    fn export(&mut self) {
        let Some(preview) = self.preview.as_ref() else {
            self.status = "no GPU".into();
            return;
        };
        let out = self
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("export.jpg"))
            .with_extension("edited.jpg");

        // The crop decides how much picture there is and the resize decides
        // how many pixels it comes in; neither is the source's size, and
        // assuming it was would hand the encoder the wrong dimensions for
        // every cropped export.
        let (w, h) = pe_render::export::output_size(
            self.history.document(),
            self.image.width,
            self.image.height,
        );

        match preview.export(&self.image, self.history.document()) {
            Ok(pixels) => {
                let saved = pe_io::DecodedImage::new(w, h, pixels)
                    .and_then(|img| pe_io::save_jpeg(&img, &out, 95));
                self.status = match saved {
                    Ok(()) => format!("exported {} at {w}x{h}", out.display()),
                    Err(e) => format!("export failed: {e}"),
                };
            }
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut open_requested = false;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                self.history.undo();
            }
            if i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) {
                self.history.redo();
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::C) {
                self.cropping = !self.cropping;
                self.view.fit();
            }
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::D) {
                self.bypass_all = !self.bypass_all;
            }
            open_requested = i.consume_key(egui::Modifiers::COMMAND, egui::Key::O);
        });
        if open_requested {
            self.open_dialog(ctx);
        }

        // Drag a photo onto the window. Cheaper for the user than any menu,
        // and the first thing people try.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.open_image(path, ctx);
        }

        if !self.titled {
            self.set_title(ctx);
            self.titled = true;
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
                    self.open_dialog(ctx);
                }
                ui.separator();
                ui.add_enabled_ui(self.history.can_undo(), |ui| {
                    let label = self.history.undo_label().unwrap_or("").to_string();
                    if ui.button("Undo").on_hover_text(label).clicked() {
                        self.history.undo();
                    }
                });
                ui.add_enabled_ui(self.history.can_redo(), |ui| {
                    if ui.button("Redo").clicked() {
                        self.history.redo();
                    }
                });
                ui.separator();
                ui.toggle_value(&mut self.bypass_all, "Bypass all")
                    .on_hover_text("Shift+D — flatten the stack for an honest before/after");
                ui.separator();
                ui.add_enabled_ui(self.path.is_some(), |ui| {
                    if ui
                        .button("Save edit")
                        .on_hover_text("Writes <photo>.peproj beside the original")
                        .clicked()
                    {
                        self.save_edit();
                    }
                    if ui.button("Load edit").clicked() {
                        self.load_edit();
                    }
                });
                ui.separator();
                if ui.button("Export JPEG").clicked() {
                    self.export();
                }
                ui.separator();
                if ui
                    .toggle_value(&mut self.cropping, "Crop")
                    .on_hover_text("C")
                    .clicked()
                {
                    self.view.fit();
                }
                ui.separator();
                ui.add_enabled_ui(!self.view.is_fit(), |ui| {
                    if ui
                        .button("Fit")
                        .on_hover_text("Double-click the image")
                        .clicked()
                    {
                        self.view.fit();
                    }
                });
                if ui.button("100%").clicked() {
                    self.zoom_to_actual_pixels();
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{:.0}%",
                        self.last.map_or(100.0, |l| l.0 * 100.0)
                    ))
                    .monospace(),
                )
                .on_hover_text("Screen pixels per image pixel");
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("{} passes", self.last_passes))
                        .monospace()
                        .strong(),
                )
                .on_hover_text(
                    "GPU passes executed this frame. Dragging one slider in a deep \
                     stack should read 1 — that is the stage cache doing its job.",
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(&self.gpu_name).weak().small());
                });
            });
        });

        if !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(self.status.clone());
                    if ui.small_button("dismiss").clicked() {
                        self.status.clear();
                    }
                });
            });
        }

        egui::SidePanel::right("inspector")
            .default_width(340.0)
            .width_range(300.0..=560.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                basic::histogram(ui, self.preview.as_ref().and_then(|p| p.histogram()));
                ui.add_space(4.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::CollapsingHeader::new("Basic")
                        .default_open(true)
                        .show(ui, |ui| {
                            basic::panel(ui, &mut self.history);
                            if ui.small_button("Reset Basic").clicked() {
                                basic::reset(&mut self.history);
                            }
                        });

                    egui::CollapsingHeader::new("Crop & Size")
                        .default_open(false)
                        .show(ui, |ui| {
                            let source = (self.image.width, self.image.height);
                            let was = self.cropping;
                            crop::panel(ui, &mut self.history, source, &mut self.cropping);
                            if was != self.cropping {
                                self.view.fit();
                            }
                        });

                    egui::CollapsingHeader::new("Tone Curve").show(ui, |ui| {
                        curve::editor(ui, &mut self.history);
                    });

                    egui::CollapsingHeader::new("Colour Wheels").show(ui, |ui| {
                        wheels::panel(ui, &mut self.history);
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    egui::CollapsingHeader::new("Colour Mixer").show(ui, |ui| {
                        mixer::panel(ui, &mut self.history);
                    });

                    egui::CollapsingHeader::new("Effects")
                        .default_open(true)
                        .show(ui, |ui| {
                            inspector::show(ui, &mut self.history, &mut self.ids);
                        });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_gray(24)))
            .show(ctx, |ui| {
                let viewport = ui.available_size();
                if self.preview.is_none() {
                    ui.centered_and_justified(|ui| ui.label("no GPU available"));
                    return;
                }

                // Claim the whole viewport before rendering, so scroll and drag
                // are handled against the same rectangle the image is drawn in.
                let (rect, response) =
                    ui.allocate_exact_size(viewport, egui::Sense::click_and_drag());
                if !self.cropping {
                    self.handle_view_input(ui, &response, rect);
                }

                let doc = if self.bypass_all {
                    // The cheapest honest bypass: render an empty stack. It
                    // costs one frame of invalidation, and toggling back is
                    // free because the row fingerprints have not changed.
                    let mut d = self.history.document().clone();
                    d.stack.rows.clear();
                    d
                } else {
                    self.history.document().clone()
                };

                let source = (self.image.width, self.image.height);
                // The crop tool shows the whole straightened frame so the user
                // can see what is outside the rectangle; everything else shows
                // the cropped result, which is what will be exported.
                let framing_geometry = if self.cropping {
                    doc.geometry.enclosing(source.0, source.1)
                } else {
                    doc.geometry
                };

                let image = &self.image;
                let view = if self.cropping {
                    View::default()
                } else {
                    self.view
                };
                let preview = self.preview.as_mut().expect("checked above");
                match preview.render(image, &doc, framing_geometry, view, viewport) {
                    Ok(framing) => {
                        self.last_passes = framing.passes;
                        self.last = Some((framing.scale, framing.visible));
                        self.last_frame = framing.frame;
                        let target = draw(ui, rect, &framing);
                        if self.cropping
                            && let Some(next) = crop::overlay(
                                ui,
                                &response,
                                target,
                                self.history.document().geometry,
                                source,
                            )
                        {
                            self.history
                                .edit("Crop", Some("crop.drag".into()), move |d| d.geometry = next);
                        }
                        if response.drag_stopped() {
                            self.history.break_coalescing();
                        }
                    }
                    Err(e) => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("render failed: {e}"),
                            egui::FontId::proportional(14.0),
                            ui.visuals().error_fg_color,
                        );
                    }
                }
            });
    }
}

/// Draw the graded texture centred in the viewport.
///
/// The uv rectangle trims the margin that was rendered for the benefit of
/// spatial effects but is not meant to be seen.
fn draw(ui: &egui::Ui, rect: egui::Rect, framing: &Framing) -> egui::Rect {
    let target = egui::Rect::from_center_size(rect.center(), framing.size);
    ui.painter().add(egui::Shape::image(
        framing.texture,
        target,
        framing.uv,
        egui::Color32::WHITE,
    ));
    target
}
