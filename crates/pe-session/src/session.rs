//! The open photograph, its edit, and the GPU objects between them.
//!
//! What a shell talks to. It reads the stack, mutates a parameter, asks for a
//! frame, and draws it — which is the same vocabulary `apps/windows` has, with
//! the parts that were never about interface moved down here where the Mac and
//! the iPad can reach them.

use std::path::{Path, PathBuf};

use pe_color::space;
use pe_core::{Document, Geometry, History, ParamValue, RowId, RowIdGenerator, StackRow};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext, ImageTexture, Region, Sampling, TransformPass};

use crate::surface::Attached;
use crate::{Support, autosave, export};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("nothing is open")]
    NothingOpen,
    #[error("no GPU: {0}")]
    NoGpu(String),
    #[error("could not read {path}: {message}")]
    Read { path: String, message: String },
    #[error("no row with id {0}")]
    NoSuchRow(u64),
    #[error("{effect} has no parameter called {key}")]
    NoSuchParam { effect: String, key: String },
    #[error("{0} is not a registered effect")]
    NoSuchEffect(String),
    #[error("render failed: {0}")]
    Render(String),
    #[error("refused: {0} is one of your photographs")]
    WouldOverwriteSource(String),
    #[error("write failed: {0}")]
    Write(String),
    #[error("no layer attached")]
    NoLayer,
    #[error(transparent)]
    Surface(#[from] crate::surface::SurfaceError),
}

/// The photograph that is open, and its edit.
struct Photo {
    path: Option<PathBuf>,
    image: DecodedImage,
    history: History,
    ids: RowIdGenerator,
}

/// Everything that needs a device. Built lazily, because a session exists
/// before a window does and a headless test never needs a surface.
#[derive(Default)]
struct Gpu {
    context: Option<GpuContext>,
    /// The layer a shell handed us. Written here so the device and the
    /// surface it must be compatible with live together; read by the
    /// present path.
    attached: Option<Attached>,
    renderer: Option<EffectRenderer>,
    to_working: Option<TransformPass>,
    to_display: Option<TransformPass>,
    source: Option<ImageTexture>,
    working: Option<ImageTexture>,
    working_size: (u32, u32),
    working_geometry: Option<Geometry>,
    last_passes: usize,
}

pub struct Session {
    instance: wgpu::Instance,
    gpu: Gpu,
    photo: Option<Photo>,
    support: Support,
    /// What an export would be written as. Kept on the session because it
    /// is a property of the sitting rather than of a photograph; nothing
    /// reads it until the export path lands.
    export_settings: export::Export,
    /// Every photograph currently open, for the collision check. The one on
    /// screen is in here too. A batch writes into one folder and the name it
    /// builds for photo A can collide with photo B sitting right beside it.
    open_set: Vec<PathBuf>,
    watcher: autosave::Watcher,
    /// Bumped by every mutation, so a shell can ask "is this still what I last
    /// saw?" with one integer instead of a JSON parse.
    snapshot_version: u64,
    /// Set while a drag is in progress. Consecutive edits sharing it collapse
    /// into one undo step. See `History::edit`.
    interaction: Option<String>,
    needs_render: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            instance: GpuContext::create_instance(),
            gpu: Gpu::default(),
            photo: None,
            support: Support::default(),
            export_settings: export::Export::default(),
            open_set: Vec::new(),
            watcher: autosave::Watcher::new(),
            snapshot_version: 0,
            interaction: None,
            needs_render: true,
        }
    }

    /// Where this host keeps the application's own files. See [`Support`].
    pub fn set_support_dir(&mut self, root: impl Into<PathBuf>) {
        self.support = Support::at(root);
    }

    pub fn is_open(&self) -> bool {
        self.photo.is_some()
    }

    pub fn last_passes(&self) -> usize {
        self.gpu.last_passes
    }

    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    pub fn snapshot_version(&self) -> u64 {
        self.snapshot_version
    }

    pub fn document(&self) -> Option<&Document> {
        Some(self.photo.as_ref()?.history.document())
    }

    pub fn row_count(&self) -> usize {
        self.document().map_or(0, |d| d.stack.len())
    }

    pub fn path(&self) -> Option<&Path> {
        self.photo.as_ref()?.path.as_deref()
    }

    /// The source photograph's pixel dimensions, or zeroes when nothing is
    /// open. Zeroes rather than an `Option` because the shell divides by them
    /// to fit the view and wants one branch, not two.
    pub fn image_size(&self) -> (u32, u32) {
        self.photo
            .as_ref()
            .map_or((0, 0), |p| (p.image.width, p.image.height))
    }

    pub fn undo_label(&self) -> Option<String> {
        self.photo
            .as_ref()?
            .history
            .undo_label()
            .map(str::to_string)
    }

    pub fn redo_label(&self) -> Option<String> {
        self.photo
            .as_ref()?
            .history
            .redo_label()
            .map(str::to_string)
    }

    // ---- opening --------------------------------------------------------

    /// Open a photograph, restoring whatever was being done to it last time.
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let path = path.as_ref().to_path_buf();
        let image = pe_io::load(&path).map_err(|e| SessionError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        // The autosave wins over a fresh document, because it is where the
        // person happened to stop. A sidecar is pulled over the top explicitly.
        let doc = autosave::load(&self.support, &path)
            .unwrap_or_else(|| pe_effects::new_document(path.to_string_lossy()));
        self.adopt(Some(path), image, doc);
        Ok(())
    }

    /// Open the built-in chart, for a session with no file behind it.
    pub fn open_test_chart(&mut self, width: u32, height: u32) -> Result<(), SessionError> {
        let image = pe_io::test_chart(width, height);
        let doc = pe_effects::new_document("test-chart");
        self.adopt(None, image, doc);
        Ok(())
    }

    fn adopt(&mut self, path: Option<PathBuf>, image: DecodedImage, doc: Document) {
        // A source the file itself declared beats the document's guess, which
        // is how a Display P3 file from a phone renders as Display P3.
        let mut doc = doc;
        if let Some(declared) = image.space {
            doc.color.input = declared.to_string();
        }
        let ids = RowIdGenerator::resuming(&doc);
        let history = History::new(doc);
        self.watcher.reset(history.revision());
        self.photo = Some(Photo {
            path,
            image,
            history,
            ids,
        });
        // Every cached stage and both intermediates belong to the old picture.
        self.gpu.source = None;
        self.gpu.working = None;
        self.gpu.working_size = (0, 0);
        self.gpu.working_geometry = None;
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.touched();
    }

    fn touched(&mut self) {
        self.snapshot_version += 1;
        self.needs_render = true;
    }

    // ---- editing --------------------------------------------------------

    /// Bracket a drag so it becomes one undo step rather than three hundred.
    pub fn begin_interaction(&mut self, label: impl Into<String>) {
        self.interaction = Some(label.into());
    }

    pub fn end_interaction(&mut self) {
        self.interaction = None;
        if let Some(p) = self.photo.as_mut() {
            p.history.break_coalescing();
        }
    }

    fn edit<F>(&mut self, label: &str, f: F) -> Result<(), SessionError>
    where
        F: FnOnce(&mut Document),
    {
        let coalesce = self.interaction.clone();
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        photo.history.edit(label, coalesce, f);
        self.touched();
        Ok(())
    }

    pub fn add_effect(&mut self, key: &str) -> Result<RowId, SessionError> {
        let def =
            pe_effects::by_key(key).ok_or_else(|| SessionError::NoSuchEffect(key.to_string()))?;
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let id = photo.ids.allocate();
        let params = def.default_params();
        self.edit(&format!("Add {}", def.name), move |doc| {
            let mut row = StackRow::new(id, key);
            row.params = params;
            doc.stack.push(row);
        })?;
        Ok(id)
    }

    pub fn remove_row(&mut self, id: RowId) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Remove row", move |doc| {
            doc.stack.remove(id);
        })
    }

    pub fn move_row(&mut self, id: RowId, to: usize) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Reorder", move |doc| {
            doc.stack.reorder(id, to);
        })
    }

    pub fn set_row_enabled(&mut self, id: RowId, on: bool) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Enable row", move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.enabled = on;
            }
        })
    }

    pub fn set_row_opacity(&mut self, id: RowId, value: f32) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Opacity", move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.opacity = value.clamp(0.0, 1.0);
            }
        })
    }

    /// Set a parameter, refusing one the effect does not declare.
    ///
    /// Refused rather than inserted: a typo that silently adds a key produces
    /// a document with a parameter no shader reads and no UI shows, which is
    /// indistinguishable from the slider being broken.
    pub fn set_param(
        &mut self,
        id: RowId,
        key: &str,
        value: ParamValue,
    ) -> Result<(), SessionError> {
        let effect = self.require_row(id)?;
        let def = pe_effects::by_key(&effect)
            .ok_or_else(|| SessionError::NoSuchEffect(effect.clone()))?;
        let param =
            def.params
                .iter()
                .find(|p| p.key == key)
                .ok_or_else(|| SessionError::NoSuchParam {
                    effect: effect.clone(),
                    key: key.to_string(),
                })?;
        let label = param.name.to_string();
        let key = key.to_string();
        self.edit(&label, move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.params.set(key, value);
            }
        })
    }

    pub fn set_float(&mut self, id: RowId, key: &str, value: f32) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Float(value))
    }

    pub fn set_bool(&mut self, id: RowId, key: &str, value: bool) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Bool(value))
    }

    pub fn set_choice(&mut self, id: RowId, key: &str, value: &str) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Choice(value.to_string()))
    }

    pub fn set_rgb(&mut self, id: RowId, key: &str, value: [f32; 3]) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Rgb(value))
    }

    /// The effect key of the row, or an error naming the id that was missing.
    fn require_row(&self, id: RowId) -> Result<String, SessionError> {
        self.document()
            .ok_or(SessionError::NothingOpen)?
            .stack
            .get(id)
            .map(|r| r.effect.clone())
            .ok_or(SessionError::NoSuchRow(id.0))
    }

    pub fn undo(&mut self) -> Result<bool, SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let moved = photo.history.undo();
        if moved {
            self.touched();
        }
        Ok(moved)
    }

    pub fn redo(&mut self) -> Result<bool, SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let moved = photo.history.redo();
        if moved {
            self.touched();
        }
        Ok(moved)
    }

    pub fn can_undo(&self) -> bool {
        self.photo.as_ref().is_some_and(|p| p.history.can_undo())
    }

    pub fn can_redo(&self) -> bool {
        self.photo.as_ref().is_some_and(|p| p.history.can_redo())
    }

    // ---- rendering ------------------------------------------------------

    fn context(&mut self) -> Result<&GpuContext, SessionError> {
        if self.gpu.context.is_none() {
            let gpu = pollster::block_on(GpuContext::from_instance(&self.instance, None))
                .map_err(|e| SessionError::NoGpu(e.to_string()))?;
            self.gpu.context = Some(gpu);
        }
        Ok(self.gpu.context.as_ref().expect("built above"))
    }

    /// Run the stack, returning a view of the graded frame in the working space.
    ///
    /// A view rather than the texture: `EffectRenderer::render` hands back a
    /// reference borrowed from the renderer, which cannot escape a method that
    /// also writes to `self`. A `TextureView` is a cheap clone of a handle and
    /// is all any consumer of a rendered frame wants.
    fn graded(&mut self, width: u32, height: u32) -> Result<wgpu::TextureView, SessionError> {
        self.context()?;
        let gpu = self.gpu.context.as_ref().expect("context built above");
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let doc = photo.history.document();

        if self.gpu.source.is_none() {
            self.gpu.source = Some(
                ImageTexture::upload_rgba8(
                    &gpu.device,
                    &gpu.queue,
                    photo.image.width,
                    photo.image.height,
                    &photo.image.pixels,
                    "source",
                )
                .map_err(|e| SessionError::Render(e.to_string()))?,
            );
        }
        if self.gpu.to_working.is_none() {
            self.gpu.to_working = Some(TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT));
            self.gpu.to_display = Some(TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT));
            self.gpu.renderer = Some(EffectRenderer::new(&gpu.device));
        }

        // Guarded, not done every frame. Rebuilding the working texture costs
        // a resample, and — because the stage cache is keyed on the frame's
        // size and geometry — throws away every cached row with it. That is
        // the whole of `last_passes` reading 1 rather than the stack depth.
        let geometry = doc.geometry;
        if self.gpu.working_size != (width, height) || self.gpu.working_geometry != Some(geometry) {
            let sampling =
                Sampling::of(&geometry, photo.image.width, photo.image.height).within(Region::FULL);
            let source = self.gpu.source.as_ref().expect("uploaded above");
            self.gpu.working = Some(
                self.gpu
                    .to_working
                    .as_mut()
                    .expect("built above")
                    .to_working_mapped(
                        gpu,
                        source,
                        &doc.color.pipeline().input,
                        width,
                        height,
                        sampling,
                    ),
            );
            self.gpu.working_size = (width, height);
            self.gpu.working_geometry = Some(geometry);
        }

        let working = self.gpu.working.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_mut().expect("built above");
        renderer.set_region(Region::FULL);
        let graded = renderer.render(gpu, working, doc, 1);
        let view = graded.view.clone();
        self.gpu.last_passes = renderer.last_pass_count();
        Ok(view)
    }

    /// Render at `width`×`height` and read the result back as RGBA8.
    ///
    /// Used by the tests and, later, by the thumbnail path. The interactive
    /// route writes to the attached layer and never stalls the GPU reading
    /// anything back.
    pub fn render_offscreen(&mut self, width: u32, height: u32) -> Result<Vec<u8>, SessionError> {
        let graded_view = self.graded(width, height)?;
        let gpu = self.gpu.context.as_ref().expect("built by graded");
        let output = self
            .photo
            .as_ref()
            .expect("graded checked this")
            .history
            .document()
            .color
            .pipeline()
            .output;

        let target = ImageTexture::new(
            &gpu.device,
            width,
            height,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            "offscreen",
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("offscreen"),
            });
        self.gpu
            .to_display
            .as_ref()
            .expect("built by graded")
            .encode(
                gpu,
                &mut encoder,
                &graded_view,
                &target.view,
                &space::ACESCG,
                &output,
            );
        gpu.queue.submit([encoder.finish()]);
        self.needs_render = false;
        pe_render::read_rgba8(gpu, &target).map_err(|e| SessionError::Render(e.to_string()))
    }

    // ---- the screen -----------------------------------------------------

    /// Adopt a `CAMetalLayer` the host owns.
    ///
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives the attachment. The
    /// Swift side guarantees this by holding it on a view it owns and calling
    /// [`Session::detach_layer`] before that view goes away.
    pub unsafe fn attach_layer(
        &mut self,
        layer: *mut std::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<(), SessionError> {
        if layer.is_null() {
            return Err(SessionError::NoLayer);
        }
        // The adapter must come from the instance the surface belongs to, and
        // must be told about the surface: on a machine with more than one GPU,
        // the one picked otherwise may not be able to present to this window.
        let probe = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| SessionError::NoGpu(e.to_string()))?;
        if self.gpu.context.is_none() {
            let gpu = pollster::block_on(GpuContext::from_instance(&self.instance, Some(&probe)))
                .map_err(|e| SessionError::NoGpu(e.to_string()))?;
            self.gpu.context = Some(gpu);
        }
        drop(probe);

        let gpu = self.gpu.context.as_ref().expect("built above");
        let attached = unsafe {
            Attached::new(
                &self.instance,
                &gpu.adapter,
                &gpu.device,
                layer,
                width,
                height,
            )
        }?;
        self.gpu.attached = Some(attached);
        self.needs_render = true;
        Ok(())
    }

    pub fn resize_layer(&mut self, width: u32, height: u32) {
        if let (Some(a), Some(gpu)) = (self.gpu.attached.as_mut(), self.gpu.context.as_ref()) {
            a.resize(&gpu.device, width, height);
            // The working texture was built for the old size, and so was every
            // cached stage that reads it.
            self.gpu.working_size = (0, 0);
            self.needs_render = true;
        }
    }

    pub fn detach_layer(&mut self) {
        self.gpu.attached = None;
    }

    /// Draw the current state into the attached layer and present it.
    pub fn present(&mut self) -> Result<(), SessionError> {
        let (width, height) = match self.gpu.attached.as_ref() {
            Some(a) => a.size(),
            None => return Err(SessionError::NoLayer),
        };
        if self.photo.is_none() {
            // Nothing open yet — the viewer's background, not an error.
            //
            // `self.context()` returns a reference tied to `&mut self`, which
            // would keep the whole of `self` borrowed for as long as `gpu` is
            // alive — colliding with the separate borrow of `self.gpu.attached`
            // below. Called for its side effect only and re-borrowed through
            // the field, the two borrows are disjoint and coexist, the same
            // split `graded` relies on.
            self.context()?;
            let gpu = self.gpu.context.as_ref().expect("built above");
            let attached = self.gpu.attached.as_ref().expect("checked above");
            // `false` means the swapchain was rebuilt and nothing was drawn, so
            // `needs_render` deliberately stays set and the next tick tries again.
            if attached.present_clear(&gpu.device, &gpu.queue, [0.06, 0.06, 0.07, 1.0])? {
                self.needs_render = false;
            }
            return Ok(());
        }

        let graded_view = self.graded(width, height)?;
        let output = self
            .photo
            .as_ref()
            .expect("checked above")
            .history
            .document()
            .color
            .pipeline()
            .output;
        let gpu = self.gpu.context.as_ref().expect("built by graded");
        let attached = self.gpu.attached.as_ref().expect("checked above");

        // Through `acquire` rather than the surface directly: a resize or a
        // move to another display invalidates the swapchain, and rebuilding it
        // is the whole recovery. `None` is "no frame this tick", not a failure.
        let Some(frame) = attached.acquire(&gpu.device)? else {
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present"),
            });
        self.gpu
            .to_display
            .as_ref()
            .expect("built by graded")
            .encode(
                gpu,
                &mut encoder,
                &graded_view,
                &view,
                &space::ACESCG,
                &output,
            );
        gpu.queue.submit([encoder.finish()]);
        frame.present();
        self.needs_render = false;
        Ok(())
    }

    // ---- persistence ------------------------------------------------------

    /// Every photograph the collision check must consider.
    pub fn set_open_set(&mut self, paths: Vec<PathBuf>) {
        self.open_set = paths;
    }

    /// Called every frame. Writes the work in progress once the user has
    /// stopped moving. See [`autosave::Watcher`].
    pub fn tick(&mut self) {
        let Some(photo) = self.photo.as_ref() else {
            return;
        };
        let revision = photo.history.revision();
        if self.watcher.tick(revision, std::time::Instant::now()) {
            self.write_autosave();
        }
    }

    /// Write the work in progress now, throttle or no throttle.
    ///
    /// Called when leaving a photograph, where the throttle is beside the
    /// point: the thing that would have triggered the write is about to stop
    /// being the thing on screen.
    pub fn write_autosave(&mut self) {
        let Some(photo) = self.photo.as_ref() else {
            return;
        };
        let Some(path) = photo.path.as_ref() else {
            return;
        };
        autosave::store(&self.support, path, photo.history.document());
        let revision = photo.history.revision();
        self.watcher.reset(revision);
    }

    /// The explicit save: a `.peproj` beside the photograph.
    ///
    /// A sidecar is a decision — *this* is the edit, keep it, move it with the
    /// photograph. The autosave is just where you happened to stop.
    pub fn save_sidecar(&mut self) -> Result<PathBuf, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let path = photo.path.as_ref().ok_or(SessionError::NothingOpen)?;
        let out = path.with_extension("peproj");
        let json = photo
            .history
            .document()
            .to_json()
            .map_err(|e| SessionError::Write(e.to_string()))?;
        pe_io::write_bytes_atomically(&out, json.as_bytes())
            .map_err(|e| SessionError::Write(e.to_string()))?;
        Ok(out)
    }

    /// Pull a sidecar back over the top of whatever is showing.
    pub fn load_sidecar(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| SessionError::Read {
            path: path.as_ref().display().to_string(),
            message: e.to_string(),
        })?;
        let doc = Document::from_json(&text).map_err(|e| SessionError::Read {
            path: path.as_ref().display().to_string(),
            message: e.to_string(),
        })?;
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        photo.ids = RowIdGenerator::resuming(&doc);
        photo.history.edit("Load edit", None, move |d| *d = doc);
        self.gpu.working_geometry = None;
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.touched();
        Ok(())
    }

    /// Throw the edit and the saved work away.
    ///
    /// An edit that comes back every time you open a photograph, with no way
    /// to be rid of it, is not a convenience — it is a photograph you can no
    /// longer see.
    pub fn revert(&mut self) -> Result<(), SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let source = photo
            .path
            .clone()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "test-chart".to_string());
        let fresh = pe_effects::new_document(source);
        photo.ids = RowIdGenerator::resuming(&fresh);
        photo.history.edit("Revert", None, move |d| *d = fresh);
        if let Some(path) = photo.path.clone() {
            autosave::forget(&self.support, &path);
        }
        let revision = self.photo.as_ref().expect("still open").history.revision();
        self.watcher.reset(revision);
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.gpu.working_geometry = None;
        self.touched();
        Ok(())
    }

    // ---- export -------------------------------------------------------

    pub fn set_export(&mut self, format: export::Format, quality: u8) {
        self.export_settings = export::Export {
            format,
            quality: quality.clamp(1, 100),
        };
    }

    pub fn export_settings(&self) -> export::Export {
        self.export_settings
    }

    /// Write the graded photograph beside its original, refusing a collision.
    pub fn export_current(&mut self) -> Result<PathBuf, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let source = photo
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("export"));
        let chosen = self.export_settings;
        let out = source.with_file_name(export::export_name(&source, chosen.format));

        // Both defences, in order. The naming keeps them apart; the check is
        // what makes it a guarantee rather than a scheme that happens to work.
        let mut open = self.open_set.clone();
        open.push(source.clone());
        if export::would_overwrite_a_source(&open, &out) {
            return Err(SessionError::WouldOverwriteSource(
                out.display().to_string(),
            ));
        }

        self.context()?;
        let gpu = self.gpu.context.as_ref().expect("built above");
        if self.gpu.renderer.is_none() {
            self.gpu.renderer = Some(EffectRenderer::new(&gpu.device));
        }
        let renderer = self.gpu.renderer.as_ref().expect("built above");
        let photo = self.photo.as_ref().expect("checked above");
        let doc = photo.history.document();
        let (w, h) = pe_render::export::output_size(doc, photo.image.width, photo.image.height);
        // The space the pipeline actually rendered to, which is what the file
        // has to say it is in. Taken from the same settings the render read, so
        // the two cannot disagree — a file labelled with anything else is a
        // wrong answer stated confidently, and every reader will believe it.
        let out_space = doc.color.pipeline().output;

        if chosen.format.is_sixteen_bit() {
            let pixels = pe_render::export::render_full_16(
                gpu,
                renderer,
                photo.image.width,
                photo.image.height,
                &photo.image.pixels,
                doc,
            )
            .map_err(|e| SessionError::Render(e.to_string()))?;
            pe_io::save_png16(w, h, &pixels, &out, &out_space)
                .map_err(|e| SessionError::Write(e.to_string()))?;
        } else {
            let pixels = pe_render::render_full(
                gpu,
                renderer,
                photo.image.width,
                photo.image.height,
                &photo.image.pixels,
                doc,
            )
            .map_err(|e| SessionError::Render(e.to_string()))?;
            let img = pe_io::DecodedImage::new(w, h, pixels)
                .map_err(|e| SessionError::Write(e.to_string()))?;
            match chosen.format {
                export::Format::Jpeg => pe_io::save_jpeg(&img, &out, chosen.quality, &out_space),
                _ => pe_io::save_png(&img, &out, &out_space),
            }
            .map_err(|e| SessionError::Write(e.to_string()))?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean(pixels: &[u8]) -> f32 {
        // Alpha excluded — it is 255 everywhere and would flatten the answer.
        let sum: u64 = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| p[..3].iter().map(|c| u64::from(*c)))
            .sum();
        sum as f32 / (pixels.len() as f32 / 4.0 * 3.0)
    }

    fn chart_session() -> Session {
        let mut s = Session::new();
        s.open_test_chart(256, 256)
            .expect("a test chart always opens");
        s
    }

    #[test]
    fn a_fresh_session_has_nothing_open() {
        let s = Session::new();
        assert!(!s.is_open());
        assert_eq!(s.row_count(), 0);
    }

    #[test]
    fn opening_a_chart_gives_it_the_pinned_rows() {
        let s = chart_session();
        assert!(s.is_open());
        // new_document seeds the pinned rows the inspector shows as fixed
        // panels, so an opened photograph is never an empty stack.
        assert_eq!(s.row_count(), pe_effects::PINNED_ROWS.len());
    }

    #[test]
    fn exposure_makes_the_picture_brighter() {
        let mut s = chart_session();
        let before = mean(&s.render_offscreen(256, 256).unwrap());

        let row = s
            .add_effect("exposure")
            .expect("exposure is a registered effect");
        s.set_float(row, "ev", 2.0).unwrap();

        let after = mean(&s.render_offscreen(256, 256).unwrap());
        assert!(
            after > before + 5.0,
            "two stops did nothing: {before} -> {after}"
        );
    }

    #[test]
    fn an_unchanged_document_costs_no_passes() {
        let mut s = chart_session();
        s.render_offscreen(256, 256).unwrap();
        s.render_offscreen(256, 256).unwrap();
        assert_eq!(
            s.last_passes(),
            0,
            "re-rendered something that had not changed"
        );
    }

    #[test]
    fn moving_one_slider_in_a_deep_stack_costs_one_pass() {
        // The number the toolbar shows, and the reason the application does not
        // get slower as you do more to an image. If this ever reads the stack
        // depth, the stage cache has stopped working.
        let mut s = chart_session();
        let mut deepest = None;
        for _ in 0..4 {
            deepest = Some(s.add_effect("exposure").unwrap());
        }
        s.render_offscreen(256, 256).unwrap();

        s.set_float(deepest.unwrap(), "ev", 0.5).unwrap();
        s.render_offscreen(256, 256).unwrap();
        assert_eq!(s.last_passes(), 1);
    }

    #[test]
    fn a_parameter_that_is_not_there_is_refused_rather_than_invented() {
        let mut s = chart_session();
        let row = s.add_effect("exposure").unwrap();
        assert!(s.set_float(row, "not_a_parameter", 1.0).is_err());
    }

    #[test]
    fn presenting_without_a_layer_says_so_rather_than_crashing() {
        let mut s = chart_session();
        assert!(matches!(s.present(), Err(SessionError::NoLayer)));
    }

    #[test]
    fn attaching_a_null_layer_is_refused() {
        let mut s = Session::new();
        let rc = unsafe { s.attach_layer(std::ptr::null_mut(), 100, 100) };
        assert!(rc.is_err());
    }

    #[test]
    fn work_in_progress_comes_back_when_the_photograph_is_reopened() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.png");
        let chart = pe_io::test_chart(64, 64);
        pe_io::save_png(&chart, &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_path(&photo).unwrap();
        // Sharpen rather than exposure: exposure is one of the pinned rows
        // every fresh document already carries, so `find` on that key would
        // hit the pinned row rather than the one this test adds and edits.
        let row = s.add_effect("sharpen").unwrap();
        s.set_float(row, "amount", 1.5).unwrap();
        s.write_autosave();

        let mut again = Session::new();
        again.set_support_dir(tmp.path().join("support"));
        again.open_path(&photo).unwrap();
        let doc = again.document().unwrap();
        let restored = doc
            .stack
            .iter()
            .find(|r| r.effect == "sharpen")
            .and_then(|r| r.params.get("amount"))
            .and_then(|v| v.as_float());
        assert_eq!(restored, Some(1.5));
    }

    #[test]
    fn reverting_leaves_nothing_to_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_path(&photo).unwrap();
        // Sharpen rather than exposure: exposure is one of the pinned rows
        // every fresh document already carries, so a document with none of
        // that key can never exist and the assertion below could never pass.
        let row = s.add_effect("sharpen").unwrap();
        s.set_float(row, "amount", 1.5).unwrap();
        s.write_autosave();
        s.revert().unwrap();

        let mut again = Session::new();
        again.set_support_dir(tmp.path().join("support"));
        again.open_path(&photo).unwrap();
        assert!(
            again
                .document()
                .unwrap()
                .stack
                .iter()
                .all(|r| r.effect != "sharpen"),
            "the reverted edit came back"
        );
    }

    #[test]
    fn an_export_is_written_beside_the_original_with_the_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("sunset.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        let out = s.export_current().unwrap();
        assert_eq!(out.file_name().unwrap(), "sunset_KROMA.jpg");
        assert!(out.exists());
        // And the original is untouched.
        assert!(photo.exists());
    }

    #[test]
    fn an_export_that_would_land_on_an_original_is_refused() {
        // Contrived deliberately: a file already named as an export. Opening it
        // and exporting must not write over it.
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("sunset_KROMA.jpg");
        let chart = pe_io::test_chart(64, 64);
        pe_io::save_jpeg(&chart, &photo, 95, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        // Its export would be sunset_KROMA_KROMA.jpg, which is safe — so to
        // exercise the refusal, claim the output name is one of ours.
        s.set_open_set(vec![tmp.path().join("sunset_KROMA_KROMA.jpg")]);
        assert!(matches!(
            s.export_current(),
            Err(SessionError::WouldOverwriteSource(_))
        ));
    }
}
