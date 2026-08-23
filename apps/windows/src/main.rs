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
mod locus;
mod mixer;
mod preview;
mod resolve;
mod scopes;
mod settings;
mod theme;
mod warper;
mod wheels;

use std::path::{Path, PathBuf};

use pe_core::{Document, History, RowIdGenerator, Stack};
use pe_session::export::{export_name, same_file, unclaimed_export_path};
use pe_session::{Support, autosave};

use crate::library::Library;
use pe_render::GpuContext;

use crate::preview::{Framing, Preview, View};

/// How wide the toolbar's menus open.
///
/// Fixed rather than fitted to the longest item, because the items are enabled
/// and disabled as the session goes on: a menu sized to its contents would be
/// a different width every time you opened it, and a menu that moves is a menu
/// you have to read instead of aim at.
const MENU_W: f32 = 190.0;

fn main() -> eframe::Result {
    // Everything after the executable name, so several photographs can be
    // opened at once from a shell or a "send to".
    let mut paths: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let mut index = 0;
    if paths.is_empty() {
        // Nothing asked for, so reopen what the last session had. Opening on
        // the test chart when the user has a folder of photographs they were
        // halfway through is a worse guess than any.
        let (saved, at) = settings::Settings::load().session();
        paths = saved;
        index = at;
    }
    let (image, path, trouble) = open_something(&paths, &mut index);

    // eframe asks the GPU for a texture limit of 8192 on a side, which is a 4K
    // display with room over and a camera from about 2015. It is not a
    // photograph: a 45-megapixel frame is 8256 across and a stitched panorama
    // is several times that. Past the limit wgpu refuses the texture as a
    // validation error, and its default answer to one of those is to end the
    // process — so the window used to vanish rather than the photograph being
    // turned away. Desktop GPUs report 16384, so ask for what is really there.
    let mut wgpu_options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        let eframes = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            // Everything else eframe decided stands. Only the one limit that
            // is about pictures rather than about screens is ours to raise,
            // and asking for the adapter's own figure can never be refused.
            let base = eframes(adapter);
            let mut limits = base.required_limits.clone();
            limits.max_texture_dimension_2d = adapter.limits().max_texture_dimension_2d;
            wgpu::DeviceDescriptor {
                required_limits: limits,
                ..base
            }
        });
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("KROMA"),
        ..Default::default()
    };

    eframe::run_native(
        "KROMA",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, image, path, paths, index, trouble)))),
    )
}

/// Open the photograph the session wanted, or the next one that will open, or
/// the test chart.
///
/// The window opens either way. This used to print to stderr and exit, which is
/// a defensible answer to a bad file named on a command line and no answer at
/// all to the restored session — nobody asked for that photograph, nobody sees
/// stderr when the application was started from a folder, and the only symptom
/// is a window that never appears. One unreadable file in a remembered session
/// would keep the application from ever starting again, with no way to say so
/// from inside it and no way out but deleting a settings file.
fn open_something(
    paths: &[PathBuf],
    index: &mut usize,
) -> (pe_io::DecodedImage, Option<PathBuf>, Option<String>) {
    if paths.is_empty() {
        return (pe_io::test_chart(1600, 1200), None, None);
    }

    // The one asked for first, then the rest of the set in order. A set of
    // sixty where the first is corrupt should open on the second, not on a
    // test chart.
    let order = std::iter::once(*index).chain(0..paths.len());
    let mut first_failure = None;
    for candidate in order {
        let Some(path) = paths.get(candidate) else {
            continue;
        };
        match pe_io::load(path) {
            Ok(image) => {
                let trouble = first_failure.map(|(p, e): (&PathBuf, String)| {
                    format!("could not open {}: {e}", p.display())
                });
                *index = candidate;
                return (image, Some(path.clone()), trouble);
            }
            Err(e) => {
                if first_failure.is_none() {
                    first_failure = Some((path, e.to_string()));
                }
            }
        }
    }

    let trouble = first_failure.map(|(p, e)| {
        format!(
            "could not open {} ({e}) — showing the test chart",
            p.display()
        )
    });
    (pe_io::test_chart(1600, 1200), None, trouble)
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
    /// The last thing the application said, and whether it went well.
    status: Status,
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
    /// What the application remembers between runs: starred effects, and
    /// whatever joins that list later.
    settings: settings::Settings,
    /// Where this platform keeps what belongs to the application. The one
    /// place a `cfg!` about directories is correct: the shell knows what
    /// platform it is.
    support: Support,
    /// Decides when the work in progress is written out. See `autosave`.
    autosave: autosave::Watcher,
    /// The effect under the pointer in the browser, previewed on the picture
    /// for as long as the pointer is there.
    preview_effect: Option<&'static str>,
    /// The effect being dragged out of the browser. Held here rather than in
    /// the panel because the drop can land on the picture, which the panel
    /// cannot see.
    dragging_effect: Option<&'static str>,
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
        session: Vec<PathBuf>,
        at: usize,
        trouble: Option<String>,
    ) -> Self {
        // The scheme, before a single frame is drawn with it.
        theme::apply(&cc.egui_ctx);

        let doc = match &path {
            Some(p) => library::fresh_document(p, image.space),
            None => pe_effects::new_document("<test chart>"),
        };

        let (preview, gpu_name, gpu_trouble) = match cc.wgpu_render_state.as_ref() {
            Some(rs) => {
                let gpu =
                    GpuContext::from_parts(rs.adapter.clone(), rs.device.clone(), rs.queue.clone());
                let name = gpu.describe();
                match Preview::new(gpu, rs.renderer.clone(), &image) {
                    Ok(preview) => (Some(preview), name, None),
                    // The window still opens. There is no picture in it, but
                    // there is a sentence saying why, which is a great deal
                    // better than a process that went away before it drew its
                    // first frame.
                    Err(e) => (None, name, Some(e.to_string())),
                }
            }
            None => (None, "no wgpu render state".to_string(), None),
        };
        let mut status = Status::default();
        // Whichever went wrong; the GPU one is the more serious if both did.
        if let Some(trouble) = gpu_trouble.or(trouble) {
            status.problem(trouble);
        }

        // Whatever the window opened with is the set, so the filmstrip and
        // the batch export have something to work with from the first frame.
        // That is the whole restored session when there was one, not just the
        // photograph being shown.
        let library_paths: Vec<PathBuf> = if session.is_empty() {
            path.iter().cloned().collect()
        } else {
            session
        };
        let support = platform_support();
        let mut library = Library::new(library_paths, support.clone());
        library.focus(at);
        let show_strip = library.len() > 1;

        Self {
            image,
            path,
            ids: ids_for(&doc),
            history: History::new(doc),
            preview,
            gpu_name,
            bypass_all: false,
            last_passes: 0,
            status,
            titled: false,
            view: View::default(),
            library,
            show_strip,
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
            settings: settings::Settings::load(),
            support,
            autosave: autosave::Watcher::new(),
            preview_effect: None,
            dragging_effect: None,
            cropping: false,
            last: None,
            last_frame: (1, 1),
        }
    }

    /// Move to a different photograph in the set.
    ///
    /// The outgoing edit is parked whole — history and all — so that clicking
    /// the wrong thumbnail and clicking back does not cost an hour of undo.
    /// Write down what is open, so the next launch can reopen it.
    ///
    /// Called from every path that changes the set or the selection rather
    /// than once at exit: a window can be closed by the operating system, by
    /// a crash, or by a user who does not think of "which photo I was on" as
    /// something that needs saving.
    fn remember_session(&mut self) {
        let paths = self.library.paths();
        let index = self.library.current();
        self.settings.remember_session(&paths, index);
    }

    /// Write the work in progress now, throttle or no throttle.
    ///
    /// The timer exists so that a slider drag is one write rather than sixty.
    /// It is the right answer while editing and the wrong one at every moment
    /// where the thing being edited is about to stop being what is in front of
    /// you — and those moments are exactly when the last edit is the one most
    /// likely to be lost.
    fn flush_autosave(&mut self) {
        if let Some(path) = self.path.clone()
            && self.autosave.pending()
        {
            autosave::store(&self.support, &path, self.history.document());
        }
    }

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
                self.status
                    .problem(format!("could not open {}: {e}", path.display()));
                return;
            }
        };
        if let Some(preview) = self.preview.as_mut()
            && let Err(e) = preview.set_source(&image)
        {
            self.status
                .problem(format!("could not upload {}: {e}", path.display()));
            return;
        }

        // Anything unwritten goes out before the photograph does. The
        // throttle is beside the point here: the thing that would have
        // triggered the write is about to stop being the thing on screen.
        self.flush_autosave();

        // Swap in a placeholder so the outgoing history can be moved out
        // wholesale rather than cloned; `History` deliberately is not `Clone`,
        // because an undo stack with two owners is a bug waiting to happen.
        let outgoing = std::mem::replace(
            &mut self.history,
            History::new(Document::from_path(String::new())),
        );
        let outgoing_ids = std::mem::take(&mut self.ids);
        let (history, ids) = self
            .library
            .switch(index, outgoing, outgoing_ids, image.space);
        self.history = history;
        self.ids = ids;

        self.image = image;
        self.path = Some(path);
        self.view.fit();
        self.cropping = false;
        self.last = None;
        self.autosave.reset(self.history.revision());
        self.set_title(ctx);
        self.remember_session();
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
                self.status
                    .problem(format!("could not open {}: {e}", path.display()));
                return;
            }
        };
        if let Some(preview) = self.preview.as_mut()
            && let Err(e) = preview.set_source(&image)
        {
            self.status
                .problem(format!("could not upload {}: {e}", path.display()));
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
        // Its edit goes out first. Taking a photograph out of the set is not
        // "throw away what I did to it" — the file is untouched, and if it is
        // opened again the work should still be there.
        if was_current {
            self.flush_autosave();
        }
        self.library.remove(index);
        self.remember_session();
        if self.library.is_empty() {
            self.status.problem("no photos open");
            return;
        }
        if was_current {
            // The edit in hand belonged to the photograph just removed, so
            // there is nothing to park — and no index to compare against
            // either, which is why this cannot go through `select`.
            let (history, ids) = self.library.take_current(None);
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
                self.status
                    .problem(format!("could not open {}: {e}", path.display()));
                return;
            }
        };

        if let Some(preview) = self.preview.as_mut()
            && let Err(e) = preview.set_source(&image)
        {
            self.status
                .problem(format!("could not upload {}: {e}", path.display()));
            return;
        }

        let doc = library::load_edit(&self.support, &path)
            .unwrap_or_else(|| library::fresh_document(&path, image.space));
        self.ids = RowIdGenerator::resuming(&doc);
        self.history = History::new(doc);
        self.status.done(format!("opened {}", path.display()));
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
            self.view.zoom =
                (self.view.zoom / scale.max(1e-4)).clamp(preview::MIN_ZOOM, preview::MAX_ZOOM);
        }
    }

    /// Scroll to zoom about the cursor, drag to pan, double-click to fit.
    ///
    /// Zooming keeps the point under the cursor fixed, which is the difference
    /// between a viewer that feels direct and one that feels like it is
    /// fighting you.
    fn handle_view_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        pan: bool,
    ) {
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

        if pan && response.dragged() {
            // Screen points -> image pixels -> frame uv.
            let delta = response.drag_delta() / scale.max(1e-4);
            self.view.centre -= egui::vec2(delta.x / image.x, delta.y / image.y);
        }

        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1
            && let Some(pointer) = response.hover_pos()
        {
            let factor = (scroll * 0.004).exp();
            let new_zoom = (self.view.zoom * factor).clamp(preview::MIN_ZOOM, preview::MAX_ZOOM);
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
            "{name}{position} — {}x{} — KROMA",
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
            self.status
                .problem(format!("no images in {}", dir.display()));
            return;
        }
        let n = found.len();
        self.add_and_show(found, ctx);
        self.status
            .done(format!("opened {n} photos from {}", dir.display()));
    }

    /// Add photographs to the set and move to the first genuinely new one.
    fn add_and_show(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let first_run = self.library.is_empty();
        let Some(index) = self.library.add(paths) else {
            self.status.done("already open");
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
        // `select` records it too, but the first-run branch does not go
        // through `select` — and that is precisely the branch where the set
        // was empty and has just become worth remembering.
        self.remember_session();
    }

    /// Start a batch export of every photograph in the set.
    fn batch_export(&mut self) {
        if self.library.is_empty() {
            self.status.problem("no photos open");
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
            targets: self
                .library
                .paths()
                .iter()
                .map(|p| p.to_path_buf())
                .collect(),
            next: 0,
            dir,
            done: 0,
            failed: 0,
            export: self.settings.export,
            taken: std::collections::HashSet::new(),
        });
    }

    /// Export one photograph of a batch. Returns false when there is no more
    /// to do.
    fn batch_step(&mut self) -> bool {
        let Some(batch) = self.batch.as_mut() else {
            return false;
        };
        let Some(path) = batch.targets.get(batch.next).cloned() else {
            let (done, failed) = (batch.done, batch.failed);
            let dir = batch.dir.clone();
            self.batch = None;
            if failed == 0 {
                self.status
                    .done(format!("exported {done} photos to {}", dir.display()));
            } else {
                self.status.problem(format!(
                    "exported {done} to {}, {failed} failed",
                    dir.display()
                ));
            }
            return false;
        };
        batch.next += 1;

        let Some(preview) = self.preview.as_ref() else {
            batch.failed += 1;
            return true;
        };
        // Where it sits *now*, which is not where it sat when the run started
        // and may be nowhere at all — a photograph taken out of the set part
        // way through is still on disc and still worth exporting.
        let index = self.library.index_of(&path);
        let chosen = batch.export;
        let dir = batch.dir.clone();
        let Some(b) = self.batch.as_mut() else {
            return false;
        };
        let out = unclaimed_export_path(&dir, &path, chosen.format, &mut b.taken);
        if self.would_overwrite_a_source(&out) {
            // Counted as a failure rather than stopping the run: one collision
            // should not abandon the other sixty-five, and the summary at the
            // end says how many did not make it.
            if let Some(b) = self.batch.as_mut() {
                b.failed += 1;
            }
            return true;
        }

        // Decoded here rather than held: the whole reason a set is navigable
        // is that only one frame is in memory at a time.
        //
        // Before the document, not after, because a photograph that has never
        // been opened has no document yet and the file is the only thing that
        // can say what colour space it is in.
        let in_hand = index == Some(self.library.current());
        let image = if in_hand {
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

        // The photograph in hand has its edit in the live history; every other
        // one has it parked, or has none at all and gets the defaults.
        let doc = if in_hand {
            self.history.document().clone()
        } else {
            match index.and_then(|i| self.library.entries()[i].document()) {
                Some(d) => d.clone(),
                None => library::load_edit(&self.support, &path)
                    .unwrap_or_else(|| library::fresh_document(&path, image.space)),
            }
        };

        let result = write_export(preview, &image, &doc, &out, chosen);
        if let Some(b) = self.batch.as_mut() {
            match result {
                Ok(_) => b.done += 1,
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
            self.status.problem("open a photo first");
            return;
        };
        // A sidecar could only collide with a photograph if one were named
        // `something.peproj`, which is close to impossible — and checked
        // anyway, because "never write over an original" is worth being a rule
        // rather than a set of places the rule happens to hold.
        if self.would_overwrite_a_source(&path) {
            self.status.problem(format!(
                "refused: {} is one of your photographs",
                path.display()
            ));
            return;
        }
        match self
            .history
            .document()
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|json| {
                pe_io::write_bytes_atomically(&path, json.as_bytes()).map_err(|e| e.to_string())
            }) {
            Ok(()) => self.status.done(format!("saved {}", path.display())),
            Err(e) => self.status.problem(format!("save failed: {e}")),
        }
    }

    /// Throw away the edit and the work saved for it.
    ///
    /// Undoable in the ordinary way, which matters: it is one click next to
    /// two other buttons, and the first thing anybody does after pressing it
    /// by mistake is reach for Ctrl-Z.
    fn revert(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status.problem("open a photo first");
            return;
        };
        // Through the same helper every other fresh document goes through, so
        // reverting returns the photograph to how it opened rather than to a
        // guess. Built by hand, this threw away what the file said its colour
        // space was and reset it to sRGB — undoing a correction the user never
        // made and cannot see was made.
        let fresh = library::fresh_document(&path, self.image.space);
        self.history.edit("Revert", None, move |doc| *doc = fresh);
        self.ids = RowIdGenerator::resuming(self.history.document());
        autosave::forget(&self.support, &path);
        self.autosave.reset(self.history.revision());
        self.status.done(format!("reverted {}", path.display()));
    }

    fn load_edit(&mut self) {
        let Some(path) = self.edit_path() else {
            self.status.problem("open a photo first");
            return;
        };
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| Document::from_json(&json).map_err(|e| e.to_string()));
        match loaded {
            Ok(doc) => {
                self.ids = RowIdGenerator::resuming(&doc);
                self.history = History::new(doc);
                self.autosave.reset(self.history.revision());
                self.status.done(format!("loaded {}", path.display()));
            }
            Err(e) => self.status.problem(format!("load failed: {e}")),
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

        // Collected first, so the closure below can check against them
        // without borrowing `self` while it also borrows the library.
        let sources: Vec<PathBuf> = self
            .library
            .paths()
            .iter()
            .map(|p| p.to_path_buf())
            .collect();
        let mut write = |path: &Path, doc: &Document| {
            let out = path.with_extension("peproj");
            if sources.iter().any(|p| same_file(p, &out)) {
                failed += 1;
                return;
            }
            match doc.to_json().map_err(|e| e.to_string()).and_then(|json| {
                pe_io::write_bytes_atomically(&out, json.as_bytes()).map_err(|e| e.to_string())
            }) {
                Ok(()) => written += 1,
                Err(_) => failed += 1,
            }
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

        if failed == 0 {
            self.status.done(format!("saved {written} edits"));
        } else {
            self.status
                .problem(format!("saved {written} edits, {failed} failed"));
        }
    }

    fn edit_path(&self) -> Option<PathBuf> {
        Some(self.path.as_ref()?.with_extension("peproj"))
    }

    /// Whether writing here would land on a photograph we were given.
    ///
    /// Checked against every photograph in the set, not only the one on
    /// screen: a batch export writes into one folder, and the name it builds
    /// for photo A can collide with photo B sitting right beside it.
    ///
    /// This is a hard refusal rather than a warning. The application is
    /// allowed to be annoying about this exactly once — losing somebody's
    /// original is not a thing to recover from, and there is no undo that
    /// reaches outside the process.
    fn would_overwrite_a_source(&self, out: &Path) -> bool {
        let open: Vec<PathBuf> = self
            .path
            .iter()
            .cloned()
            .chain(self.library.paths().iter().map(|p| p.to_path_buf()))
            .collect();
        pe_session::export::would_overwrite_a_source(&open, out)
    }

    fn export(&mut self) {
        let Some(preview) = self.preview.as_ref() else {
            self.status.problem("no GPU");
            return;
        };
        let source = self.path.clone().unwrap_or_else(|| PathBuf::from("export"));
        let chosen = self.settings.export;
        let out = source.with_file_name(export_name(&source, chosen.format));
        if self.would_overwrite_a_source(&out) {
            self.status.problem(format!(
                "refused: {} is one of your photographs",
                out.display()
            ));
            return;
        }

        match write_export(preview, &self.image, self.history.document(), &out, chosen) {
            Ok((w, h)) => self
                .status
                .done(format!("exported {} at {w}x{h}", out.display())),
            Err(e) => self.status.problem(format!("export failed: {e}")),
        }
    }
}

impl eframe::App for App {
    /// The last thing that happens, and what makes closing the window free.
    ///
    /// The autosave writes a moment after you stop moving, which leaves a gap
    /// under a second wide. It is a small gap and it is precisely the one
    /// somebody falls into: the last thing you do before closing a window is
    /// the last thing you did, and it is the edit most likely to be lost.
    ///
    /// Belt and braces with the throttle, not a replacement for it. This does
    /// not run if the process is killed or the machine loses power, which is
    /// what the atomic write and the nine-hundred-millisecond timer are for.
    fn on_exit(&mut self) {
        self.flush_autosave();
    }

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
        let mut save_requested = false;
        let mut export_requested = false;
        let mut stop_batch = false;
        // A parameter's number box is a text field while you are typing in it,
        // and it wants the same keys these shortcuts do: the arrows move the
        // caret, Ctrl+Z takes back a digit. Shortcuts are read here, at the top
        // of the frame and before a single widget is drawn, so the field never
        // gets the chance to swallow them first — it has to be asked whether it
        // is busy. Without this, correcting a typed value with the left arrow
        // changes which photograph you are looking at.
        //
        // Opening is deliberately left outside the guard. No text field claims
        // Ctrl+O, and it is the one shortcut somebody might reach for while a
        // field happens to still have focus.
        // A parameter's number box is a text field while you are typing in it,
        // and it wants the same keys these shortcuts do: the arrows move the
        // caret, Ctrl+Z takes back a digit, and the bare letters are letters.
        // Shortcuts are read here, at the top of the frame and before a single
        // widget is drawn, so the field never gets the chance to swallow them
        // first — it has to be asked whether it is busy. Without this, nudging
        // a typed value with the left arrow changes which photograph you are
        // looking at and throws away what you were typing.
        let typing = ctx.wants_keyboard_input();
        ctx.input_mut(|i| {
            // Opening is deliberately outside the guard: no text field claims
            // Ctrl+O, and it is the one shortcut somebody might reach for while
            // a field still happens to have focus.
            open_requested = i.consume_key(egui::Modifiers::COMMAND, egui::Key::O);
            open_folder_requested = i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::O,
            );
            if typing {
                return;
            }

            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                self.history.undo();
            }
            if i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) {
                self.history.redo();
            }
            // Saving and exporting went into menus, which cost them a click
            // each. A shortcut hands it back to whoever reaches for them often,
            // and the menu items name the keys beside them.
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                save_requested = true;
            }
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::E) {
                export_requested = true;
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
            }
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::D) {
                self.bypass_all = !self.bypass_all;
            }
        });
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

        // Work in progress, written out a moment after you stop moving.
        //
        // On a timer rather than on every change: a slider drag would
        // otherwise be sixty writes a second, and on a photo directory over a
        // network that is not a small thing. A pause of under a second is what
        // counts as stopping, which makes closing the window a decision about
        // nothing.
        if let Some(path) = self.path.clone()
            && self
                .autosave
                .tick(self.history.revision(), std::time::Instant::now())
        {
            autosave::store(&self.support, &path, self.history.document());
        }
        // Kept awake so the write happens even if nothing else is moving. A
        // repaint a second after the last edit is not a cost worth measuring,
        // and without it an idle window would sit on unsaved work until
        // something else woke it.
        if self.autosave.pending() {
            ctx.request_repaint_after(autosave::IDLE);
        }

        if !self.titled {
            self.set_title(ctx);
            self.titled = true;
            // Once, on the first frame. Opening from a command line or a file
            // association reaches none of the paths that record the set, and
            // "the photograph I opened it on" is exactly what the next launch
            // should come back to.
            self.remember_session();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Ten buttons across the top were ten things to read every
                // time you wanted one of them. A menu is one word to read and
                // a list you only look at once you have already decided to —
                // and the decision is nearly always "the photo", "the file" or
                // "the grade", which is what these three are.
                ui.menu_button("File", |ui| {
                    ui.set_min_width(MENU_W);
                    if ui
                        .add(egui::Button::new("Open…").shortcut_text("Ctrl+O"))
                        .clicked()
                    {
                        open_requested = true;
                    }
                    if ui
                        .add(egui::Button::new("Open folder…").shortcut_text("Ctrl+Shift+O"))
                        .clicked()
                    {
                        open_folder_requested = true;
                    }
                    ui.separator();
                    ui.add_enabled_ui(self.path.is_some(), |ui| {
                        if ui
                            .add(egui::Button::new("Save edit").shortcut_text("Ctrl+S"))
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
                                "A .peproj beside every photo that has been edited —                                  including ones a grade was pasted onto",
                            )
                            .clicked()
                        {
                            self.save_all_edits();
                        }
                    });
                    ui.separator();
                    // The counterpart to saving without being asked. An edit
                    // that comes back every time you open a photograph, with
                    // no way to be rid of it, is not a convenience — it is a
                    // photograph you can no longer see.
                    ui.add_enabled_ui(self.path.is_some(), |ui| {
                        if ui
                            .button("Revert")
                            .on_hover_text(
                                "Back to the photograph as it was, and forget the autosave",
                            )
                            .clicked()
                        {
                            self.revert();
                        }
                    });
                });
                ui.menu_button("Export", |ui| {
                    ui.set_min_width(MENU_W);
                    // Named for what it does, not for a format: which format
                    // is a setting on the File page now, and a menu item that
                    // says JPEG while the panel says PNG is a menu item that
                    // lies.
                    let writes = format!(
                        "Beside the original, named <photo>_KROMA.{}",
                        self.settings.export.format.extension()
                    );
                    if ui
                        .add(egui::Button::new("Export").shortcut_text("Ctrl+E"))
                        .on_hover_text(writes)
                        .clicked()
                    {
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
                });
                ui.menu_button("Grade", |ui| {
                    ui.set_min_width(MENU_W);
                    ui.add_enabled_ui(self.library.len() > 1, |ui| {
                        if ui
                            .button("Copy")
                            .on_hover_text("The whole stack, to put on another photo")
                            .clicked()
                        {
                            self.clipboard = Some(self.history.document().stack.clone());
                            self.status.done("grade copied");
                        }
                    });
                    ui.add_enabled_ui(self.clipboard.is_some(), |ui| {
                        if ui.button("Paste").clicked()
                            && let Some(stack) = self.clipboard.clone()
                        {
                            self.history
                                .edit("Paste Grade", None, move |doc| doc.stack = stack);
                            self.ids = RowIdGenerator::resuming(self.history.document());
                            self.status.done("grade pasted");
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
                            self.status.done(format!("grade pasted to {n} photos"));
                        }
                    });
                });
                ui.separator();
                ui.add_enabled_ui(self.history.can_undo(), |ui| {
                    let what = self.history.undo_label().unwrap_or("").to_string();
                    let tip = if what.is_empty() {
                        "Undo — Ctrl+Z".to_string()
                    } else {
                        format!("Undo {what} — Ctrl+Z")
                    };
                    if resolve::icon_button(ui, resolve::Glyph::Undo, &tip) {
                        self.history.undo();
                    }
                });
                ui.add_enabled_ui(self.history.can_redo(), |ui| {
                    let what = self.history.redo_label().unwrap_or("").to_string();
                    let tip = if what.is_empty() {
                        "Redo — Ctrl+Shift+Z".to_string()
                    } else {
                        format!("Redo {what} — Ctrl+Shift+Z")
                    };
                    if resolve::icon_button(ui, resolve::Glyph::Redo, &tip) {
                        self.history.redo();
                    }
                });
                ui.separator();
                ui.toggle_value(&mut self.bypass_all, "Bypass all")
                    .on_hover_text("Shift+D — flatten the stack for an honest before/after");
                ui.separator();
                // One button that cycles, rather than three that are mostly
                // off. A three-way choice where two thirds of the control is
                // always the wrong answer is three times the width for the
                // same one fact — which mode is on — and the fact is already
                // written on the button.
                if ui
                    .selectable_label(self.compare.on(), self.compare.label())
                    .on_hover_text("Click to cycle: off, wipe, side by side")
                    .clicked()
                {
                    self.compare = self.compare.next();
                }
                ui.separator();
                // The filmstrip had no control at all until now — only the
                // bare F key, which is not a thing anybody finds. A panel you
                // can hide and cannot get back is a panel you have lost.
                ui.add_enabled_ui(self.library.len() > 1, |ui| {
                    ui.toggle_value(&mut self.show_strip, "Filmstrip")
                        .on_hover_text("F — the other photographs in the set");
                });
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

        // A message that went well clears itself, but egui only redraws when
        // something happens — without this the message sits there until the
        // next time the mouse moves, which is not "six seconds".
        if let Some(left) = self.status.expires_in() {
            if left.is_zero() {
                self.status.clear();
            } else {
                ctx.request_repaint_after(left);
            }
        }

        // Always present, rather than appearing with the first message. A bar
        // that comes and goes moves the photograph up and down under it, and a
        // photograph that jumps while you are judging it is worse than a strip
        // of window spent on the name of what you are looking at.
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.status.text.is_empty() {
                    let idle = match self.path.as_ref() {
                        Some(p) => format!(
                            "{} — {}x{}",
                            p.file_name().unwrap_or(p.as_os_str()).to_string_lossy(),
                            self.image.width,
                            self.image.height
                        ),
                        None => "no photograph open".to_string(),
                    };
                    ui.label(egui::RichText::new(idle).color(theme::colour::DIM));
                } else {
                    // ERROR rather than WARN: the palette already draws the
                    // distinction between "be careful" and "this did not
                    // happen", and every message that lands here is the
                    // second kind.
                    let tint = if self.status.bad {
                        theme::colour::ERROR
                    } else {
                        theme::colour::LABEL
                    };
                    ui.label(egui::RichText::new(&self.status.text).color(tint));
                    if ui.small_button("dismiss").clicked() {
                        self.status.clear();
                    }
                }
            });
        });

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

        // Declared last of the three, which puts it closest to the picture.
        // egui stacks bottom panels in the order they are added, outermost
        // first — so a status bar declared after the scopes would sit between
        // them and the photograph, and the scopes' resize handle would be
        // under it.
        if self.show_scopes {
            egui::TopBottomPanel::bottom("scopes")
                .resizable(true)
                // Three scopes side by side need real height to be worth
                // reading. A waveform two hundred points tall is a smear.
                .default_height(300.0)
                .height_range(160.0..=680.0)
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
                    // Claim whatever is left, so the panel's content is exactly
                    // the panel.
                    //
                    // egui stores a resizable panel's *content* rect, not the
                    // rect it was dragged to. Content a few points shy of its
                    // panel therefore makes the panel a few points shorter next
                    // frame, which makes the content shorter again — and over a
                    // second of frames the whole thing walks down to its
                    // minimum. That is what "the scopes are crushed at the
                    // bottom and spring back when I drag them up" was.
                    //
                    // Claiming the remainder rather than getting the arithmetic
                    // exact everywhere: the slack came from a label here and a
                    // margin there, and one line that cannot be off by a point
                    // beats four that must each be right.
                    let left = ui.available_height();
                    if left > 0.0 {
                        ui.allocate_space(egui::vec2(ui.available_width(), left));
                    }
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

                if self.tab != Tab::Effects {
                    self.preview_effect = None;
                }
                egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                    Tab::Colour => {
                        // The curve carries the histogram, so there is one
                        // rather than two, and it is at the top where a
                        // histogram belongs.
                        egui::CollapsingHeader::new("Curves")
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
                        egui::CollapsingHeader::new("Colour Warper")
                            .default_open(false)
                            .show(ui, |ui| {
                                if let Some(id) = self
                                    .history
                                    .document()
                                    .stack
                                    .find_by_effect("colour_warper")
                                {
                                    let seen = self
                                        .preview
                                        .as_ref()
                                        .and_then(|p| p.scopes())
                                        .map(|s| &s.warper);
                                    warper::panel(
                                        ui,
                                        &mut self.history,
                                        id,
                                        ui.id().with("warper"),
                                        seen,
                                    );
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
                        self.preview_effect = inspector::show(
                            ui,
                            &mut self.history,
                            &mut self.ids,
                            &mut self.dragging_effect,
                            &mut self.settings,
                        );
                    }
                    Tab::Image => {
                        let source = (self.image.width, self.image.height);
                        let was = self.cropping;
                        crop::panel(ui, &mut self.history, source, &mut self.cropping);
                        if was != self.cropping {
                            self.view.fit();
                        }
                    }
                    Tab::File => {
                        if file_page(ui, self) {
                            export_requested = true;
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::theme::colour::VIEWER))
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
                // Panning is a drag, and while the crop tool is open the drag
                // belongs to the crop rectangle. Zooming is not — it is the
                // wheel — so it keeps working, which is the whole point of the
                // two being separate controls. Blocking all of it was the
                // other half of the coupling: the tool did not force the view
                // back to fit any more, it just would not let you leave it.
                let pan = !self.cropping && !self.dragging_wipe;
                self.handle_view_input(ui, &response, rect, pan);

                // Dropping an effect on the picture adds it. It is the same
                // gesture as dropping it on the list, and the picture is the
                // larger target — which matters when the thing you are
                // deciding about is what the picture will look like.
                if let Some(key) = self.dragging_effect
                    && response.drag_stopped()
                {
                    if response.hover_pos().is_some_and(|p| rect.contains(p))
                        && let Some(def) = pe_effects::by_key(key)
                    {
                        let id = self.ids.allocate();
                        self.history
                            .edit(format!("Add {}", def.name), None, move |doc| {
                                let mut row = pe_core::StackRow::new(id, def.key);
                                row.params = def.default_params();
                                doc.stack.push(row);
                            });
                        ctx.data_mut(|d| d.insert_temp(inspector::open_flag(id), true));
                    }
                    self.dragging_effect = None;
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
                // The hovered effect, appended for as long as the pointer is
                // on it. One extra pass: the stage cache re-runs from the
                // first changed row, and this one is last.
                let mut doc = doc;
                if let Some(def) = self.preview_effect.and_then(pe_effects::by_key) {
                    let mut row = pe_core::StackRow::new(PREVIEW_ROW, def.key);
                    row.params = def.default_params();
                    doc.stack.push(row);
                }

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
                let view = self.view;
                let preview = self.preview.as_mut().expect("checked above");
                let compare = self.compare;
                match preview.render(image, &doc, framing_geometry, view, viewport, compare.on()) {
                    Ok(framing) => {
                        self.last_passes = framing.passes;
                        self.last = Some((framing.scale, framing.visible));
                        self.last_frame = framing.frame;
                        let target = draw(ui, rect, &framing);
                        if let Some(def) = self.preview_effect.and_then(pe_effects::by_key) {
                            previewing(ui, target, def.name);
                        }
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
                                framing.visible,
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

        // Deferred requests, serviced once the whole interface has had its
        // say. The ordering is not incidental: a button drawn at line 800
        // cannot be read at line 750, and doing it there is what left Open
        // Folder dead — the flag was already false again by the time the
        // button set it.
        if open_requested {
            self.open_dialog(ctx);
        }
        if open_folder_requested {
            self.open_folder_dialog(ctx);
        }
        // Both refuse politely with no photograph open, which is why they are
        // called unconditionally rather than gated on `self.path` here.
        if save_requested {
            self.save_edit();
        }
        if export_requested {
            self.export();
        }

        if stop_batch && let Some(batch) = self.batch.take() {
            self.status.done(format!(
                "stopped after {} of {}",
                batch.done,
                batch.targets.len()
            ));
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
fn file_page(ui: &mut egui::Ui, app: &mut App) -> bool {
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

    ui.add_space(10.0);
    colour_section(ui, app);
    ui.add_space(10.0);
    export_section(ui, app)
}

/// The colour spaces a file may be read as or written in.
///
/// Not all seven that `pe-color` knows, and the reason is the same at both
/// ends. Textures carry the transfer function: the source is sampled from an
/// `...Srgb` texture and the 8-bit export is written into one, so the hardware
/// applies the sRGB curve in both directions and the transform shader only ever
/// rotates the gamut — it says so in its own header. A space belongs here
/// exactly when its transfer function *is* the sRGB one. Anything else would be
/// decoded, or encoded, with a curve it does not use, and the result would be
/// wrong in the way that is hardest to catch: it looks like a grade.
///
/// Derived from that rule rather than typed out, so the list cannot drift away
/// from the reason for it. In practice nothing is lost — an 8-bit photograph is
/// sRGB or Display P3, linear 8-bit files do not exist, and no camera writes
/// 8-bit Rec.2020.
fn display_spaces() -> impl Iterator<Item = &'static pe_color::ColorSpace> {
    pe_color::space::ALL
        .iter()
        .filter(|s| s.transfer == pe_color::TransferFn::Srgb)
}

/// What the file is, and what we are rendering it to.
///
/// The input is a fact about the photograph that only the person holding it
/// knows — unless the file says, which it now gets asked. Assume sRGB and a
/// Display P3 file renders with its colours pulled in towards the sRGB
/// primaries: not obviously broken, just quietly flatter than the photograph
/// is, which is the worst way for a colour tool to be wrong.
///
/// The output was a fact rather than a control until exports could say what
/// they were rendered in. An untagged file is read as sRGB by every viewer
/// there is, so offering Display P3 out would have been offering a file that
/// is correct in this window and wrong everywhere else. They carry a profile
/// now, so it is a choice.
fn colour_section(ui: &mut egui::Ui, app: &mut App) {
    ui.label(
        egui::RichText::new("COLOUR")
            .small()
            .color(resolve::colour::DIM),
    );
    ui.add_space(4.0);

    let picked_input = space_row(
        ui,
        "Source is",
        "input_space",
        &app.history.document().color.input,
        "What the file is. Set from its ICC profile when it has one",
    );
    if let Some(space) = picked_input {
        app.history.edit("Source Colour Space", None, move |doc| {
            doc.color.input = space;
        });
    }

    ui.add_space(2.0);
    let picked_output = space_row(
        ui,
        "Rendered to",
        "output_space",
        &app.history.document().color.output,
        "What the screen shows and what exports are written in — and say they          are written in, in an embedded profile",
    );
    if let Some(space) = picked_output {
        app.history.edit("Output Colour Space", None, move |doc| {
            doc.color.output = space;
        });
    }
}

/// One labelled colour-space chooser. Returns the new name if it changed.
fn space_row(
    ui: &mut egui::Ui,
    label: &str,
    salt: &str,
    current: &str,
    hover: &str,
) -> Option<String> {
    let mut chosen = current.to_string();
    ui.horizontal(|ui| {
        ui.add_sized(
            [resolve::LABEL_WIDTH, 18.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(resolve::colour::LABEL),
            ),
        )
        .on_hover_text(hover);
        egui::ComboBox::from_id_salt(salt)
            .selected_text(current)
            .width(150.0)
            .show_ui(ui, |ui| {
                for space in display_spaces() {
                    ui.selectable_value(&mut chosen, space.name.to_string(), space.name);
                }
            });
    });
    (chosen != current).then_some(chosen)
}

/// What the photograph gets written as, and the button that writes it.
///
/// On the File page rather than behind the Export button, because these are
/// settings and not a question. A dialog asks the same thing every time and is
/// answered the same way every time; a panel states the answer, keeps it, and
/// stays out of the way of somebody exporting sixty frames.
fn export_section(ui: &mut egui::Ui, app: &mut App) -> bool {
    ui.label(
        egui::RichText::new("EXPORT")
            .small()
            .color(resolve::colour::DIM),
    );
    ui.add_space(4.0);

    let mut chosen = app.settings.export;

    ui.horizontal(|ui| {
        ui.add_sized(
            [resolve::LABEL_WIDTH, 18.0],
            egui::Label::new(
                egui::RichText::new("Format")
                    .small()
                    .color(resolve::colour::LABEL),
            ),
        );
        for format in [
            settings::Format::Jpeg,
            settings::Format::Png,
            settings::Format::Png16,
        ] {
            if ui
                .selectable_label(chosen.format == format, format.label())
                .clicked()
            {
                chosen.format = format;
            }
        }
    });
    ui.add_space(2.0);

    // Greyed rather than hidden for a PNG. A control that vanishes takes its
    // explanation with it — the row staying put, dimmed, says "quality is a
    // JPEG idea" far better than an empty space does.
    let is_jpeg = chosen.format == settings::Format::Jpeg;
    ui.add_enabled_ui(is_jpeg, |ui| {
        let mut quality = chosen.quality as f32;
        if resolve::slider_row(
            ui,
            ui.id().with("export_quality"),
            "Quality",
            &mut quality,
            1.0..=100.0,
            0,
        )
        .changed
        {
            chosen.quality = quality.round().clamp(1.0, 100.0) as u8;
        }
    });

    // What it will actually be called, spelled out. The _KROMA rule is the
    // thing standing between an export and somebody's original, and a rule you
    // can see working is one you can trust.
    let name = match app.path.as_ref() {
        Some(p) => export_name(p, chosen.format),
        None => format!("<photo>_KROMA.{}", chosen.format.extension()),
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_sized(
            [resolve::LABEL_WIDTH, 18.0],
            egui::Label::new(
                egui::RichText::new("Writes")
                    .small()
                    .color(resolve::colour::LABEL),
            ),
        );
        ui.add(egui::Label::new(egui::RichText::new(name).small().monospace()).wrap());
    });

    if chosen != app.settings.export {
        app.settings.export = chosen;
        app.settings.save();
    }

    ui.add_space(8.0);
    let mut pressed = false;
    ui.horizontal(|ui| {
        ui.add_space(resolve::LABEL_WIDTH + 4.0);
        pressed = ui
            .add(egui::Button::new("Export").shortcut_text("Ctrl+E"))
            .on_hover_text("Beside the original, never over it")
            .clicked();
        ui.add_enabled_ui(app.library.len() > 1 && app.batch.is_none(), |ui| {
            if ui
                .button("Export all…")
                .on_hover_text("Every photo in the set, into a folder you choose")
                .clicked()
            {
                app.batch_export();
            }
        });
    });
    pressed
}

/// One line of feedback at the bottom of the window.
///
/// Two kinds, because they want opposite lifetimes. "exported at 6000x4000"
/// has been read by the time it has finished appearing and should then get out
/// of the way. "export failed: permission denied" is the only place that
/// failure is ever reported, and a message that clears itself is a failure
/// nobody sees.
#[derive(Default)]
struct Status {
    text: String,
    /// Drawn in the warning colour, and never expires.
    bad: bool,
    /// When it was said. `None` for the ones that are staying.
    said: Option<std::time::Instant>,
}

/// How long a message that went well stays up.
///
/// Long enough to read a path in, short enough that it is gone before you have
/// finished the next thing.
const STATUS_LINGER: std::time::Duration = std::time::Duration::from_secs(6);

impl Status {
    /// It worked. Say so, briefly.
    fn done(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.bad = false;
        self.said = Some(std::time::Instant::now());
    }

    /// It did not work, or it was refused. Stays until it is dismissed.
    fn problem(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.bad = true;
        self.said = None;
    }

    fn clear(&mut self) {
        self.text.clear();
        self.said = None;
    }

    /// How long this message has left, or `None` if it is not going anywhere.
    fn expires_in(&self) -> Option<std::time::Duration> {
        self.said
            .map(|at| STATUS_LINGER.saturating_sub(at.elapsed()))
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
    /// The photographs to write, by path rather than by position.
    ///
    /// The set can change underneath a run — "Remove from set" is right there
    /// in the filmstrip and nothing disables it — and every position after a
    /// removal slides down by one. A list of indices would then export one
    /// photograph twice, miss another entirely, and report both as successes.
    /// A path means the same photograph whatever happens to the list.
    targets: Vec<PathBuf>,
    next: usize,
    dir: PathBuf,
    done: usize,
    failed: usize,
    /// Taken once, when the run starts, rather than read per photograph.
    /// Changing the format halfway through a batch would otherwise leave a
    /// folder half JPEG and half PNG, with no record of where the line fell.
    export: settings::Export,
    /// Names already written by this run, folded to lower case.
    ///
    /// A batch writes into one directory, and the set it is writing can come
    /// from several: open `holiday/a.jpg` and `work/a.jpg` together and both
    /// want to be `a_KROMA.jpg`.
    taken: std::collections::HashSet<String>,
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
/// What a graded photograph is called: the original's name with `_KROMA` on
/// the end.
///
/// The suffix is not decoration. An export named after its source, in the
/// folder its source lives in, *is* its source on any filesystem that does not
/// care about case — and Windows does not. This used to be `<stem>.jpg`, which
/// meant one "Export all…" into the folder you opened would have written over
/// every original in it.
///
/// The name is one half of that fix and [`would_overwrite`] is the other. A
/// naming scheme that happens to differ is not a guarantee; a check is.
/// Render one photograph and write it, in whichever format was chosen.
///
/// One function for both the single export and the batch. They used to hold
/// two copies of the same three lines, which is two places for a format to be
/// handled and one of them to be forgotten — and the failure would be a folder
/// of JPEGs from a run that said PNG.
///
/// Returns the size actually written, which is not the source's: the crop
/// decides how much picture there is and the resize decides how many pixels it
/// comes in.
fn write_export(
    preview: &Preview,
    image: &pe_io::DecodedImage,
    doc: &pe_core::Document,
    out: &Path,
    chosen: settings::Export,
) -> Result<(u32, u32), String> {
    let (w, h) = pe_render::export::output_size(doc, image.width, image.height);
    // The space the pipeline actually rendered to, which is what the file has
    // to say it is in. Taken from the same settings the render read, so the two
    // cannot disagree — a file labelled with anything else is a wrong answer
    // stated confidently, and every reader will believe it.
    let space = doc.color.pipeline().output;
    if chosen.format.is_sixteen_bit() {
        let pixels = preview.export_16(image, doc).map_err(|e| e.to_string())?;
        pe_io::save_png16(w, h, &pixels, out, &space).map_err(|e| e.to_string())?;
    } else {
        let pixels = preview.export(image, doc).map_err(|e| e.to_string())?;
        let img = pe_io::DecodedImage::new(w, h, pixels).map_err(|e| e.to_string())?;
        match chosen.format {
            settings::Format::Jpeg => pe_io::save_jpeg(&img, out, chosen.quality, &space),
            _ => pe_io::save_png(&img, out, &space),
        }
        .map_err(|e| e.to_string())?;
    }
    Ok((w, h))
}

/// How the graded picture is being held up against the ungraded one.
///
/// Both modes exist because they answer different questions. A wipe is for
/// "did that move go too far" — the eye reads a discontinuity across a seam far
/// more finely than it reads two pictures a hand's width apart. Side by side is
/// for "which of these do I prefer", where a seam would fuse the two into one
/// image and stop you seeing either.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Compare {
    /// The default, and the one the cycling button starts and returns to.
    #[default]
    Off,
    Wipe,
    Side,
}

impl Compare {
    /// The next mode round, so one button can be the whole control.
    ///
    /// Off is in the cycle rather than being a separate way out: a comparison
    /// you cannot turn off with the button that turned it on is a control that
    /// only works in one direction.
    fn next(self) -> Self {
        match self {
            Compare::Off => Compare::Wipe,
            Compare::Wipe => Compare::Side,
            Compare::Side => Compare::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Compare::Off => "Compare",
            Compare::Wipe => "Compare · Wipe",
            Compare::Side => "Compare · Side",
        }
    }

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
                .rect_filled(rect, 0.0, crate::theme::colour::VIEWER);
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

/// A generator that will not collide with what the document already holds.
///
/// Resuming, not default. A new document arrives with its pinned rows in
/// place and they hold ids from zero upwards, so a generator starting at zero
/// hands the first added effect an id another row already owns — and from
/// then on every lookup by id finds whichever comes first, which is the
/// pinned one. The row draws, and nothing that acts on it works.
fn ids_for(doc: &Document) -> RowIdGenerator {
    RowIdGenerator::resuming(doc)
}

/// Where a Windows or Linux build keeps its own files.
///
/// The Mac and iOS shells answer this differently and pass their own answer
/// down, which is why `pe-session` takes it as an argument rather than working
/// it out.
fn platform_support() -> Support {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(v) => Some(PathBuf::from(v)),
            None => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")),
        }
    };
    match base {
        Some(b) => Support::at(b.join("Kroma")),
        None => Support::default(),
    }
}

/// The id the hover preview borrows.
///
/// A sentinel rather than a real allocation: the row exists for one frame and
/// is never in the document, so it must not consume an id the document might
/// later want. `RowIdGenerator` counts up from zero, so the top of the range
/// is the one value it will never hand out.
const PREVIEW_ROW: pe_core::RowId = pe_core::RowId(u64::MAX);

/// Say that the picture is showing something that has not been added yet.
///
/// Without it a hover preview is indistinguishable from an edit, and the first
/// thing anyone would do is move the pointer away and wonder what they broke.
fn previewing(ui: &egui::Ui, target: egui::Rect, name: &str) {
    let painter = ui.painter();
    let font = egui::FontId::proportional(11.0);
    let text = format!("previewing {name}");
    let galley = painter.layout_no_wrap(text, font, egui::Color32::WHITE);
    let at = egui::pos2(target.center().x, target.min.y + 10.0);
    let chip = egui::Rect::from_center_size(at, galley.size()).expand2(egui::vec2(9.0, 5.0));
    painter.rect_filled(chip, 4.0, egui::Color32::from_black_alpha(190));
    painter.rect_stroke(
        chip,
        4.0,
        egui::Stroke::new(1.0_f32, resolve::colour::ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        chip.min + egui::vec2(9.0, 5.0),
        galley,
        egui::Color32::WHITE,
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One unreadable photograph must not keep the window from opening.
    ///
    /// The restored session is the case that matters. Nobody asked for those
    /// photographs, nobody sees stderr when the application was started from a
    /// folder, and a corrupt file in the remembered set used to mean the window
    /// simply never appeared — every launch, until somebody thought to delete a
    /// settings file.
    #[test]
    fn a_broken_photograph_does_not_stop_the_window_opening() {
        let dir = std::env::temp_dir().join("kroma-startup-test");
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.jpg");
        std::fs::write(&broken, b"this is not a photograph").unwrap();
        let good = dir.join("good.png");
        pe_io::save_png(&pe_io::test_chart(16, 16), &good, &pe_color::space::SRGB).unwrap();

        // Asked for the broken one; the set also holds a good one.
        let paths = vec![broken.clone(), good.clone()];
        let mut index = 0;
        let (image, path, trouble) = open_something(&paths, &mut index);

        assert_eq!(
            path.as_deref(),
            Some(good.as_path()),
            "it did not fall through to the photograph that opens"
        );
        assert_eq!(index, 1, "the index must follow what actually opened");
        assert_eq!(image.size(), (16, 16));
        assert!(
            trouble.is_some_and(|t| t.contains("broken")),
            "the failure was swallowed instead of being reported"
        );
    }

    /// And when nothing in the set opens, it still opens.
    #[test]
    fn a_set_of_nothing_readable_still_gives_a_window() {
        let dir = std::env::temp_dir().join("kroma-startup-test");
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("all-broken.jpg");
        std::fs::write(&broken, b"nope").unwrap();

        let mut index = 0;
        let (image, path, trouble) = open_something(&[broken], &mut index);
        assert!(path.is_none(), "it claimed to have opened something");
        assert!(image.width > 0, "no test chart to fall back on");
        assert!(trouble.is_some(), "it failed silently");
    }

    /// The source dropdown may only offer spaces the upload path can honestly
    /// decode.
    ///
    /// The source texture is `Rgba8UnormSrgb`, so the hardware applies the sRGB
    /// EOTF on every sample and the shader never touches the transfer function
    /// at all. Offer a space with any other curve and the picture is decoded
    /// wrongly — not crashed, not obviously broken, just wrong in a way that
    /// reads as somebody's grade.
    ///
    /// This is the assertion, not the filter: the filter can be edited, and
    /// the day somebody offers "the user should be able to pick ACEScg" this
    /// is what says why not.
    #[test]
    fn every_offered_source_space_decodes_as_srgb() {
        let offered: Vec<&str> = display_spaces().map(|s| s.name).collect();
        assert!(!offered.is_empty(), "the source dropdown offers nothing");
        for space in display_spaces() {
            assert_eq!(
                space.transfer,
                pe_color::TransferFn::Srgb,
                "{} is offered as a source space but is not sRGB-encoded — the                  hardware would decode it with the wrong curve",
                space.name
            );
        }
        // The two that matter, and the reason the control exists at all.
        assert!(offered.contains(&"sRGB"));
        assert!(
            offered.contains(&"Display P3"),
            "Display P3 is what every iPhone writes; it has to be offerable"
        );
    }

    /// The whole point of the two kinds: one leaves on its own, the other
    /// waits to be read.
    ///
    /// A failure that clears itself is a failure nobody sees, and the status
    /// bar is the only place this application ever reports one.
    #[test]
    fn only_the_good_news_expires() {
        let mut status = Status::default();

        status.done("exported at 6000x4000");
        let left = status.expires_in().expect("good news should be on a clock");
        assert!(!left.is_zero(), "it expired before it was drawn once");
        assert!(left <= STATUS_LINGER);

        status.problem("export failed: permission denied");
        assert!(
            status.expires_in().is_none(),
            "a failure was put on a timer — it would clear itself unread"
        );
        assert!(status.bad, "a failure must be drawn as one");

        // And saying something good again puts it back on the clock, rather
        // than inheriting the failure's stay-forever.
        status.done("saved");
        assert!(!status.bad);
        assert!(status.expires_in().is_some());
    }

    /// A cycling button has to come back round, or it is a one-way trip: the
    /// control that turned the comparison on is the only one there is to turn
    /// it off again.
    #[test]
    fn compare_cycles_back_to_off() {
        let mut mode = Compare::default();
        assert_eq!(mode, Compare::Off, "a comparison should start off");
        let mut seen = vec![mode];
        for _ in 0..3 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(
            seen,
            vec![Compare::Off, Compare::Wipe, Compare::Side, Compare::Off],
            "three presses should visit every mode and land back on off"
        );
    }
}
