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
mod filmstrip;
mod inspector;
mod library;
mod mixer;
mod preview;
mod resolve;
mod scopes;
mod wheels;

use std::path::{Path, PathBuf};

use pe_core::{Document, History, RowIdGenerator, Stack};

use crate::library::Library;
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
    /// Every photograph open at once. The current one's pixels and edit
    /// live in the fields above; the rest are parked here.
    library: Library,
    show_strip: bool,
    /// A grade waiting to be applied to another photograph.
    clipboard: Option<Stack>,
    batch: Option<Batch>,
    /// Which comparison view is running, and where its divider sits.
    compare: Compare,
    /// The wipe position, as a fraction across the picture.
    wipe: f32,
    /// Where the divider was drawn last frame, and whether the drag in
    /// progress grabbed it.
    ///
    /// The divider and the pan share one drag, so which of them gets it has to
    /// be decided once, when the drag starts — otherwise dragging the divider
    /// would slide the photograph out from under it at the same time.
    wipe_x: Option<f32>,
    dragging_wipe: bool,
    /// Whether the scopes panel is open, and which scopes it shows.
    show_scopes: bool,
    shown: scopes::Shown,
    scope_textures: scopes::Textures,
    /// Which inspector page is showing.
    tab: Tab,
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

        // Whatever the window opened with is the set, so the filmstrip and
        // the batch export have something to work with from the first frame.
        let library_paths: Vec<PathBuf> = path.iter().cloned().collect();

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
            library: Library::new(library_paths),
            show_strip: true,
            clipboard: None,
            batch: None,
            compare: Compare::Off,
            wipe: 0.5,
            wipe_x: None,
            dragging_wipe: false,
            show_scopes: false,
            shown: scopes::Shown::default(),
            scope_textures: scopes::Textures::default(),
            tab: Tab::Colour,
            cropping: false,
            last: None,
            last_frame: (1, 1),
        }
    }

    /// Move to a different photograph in the set.
    ///
    /// The outgoing edit is parked whole — history and all — so that clicking
    /// the wrong thumbnail and clicking back does not cost an hour of undo.
    fn select(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.library.len() || index == self.library.current() {
            return;
        }
        let Some(path) = self.library.path(index).map(|p| p.to_path_buf()) else {
            return;
        };
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

        // Swap in a placeholder so the outgoing history can be moved out
        // wholesale rather than cloned; `History` deliberately is not `Clone`,
        // because an undo stack with two owners is a bug waiting to happen.
        let outgoing = std::mem::replace(
            &mut self.history,
            History::new(Document::from_path(String::new())),
        );
        let outgoing_ids = std::mem::take(&mut self.ids);
        let (history, ids) = self.library.switch(index, outgoing, outgoing_ids);
        self.history = history;
        self.ids = ids;

        self.image = image;
        self.path = Some(path);
        self.view.fit();
        self.cropping = false;
        self.last = None;
        self.set_title(ctx);
    }

    /// Decode and upload whatever photograph the library is pointing at.
    fn load_current(&mut self, ctx: &egui::Context) {
        let Some(path) = self
            .library
            .path(self.library.current())
            .map(|p| p.to_path_buf())
        else {
            return;
        };
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
        self.image = image;
        self.path = Some(path);
        self.view.fit();
        self.cropping = false;
        self.last = None;
        self.set_title(ctx);
    }

    /// Take a photograph out of the set.
    ///
    /// The file is untouched. This is a list of what is open, not a folder,
    /// and nothing in this program deletes anything off a disc.
    fn remove_photo(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.library.len() {
            return;
        }
        let was_current = index == self.library.current();
        self.library.remove(index);
        if self.library.is_empty() {
            self.status = "no photos open".into();
            return;
        }
        if was_current {
            // The edit in hand belonged to the photograph just removed, so
            // there is nothing to park — and no index to compare against
            // either, which is why this cannot go through `select`.
            let (history, ids) = self.library.take_current();
            self.history = history;
            self.ids = ids;
            self.load_current(ctx);
        }
        self.set_title(ctx);
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

        let doc = library::load_edit(&path)
            .unwrap_or_else(|| pe_effects::new_document(path.to_string_lossy().to_string()));
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
        let position = if self.library.len() > 1 {
            format!(" [{}/{}]", self.library.current() + 1, self.library.len())
        } else {
            String::new()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{name}{position} — {}x{} — Photo Editor",
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
        if let Some(paths) = dialog.pick_files() {
            self.add_and_show(paths, ctx);
        }
    }

    fn open_folder_dialog(&mut self, ctx: &egui::Context) {
        let start = self.path.as_ref().and_then(|p| p.parent());
        let mut dialog = rfd::FileDialog::new().set_title("Open folder");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        let Some(dir) = dialog.pick_folder() else {
            return;
        };
        let found = Library::scan(&dir);
        if found.is_empty() {
            self.status = format!("no images in {}", dir.display());
            return;
        }
        let n = found.len();
        self.add_and_show(found, ctx);
        self.status = format!("opened {n} photos from {}", dir.display());
    }

    /// Add photographs to the set and move to the first genuinely new one.
    fn add_and_show(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let first_run = self.library.is_empty();
        let Some(index) = self.library.add(paths) else {
            self.status = "already open".into();
            return;
        };
        self.show_strip = true;
        if first_run {
            // The set was empty, so this is index zero and the library is
            // already pointing at it. Nothing to park either: the document the
            // window started with belongs to no photograph.
            let Some(path) = self.library.path(index).map(|p| p.to_path_buf()) else {
                return;
            };
            self.open_image(path, ctx);
        } else {
            self.select(index, ctx);
        }
    }

    /// Start a batch export of every photograph in the set.
    fn batch_export(&mut self) {
        if self.library.is_empty() {
            self.status = "no photos open".into();
            return;
        }
        let start = self.path.as_ref().and_then(|p| p.parent());
        let mut dialog = rfd::FileDialog::new().set_title("Export all to");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        let Some(dir) = dialog.pick_folder() else {
            return;
        };
        self.batch = Some(Batch {
            targets: (0..self.library.len()).collect(),
            next: 0,
            dir,
            done: 0,
            failed: 0,
        });
    }

    /// Export one photograph of a batch. Returns false when there is no more
    /// to do.
    fn batch_step(&mut self) -> bool {
        let Some(batch) = self.batch.as_mut() else {
            return false;
        };
        let Some(&index) = batch.targets.get(batch.next) else {
            let (done, failed) = (batch.done, batch.failed);
            let dir = batch.dir.clone();
            self.batch = None;
            self.status = if failed == 0 {
                format!("exported {done} photos to {}", dir.display())
            } else {
                format!("exported {done} to {}, {failed} failed", dir.display())
            };
            return false;
        };
        batch.next += 1;

        let Some(preview) = self.preview.as_ref() else {
            batch.failed += 1;
            return true;
        };
        let Some(path) = self.library.path(index).map(|p| p.to_path_buf()) else {
            batch.failed += 1;
            return true;
        };
        let out = export_path(&batch.dir, &path);

        // The photograph in hand has its edit in the live history; every other
        // one has it parked, or has none at all and gets the defaults.
        let doc = if index == self.library.current() {
            self.history.document().clone()
        } else {
            match self.library.entries()[index].document() {
                Some(d) => d.clone(),
                None => library::load_edit(&path).unwrap_or_else(|| {
                    pe_effects::new_document(path.to_string_lossy().to_string())
                }),
            }
        };

        // Decoded here rather than held: the whole reason a set is navigable
        // is that only one frame is in memory at a time.
        let image = if index == self.library.current() {
            self.image.clone()
        } else {
            match pe_io::load(&path) {
                Ok(img) => img,
                Err(_) => {
                    if let Some(b) = self.batch.as_mut() {
                        b.failed += 1;
                    }
                    return true;
                }
            }
        };

        let (w, h) = pe_render::export::output_size(&doc, image.width, image.height);
        let result = preview
            .export(&image, &doc)
            .map_err(|e| e.to_string())
            .and_then(|pixels| {
                pe_io::DecodedImage::new(w, h, pixels)
                    .and_then(|img| pe_io::save_jpeg(&img, &out, 95))
                    .map_err(|e| e.to_string())
            });
        if let Some(b) = self.batch.as_mut() {
            match result {
                Ok(()) => b.done += 1,
                Err(_) => b.failed += 1,
            }
        }
        true
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

    /// Write a `.peproj` beside every photograph that has one to write.
    ///
    /// Without this, pasting a grade across fifty photographs would leave
    /// forty-nine of them holding an edit that exists only in memory — which
    /// is a fine way to lose an afternoon to a crash.
    fn save_all_edits(&mut self) {
        let mut written = 0;
        let mut failed = 0;
        let current = self.library.current();

        let mut write = |path: &Path, doc: &Document| match doc
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|json| {
                std::fs::write(path.with_extension("peproj"), json).map_err(|e| e.to_string())
            }) {
            Ok(()) => written += 1,
            Err(_) => failed += 1,
        };

        for (i, entry) in self.library.entries().iter().enumerate() {
            if i == current {
                continue;
            }
            // A photograph nobody has opened and nobody has pasted onto has
            // no edit, and writing a file full of defaults beside it would be
            // noise in the user's folder.
            if let Some(doc) = entry.document() {
                write(&entry.path, doc);
            }
        }
        if let Some(path) = self.path.clone() {
            write(&path, self.history.document());
        }

        self.status = if failed == 0 {
            format!("saved {written} edits")
        } else {
            format!("saved {written} edits, {failed} failed")
        };
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
        // Thumbnails arriving from the worker.
        if self.library.collect(ctx) {
            ctx.request_repaint();
        }
        // One photograph of a batch per frame, so the window keeps drawing and
        // the progress readout means something.
        if self.batch.is_some() {
            self.batch_step();
            ctx.request_repaint();
        }

        let mut selected: Option<usize> = None;
        let mut action: Option<filmstrip::Action> = None;
        let mut open_requested = false;
        let mut open_folder_requested = false;
        let mut stop_batch = false;
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
            if !self.library.is_empty() {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    selected = Some((self.library.current() + 1).min(self.library.len() - 1));
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    selected = Some(self.library.current().saturating_sub(1));
                }
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::F) {
                self.show_strip = !self.show_strip;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::S) {
                self.show_scopes = !self.show_scopes;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::C) {
                self.cropping = !self.cropping;
                self.view.fit();
            }
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::D) {
                self.bypass_all = !self.bypass_all;
            }
            open_requested = i.consume_key(egui::Modifiers::COMMAND, egui::Key::O);
            open_folder_requested = i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::O,
            );
        });
        if open_requested {
            self.open_dialog(ctx);
        }
        if open_folder_requested {
            self.open_folder_dialog(ctx);
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
        if !dropped.is_empty() {
            // A dropped folder is as good as a dropped file, and dropping the
            // folder is what people do when they mean the whole shoot.
            let mut paths = Vec::new();
            for path in dropped {
                if path.is_dir() {
                    paths.extend(Library::scan(&path));
                } else {
                    paths.push(path);
                }
            }
            self.add_and_show(paths, ctx);
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
                if ui
                    .button("Open folder…")
                    .on_hover_text("Ctrl+Shift+O")
                    .clicked()
                {
                    open_folder_requested = true;
                }
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
                ui.add_enabled_ui(self.library.len() > 1, |ui| {
                    if ui
                        .button("Save all")
                        .on_hover_text(
                            "A .peproj beside every photo that has been edited —                              including ones a grade was pasted onto",
                        )
                        .clicked()
                    {
                        self.save_all_edits();
                    }
                });
                ui.separator();
                if ui.button("Export JPEG").clicked() {
                    self.export();
                }
                ui.add_enabled_ui(self.library.len() > 1 && self.batch.is_none(), |ui| {
                    if ui
                        .button("Export all…")
                        .on_hover_text("Every photo in the set, into a folder you choose")
                        .clicked()
                    {
                        self.batch_export();
                    }
                });
                ui.separator();
                ui.add_enabled_ui(self.library.len() > 1, |ui| {
                    if ui.button("Copy grade").clicked() {
                        self.clipboard = Some(self.history.document().stack.clone());
                        self.status = "grade copied".into();
                    }
                });
                ui.add_enabled_ui(self.clipboard.is_some(), |ui| {
                    if ui.button("Paste").clicked()
                        && let Some(stack) = self.clipboard.clone()
                    {
                        self.history
                            .edit("Paste Grade", None, move |doc| doc.stack = stack);
                        self.ids = RowIdGenerator::resuming(self.history.document());
                        self.status = "grade pasted".into();
                    }
                    if ui
                        .button("Paste to all")
                        .on_hover_text(
                            "The grade only — a crop belongs to the frame it was drawn on",
                        )
                        .clicked()
                        && let Some(stack) = self.clipboard.clone()
                    {
                        let n = self.library.paste_stack_to_all(&stack);
                        self.status = format!("grade pasted to {n} photos");
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("Compare").small().weak());
                for (label, mode) in [
                    ("Off", Compare::Off),
                    ("Wipe", Compare::Wipe),
                    ("Side", Compare::Side),
                ] {
                    if ui.selectable_label(self.compare == mode, label).clicked() {
                        self.compare = mode;
                    }
                }
                ui.separator();
                ui.toggle_value(&mut self.show_scopes, "Scopes")
                    .on_hover_text("S — waveform, parade and vectorscope");
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

        if self.show_strip && !self.library.is_empty() {
            // Down the left rather than across the bottom. The window is wider
            // than it is tall and a photograph is not, so a horizontal strip
            // costs height — the dimension the picture is already short of.
            egui::SidePanel::left("filmstrip")
                .exact_width(124.0)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.add_space(2.0);
                    action = filmstrip::strip(ui, &mut self.library);
                });
        }

        if self.show_scopes {
            egui::TopBottomPanel::bottom("scopes")
                .resizable(true)
                .default_height(210.0)
                .height_range(120.0..=420.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.toggle_value(&mut self.shown.waveform, "Waveform");
                        ui.add_enabled_ui(self.shown.waveform, |ui| {
                            ui.toggle_value(&mut self.shown.waveform_rgb, "RGB")
                                .on_hover_text("Overlay the three channels instead of luma");
                        });
                        ui.toggle_value(&mut self.shown.parade, "Parade");
                        ui.toggle_value(&mut self.shown.vectorscope, "Vectorscope");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "measured on the whole photograph, not the visible part",
                                )
                                .small()
                                .weak(),
                            );
                        });
                    });
                    ui.add_space(2.0);
                    scopes::panel(
                        ui,
                        &mut self.scope_textures,
                        self.preview.as_ref().and_then(|p| p.scopes()),
                        &self.shown,
                    );
                });
        }

        if let Some(batch) = self.batch.as_ref() {
            let total = batch.targets.len();
            let left = batch.remaining();
            egui::TopBottomPanel::bottom("batch").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new((total - left) as f32 / total.max(1) as f32)
                            .desired_width(220.0)
                            .text(format!("exporting {} of {total}", total - left)),
                    );
                    if ui.small_button("Stop").clicked() {
                        stop_batch = true;
                    }
                });
            });
        }

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
            .default_width(420.0)
            .width_range(320.0..=640.0)
            .show(ctx, |ui| {
                let name = self
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "test chart".into());
                inspector_header(ui, &name, (self.image.width, self.image.height));
                tab_row(ui, &mut self.tab);
                ui.add_space(6.0);

                egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                    Tab::Colour => {
                        // The curve carries the histogram, so there is one
                        // rather than two, and it is at the top where a
                        // histogram belongs.
                        egui::CollapsingHeader::new("Curves - Custom")
                            .default_open(true)
                            .show(ui, |ui| {
                                let scopes = self.preview.as_ref().and_then(|p| p.scopes());
                                curve::editor(ui, &mut self.history, scopes);
                            });
                        egui::CollapsingHeader::new("Basic")
                            .default_open(true)
                            .show(ui, |ui| {
                                basic::panel(ui, &mut self.history);
                                if ui.small_button("Reset Basic").clicked() {
                                    basic::reset(&mut self.history);
                                }
                            });
                        egui::CollapsingHeader::new("Primaries - Color Wheels")
                            .default_open(true)
                            .show(ui, |ui| {
                                wheels::panel(ui, &mut self.history);
                            });
                        egui::CollapsingHeader::new("Colour Mixer").show(ui, |ui| {
                            mixer::panel(ui, &mut self.history);
                        });
                    }
                    Tab::Effects => {
                        inspector::show(ui, &mut self.history, &mut self.ids);
                    }
                    Tab::Image => {
                        let source = (self.image.width, self.image.height);
                        let was = self.cropping;
                        crop::panel(ui, &mut self.history, source, &mut self.cropping);
                        if was != self.cropping {
                            self.view.fit();
                        }
                    }
                    Tab::File => file_page(ui, self),
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
                if response.drag_started() {
                    self.dragging_wipe = self.compare == Compare::Wipe
                        && self
                            .wipe_x
                            .zip(response.interact_pointer_pos())
                            .is_some_and(|(x, p)| (p.x - x).abs() <= 24.0);
                }
                if response.drag_stopped() {
                    self.dragging_wipe = false;
                }
                if !self.cropping && !self.dragging_wipe {
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
                let compare = self.compare;
                match preview.render(image, &doc, framing_geometry, view, viewport, compare.on()) {
                    Ok(framing) => {
                        self.last_passes = framing.passes;
                        self.last = Some((framing.scale, framing.visible));
                        self.last_frame = framing.frame;
                        let target = draw(ui, rect, &framing);
                        self.wipe_x = draw_compare(ui, rect, &framing, compare, self.wipe);
                        if self.dragging_wipe
                            && response.dragged()
                            && let Some(pos) = response.interact_pointer_pos()
                        {
                            self.wipe =
                                ((pos.x - target.min.x) / target.width().max(1e-4)).clamp(0.0, 1.0);
                        }
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

        if stop_batch && let Some(batch) = self.batch.take() {
            self.status = format!("stopped after {} of {}", batch.done, batch.targets.len());
        }
        match action {
            Some(filmstrip::Action::Show(index)) => selected = Some(index),
            Some(filmstrip::Action::Remove(index)) => self.remove_photo(index, ctx),
            None => {}
        }
        if let Some(index) = selected {
            self.select(index, ctx);
        }
    }
}

/// Which page of the inspector is showing.
///
/// Resolve's tab row, with the tabs a photo editor can honestly fill. Video,
/// Audio and Transition are clip properties that do not exist here, and a tab
/// that opens onto nothing is worse than one that is not there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Colour,
    Effects,
    Image,
    File,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Colour, Tab::Effects, Tab::Image, Tab::File];

    fn label(self) -> &'static str {
        match self {
            Tab::Colour => "Colour",
            Tab::Effects => "Effects",
            Tab::Image => "Image",
            Tab::File => "File",
        }
    }
}

/// The inspector's title bar: what is being edited, and how big it is.
fn inspector_header(ui: &mut egui::Ui, name: &str, size: (u32, u32)) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();

    // A small picture glyph, drawn rather than typed — the bundled fonts have
    // no dingbats, and one icon is not worth shipping a font for.
    let icon = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 6.0, rect.center().y - 8.0),
        egui::vec2(18.0, 16.0),
    );
    painter.rect_stroke(
        icon,
        2.0,
        egui::Stroke::new(1.2_f32, resolve::colour::ICON),
        egui::StrokeKind::Inside,
    );
    painter.circle_filled(
        egui::pos2(icon.min.x + 5.0, icon.min.y + 5.0),
        1.8,
        resolve::colour::ICON,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(icon.min.x + 3.0, icon.max.y - 2.5),
            egui::pos2(icon.min.x + 8.5, icon.min.y + 7.0),
            egui::pos2(icon.max.x - 2.5, icon.max.y - 2.5),
        ],
        resolve::colour::ICON,
        egui::Stroke::NONE,
    ));

    painter.text(
        egui::pos2(icon.max.x + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(14.0),
        resolve::colour::TITLE,
    );
    painter.text(
        egui::pos2(rect.max.x - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{} x {}", size.0, size.1),
        egui::FontId::proportional(11.0),
        resolve::colour::LABEL,
    );
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y),
            egui::pos2(rect.max.x, rect.max.y),
        ],
        egui::Stroke::new(1.0_f32, resolve::colour::RULE),
    );
}

/// One tab's glyph, drawn rather than typed — the bundled fonts have no
/// dingbats, and four icons are not worth shipping a font for.
fn tab_icon(painter: &egui::Painter, at: egui::Pos2, tab: Tab, tint: egui::Color32) {
    let stroke = egui::Stroke::new(1.3_f32, tint);
    match tab {
        // A colour wheel: a ring with a puck off centre.
        Tab::Colour => {
            painter.circle_stroke(at, 6.0, stroke);
            painter.circle_filled(at + egui::vec2(2.4, -1.6), 2.0, tint);
        }
        // A wand throwing sparks, which is the icon Resolve uses.
        Tab::Effects => {
            painter.line_segment(
                [at + egui::vec2(-5.0, 5.0), at + egui::vec2(3.0, -3.0)],
                stroke,
            );
            for (dx, dy, r) in [(4.5, -4.5, 2.0), (1.0, -6.0, 1.2), (6.5, -1.5, 1.2)] {
                painter.circle_filled(at + egui::vec2(dx, dy), r, tint);
            }
        }
        // A frame with a horizon in it.
        Tab::Image => {
            let r = egui::Rect::from_center_size(at, egui::vec2(13.0, 10.0));
            painter.rect_stroke(r, 1.5, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.min.x + 2.0, r.max.y - 2.0),
                    egui::pos2(r.min.x + 6.0, r.min.y + 4.0),
                    egui::pos2(r.max.x - 2.0, r.max.y - 2.0),
                ],
                tint,
                egui::Stroke::NONE,
            ));
        }
        // A sheet with a folded corner.
        Tab::File => {
            let r = egui::Rect::from_center_size(at, egui::vec2(10.0, 12.0));
            painter.add(egui::Shape::closed_line(
                vec![
                    r.left_top(),
                    egui::pos2(r.max.x - 3.5, r.min.y),
                    egui::pos2(r.max.x, r.min.y + 3.5),
                    r.right_bottom(),
                    r.left_bottom(),
                ],
                stroke,
            ));
        }
    }
}

/// Resolve's tab row: an underline under the one you are on, nothing else.
///
/// Drawn before the scroll area, so it stays put while the page under it
/// moves. A tab strip that scrolls away is a tab strip you have to scroll back
/// to in order to leave the page.
fn tab_row(ui: &mut egui::Ui, current: &mut Tab) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::hover());
    let width = rect.width() / Tab::ALL.len() as f32;
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + i as f32 * width, rect.min.y),
            egui::vec2(width, rect.height()),
        );
        let response = ui.interact(cell, ui.id().with(("tab", i)), egui::Sense::click());
        if response.clicked() {
            *current = *tab;
        }
        if !ui.is_rect_visible(cell) {
            continue;
        }
        let active = *current == *tab;
        let tint = if active {
            resolve::colour::TITLE
        } else if response.hovered() {
            resolve::colour::HANDLE
        } else {
            resolve::colour::LABEL
        };
        let painter = ui.painter();
        tab_icon(
            painter,
            egui::pos2(cell.center().x, cell.min.y + 13.0),
            *tab,
            tint,
        );
        painter.text(
            egui::pos2(cell.center().x, cell.max.y - 10.0),
            egui::Align2::CENTER_CENTER,
            tab.label(),
            egui::FontId::proportional(10.5),
            tint,
        );
        if active {
            painter.line_segment(
                [
                    egui::pos2(cell.min.x + 10.0, cell.max.y - 1.0),
                    egui::pos2(cell.max.x - 10.0, cell.max.y - 1.0),
                ],
                egui::Stroke::new(2.0_f32, resolve::colour::ACCENT),
            );
        }
    }
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y),
            egui::pos2(rect.max.x, rect.max.y),
        ],
        egui::Stroke::new(1.0_f32, resolve::colour::RULE),
    );
}

/// The File page: where the photograph came from and what it is.
fn file_page(ui: &mut egui::Ui, app: &App) {
    let rows: Vec<(String, String)> = vec![
        (
            "Name".into(),
            app.path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "test chart".into()),
        ),
        (
            "Folder".into(),
            app.path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
        (
            "Source".into(),
            format!("{} x {}", app.image.width, app.image.height),
        ),
        ("Output".into(), {
            let (w, h) = pe_render::export::output_size(
                app.history.document(),
                app.image.width,
                app.image.height,
            );
            format!("{w} x {h}")
        }),
        (
            "Working space".into(),
            format!(
                "{} in, {} out",
                app.history.document().color.input,
                app.history.document().color.output
            ),
        ),
        (
            "In the set".into(),
            format!(
                "{} of {}",
                app.library.current() + 1,
                app.library.len().max(1)
            ),
        ),
    ];
    // Measured before the row, because inside a horizontal layout the
    // available width is whatever the content asks for — which is the whole
    // problem. A label told to wrap with no width to wrap *to* does not wrap.
    let full = ui.available_width();
    let value_width = (full - resolve::LABEL_WIDTH - 10.0).max(60.0);

    for (label, value) in rows {
        ui.horizontal_top(|ui| {
            ui.add_sized(
                [resolve::LABEL_WIDTH, 18.0],
                egui::Label::new(
                    egui::RichText::new(label)
                        .small()
                        .color(resolve::colour::LABEL),
                ),
            );
            // Wrapped and held to the column. A folder path is as long as it
            // is, and a side panel that grows to fit one takes the space from
            // the picture — the one thing on screen that cannot be scrolled
            // to.
            ui.allocate_ui(egui::vec2(value_width, 0.0), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(value).small().monospace()).wrap());
            });
        });
        ui.add_space(2.0);
    }
}

/// A batch export in progress.
///
/// One photograph per frame, on the main thread. The obvious alternative is a
/// worker, but the GPU work would have to be marshalled back anyway and the
/// window would still need to know how far along it was — so this trades a
/// visible hitch per frame for a progress readout that cannot lie and no
/// second render path to keep in step with the first.
struct Batch {
    targets: Vec<usize>,
    next: usize,
    dir: PathBuf,
    done: usize,
    failed: usize,
}

impl Batch {
    fn remaining(&self) -> usize {
        self.targets.len().saturating_sub(self.next)
    }
}

/// Where a batch writes one photograph.
///
/// Beside the original would overwrite the next run's input the moment someone
/// exports JPEGs into the folder they came from, so a batch always goes
/// somewhere chosen.
fn export_path(dir: &Path, source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());
    dir.join(format!("{stem}.jpg"))
}

/// How the graded picture is being held up against the ungraded one.
///
/// Both modes exist because they answer different questions. A wipe is for
/// "did that move go too far" — the eye reads a discontinuity across a seam far
/// more finely than it reads two pictures a hand's width apart. Side by side is
/// for "which of these do I prefer", where a seam would fuse the two into one
/// image and stop you seeing either.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compare {
    Off,
    Wipe,
    Side,
}

impl Compare {
    fn on(self) -> bool {
        self != Compare::Off
    }
}

/// Draw the before image against the after one.
///
/// Returns the screen rectangle the divider sits in, so the caller can drag it.
fn draw_compare(
    ui: &egui::Ui,
    rect: egui::Rect,
    framing: &Framing,
    mode: Compare,
    wipe: f32,
) -> Option<f32> {
    let before = framing.before?;
    let whole = egui::Rect::from_center_size(rect.center(), framing.size);

    match mode {
        Compare::Off => None,
        Compare::Wipe => {
            // The after image is already drawn underneath; the before is laid
            // over the left of it and clipped, so the two meet on one seam
            // with no gap and no scaling difference between them.
            let x = whole.min.x + wipe.clamp(0.0, 1.0) * whole.width();
            let left = egui::Rect::from_min_max(whole.min, egui::pos2(x, whole.max.y));
            let uv = egui::Rect::from_min_max(
                framing.uv.min,
                egui::pos2(
                    framing.uv.min.x + framing.uv.width() * wipe.clamp(0.0, 1.0),
                    framing.uv.max.y,
                ),
            );
            ui.painter()
                .add(egui::Shape::image(before, left, uv, egui::Color32::WHITE));

            ui.painter().line_segment(
                [egui::pos2(x, whole.min.y), egui::pos2(x, whole.max.y)],
                egui::Stroke::new(1.5_f32, egui::Color32::from_white_alpha(210)),
            );
            label(
                ui,
                egui::pos2(whole.min.x + 8.0, whole.min.y + 8.0),
                "Before",
                egui::Align2::LEFT_TOP,
            );
            label(
                ui,
                egui::pos2(whole.max.x - 8.0, whole.min.y + 8.0),
                "After",
                egui::Align2::RIGHT_TOP,
            );
            Some(x)
        }
        Compare::Side => {
            // Half size each, so both fit where one did.
            let half = framing.size * 0.5;
            let gap = 6.0;
            let left = egui::Rect::from_min_size(
                egui::pos2(
                    rect.center().x - half.x - gap * 0.5,
                    rect.center().y - half.y * 0.5,
                ),
                half,
            );
            let right = egui::Rect::from_min_size(
                egui::pos2(rect.center().x + gap * 0.5, rect.center().y - half.y * 0.5),
                half,
            );
            // Repaint the background over the full-size after image first, or
            // it would still be showing behind the two halves.
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_gray(24));
            for (target, texture) in [(left, before), (right, framing.texture)] {
                ui.painter().add(egui::Shape::image(
                    texture,
                    target,
                    framing.uv,
                    egui::Color32::WHITE,
                ));
            }
            label(
                ui,
                left.left_top() + egui::vec2(6.0, 6.0),
                "Before",
                egui::Align2::LEFT_TOP,
            );
            label(
                ui,
                right.left_top() + egui::vec2(6.0, 6.0),
                "After",
                egui::Align2::LEFT_TOP,
            );
            None
        }
    }
}

fn label(ui: &egui::Ui, at: egui::Pos2, text: &str, align: egui::Align2) {
    let font = egui::FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
    let rect = align.anchor_size(at, galley.size()).expand(3.0);
    ui.painter()
        .rect_filled(rect, 2.0, egui::Color32::from_black_alpha(150));
    ui.painter()
        .text(at, align, text, font, egui::Color32::from_white_alpha(230));
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
