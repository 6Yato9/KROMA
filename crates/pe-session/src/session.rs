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

use crate::scopes::Scopes;
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
    #[error("a curve needs at least two points, got {0}")]
    TooFewPoints(usize),
    #[error("{effect}.{key} is not a curve")]
    NotACurve { effect: String, key: String },
    #[error("{effect} has no lattice called {key}")]
    NotAWarp { effect: String, key: String },
    #[error("no vertex at {col}, {row} in a {cols} by {rows} grid")]
    NoSuchVertex {
        col: u32,
        row: u32,
        cols: u32,
        rows: u32,
    },
    #[error("{effect} has no pin set called {key}")]
    NotAPinSet { effect: String, key: String },
    #[error("no pin at {index}, of {count}")]
    NoSuchPin { index: usize, count: usize },
    #[error("a warper takes at most {0} pins")]
    TooManyPins(usize),
    #[error("no layer attached")]
    NoLayer,
    #[error(transparent)]
    Surface(#[from] crate::surface::SurfaceError),
}

/// Which lattices a divisions control governs, and which axis of each.
///
/// The Colour Warper's grid size lives in two places that have to agree: the
/// `Choice` the user sets, and the `Warp`'s own `cols`/`rows`, which is what
/// gets uploaded and what the shader's index arithmetic assumes. Keeping them
/// together is not optional — disagree, and the renderer reads real
/// displacements from the wrong vertices.
const WARP_DIVISIONS: &[(&str, &[&str], Axis)] = &[
    ("hue_divisions", &["hue_sat"], Axis::Cols),
    ("sat_divisions", &["hue_sat"], Axis::Rows),
    (
        "chroma_divisions",
        &["chroma_luma_1", "chroma_luma_2"],
        Axis::Cols,
    ),
    (
        "luma_divisions",
        &["chroma_luma_1", "chroma_luma_2"],
        Axis::Rows,
    ),
];

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    Cols,
    Rows,
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
    /// The display transform for the attached layer, and the format it was
    /// built for.
    ///
    /// Separate from `to_display`, which targets `SOURCE_FORMAT` because that
    /// is what an offscreen read-back is. A layer picks its own format — a
    /// `CAMetalLayer` prefers `Bgra8UnormSrgb` — and a pipeline built for one
    /// format cannot be bound in a pass targeting another; wgpu refuses it as
    /// a validation error rather than swizzling quietly. Both are sRGB, so the
    /// transfer function is still applied exactly once, on write.
    to_screen: Option<TransformPass>,
    screen_format: Option<wgpu::TextureFormat>,
    source: Option<ImageTexture>,
    working: Option<ImageTexture>,
    working_size: (u32, u32),
    working_geometry: Option<Geometry>,
    /// The rectangle of the frame the working texture was built for. A texture
    /// built for one rectangle is the wrong picture for another, so this sits
    /// in the rebuild guard beside the size and the geometry.
    working_region: Option<Region>,
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
    /// Which rectangle of the frame the viewer is showing. A property of the
    /// window rather than of the document: two windows on one photograph would
    /// disagree about it and both be right.
    view: Region,
    watcher: autosave::Watcher,
    /// Bumped by every mutation, so a shell can ask "is this still what I last
    /// saw?" with one integer instead of a JSON parse.
    snapshot_version: u64,
    /// Set while a drag is in progress. Consecutive edits sharing it collapse
    /// into one undo step. See `History::edit`.
    interaction: Option<String>,
    needs_render: bool,
    /// The last measurement of the graded frame, thrown away by every edit.
    /// See [`crate::scopes`] for why it is dropped rather than kept and
    /// questioned.
    scopes: Option<Scopes>,
    /// Which measurement `scopes` is. Never reset, so a shell holding a copy
    /// can compare one integer instead of a 2.6 MB waveform.
    scope_generation: u64,
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
            view: Region::FULL,
            watcher: autosave::Watcher::new(),
            snapshot_version: 0,
            interaction: None,
            needs_render: true,
            scopes: None,
            scope_generation: 0,
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
        self.gpu.working_region = None;
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.touched();
    }

    /// The one place every mutation lands: `edit` (and so every setter),
    /// `undo`, `redo`, `revert`, `load_sidecar` and `adopt` all pass through
    /// here. Which is why the measurement is dropped here and nowhere else —
    /// a `self.scopes = None` in twenty setters is a line the twenty-first
    /// setter will not have.
    fn touched(&mut self) {
        self.snapshot_version += 1;
        self.needs_render = true;
        self.scopes = None;
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

    /// Set a choice, and move anything else that choice describes.
    ///
    /// The whole thing is one undo step. A divisions choice writes the choice
    /// and then resizes the lattices it names, and those two have to travel
    /// together: undoing half of it would leave the choice saying 8 over a 6
    /// by 6 grid, which is exactly the disagreement `follow_divisions` exists
    /// to prevent — and from the outside it was one press of one control.
    ///
    /// Coalescing is how this file already spells "these edits are one step".
    /// A bracket already in progress is left to own the run and close it.
    pub fn set_choice(&mut self, id: RowId, key: &str, value: &str) -> Result<(), SessionError> {
        let ours = self.interaction.is_none();
        if ours {
            self.begin_interaction(format!("choice:{}:{key}", id.0));
        }
        let result = self
            .set_param(id, key, ParamValue::Choice(value.to_string()))
            .and_then(|()| self.follow_divisions(id, key));
        if ours {
            self.end_interaction();
        }
        result
    }

    /// Bring a warper's lattices in line with a divisions control that just
    /// changed. A no-op for every other effect and every other parameter.
    ///
    /// Resizing resamples rather than clearing: see `Warp::resize`.
    fn follow_divisions(&mut self, id: RowId, key: &str) -> Result<(), SessionError> {
        let Some((_, keys, axis)) = WARP_DIVISIONS.iter().find(|(k, _, _)| *k == key) else {
            return Ok(());
        };

        // Everything is read out of the document before anything is written
        // back to it. `set_param` takes the whole session, and the row is
        // borrowed from it — so the two cannot overlap.
        let resized: Vec<(&str, pe_core::Warp)> = {
            let Some(doc) = self.document() else {
                return Ok(());
            };
            let Some(row) = doc.stack.get(id) else {
                return Ok(());
            };
            if row.effect != "colour_warper" {
                return Ok(());
            }
            // The option text is the number: "4", "6", "8", "12", "16".
            let Some(n) = row
                .params
                .get(key)
                .and_then(ParamValue::as_choice)
                .and_then(|s| s.parse::<u32>().ok())
            else {
                return Ok(());
            };

            keys.iter()
                .filter_map(|warp_key| {
                    let w = row.params.get(warp_key).and_then(ParamValue::as_warp)?;
                    let (cols, rows) = match axis {
                        Axis::Cols => (n, w.rows()),
                        Axis::Rows => (w.cols(), n),
                    };
                    if (cols, rows) == (w.cols(), w.rows()) {
                        return None;
                    }
                    let mut next = w.clone();
                    next.resize(cols, rows);
                    Some((*warp_key, next))
                })
                .collect()
        };

        for (warp_key, warp) in resized {
            self.set_param(id, warp_key, ParamValue::Warp(warp))?;
        }
        Ok(())
    }

    pub fn set_rgb(&mut self, id: RowId, key: &str, value: [f32; 3]) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Rgb(value))
    }

    /// A four-way wheel: the three channels and the ring around the outside.
    ///
    /// The master travels separately rather than being folded into the
    /// channels, because resetting only the ring has to stay possible — the
    /// same reason `pe_core::Wheel` keeps them apart.
    pub fn set_wheel(
        &mut self,
        id: RowId,
        key: &str,
        master: f32,
        rgb: [f32; 3],
    ) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Wheel(pe_core::Wheel { rgb, master }))
    }

    /// Replace a curve with a list of control points.
    ///
    /// Refused below two points. The evaluator treats a shorter list as the
    /// identity, so storing one would quietly replace the user's edit with a
    /// straight line and then show them that line as though they had drawn it.
    pub fn set_curve(
        &mut self,
        id: RowId,
        key: &str,
        points: &[[f32; 2]],
    ) -> Result<(), SessionError> {
        if points.len() < 2 {
            return Err(SessionError::TooFewPoints(points.len()));
        }
        // `set_param` checks that the key exists, not that it holds a curve.
        // A curve stored on a float is a value no shader slot reads and no
        // control draws — the same silence the key check exists to prevent.
        let effect = self.require_row(id)?;
        if let Some(p) =
            pe_effects::by_key(&effect).and_then(|d| d.params.iter().find(|p| p.key == key))
            && !matches!(p.kind, pe_effects::ParamKind::Curve { .. })
        {
            return Err(SessionError::NotACurve {
                effect,
                key: key.to_string(),
            });
        }
        self.set_param(
            id,
            key,
            ParamValue::Curve(pe_core::Curve {
                points: points.to_vec(),
            }),
        )
    }

    /// Move one vertex of a lattice.
    ///
    /// The offset is a displacement from where the vertex would sit if it had
    /// never been touched, which is what a warp stores. Refused for a vertex
    /// the grid does not have: `Warp::set` ignores one silently, and over the C
    /// ABI a call that reports success and does nothing is the hardest kind of
    /// bug to see from the far side.
    pub fn set_warp_vertex(
        &mut self,
        id: RowId,
        key: &str,
        col: u32,
        row: u32,
        offset: [f32; 2],
    ) -> Result<(), SessionError> {
        let mut warp = self.require_warp(id, key)?;
        if col >= warp.cols() || row >= warp.rows() {
            return Err(SessionError::NoSuchVertex {
                col,
                row,
                cols: warp.cols(),
                rows: warp.rows(),
            });
        }
        warp.set(col, row, offset);
        self.set_param(id, key, ParamValue::Warp(warp))
    }

    /// Put a lattice back to identity, keeping its grid size — this undoes the
    /// dragging, not the setting up.
    pub fn clear_warp(&mut self, id: RowId, key: &str) -> Result<(), SessionError> {
        let mut warp = self.require_warp(id, key)?;
        warp.clear();
        self.set_param(id, key, ParamValue::Warp(warp))
    }

    /// The lattice at a key, or an error naming what was actually there.
    fn require_warp(&self, id: RowId, key: &str) -> Result<pe_core::Warp, SessionError> {
        let effect = self.require_row(id)?;
        self.document()
            .ok_or(SessionError::NothingOpen)?
            .stack
            .get(id)
            .and_then(|r| r.params.get(key))
            .and_then(ParamValue::as_warp)
            .cloned()
            .ok_or(SessionError::NotAWarp {
                effect,
                key: key.to_string(),
            })
    }

    /// Place a pin, returning its index.
    ///
    /// Refused once the set is full: `Pins::add` returns `None` there, and a
    /// call that reports success and adds nothing is the same silence the
    /// vertex check exists to prevent.
    pub fn add_pin(&mut self, id: RowId, key: &str, at: [f32; 2]) -> Result<usize, SessionError> {
        let mut pins = self.require_pins(id, key)?;
        let index = pins
            .add(pe_core::pins::Pin::placed(at))
            .ok_or(SessionError::TooManyPins(pe_core::pins::MAX_PINS))?;
        self.set_param(id, key, ParamValue::Pins(pins))?;
        Ok(index)
    }

    /// Drag a pin. The origin stays where it was put — `at` is where the
    /// colour is, `to` is where it should go, and only the second moves.
    ///
    /// Refused for an index the set does not have: `Pins::get_mut` returns
    /// `None` and the drag would otherwise be swallowed.
    pub fn move_pin(
        &mut self,
        id: RowId,
        key: &str,
        index: usize,
        to: [f32; 2],
    ) -> Result<(), SessionError> {
        let mut pins = self.require_pins(id, key)?;
        let count = pins.len();
        let pin = pins
            .get_mut(index)
            .ok_or(SessionError::NoSuchPin { index, count })?;
        pin.to = to;
        self.set_param(id, key, ParamValue::Pins(pins))
    }

    /// The five controls that say how far a pin reaches and what it does
    /// there, set together — they are one panel and one undo step.
    #[allow(clippy::too_many_arguments)]
    pub fn set_pin_shape(
        &mut self,
        id: RowId,
        key: &str,
        index: usize,
        chroma_range: f32,
        tonal_low: f32,
        tonal_high: f32,
        tonal_pivot: f32,
        exposure: f32,
    ) -> Result<(), SessionError> {
        let mut pins = self.require_pins(id, key)?;
        let count = pins.len();
        let pin = pins
            .get_mut(index)
            .ok_or(SessionError::NoSuchPin { index, count })?;
        pin.chroma_range = chroma_range;
        pin.tonal_low = tonal_low;
        pin.tonal_high = tonal_high;
        pin.tonal_pivot = tonal_pivot;
        pin.exposure = exposure;
        self.set_param(id, key, ParamValue::Pins(pins))
    }

    /// Take a pin away.
    ///
    /// Refused past the end: `Pins::remove` ignores one silently, and a
    /// deletion that reports success and deletes nothing is worse than a
    /// deletion that fails.
    pub fn remove_pin(&mut self, id: RowId, key: &str, index: usize) -> Result<(), SessionError> {
        let mut pins = self.require_pins(id, key)?;
        let count = pins.len();
        if index >= count {
            return Err(SessionError::NoSuchPin { index, count });
        }
        pins.remove(index);
        self.set_param(id, key, ParamValue::Pins(pins))
    }

    /// The pin set at a key, or an error naming what was actually there.
    fn require_pins(&self, id: RowId, key: &str) -> Result<pe_core::pins::Pins, SessionError> {
        let effect = self.require_row(id)?;
        self.document()
            .ok_or(SessionError::NothingOpen)?
            .stack
            .get(id)
            .and_then(|r| r.params.get(key))
            .and_then(ParamValue::as_pins)
            .cloned()
            .ok_or(SessionError::NotAPinSet {
                effect,
                key: key.to_string(),
            })
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

    /// Show this rectangle of the frame, in frame coordinates.
    ///
    /// The working texture is built for a particular rectangle, so moving it
    /// invalidates that texture and every cached stage that reads it — which
    /// the stage cache already knows, because `Region` is part of its key.
    pub fn set_view(&mut self, x: f32, y: f32, size: f32) {
        let size = size.clamp(1.0 / 32.0, 1.0);
        let region = Region {
            offset: [x.clamp(0.0, 1.0 - size), y.clamp(0.0, 1.0 - size)],
            size: [size, size],
        };
        if region != self.view {
            self.view = region;
            self.gpu.working_size = (0, 0);
            self.gpu.working_region = None;
            self.needs_render = true;
        }
    }

    /// The visible rectangle, as the shell gave it: x, y and size in frame
    /// coordinates.
    pub fn view_region(&self) -> (f32, f32, f32) {
        (self.view.offset[0], self.view.offset[1], self.view.size[0])
    }

    /// Run the stack, returning a view of the graded frame in the working space.
    ///
    /// A view rather than the texture: `EffectRenderer::render` hands back a
    /// reference borrowed from the renderer, which cannot escape a method that
    /// also writes to `self`. A `TextureView` is a cheap clone of a handle and
    /// is all any consumer of a rendered frame wants.
    fn graded(
        &mut self,
        width: u32,
        height: u32,
        region: Region,
    ) -> Result<wgpu::TextureView, SessionError> {
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
        if self.gpu.working_size != (width, height)
            || self.gpu.working_geometry != Some(geometry)
            || self.gpu.working_region != Some(region)
        {
            let sampling =
                Sampling::of(&geometry, photo.image.width, photo.image.height).within(region);
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
            self.gpu.working_region = Some(region);
        }

        let working = self.gpu.working.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_mut().expect("built above");
        renderer.set_region(region);
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
        // Never `self.view`: an export is the photograph, not what happens to
        // be on screen. Passing the viewer's rectangle here would write out a
        // crop of the file that nobody asked for.
        let graded_view = self.graded(width, height, Region::FULL)?;
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

    // ---- scopes ---------------------------------------------------------

    /// Render the current grade at `width` by `height` and bin it.
    ///
    /// A separate, smaller render than the preview: 640 by 480 is three hundred
    /// thousand pixels, a 1.2 MB readback and a couple of milliseconds to bin,
    /// and the counts do not get better from more of them. The preview's own
    /// size is driven by the window and would make this cost whatever the user
    /// last dragged their corner to.
    pub fn measure_scopes(&mut self, width: u32, height: u32) -> Result<(), SessionError> {
        let pixels = self.render_offscreen(width, height)?;
        self.scopes = Some(Scopes::measure(&pixels, width as usize, height as usize));
        self.scope_generation += 1;
        Ok(())
    }

    /// The last measurement, if one has been taken since the last edit.
    ///
    /// `None` means "measure before you draw" rather than "there are no
    /// scopes" — see [`crate::scopes`] on why an edit throws them away.
    pub fn scopes(&self) -> Option<&Scopes> {
        self.scopes.as_ref()
    }

    /// Which measurement the session is holding, or zero for none.
    ///
    /// Zero before the first measurement *and* after an edit has thrown one
    /// away, so this one number answers both questions a shell has: is there
    /// anything to read, and is it the same as last time. Reporting the old
    /// number after an edit would answer only the second, and a shell that
    /// compared it and skipped the copy would keep drawing counts for a grade
    /// that is no longer on screen.
    ///
    /// Non-zero values are strictly increasing, which is what makes the
    /// comparison worth doing at all: a waveform is 2.6 MB.
    pub fn scope_generation(&self) -> u64 {
        if self.scopes.is_some() {
            self.scope_generation
        } else {
            0
        }
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
        let probe = unsafe { crate::surface::surface_on_layer(&self.instance, layer) }?;
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
            self.gpu.working_region = None;
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

        let graded_view = self.graded(width, height, self.view)?;
        let output = self
            .photo
            .as_ref()
            .expect("checked above")
            .history
            .document()
            .color
            .pipeline()
            .output;
        // Built here rather than in `graded`, because the format belongs to the
        // layer and nothing knows it until one is attached.
        let format = self.gpu.attached.as_ref().expect("checked above").format();
        if self.gpu.screen_format != Some(format) {
            let device = &self.gpu.context.as_ref().expect("built by graded").device;
            self.gpu.to_screen = Some(TransformPass::new(device, format));
            self.gpu.screen_format = Some(format);
        }

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
        self.gpu.to_screen.as_ref().expect("built above").encode(
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
    pub fn tick(&mut self) -> Result<(), SessionError> {
        let Some(photo) = self.photo.as_ref() else {
            return Ok(());
        };
        let revision = photo.history.revision();
        if self.watcher.tick(revision, std::time::Instant::now()) {
            self.write_autosave()?;
        }
        Ok(())
    }

    /// Write the work in progress now, throttle or no throttle.
    ///
    /// Called when leaving a photograph, where the throttle is beside the
    /// point: the thing that would have triggered the write is about to stop
    /// being the thing on screen.
    pub fn write_autosave(&mut self) -> Result<(), SessionError> {
        let Some(photo) = self.photo.as_ref() else {
            return Ok(());
        };
        let Some(path) = photo.path.as_ref() else {
            // The built-in chart has no file to be the edit *of*, so there is
            // nothing to keep. Not a failure.
            return Ok(());
        };
        let result = autosave::store(&self.support, path, photo.history.document());
        // The watcher is reset either way. It records what has been *attempted*
        // since the last change, and leaving it un-reset after a failure would
        // retry the same doomed write on every frame for as long as the
        // photograph stays open.
        let revision = photo.history.revision();
        self.watcher.reset(revision);
        result.map_err(|e| SessionError::Write(e.to_string()))
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
        self.gpu.working_region = None;
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
        self.gpu.working_region = None;
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

    fn warper_row(s: &Session) -> RowId {
        s.document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "colour_warper")
            .map(|r| r.id)
            .expect("the warper is a pinned row")
    }

    fn warp_of(s: &Session, id: RowId, key: &str) -> pe_core::Warp {
        s.document()
            .unwrap()
            .stack
            .get(id)
            .and_then(|r| r.params.get(key))
            .and_then(pe_core::ParamValue::as_warp)
            .cloned()
            .expect("that parameter is not a lattice")
    }

    fn pins_of(s: &Session, id: RowId) -> pe_core::pins::Pins {
        s.document()
            .unwrap()
            .stack
            .get(id)
            .and_then(|r| r.params.get("pins"))
            .and_then(pe_core::ParamValue::as_pins)
            .cloned()
            .expect("pins is not a pin set")
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
    fn a_wheel_keeps_its_master_apart_from_its_channels() {
        // Four numbers, not three. Folding the ring into the channels would
        // make "reset just the ring" impossible to express, which is why the
        // document models them separately and why the setter takes both.
        let mut s = chart_session();
        let row = s
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "primaries")
            .map(|r| r.id)
            .expect("primaries is a pinned row");

        s.set_wheel(row, "lift", 0.25, [0.1, 0.2, 0.3]).unwrap();

        let doc = s.document().unwrap();
        let Some(pe_core::ParamValue::Wheel(w)) = doc
            .stack
            .get(row)
            .and_then(|r| r.params.get("lift"))
            .cloned()
        else {
            panic!("lift is not a wheel");
        };
        assert_eq!(w.master, 0.25);
        assert_eq!(w.rgb, [0.1, 0.2, 0.3]);
    }

    #[test]
    fn a_wheel_on_a_parameter_that_is_not_one_is_refused() {
        let mut s = chart_session();
        let row = s.add_effect("sharpen").unwrap();
        // `amount` is a float; sending it a wheel is a bug in the shell, and
        // silently storing one would give the shader a value no slot reads.
        assert!(s.set_wheel(row, "not_a_parameter", 0.0, [0.0; 3]).is_err());
    }

    #[test]
    fn a_curve_can_be_set_from_a_flat_list_of_points() {
        let mut s = chart_session();
        let row = s
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "curves")
            .map(|r| r.id)
            .expect("curves is a pinned row");

        s.set_curve(row, "luma", &[[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]])
            .unwrap();

        let doc = s.document().unwrap();
        let Some(pe_core::ParamValue::Curve(c)) = doc
            .stack
            .get(row)
            .and_then(|r| r.params.get("luma"))
            .cloned()
        else {
            panic!("luma is not a curve");
        };
        assert_eq!(c.points.len(), 3);
        assert_eq!(c.points[1], [0.5, 0.7]);
    }

    #[test]
    fn a_curve_with_fewer_than_two_points_is_refused() {
        // One point is not a curve, and the evaluator falls back to the
        // identity for it — so storing one would silently discard whatever the
        // user had, and show them a straight line as though that were their
        // edit.
        let mut s = chart_session();
        let row = s
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "curves")
            .map(|r| r.id)
            .expect("curves is a pinned row");
        assert!(s.set_curve(row, "luma", &[[0.5, 0.5]]).is_err());
        assert!(s.set_curve(row, "luma", &[]).is_err());
    }

    #[test]
    fn a_curve_sent_to_a_parameter_that_is_not_one_is_refused() {
        let mut s = chart_session();
        let row = s.add_effect("sharpen").unwrap();
        assert!(
            s.set_curve(row, "amount", &[[0.0, 0.0], [1.0, 1.0]])
                .is_err()
        );
    }

    /// The shader reads a lattice's grid size from the divisions choice and
    /// the lattice carries its own. Nothing kept them in agreement, so
    /// changing the choice left the renderer indexing a 6x6 grid as though it
    /// were 8x8 — real numbers read from the wrong vertices.
    #[test]
    fn changing_the_divisions_resizes_the_lattice_it_describes() {
        let mut s = chart_session();
        let row = warper_row(&s);

        let grid_of = |s: &Session, key: &str| {
            s.document()
                .unwrap()
                .stack
                .get(row)
                .and_then(|r| r.params.get(key))
                .and_then(pe_core::ParamValue::as_warp)
                .map(|w| (w.cols(), w.rows()))
                .unwrap()
        };

        assert_eq!(grid_of(&s, "hue_sat"), (6, 6), "the default grid");

        s.set_choice(row, "hue_divisions", "8").unwrap();
        assert_eq!(
            grid_of(&s, "hue_sat"),
            (8, 6),
            "the hue axis did not follow its own divisions control"
        );

        s.set_choice(row, "sat_divisions", "12").unwrap();
        assert_eq!(grid_of(&s, "hue_sat"), (8, 12));

        // The rectangular grids are driven by their own pair, and both of them
        // follow it — they are one control over two lattices.
        s.set_choice(row, "chroma_divisions", "4").unwrap();
        assert_eq!(grid_of(&s, "chroma_luma_1"), (4, 6));
        assert_eq!(grid_of(&s, "chroma_luma_2"), (4, 6));
        // And the hue web is not disturbed by them.
        assert_eq!(grid_of(&s, "hue_sat"), (8, 12));
    }

    #[test]
    fn a_vertex_is_read_back_where_it_was_put() {
        let mut s = chart_session();
        let row = warper_row(&s);
        s.set_warp_vertex(row, "hue_sat", 2, 3, [0.25, -0.1])
            .unwrap();
        let w = warp_of(&s, row, "hue_sat");
        assert_eq!(w.at(2, 3), [0.25, -0.1]);
        assert!(!w.is_identity());
    }

    #[test]
    fn a_vertex_outside_the_grid_is_refused_rather_than_dropped() {
        // `Warp::set` silently ignores an out-of-range vertex. Over the C ABI
        // that would be a call that reports success and does nothing, which is
        // the hardest kind of bug to see from the far side of a boundary.
        let mut s = chart_session();
        let row = warper_row(&s);
        assert!(
            s.set_warp_vertex(row, "hue_sat", 99, 0, [0.1, 0.1])
                .is_err()
        );
        assert!(
            s.set_warp_vertex(row, "hue_sat", 0, 99, [0.1, 0.1])
                .is_err()
        );
    }

    #[test]
    fn a_vertex_sent_to_a_parameter_that_is_not_a_lattice_is_refused() {
        let mut s = chart_session();
        let row = warper_row(&s);
        assert!(
            s.set_warp_vertex(row, "axis_angle", 0, 0, [0.1, 0.1])
                .is_err()
        );
    }

    #[test]
    fn a_lattice_can_be_put_back_to_nothing() {
        let mut s = chart_session();
        let row = warper_row(&s);
        s.set_warp_vertex(row, "hue_sat", 1, 1, [0.2, 0.2]).unwrap();
        assert!(!warp_of(&s, row, "hue_sat").is_identity());
        s.clear_warp(row, "hue_sat").unwrap();
        assert!(warp_of(&s, row, "hue_sat").is_identity());
        // Clearing keeps the grid size — it undoes the drag, not the setup.
        assert_eq!(warp_of(&s, row, "hue_sat").cols(), 6);
    }

    /// Resizing resamples. A colourist who has pulled a grid around and then
    /// wants it finer is asking for more control points, not for their work
    /// back.
    #[test]
    fn a_finer_grid_keeps_the_shape_that_was_drawn_on_the_coarse_one() {
        let mut s = chart_session();
        let row = warper_row(&s);
        for c in 0..6 {
            s.set_warp_vertex(row, "hue_sat", c, 0, [0.0, 0.3]).unwrap();
        }
        s.set_choice(row, "hue_divisions", "12").unwrap();
        let w = warp_of(&s, row, "hue_sat");
        assert_eq!(w.cols(), 12);
        assert!(
            (w.sample(0.5, 0.0, true)[1] - 0.3).abs() < 0.05,
            "the shape was lost: {:?}",
            w.sample(0.5, 0.0, true)
        );
    }

    #[test]
    fn a_pin_is_placed_where_it_was_asked_for_and_does_nothing_yet() {
        let mut s = chart_session();
        let row = warper_row(&s);
        assert_eq!(s.add_pin(row, "pins", [0.33, 0.35]).unwrap(), 0);
        let pins = pins_of(&s, row);
        assert_eq!(pins.len(), 1);
        let p = pins.get(0).unwrap();
        assert_eq!(p.at, [0.33, 0.35]);
        assert_eq!(p.to, p.at, "a fresh pin has not been dragged");
        assert!(
            p.is_neutral(),
            "placing a pin should not change the picture"
        );
    }

    #[test]
    fn a_pin_moves_where_it_is_dragged() {
        let mut s = chart_session();
        let row = warper_row(&s);
        s.add_pin(row, "pins", [0.33, 0.35]).unwrap();
        s.move_pin(row, "pins", 0, [0.40, 0.30]).unwrap();
        let pins = pins_of(&s, row);
        assert_eq!(pins.get(0).unwrap().to, [0.40, 0.30]);
        assert_eq!(
            pins.get(0).unwrap().at,
            [0.33, 0.35],
            "the origin stays put"
        );
        assert!(!pins.get(0).unwrap().is_neutral());
    }

    #[test]
    fn a_pins_shape_can_be_set_in_one_call() {
        let mut s = chart_session();
        let row = warper_row(&s);
        s.add_pin(row, "pins", [0.33, 0.35]).unwrap();
        s.set_pin_shape(row, "pins", 0, 0.12, 0.2, 0.9, 0.6, 0.75)
            .unwrap();
        let pins = pins_of(&s, row);
        let p = pins.get(0).unwrap();
        assert_eq!(p.chroma_range, 0.12);
        assert_eq!(p.tonal_low, 0.2);
        assert_eq!(p.tonal_high, 0.9);
        assert_eq!(p.tonal_pivot, 0.6);
        assert_eq!(p.exposure, 0.75);
        // Exposure alone is enough to make a pin do something, even undragged.
        assert!(!p.is_neutral());
    }

    #[test]
    fn a_pin_can_be_removed_and_the_others_stay() {
        let mut s = chart_session();
        let row = warper_row(&s);
        s.add_pin(row, "pins", [0.1, 0.1]).unwrap();
        s.add_pin(row, "pins", [0.2, 0.2]).unwrap();
        s.remove_pin(row, "pins", 0).unwrap();
        let pins = pins_of(&s, row);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins.get(0).unwrap().at, [0.2, 0.2]);
    }

    #[test]
    fn a_pin_that_is_not_there_is_refused_rather_than_ignored() {
        // `Pins::remove` and `get_mut` both ignore an out-of-range index. Over
        // the C ABI that is a call reporting success and doing nothing, which
        // is the hardest kind of bug to see from the far side.
        let mut s = chart_session();
        let row = warper_row(&s);
        assert!(s.move_pin(row, "pins", 0, [0.4, 0.4]).is_err());
        assert!(s.remove_pin(row, "pins", 0).is_err());
        assert!(
            s.set_pin_shape(row, "pins", 0, 0.1, 1.0, 1.0, 0.5, 0.0)
                .is_err()
        );
    }

    #[test]
    fn a_ninth_pin_is_refused() {
        // Bounded because they travel to the GPU inside the curve LUT, and
        // because the honest number is small: past a handful you are
        // describing a field, and the grid views already do fields.
        let mut s = chart_session();
        let row = warper_row(&s);
        for i in 0..pe_core::pins::MAX_PINS {
            assert_eq!(s.add_pin(row, "pins", [0.1 * i as f32, 0.3]).unwrap(), i);
        }
        assert!(s.add_pin(row, "pins", [0.5, 0.5]).is_err());
        assert_eq!(pins_of(&s, row).len(), pe_core::pins::MAX_PINS);
    }

    #[test]
    fn pins_sent_to_a_parameter_that_is_not_a_pin_set_are_refused() {
        let mut s = chart_session();
        let row = warper_row(&s);
        assert!(s.add_pin(row, "axis_angle", [0.3, 0.3]).is_err());
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
        s.write_autosave().unwrap();

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
        s.write_autosave().unwrap();
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
    #[test]
    fn moving_the_view_invalidates_the_texture_built_for_the_old_one() {
        // The working texture is built for a particular rectangle of the frame.
        // Leaving it in place after the view moves would show the previous
        // rectangle, scaled — the picture would appear to zoom and then not
        // resolve.
        let mut s = chart_session();
        s.render_offscreen(64, 64).unwrap();
        assert!(!s.needs_render());

        s.set_view(0.25, 0.25, 0.25);
        assert!(s.needs_render(), "moving the view did not ask for a frame");
        assert_eq!(
            s.view_region(),
            (0.25, 0.25, 0.25),
            "the view did not go where it was sent"
        );
    }

    #[test]
    fn a_view_cannot_be_pushed_off_the_frame() {
        // Clamped, so there is never a band of nothing along an edge.
        let mut s = chart_session();
        s.set_view(5.0, -5.0, 0.5);
        let (x, y, size) = s.view_region();
        assert_eq!(size, 0.5);
        assert!((0.0..=0.5).contains(&x), "x escaped the frame: {x}");
        assert!((0.0..=0.5).contains(&y), "y escaped the frame: {y}");
    }

    #[test]
    fn a_view_cannot_zoom_past_a_single_pixel_of_use() {
        let mut s = chart_session();
        s.set_view(0.0, 0.0, 0.0001);
        assert_eq!(s.view_region().2, 1.0 / 32.0);
    }

    #[test]
    fn an_export_renders_the_whole_frame_however_the_viewer_is_zoomed() {
        // The one that would be a real bug: exporting what is on screen rather
        // than what is in the file.
        let mut s = chart_session();
        let fitted = s.render_offscreen(64, 64).unwrap();
        s.set_view(0.25, 0.25, 0.25);
        let zoomed = s.render_offscreen(64, 64).unwrap();
        assert_eq!(fitted, zoomed, "the export followed the viewer");
    }

    #[test]
    fn measuring_bins_the_frame_that_was_graded() {
        let mut s = chart_session();
        // The test chart is a colour target: it must produce counts in more
        // than one bin, or the scope is measuring a blank.
        s.measure_scopes(160, 120).unwrap();
        let scopes = s.scopes().expect("measured");
        assert!(scopes.histogram.total > 0);
        assert_eq!(
            scopes.histogram.total,
            160 * 120,
            "every pixel should be counted exactly once"
        );
        let occupied = scopes.histogram.luma.iter().filter(|c| **c > 0).count();
        assert!(occupied > 1, "a colour chart binned into one level");
        assert_eq!(scopes.waveform.columns(), 160);
        assert_eq!(scopes.waveform.rows(), 120);
    }

    #[test]
    fn the_generation_moves_only_when_something_was_measured() {
        let mut s = chart_session();
        assert_eq!(s.scope_generation(), 0, "nothing measured yet");
        s.measure_scopes(64, 64).unwrap();
        let first = s.scope_generation();
        assert!(first > 0);
        s.measure_scopes(64, 64).unwrap();
        assert!(
            s.scope_generation() > first,
            "a second measurement should be tellable from the first"
        );
    }

    /// The generation is how a shell decides whether to copy 2.6 MB again. If
    /// it kept reporting the old number after an edit dropped the measurement,
    /// a shell would compare it, see no change, skip the copy, and go on
    /// drawing a scope of a photograph that is no longer on screen.
    #[test]
    fn the_generation_goes_back_to_nothing_when_an_edit_drops_the_measurement() {
        let mut s = chart_session();
        s.measure_scopes(64, 64).unwrap();
        assert!(s.scope_generation() > 0);

        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 1.5).unwrap();
        assert_eq!(
            s.scope_generation(),
            0,
            "a stale generation would let a shell skip the copy it needed"
        );

        // And measuring again is tellable from the first, rather than starting
        // over at one and colliding with a number a shell already holds.
        s.measure_scopes(64, 64).unwrap();
        assert!(s.scope_generation() > 1);
    }

    #[test]
    fn measuring_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(s.measure_scopes(64, 64).is_err());
        assert!(s.scopes().is_none());
    }

    #[test]
    fn an_edit_does_not_silently_leave_stale_scopes_behind() {
        // The counts describe a particular grade. Handing back numbers measured
        // before an edit would draw a scope of a picture that is no longer on
        // screen, which is the one thing a scope must never do.
        let mut s = chart_session();
        s.measure_scopes(64, 64).unwrap();
        assert!(s.scopes().is_some());
        let row = s.add_effect("exposure").unwrap();
        // "ev" is the exposure effect's only parameter; the plan wrote
        // "exposure", which the session rightly refuses.
        s.set_float(row, "ev", 1.5).unwrap();
        assert!(
            s.scopes().is_none(),
            "an edit should discard the measurement it invalidated"
        );
    }
}
