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

mod inspector;
mod preview;

use std::path::PathBuf;

use pe_core::{Document, History, RowIdGenerator};
use pe_render::GpuContext;

use crate::preview::Preview;

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
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        image: pe_io::DecodedImage,
        path: Option<PathBuf>,
    ) -> Self {
        let doc = match &path {
            Some(p) => Document::from_path(p.to_string_lossy().to_string()),
            None => Document::from_path("<test chart>"),
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

        self.history = History::new(Document::from_path(path.to_string_lossy().to_string()));
        self.ids = RowIdGenerator::default();
        self.status = format!("opened {}", path.display());
        self.image = image;
        self.path = Some(path);
        self.set_title(ctx);
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

        match preview.export(&self.image, self.history.document()) {
            Ok(pixels) => {
                let saved = pe_io::DecodedImage::new(self.image.width, self.image.height, pixels)
                    .and_then(|img| pe_io::save_jpeg(&img, &out, 95));
                self.status = match saved {
                    Ok(()) => format!("exported {}", out.display()),
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
                inspector::show(ui, &mut self.history, &mut self.ids);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_gray(24)))
            .show(ctx, |ui| {
                let size = ui.available_size();
                let Some(preview) = self.preview.as_mut() else {
                    ui.centered_and_justified(|ui| ui.label("no GPU available"));
                    return;
                };

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

                match preview.render(&self.image, &doc, size) {
                    Ok((texture, passes)) => {
                        self.last_passes = passes;
                        let aspect = self.image.width as f32 / self.image.height.max(1) as f32;
                        let mut w = size.x;
                        let mut h = w / aspect;
                        if h > size.y {
                            h = size.y;
                            w = h * aspect;
                        }
                        ui.centered_and_justified(|ui| {
                            ui.add(
                                egui::Image::new(egui::load::SizedTexture::new(
                                    texture,
                                    egui::vec2(w, h),
                                ))
                                .fit_to_exact_size(egui::vec2(w, h)),
                            );
                        });
                    }
                    Err(e) => {
                        ui.centered_and_justified(|ui| ui.label(format!("render failed: {e}")));
                    }
                }
            });
    }
}
