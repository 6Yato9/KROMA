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
    #[allow(dead_code, reason = "settings the export path will read")]
    export_settings: export::Export,
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
}
