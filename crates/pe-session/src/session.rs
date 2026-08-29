//! The open photograph, its edit, and the GPU objects between them.
//!
//! What a shell talks to. It reads the stack, mutates a parameter, asks for a
//! frame, and draws it — which is the same vocabulary `apps/windows` has, with
//! the parts that were never about interface moved down here where the Mac and
//! the iPad can reach them.

use std::ops::Range;
use std::path::{Path, PathBuf};

use pe_color::space;
use pe_core::{Document, Geometry, History, ParamValue, RowId, RowIdGenerator, StackRow};
use pe_io::DecodedImage;
use pe_render::{
    EffectRenderer, GpuContext, ImageTexture, Part, Placement, Rect, Region, Sampling,
    TransformPass,
};

use crate::library::{self, Library};
use crate::scopes::Scopes;
use crate::surface::Attached;
use crate::{Settings, Support, autosave, export};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("nothing is open")]
    NothingOpen,
    /// Distinct from [`SessionError::NothingOpen`] on purpose: a shell that
    /// greyed its Paste correctly should never see this, and one that did not
    /// should be told which of the two it got wrong.
    #[error("no grade has been copied")]
    NothingCopied,
    /// Distinct from a read failure: the folder opened and had nothing in it
    /// this application can read, which is worth saying differently from "that
    /// folder is not there".
    #[error("no photographs in {0}")]
    NoPhotographs(String),
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
    #[error("no photograph at {index}, of {count} open")]
    NoSuchPhoto { index: usize, count: usize },
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

/// How the graded picture is being held up against the ungraded one.
///
/// Both modes exist because they answer different questions. A wipe is for
/// "did that move go too far" — the eye reads a discontinuity across a seam far
/// more finely than it reads two pictures a hand's width apart. Side by side is
/// for "which of these do I prefer", where a seam would fuse the two into one
/// image and stop you seeing either.
///
/// That argument decides the shapes: a wipe has **no gap and no scaling
/// difference** between its halves, because they are one picture with a seam,
/// and a side by side has a real gap. See [`Session::set_compare`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Compare {
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
    pub fn next(self) -> Self {
        match self {
            Compare::Off => Compare::Wipe,
            Compare::Wipe => Compare::Side,
            Compare::Side => Compare::Off,
        }
    }

    /// Whether anything is being compared at all.
    pub fn on(self) -> bool {
        self != Compare::Off
    }
}

/// The gap between the two halves of a side by side, in pixels.
const SIDE_GAP: u32 = 8;

/// Where the two half-size pictures sit in a target that size: before, after.
///
/// Half the width each less the gap, with the height brought down by the same
/// factor so neither picture is stretched, and both centred vertically. The gap
/// is the whole point of the mode — two pictures that touch fuse into one — and
/// it is bounded by a quarter of the frame so that a very small target still
/// gets two pictures rather than a gap with slivers either side.
fn side_rects(width: u32, height: u32) -> (Rect, Rect) {
    let gap = SIDE_GAP.min(width / 4);
    let w = (width - gap) / 2;
    let h = (u64::from(height) * u64::from(w) / u64::from(width.max(1))) as u32;
    let y = (height - h) / 2;
    (
        Rect {
            x: 0,
            y,
            width: w,
            height: h,
        },
        Rect {
            x: width - w,
            y,
            width: w,
            height: h,
        },
    )
}

/// The viewer's surround, as a clear value.
///
/// Read from the one palette both shells read, so a side by side does not sit
/// on a different grey depending on which of them drew it. Linear, because a
/// clear value is: the transfer function belongs to the render target's format,
/// which is the same rule the transform pass is built on.
fn surround() -> wgpu::Color {
    let linear = |v: u8| pe_color::TransferFn::Srgb.decode(f64::from(v) / 255.0);
    let c = pe_theme::colour::VIEWER;
    wgpu::Color {
        r: linear(c.r),
        g: linear(c.g),
        b: linear(c.b),
        a: 1.0,
    }
}

/// The photograph that is open, and its edit.
/// Screen pixels per image pixel, from the three numbers that decide it.
///
/// A free function so it can be checked without a window: [`Session::view_scale`]
/// needs a layer attached, and a headless test has none.
///
/// `region` is the visible fraction of the frame on each axis. The picture is
/// letterboxed into the viewport, so the axis that runs out first is the one
/// that sets the scale — which is why this is a `min` and not a choice of one
/// axis.
fn view_scale_of(viewport: (u32, u32), frame: (u32, u32), region: [f32; 2]) -> Option<f32> {
    let visible_w = frame.0 as f32 * region[0];
    let visible_h = frame.1 as f32 * region[1];
    if visible_w <= 0.0 || visible_h <= 0.0 {
        return None;
    }
    Some((viewport.0 as f32 / visible_w).min(viewport.1 as f32 / visible_h))
}

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
    /// The geometry the working texture was built *from* — the framing, which
    /// is the document's crop or, while the crop tool is open, the enclosing
    /// frame. Holding the framing rather than the document's own is what makes
    /// `Session::set_cropping` invalidate this guard, and only when the two
    /// frames actually differ.
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
    /// The set the open photograph belongs to, and the parked edit of every
    /// other photograph in it.
    ///
    /// `None` until a set is opened, because a [`Library`] is built around the
    /// paths it holds and the support directory its edits are kept in — and at
    /// [`Session::new`] the host has said neither. A session showing the
    /// built-in chart has no set either: there is no file for a filmstrip to be
    /// a strip of.
    library: Option<Library>,
    /// The grade copied off a photograph, waiting to be put on another.
    ///
    /// On the session rather than in a shell, which is where it was: two shells
    /// would otherwise each keep their own, and the one thing a clipboard must
    /// not be is two clipboards. It is not persisted — a grade in hand belongs
    /// to the sitting you copied it in.
    ///
    /// The *stack* and nothing else. A crop is about the frame it was drawn on,
    /// and carrying one onto a photograph of another shape is almost never what
    /// anybody meant; see [`Library::paste_stack_to_all`], which says the same.
    clipboard: Option<pe_core::Stack>,
    support: Support,
    /// What this person keeps between runs: the effects they have starred,
    /// the set they had open, and how they export.
    ///
    /// On the session rather than in a shell because none of it is a question
    /// about a window — see [`Settings`]. It holds the export choice too,
    /// which is why there is no separate field for it: two homes for one
    /// answer is two answers, and the one on disc would be the one that was
    /// wrong.
    settings: Settings,
    /// The batch export in progress, if one is.
    ///
    /// On the session rather than on the shell because the run *is* the
    /// engine's: which document each photograph is exported with is a question
    /// only the thing holding the set and the histories can answer, and two
    /// shells answering it separately is two answers.
    batch: Option<export::Batch>,
    /// Every photograph currently open, for the collision check. The one on
    /// screen is in here too. A batch writes into one folder and the name it
    /// builds for photo A can collide with photo B sitting right beside it.
    open_set: Vec<PathBuf>,
    /// Which rectangle of the frame the viewer is showing. A property of the
    /// window rather than of the document: two windows on one photograph would
    /// disagree about it and both be right.
    view: Region,
    /// Whether the viewer is showing the whole straightened source rather than
    /// the crop. See [`Session::set_cropping`]. A property of the window like
    /// `view`, not of the document: it changes what is drawn and nothing about
    /// what would be exported.
    cropping: bool,
    /// Whether the viewer is showing the photograph with the stack switched
    /// off. See [`Session::set_bypass_all`].
    bypass_all: bool,
    /// Which comparison the viewer is showing, and where its seam sits as a
    /// fraction of the frame's width. A property of the window like `view` and
    /// `cropping`: it changes what is drawn and nothing about what would be
    /// exported.
    compare: Compare,
    wipe: f32,
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
            library: None,
            clipboard: None,
            support: Support::default(),
            settings: Settings::default(),
            batch: None,
            open_set: Vec::new(),
            view: Region::FULL,
            cropping: false,
            bypass_all: false,
            compare: Compare::default(),
            wipe: 0.5,
            watcher: autosave::Watcher::new(),
            snapshot_version: 0,
            interaction: None,
            needs_render: true,
            scopes: None,
            scope_generation: 0,
        }
    }

    /// Where this host keeps the application's own files. See [`Support`].
    ///
    /// Said once, at start-up, before anything is opened. A [`Library`] is
    /// built around the support directory in force when the set was opened, so
    /// moving it afterwards would leave the parked edits reading from the old
    /// one — and rebuilding the library to fix that would throw those edits
    /// away, which is worse than the problem.
    pub fn set_support_dir(&mut self, root: impl Into<PathBuf>) {
        self.support = Support::at(root);
        // And this is the moment there is anywhere to read the last run's
        // answers from. Before it a session has the defaults, because a host
        // that has named no directory has not said where they were kept.
        self.settings = Settings::load(&self.support);
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

    /// The crop, straighten and flips the document holds, or `None` when
    /// nothing is open.
    pub fn geometry(&self) -> Option<Geometry> {
        Some(self.document()?.geometry)
    }

    /// Set the crop, straighten and flips, and return what was actually
    /// stored.
    ///
    /// The engine corrects: a crop is brought inside the straightened source
    /// and a locked aspect is honoured. The caller is handed the corrected
    /// value rather than the one it asked for, because the alternative is a
    /// shell drawing a rectangle the renderer will not produce — and because
    /// the rules live on [`Geometry`], where they are tested, rather than in
    /// each shell.
    ///
    /// The order is `apps/windows/src/crop.rs`'s. Its aspect button sets the
    /// lock and calls `apply_aspect` before the fit, and its `edit` closes
    /// every change with `shrink_to_fit`; the one exception is Position, which
    /// slides instead, because *moving* a rectangle does not make it stop
    /// fitting the way *straightening* it does, and shrinking there would let
    /// one control quietly write another's value. A whole proposed geometry is
    /// both at once, so it gets both, in that order: shape it, slide it back
    /// from wherever the crop legally was, and shrink only if it still cannot
    /// fit anywhere.
    pub fn set_geometry(&mut self, want: Geometry) -> Result<Geometry, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let (w, h) = (photo.image.width, photo.image.height);
        let from = photo.history.document().geometry.centre;

        let mut g = want;
        g.turns %= 4;
        g.apply_aspect(w, h);
        g.slide_to_fit(from, w, h);
        g.shrink_to_fit(w, h);

        self.edit("Crop", move |doc| doc.geometry = g)?;
        Ok(g)
    }

    /// Show the whole straightened source rather than the crop.
    ///
    /// While the crop tool is open the viewer has to show what is being cut
    /// away, or there is nothing to drag back into. [`Geometry::enclosing`] is
    /// what that frame is, and it is computed here rather than passed in so no
    /// shell has to know how — `apps/windows` passes the frame itself, and the
    /// two shells would then hold two copies of the same rule.
    ///
    /// A property of the window, not of the document: it is not an edit, it is
    /// not in the history, and [`Session::export_current`] renders the document
    /// either way — it does not go through the framing at all.
    ///
    /// [`Session::measure_scopes`] does, because it reads back the same graded
    /// frame, so while this is on the counts are the enclosing frame's rather
    /// than the crop's — the blank corners included. That is what "the scopes
    /// describe what was just drawn" means here, and it is where `apps/windows`
    /// differs: it keeps a second texture for the scopes and measures the
    /// document's crop through it. Worth revisiting when the two panels are
    /// open together often enough for anyone to mind.
    pub fn set_cropping(&mut self, cropping: bool) {
        if self.cropping == cropping {
            return;
        }
        self.cropping = cropping;
        // The working texture is guarded on the geometry it was *built from*,
        // which is the framing rather than the document's crop — so the flag
        // invalidates that guard by itself, and only when the two frames
        // actually differ. `needs_render` is the other half: without it no
        // frame is asked for at all, and the viewer would sit on the old
        // picture until something else moved.
        self.needs_render = true;
    }

    /// Whether the viewer is showing the whole straightened source.
    pub fn cropping(&self) -> bool {
        self.cropping
    }

    /// Show the photograph with the whole stack switched off, or stop.
    ///
    /// The cheapest honest bypass: render an empty stack. It costs one frame of
    /// invalidation, and toggling back is free because the row fingerprints
    /// have not changed — the stage cache still holds every one of them.
    ///
    /// The *stack* only. The colour pipeline stays, because that is what the
    /// file's pixels mean rather than anything done to them, and the geometry
    /// stays because a bypass is not a way to see outside the crop — that is
    /// what the Image tab is for.
    ///
    /// A property of the window like [`Session::set_cropping`]: not an edit,
    /// not in the history, and an export renders the document either way.
    /// Somebody who bypassed the stack to look at the original and then
    /// exported would otherwise write the original out over their work.
    pub fn set_bypass_all(&mut self, bypass: bool) {
        if self.bypass_all == bypass {
            return;
        }
        self.bypass_all = bypass;
        self.needs_render = true;
    }

    /// Whether the viewer is showing the photograph with the stack off.
    pub fn bypass_all(&self) -> bool {
        self.bypass_all
    }

    /// Hold the graded picture up against the ungraded one, or stop.
    ///
    /// `wipe` is where the seam sits, as a fraction of the frame's width from
    /// the left, and it is kept while the mode is [`Compare::Off`] so that
    /// cycling back round into a wipe puts the seam where the user left it. It
    /// is clamped here, so a shell dragging past an edge gets the edge and the
    /// scissor arithmetic never sees anything it cannot use.
    ///
    /// A property of the window, not of the document: it is not an edit, it is
    /// not in the history, and [`Session::export_current`] renders the document
    /// either way — the export path does not go through this at all.
    ///
    /// [`Session::measure_scopes`] does, because it reads back the composited
    /// frame, so while a comparison is on the counts describe what is on screen,
    /// both halves of it. That is the same bargain [`Session::set_cropping`]
    /// makes, and for the same reason: the scopes describe what was drawn.
    pub fn set_compare(&mut self, mode: Compare, wipe: f32) {
        let wipe = if wipe.is_nan() {
            0.0
        } else {
            wipe.clamp(0.0, 1.0)
        };
        if self.compare == mode && self.wipe == wipe {
            return;
        }
        self.compare = mode;
        self.wipe = wipe;
        // Nothing here invalidates the working texture — the ungraded frame
        // *is* the working texture — so this is the whole of the invalidation.
        // Without it no frame is asked for at all, and the viewer would sit on
        // the uncompared picture until something else moved.
        self.needs_render = true;
    }

    /// Which comparison the viewer is showing, if any.
    pub fn compare(&self) -> Compare {
        self.compare
    }

    /// Where a wipe's seam sits, as a fraction of the frame's width.
    pub fn wipe(&self) -> f32 {
        self.wipe
    }

    /// The geometry the *viewer* is showing, which is normally the document's
    /// own, or `None` when nothing is open.
    ///
    /// The counterpart of `framing` in `apps/windows/src/preview.rs::render`,
    /// except that this is derived from [`Session::set_cropping`] rather than
    /// handed in.
    pub fn framing(&self) -> Option<Geometry> {
        let photo = self.photo.as_ref()?;
        Some(Self::framing_of(
            photo.history.document().geometry,
            self.cropping,
            photo.image.width,
            photo.image.height,
        ))
    }

    /// The one place the flag turns into a frame. Everything that renders, and
    /// everything that answers where the crop is, goes through it.
    fn framing_of(geometry: Geometry, cropping: bool, source_w: u32, source_h: u32) -> Geometry {
        if cropping {
            geometry.enclosing(source_w, source_h)
        } else {
            geometry
        }
    }

    /// Where the crop sits inside the frame the viewer is showing, as min x,
    /// min y, max x, max y in that frame's uv.
    ///
    /// [`Geometry::crop_uv_in`] against [`Session::framing`]. It exists so no
    /// shell has to hold a second copy of it: with the tool closed the crop
    /// *is* the frame and the answer is the whole of it, and with the tool open
    /// it is the rectangle the overlay draws.
    pub fn crop_in_frame(&self) -> Result<[f32; 4], SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let (w, h) = (photo.image.width, photo.image.height);
        let geometry = photo.history.document().geometry;
        Ok(geometry.crop_uv_in(&Self::framing_of(geometry, self.cropping, w, h), w, h))
    }

    /// Move the crop to a rectangle of the frame being shown, and answer where
    /// it actually landed.
    ///
    /// [`Geometry::set_crop_uv_in`] followed by [`Session::set_geometry`], so
    /// the same corrections apply — a locked aspect re-shapes it, and it is
    /// slid, then shrunk, back inside the straightened source. **What comes
    /// back is frequently not the rectangle passed in, and that is the point:**
    /// it is where the crop now is, in the same frame it was read from, so a
    /// shell can draw the answer rather than its own proposal.
    pub fn set_crop_in_frame(&mut self, rect: [f32; 4]) -> Result<[f32; 4], SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let (w, h) = (photo.image.width, photo.image.height);
        let geometry = photo.history.document().geometry;
        let mut want = geometry;
        want.set_crop_uv_in(&Self::framing_of(geometry, self.cropping, w, h), w, h, rect);
        let got = self.set_geometry(want)?;
        // Against the frame as it is *after* the edit, so this answers exactly
        // what `crop_in_frame` would now answer. Nothing a crop drag changes
        // moves the enclosing frame — it turns on the angle, the turns and the
        // flips, and none of those is being set here — but reading it back
        // rather than reusing the old one is what makes that a fact about the
        // code instead of a fact about this comment.
        Ok(got.crop_uv_in(&Self::framing_of(got, self.cropping, w, h), w, h))
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
        self.open_paths(vec![path.as_ref().to_path_buf()])
    }

    /// Open every photograph in a folder, focused on the first.
    ///
    /// Returns how many were found, which a shell says out loud — opening a
    /// folder is the one command whose result is otherwise invisible when it
    /// works and indistinguishable from a refusal when it does not.
    ///
    /// The scanning is [`Library::scan`]'s: which extensions count and what
    /// order they come back in is one answer, and a shell enumerating the
    /// directory itself would be a second copy of it that drifts.
    ///
    /// An empty folder is refused rather than opened. A set of nothing would
    /// leave the session with no photograph and a filmstrip of no cells, which
    /// reads as a folder that failed to load rather than one with no pictures
    /// in it.
    pub fn open_folder(&mut self, dir: impl AsRef<Path>) -> Result<usize, SessionError> {
        let dir = dir.as_ref();
        let found = Library::scan(dir);
        if found.is_empty() {
            return Err(SessionError::NoPhotographs(dir.display().to_string()));
        }
        let n = found.len();
        self.open_paths(found)?;
        Ok(n)
    }

    /// Open a set of photographs, focused on the first.
    ///
    /// Only the first is decoded — a 24-megapixel frame is 96 MB of RGBA, so a
    /// folder of two hundred would be twenty gigabytes. The rest are paths and
    /// parked edits until one of them is [`Session::focus`]ed.
    ///
    /// One path is the case that existed before this took a list, and it still
    /// behaves exactly as it did: the autosave decides what the document is,
    /// and a `.peproj` beside the photograph is pulled over the top only when
    /// somebody asks for it.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), SessionError> {
        let Some(first) = paths.first().cloned() else {
            // Opening nothing is the caller's mistake, not a session with an
            // empty set in it — and saying so here is cheaper than every
            // reader of `library()` having to cope with a set of no
            // photographs.
            return Err(SessionError::NothingOpen);
        };
        // The pixels first: a photograph that will not decode leaves the
        // session exactly as it was, rather than half-swapped.
        let image = pe_io::load(&first).map_err(|e| SessionError::Read {
            path: first.display().to_string(),
            message: e.to_string(),
        })?;
        // The autosave wins over a fresh document, because it is where the
        // person happened to stop. A sidecar is pulled over the top explicitly.
        let doc = autosave::load(&self.support, &first)
            .unwrap_or_else(|| pe_effects::new_document(first.to_string_lossy()));
        self.adopt(Some(first), image, doc);
        // Every photograph in the set is one an export must not land on. The
        // name built for photo A can collide with photo B sitting right beside
        // it in the same folder, and now that the session holds the set it is
        // the one that knows.
        self.open_set = paths.clone();
        let mut library = Library::new(paths, self.support.clone());
        // The first entry's edit is in hand rather than parked, which is what
        // `focus` means here — see [`Library::focus`], which points the set at
        // an entry the caller has already opened.
        library.focus(0);
        self.library = Some(library);
        // Written down now rather than on the way out: this is what the next
        // launch reopens, and a window can be closed by the operating system
        // or by a crash long before anybody thinks to tidy up.
        self.remember_the_set();
        Ok(())
    }

    /// Open the built-in chart, for a session with no file behind it.
    pub fn open_test_chart(&mut self, width: u32, height: u32) -> Result<(), SessionError> {
        let image = pe_io::test_chart(width, height);
        let doc = pe_effects::new_document("test-chart");
        self.adopt(None, image, doc);
        Ok(())
    }

    /// Show a different photograph, parking the current edit and taking that
    /// one's.
    ///
    /// The edit is [`Library::switch`]'s business and the pixels are this
    /// function's. Both shells were orchestrating that pair by hand; one of
    /// them had to forget eventually.
    ///
    /// **The autosave goes out first.** Parking an edit is a promise to
    /// remember it, and memory is what a crash takes: an editor that parks
    /// four photographs' work and writes only the fifth loses four. So the
    /// outgoing document is written before it stops being the one on screen,
    /// with the throttle skipped — the change that would have triggered the
    /// write is exactly the one about to be put away. A write that fails is
    /// reported *and nothing moves*, because carrying on is the one outcome
    /// that turns a failed write into lost work.
    pub fn focus(&mut self, index: usize) -> Result<(), SessionError> {
        let library = self.library.as_ref().ok_or(SessionError::NothingOpen)?;
        let count = library.len();
        if index >= count {
            // The set can shrink between a strip being drawn and a thumbnail
            // in it being clicked, and a restored session names an index from
            // a folder that may have lost photographs since.
            return Err(SessionError::NoSuchPhoto { index, count });
        }
        if index == library.current() {
            return Ok(());
        }
        let path = library
            .path(index)
            .expect("in range, checked above")
            .to_path_buf();

        // The pixels before anything is parked, so that a photograph which
        // will not decode leaves the set where it was.
        let image = pe_io::load(&path).map_err(|e| SessionError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;

        self.park_the_outgoing_edit()?;

        // A placeholder to move the outgoing history out wholesale rather than
        // clone it: `History` deliberately is not `Clone`, because an undo
        // stack with two owners is a bug waiting to happen.
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let outgoing = std::mem::replace(
            &mut photo.history,
            History::new(Document::from_path(String::new())),
        );
        let outgoing_ids = std::mem::take(&mut photo.ids);
        let (history, ids) = self.library.as_mut().expect("checked above").switch(
            index,
            outgoing,
            outgoing_ids,
            image.space,
        );
        self.install(Some(path), image, history, ids);
        // Which photograph you were on is half of reopening; remembering the
        // set and forgetting the place in it puts you back at the front of a
        // folder of two hundred.
        self.remember_the_set();
        Ok(())
    }

    /// The set the open photograph belongs to, or `None` when there is no set
    /// — nothing open, or the built-in chart, which is not a file.
    pub fn library(&self) -> Option<&Library> {
        self.library.as_ref()
    }

    /// Ask for the thumbnails of a range of the set that have not been asked
    /// for yet. See [`Library::request`].
    pub fn request_thumbnails(&mut self, range: Range<usize>) {
        if let Some(library) = self.library.as_mut() {
            library.request(range);
        }
    }

    /// Take delivery of whatever the worker finished. True if anything did.
    pub fn collect_thumbnails(&mut self) -> bool {
        self.library.as_mut().is_some_and(Library::collect)
    }

    /// Write the open photograph's document out if it has moved since the last
    /// write, throttle or no throttle.
    ///
    /// The condition is the single-photograph path's: [`Session::tick`] writes
    /// when the revision has gone past what was last written, and this asks the
    /// same question without the idle timer. Not writing an unchanged document
    /// is what keeps merely *visiting* a photograph from leaving an autosave
    /// that would afterwards shadow the `.peproj` beside it.
    fn park_the_outgoing_edit(&mut self) -> Result<(), SessionError> {
        let Some(photo) = self.photo.as_ref() else {
            return Ok(());
        };
        if !self.watcher.unsaved(photo.history.revision()) {
            return Ok(());
        }
        self.write_autosave()
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
        // Opening is opening a set of one until told otherwise. Whatever set
        // was open belonged to the photograph being replaced.
        self.library = None;
        self.open_set = Vec::new();
        self.install(path, image, history, ids);
    }

    /// Put a photograph and its edit in hand, whether the edit was just built
    /// from a document or unparked by [`Library::switch`].
    ///
    /// The half of `adopt` that does not invent the document, because a parked
    /// history arrives with an undo stack that must not be flattened into a
    /// fresh one — and must not have the declared colour space written over it
    /// either. A file's claim is applied when a document is invented and never
    /// again; see [`crate::library::fresh_document`].
    fn install(
        &mut self,
        path: Option<PathBuf>,
        image: DecodedImage,
        history: History,
        ids: RowIdGenerator,
    ) {
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

    // ---- the grade in hand -------------------------------------------------

    /// Take a copy of this photograph's grade.
    ///
    /// The whole stack, pinned rows included: the eleven that every document
    /// starts with are as much of the grade as the ones that were added, and a
    /// copy that left the exposure behind would not be the look you copied.
    pub fn copy_grade(&mut self) -> Result<(), SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        self.clipboard = Some(photo.history.document().stack.clone());
        Ok(())
    }

    /// Whether there is a grade to paste.
    ///
    /// What the shells grey the Paste items by. Asked rather than inferred,
    /// because "nothing has been copied" and "the copy was empty" are two
    /// different things and only the first should disable a menu.
    pub fn has_grade(&self) -> bool {
        self.clipboard.is_some()
    }

    /// Put the copied grade on this photograph, as one undo step.
    pub fn paste_grade(&mut self) -> Result<(), SessionError> {
        let stack = self.clipboard.clone().ok_or(SessionError::NothingCopied)?;
        self.edit("Paste Grade", move |doc| doc.stack = stack)?;
        // The pasted rows carry the ids they had on the photograph they came
        // from, and this photograph's generator has never issued them. Without
        // this the next effect added would be handed an id a pasted row already
        // holds — two rows with one id, and every lookup finding whichever came
        // first.
        let photo = self.photo.as_mut().expect("edit would have refused");
        photo.ids = RowIdGenerator::resuming(photo.history.document());
        Ok(())
    }

    /// Put the copied grade on every *other* photograph in the set, and say how
    /// many took it.
    ///
    /// Not this one: it is the one you are looking at, and
    /// [`Session::paste_grade`] is how it gets the grade. Pasting to both from
    /// one command would make the count a lie and the undo step a surprise.
    pub fn paste_grade_to_all(&mut self) -> Result<usize, SessionError> {
        let stack = self.clipboard.clone().ok_or(SessionError::NothingCopied)?;
        let library = self.library.as_mut().ok_or(SessionError::NothingOpen)?;
        Ok(library.paste_stack_to_all(&stack))
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

    /// One line naming the GPU actually in use, for the status bar and for bug
    /// reports.
    ///
    /// `None` until a device exists. Deliberately not built on demand: a
    /// session that has not drawn anything has no device, and acquiring one to
    /// answer a question about it would make reading a label the most expensive
    /// thing in the frame. The first render fills it in.
    ///
    /// See [`pe_render::GpuContext::describe`] for what is in it — the maximum
    /// texture dimension is there because it is the one number that decides
    /// whether a given photograph opens at all, and "it refused my panorama" is
    /// unanswerable without it.
    pub fn gpu_name(&self) -> Option<String> {
        self.gpu.context.as_ref().map(|g| g.describe())
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

    /// Screen pixels per image pixel, at the current view and layer size.
    ///
    /// The number a zoom readout says and the one "100%" aims at, and it is not
    /// [`Session::view_region`]'s `size`: that is a fraction of the *frame*, so
    /// it reads 1 for a fitted view whether the photograph is filling the
    /// window at 3:1 or letterboxed into it at a quarter. What a person means
    /// by 100% is one image pixel to one screen pixel, which needs the layer's
    /// size and the frame's, and only the engine has both.
    ///
    /// The frame, not the source: a crop decides how much picture there is, and
    /// while the Image tab is open the frame is the whole straightened source
    /// instead. [`pe_render::export::output_size`] is the same question and the
    /// same answer.
    ///
    /// `None` with nothing open or no layer attached — there is no viewport to
    /// measure against, and a made-up 1.0 would be a readout that looks right
    /// and is not.
    pub fn view_scale(&self) -> Option<f32> {
        let viewport = self.gpu.attached.as_ref()?.size();
        let doc = self.document()?;
        let (sw, sh) = self.image_size();
        let frame = pe_render::export::output_size(doc, sw, sh);
        view_scale_of(viewport, frame, self.view.size)
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
        // Read before the borrows below, so the framing can be worked out
        // without holding all of `self` while `self.gpu` is being written.
        let cropping = self.cropping;
        let bypass = self.bypass_all;
        let gpu = self.gpu.context.as_ref().expect("context built above");
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        // Cloned only while the stack is switched off, and cleared rather than
        // skipped: everything downstream — the working texture's colour
        // pipeline, the framing, the stage cache's keys — is written in terms
        // of a document, and a second path through here that took none would be
        // a second renderer to keep in step.
        let bypassed;
        let doc = if bypass {
            let mut d = photo.history.document().clone();
            d.stack.rows.clear();
            bypassed = d;
            &bypassed
        } else {
            photo.history.document()
        };

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
        //
        // The framing rather than `doc.geometry`, so that opening the crop tool
        // rebuilds the texture for the enclosing frame and closing it rebuilds
        // for the crop. Storing the framing below is what makes the flag
        // invalidate this guard without a field of its own.
        let geometry = Self::framing_of(
            doc.geometry,
            cropping,
            photo.image.width,
            photo.image.height,
        );
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

    /// Draw the graded frame into `target`, and — when a comparison is on —
    /// the ungraded one over or beside it.
    ///
    /// The one place [`Compare`] becomes GPU work, shared by the offscreen
    /// read-back and the attached layer, so what a test measures is what a
    /// screen shows. That is the whole reason the compositing is here and not
    /// in a shell: the engine owns the textures, and a comparison assembled
    /// twice is a comparison that will differ.
    ///
    /// Two submissions rather than two passes in one encoder: `encode` writes
    /// its uniform through the queue, and two writes to one buffer before a
    /// single submit would both land before either pass ran.
    fn composite(
        &self,
        pass: &TransformPass,
        graded: &wgpu::TextureView,
        target: &wgpu::TextureView,
        size: (u32, u32),
        output: &pe_color::ColorSpace,
    ) {
        let (width, height) = size;
        let gpu = self.gpu.context.as_ref().expect("a frame was just graded");
        let (before, after) = side_rects(width, height);

        // Side by side is the only mode that does not cover its target: two
        // half-size pictures leave the surround showing, and without painting
        // it the full-size after frame would still be there behind them.
        let placement = match self.compare {
            Compare::Side => Placement {
                part: Some(Part::Into(after)),
                clear: Some(surround()),
            },
            _ => Placement::WHOLE,
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite-after"),
            });
        pass.encode(
            gpu,
            &mut encoder,
            graded,
            target,
            &space::ACESCG,
            output,
            placement,
        );
        gpu.queue.submit([encoder.finish()]);

        // The ungraded frame is the *working* texture — the frame after crop
        // and geometry, before any effect — through the same display
        // transform. One pass, no effects, cheap enough not to bother caching,
        // and **not run at all when nothing is comparing**: a comparison
        // nobody asked for should cost nothing.
        //
        // Emphatically not the file re-decoded. The question is "what did my
        // grade do", not "what did the crop do".
        if !self.compare.on() {
            return;
        }
        let Some(working) = self.gpu.working.as_ref() else {
            return;
        };
        let part = match self.compare {
            // The after frame is already there, whole; this is the same
            // picture at the same size over the left of it. One seam, no gap,
            // no scaling difference — which is the entire point of a wipe.
            Compare::Wipe => Part::Through(Rect {
                x: 0,
                y: 0,
                width: ((self.wipe * width as f32).round() as u32).min(width),
                height,
            }),
            _ => Part::Into(before),
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite-before"),
            });
        pass.encode(
            gpu,
            &mut encoder,
            &working.view,
            target,
            &space::ACESCG,
            output,
            Placement {
                part: Some(part),
                clear: None,
            },
        );
        gpu.queue.submit([encoder.finish()]);
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
        let output = self
            .photo
            .as_ref()
            .expect("graded checked this")
            .history
            .document()
            .color
            .pipeline()
            .output;

        let target = {
            let gpu = self.gpu.context.as_ref().expect("built by graded");
            ImageTexture::new(
                &gpu.device,
                width,
                height,
                pe_render::SOURCE_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                "offscreen",
            )
        };
        self.composite(
            self.gpu.to_display.as_ref().expect("built by graded"),
            &graded_view,
            &target.view,
            (width, height),
            &output,
        );
        self.needs_render = false;
        let gpu = self.gpu.context.as_ref().expect("built by graded");
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
        self.composite(
            self.gpu.to_screen.as_ref().expect("built above"),
            &graded_view,
            &view,
            (width, height),
            &output,
        );
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
        // Swapping the extension can only land on a photograph if one is
        // actually called `something.peproj`, which is close to impossible —
        // and checked anyway, because "never write over an original" is worth
        // being a rule rather than a likelihood. `apps/windows` makes the same
        // check for the same reason.
        if crate::export::would_overwrite_a_source(&self.open_set, &out) {
            return Err(SessionError::Write(format!(
                "{} is one of the photographs open",
                out.display()
            )));
        }
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

    // ---- what is remembered between runs ---------------------------------

    /// What this person keeps between runs. See [`Settings`].
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn is_favourite(&self, key: &str) -> bool {
        self.settings.is_favourite(key)
    }

    /// Star or unstar an effect, and write the change out.
    ///
    /// Saved immediately rather than on exit, for the same reason the set is:
    /// a window can be closed by the operating system, by a crash, or by
    /// somebody who does not think of starring as something that needs
    /// committing.
    pub fn toggle_favourite(&mut self, key: &str) {
        self.settings.toggle_favourite(key, &self.support);
    }

    /// The set that was open when this last ran, and which one was showing.
    ///
    /// **Only the photographs that are still there.** A remembered path can
    /// have been moved, renamed, or left on a volume that is not mounted, and
    /// one that has gone must not stop the others opening — at this point in a
    /// launch there is no window to say so in, so it is quietly left out and
    /// the rest come back.
    ///
    /// Which one was showing is remembered by name and looked up again in what
    /// survived, so losing one from the front of the set does not slide the
    /// answer onto its neighbour. When that photograph is itself the one that
    /// has gone there is no right answer, and the remembered position clamped
    /// to the last survivor at least lands near where you were.
    ///
    /// An empty set comes back with an index of nought, which is not a
    /// position in it — everything the last run had is gone. Nothing has to be
    /// done about that: [`Session::open_paths`] refuses an empty set, and a
    /// shell that passes one straight through gets `NothingOpen` rather than a
    /// panic.
    pub fn remembered_session(&self) -> (Vec<PathBuf>, usize) {
        self.settings.session()
    }

    /// Write down what is open and which one is showing.
    ///
    /// Called from both paths that can change either — opening a set and
    /// focusing within one — rather than once on the way out, because the
    /// moment worth surviving is the one nobody planned: the crash, the
    /// battery, the process the operating system decided to end.
    ///
    /// **Not throttled the way the autosave is.** [`autosave::Watcher`] waits
    /// for a pause because an edit changes at frame rate under a slider drag,
    /// and sixty writes a second of a document is real work. This changes once
    /// per photograph, and every one of those changes has just paid for a
    /// decode of the next photograph's pixels — a few hundred bytes written
    /// beside that is not measurable. What is worth guarding is the write that
    /// records nothing, and [`Settings::remember_session`] already drops it
    /// when neither the set nor the index has actually moved.
    fn remember_the_set(&mut self) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        let paths = library.paths();
        let index = library.current();
        self.settings.remember_session(&paths, index, &self.support);
    }

    // ---- export -------------------------------------------------------

    /// How exports are written from here on — and from the next run.
    ///
    /// Remembered rather than asked again because it is a decision about the
    /// work rather than about one photograph: somebody exporting JPEGs at 92
    /// is going to keep doing it.
    pub fn set_export(&mut self, format: export::Format, quality: u8) {
        let chosen = export::Export {
            format,
            quality: quality.clamp(1, 100),
        };
        // A shell with an export panel showing may hand this over on every
        // frame. Writing the same file sixty times a second is a disc write to
        // record that nothing happened.
        if chosen == self.settings.export {
            return;
        }
        self.settings.export = chosen;
        self.settings.save(&self.support);
    }

    pub fn export_settings(&self) -> export::Export {
        self.settings.export
    }

    /// Write the graded photograph beside its original, refusing a collision.
    pub fn export_current(&mut self) -> Result<PathBuf, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let source = photo
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("export"));
        let chosen = self.settings.export;
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

        self.ready_to_render()?;
        let gpu = self.gpu.context.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_ref().expect("built above");
        let photo = self.photo.as_ref().expect("checked above");
        write_rendered(
            gpu,
            renderer,
            &photo.image,
            photo.history.document(),
            &out,
            chosen,
        )?;
        Ok(out)
    }

    /// The device and the effect renderer, built if this is the first time.
    ///
    /// An export can be the first thing a session is ever asked to do — a
    /// batch over a set that has been opened and never drawn — so neither is
    /// assumed to exist already.
    fn ready_to_render(&mut self) -> Result<(), SessionError> {
        self.context()?;
        let gpu = self.gpu.context.as_ref().expect("built above");
        if self.gpu.renderer.is_none() {
            self.gpu.renderer = Some(EffectRenderer::new(&gpu.device));
        }
        Ok(())
    }

    // ---- a batch ----------------------------------------------------------

    /// Begin exporting every photograph in the set into `dir`.
    ///
    /// Refused when there is no set. The built-in chart is not one — there is
    /// no file for a run to be a run over — and a session with nothing open
    /// has nothing to export; both are the caller's mistake rather than a run
    /// of nought photographs that reports success.
    ///
    /// Somewhere chosen rather than beside each original: a batch written back
    /// into the folder it read would be the next run's input.
    pub fn start_batch(&mut self, dir: PathBuf) -> Result<(), SessionError> {
        let library = self.library.as_ref().ok_or(SessionError::NothingOpen)?;
        if library.is_empty() {
            return Err(SessionError::NothingOpen);
        }
        let targets = library.paths().into_iter().map(Path::to_path_buf).collect();
        self.batch = Some(export::Batch::new(targets, dir, self.settings.export));
        Ok(())
    }

    /// Export one photograph. `Ok(true)` while there is more to do.
    ///
    /// One photograph per call, and the caller is expected to call it once a
    /// frame: sixty photographs is sixty full-resolution renders, and a loop
    /// freezes the window for a minute with no way to tell whether it is
    /// working or hung, and no way to stop it.
    ///
    /// A photograph that cannot be written — a collision with somebody's
    /// original, a file that will not decode, a render that fails — is counted
    /// and stepped past. `Err` is reserved for what ends the whole run, which
    /// is having no device to render with.
    pub fn step_batch(&mut self) -> Result<bool, SessionError> {
        let Some(batch) = self.batch.as_mut() else {
            return Ok(false);
        };
        let Some(path) = batch.take_next() else {
            return Ok(false);
        };
        let chosen = batch.settings();
        let out = batch.claim(&path);
        let more = batch.remaining() > 0;

        // Every original this run must not land on: the set as the session has
        // it, and the run's own targets, which stop being the same list the
        // moment a photograph is taken out of the set. Something removed is
        // still somebody's photograph.
        let mut originals = self.open_set.clone();
        originals.extend(
            self.batch
                .as_ref()
                .expect("still running")
                .targets()
                .iter()
                .cloned(),
        );
        if export::would_overwrite_a_source(&originals, &out) {
            // Counted as a failure rather than stopping the run: one collision
            // should not abandon the other sixty-five, and the summary at the
            // end says how many did not make it.
            self.missed_one();
            return Ok(more);
        }

        // The photograph in hand is the one whose path is in hand, not the one
        // the library happens to be pointing at. A photograph taken out of the
        // set while it was showing is still the one whose edit is live, and
        // asking the library where it sits would get nothing and reach instead
        // for a sidecar that is a version behind.
        let in_hand = self.photo.as_ref().and_then(|p| p.path.as_deref()) == Some(path.as_path());

        // Decoded here rather than held: the whole reason a set is navigable is
        // that only one frame is in memory at a time, and a batch that loaded
        // them all would undo that in the one place it matters most.
        //
        // Before the document, not after, because a photograph that has never
        // been opened has no document yet and the file is the only thing that
        // can say what colour space it is in.
        let image = if in_hand {
            self.photo.as_ref().expect("in hand").image.clone()
        } else {
            match pe_io::load(&path) {
                Ok(image) => image,
                Err(_) => {
                    self.missed_one();
                    return Ok(more);
                }
            }
        };

        // Three places an edit can be: the live history for the photograph in
        // hand, a parked history for one that has been visited, and the
        // autosave or sidecar beside one never opened — or nowhere at all,
        // which means the defaults.
        //
        // Getting this wrong exports sixty photographs with the wrong sixty
        // edits, and the files look right until somebody opens them.
        let doc = if in_hand {
            self.photo
                .as_ref()
                .expect("in hand")
                .history
                .document()
                .clone()
        } else {
            let parked = self
                .library
                .as_ref()
                .and_then(|l| l.index_of(&path).and_then(|i| l.entries().get(i)))
                .and_then(|entry| entry.document())
                .cloned();
            parked.unwrap_or_else(|| {
                library::load_edit(&self.support, &path)
                    .unwrap_or_else(|| library::fresh_document(&path, image.space))
            })
        };

        self.ready_to_render()?;
        let gpu = self.gpu.context.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_ref().expect("built above");
        let written = write_rendered(gpu, renderer, &image, &doc, &out, chosen);
        match written {
            Ok(()) => self.wrote_one(),
            Err(_) => self.missed_one(),
        }
        Ok(more)
    }

    /// How far it has got: done, failed, total. `None` when there is no run.
    ///
    /// A finished run keeps its counts until it is cancelled or another
    /// begins, because the summary — `n exported`, or `n exported, m failed` —
    /// is read *after* the step that says there is no more to do. A run that
    /// silently stopped is indistinguishable from one that crashed.
    pub fn batch_progress(&self) -> Option<(usize, usize, usize)> {
        self.batch.as_ref().map(export::Batch::progress)
    }

    /// Stop, keeping whatever has already been written.
    ///
    /// Nothing is taken back. Half a folder of exports is the state somebody
    /// asked for when they pressed cancel; deleting the files they had already
    /// waited for would be the surprising answer.
    pub fn cancel_batch(&mut self) {
        self.batch = None;
    }

    fn wrote_one(&mut self) {
        if let Some(batch) = self.batch.as_mut() {
            batch.wrote_one();
        }
    }

    fn missed_one(&mut self) {
        if let Some(batch) = self.batch.as_mut() {
            batch.missed_one();
        }
    }
}

/// Render one photograph at full size and write it, in whichever format was
/// chosen.
///
/// One function for the single export and for a batch's every step. Two copies
/// of the same three lines is two places for a format to be handled and one of
/// them to be forgotten — and the failure would be a folder of JPEGs from a
/// run that said PNG.
///
/// A free function rather than a method because a batch's photograph is not
/// the session's: it is decoded, exported and dropped, and the document it is
/// written with may have come from a sidecar the session has never held.
fn write_rendered(
    gpu: &GpuContext,
    renderer: &EffectRenderer,
    image: &DecodedImage,
    doc: &Document,
    out: &Path,
    chosen: export::Export,
) -> Result<(), SessionError> {
    let (w, h) = pe_render::export::output_size(doc, image.width, image.height);
    // The space the pipeline actually rendered to, which is what the file has
    // to say it is in. Taken from the same settings the render read, so the two
    // cannot disagree — a file labelled with anything else is a wrong answer
    // stated confidently, and every reader will believe it.
    let out_space = doc.color.pipeline().output;

    if chosen.format.is_sixteen_bit() {
        let pixels = pe_render::export::render_full_16(
            gpu,
            renderer,
            image.width,
            image.height,
            &image.pixels,
            doc,
        )
        .map_err(|e| SessionError::Render(e.to_string()))?;
        pe_io::save_png16(w, h, &pixels, out, &out_space)
            .map_err(|e| SessionError::Write(e.to_string()))?;
    } else {
        let pixels =
            pe_render::render_full(gpu, renderer, image.width, image.height, &image.pixels, doc)
                .map_err(|e| SessionError::Render(e.to_string()))?;
        let img = pe_io::DecodedImage::new(w, h, pixels)
            .map_err(|e| SessionError::Write(e.to_string()))?;
        match chosen.format {
            export::Format::Jpeg => pe_io::save_jpeg(&img, out, chosen.quality, &out_space),
            _ => pe_io::save_png(&img, out, &out_space),
        }
        .map_err(|e| SessionError::Write(e.to_string()))?;
    }
    Ok(())
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

    // ---- a set of photographs -------------------------------------------

    /// A real file on disc, because everything about a set is about paths.
    fn photo_at(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = dir.join(name);
        pe_io::save_png(
            &pe_io::test_chart(width, height),
            &path,
            &pe_color::space::SRGB,
        )
        .expect("the temporary directory is writable");
        path
    }

    #[test]
    fn a_session_opens_a_set_and_shows_the_first() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 96, 32);
        let c = photo_at(tmp.path(), "c.png", 32, 32);

        let mut s = Session::new();
        s.open_paths(vec![a.clone(), b, c]).unwrap();

        assert!(s.is_open());
        assert_eq!(s.path(), Some(a.as_path()), "the first one is not showing");
        let library = s.library().expect("a set was opened");
        assert_eq!(library.len(), 3);
        assert_eq!(library.current(), 0);
        // And only the first is decoded. The whole reason a filmstrip exists is
        // to make a set navigable without holding it: three 24-megapixel frames
        // would be nearly 300 MB, two hundred of them twenty gigabytes.
        assert_eq!(
            s.image_size(),
            (64, 64),
            "the pixels are not the first photograph's"
        );
    }

    #[test]
    fn opening_no_photographs_at_all_is_refused() {
        let mut s = Session::new();
        assert!(matches!(
            s.open_paths(Vec::new()),
            Err(SessionError::NothingOpen)
        ));
        assert!(!s.is_open());
        assert!(s.library().is_none());
    }

    #[test]
    fn focusing_another_photograph_swaps_the_pixels_and_the_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 96, 32);

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_paths(vec![a.clone(), b.clone()]).unwrap();
        // Sharpen rather than exposure: exposure is one of the pinned rows
        // every fresh document already carries.
        let row = s.add_effect("sharpen").unwrap();
        s.set_float(row, "amount", 1.5).unwrap();

        s.focus(1).unwrap();
        assert_eq!(s.path(), Some(b.as_path()));
        assert_eq!(s.image_size(), (96, 32), "the pixels did not follow");
        assert_eq!(s.library().unwrap().current(), 1);
        assert!(
            !s.can_undo(),
            "the second photograph arrived with the first one's undo stack"
        );
        assert!(
            s.document()
                .unwrap()
                .stack
                .iter()
                .all(|r| r.effect != "sharpen"),
            "the first photograph's grade came along"
        );

        // And back: parking is not discarding, and the undo stack is the part
        // that proves it — a document alone could have been read off disc.
        s.focus(0).unwrap();
        assert_eq!(s.path(), Some(a.as_path()));
        assert_eq!(s.image_size(), (64, 64), "the pixels did not come back");
        assert!(s.can_undo(), "the parked undo stack was thrown away");
        let amount = s
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "sharpen")
            .and_then(|r| r.params.get("amount"))
            .and_then(pe_core::ParamValue::as_float);
        assert_eq!(amount, Some(1.5), "the parked edit was lost");
    }

    /// The edit that was parked is written, not merely remembered — a crash
    /// after switching should not lose it.
    ///
    /// This is the whole difference between an editor that keeps four
    /// photographs' work and one that keeps the last one. Asserted through a
    /// second session opening the photograph cold, which is what surviving a
    /// crash actually means.
    #[test]
    fn switching_away_saves_the_edit_it_parked() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone(), b]).unwrap();
        let row = s.add_effect("sharpen").unwrap();
        s.set_float(row, "amount", 1.5).unwrap();
        // Nothing has gone out yet: the throttle has not run out, nobody has
        // called `tick`, and nobody has asked for a write. So the assertion
        // below is about the switch and nothing else.
        assert!(
            autosave::load(&Support::at(&support), &a).is_none(),
            "something wrote the autosave before the switch did"
        );

        s.focus(1).unwrap();

        let mut crashed_and_reopened = Session::new();
        crashed_and_reopened.set_support_dir(&support);
        crashed_and_reopened.open_paths(vec![a]).unwrap();
        let amount = crashed_and_reopened
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "sharpen")
            .and_then(|r| r.params.get("amount"))
            .and_then(pe_core::ParamValue::as_float);
        assert_eq!(
            amount,
            Some(1.5),
            "the edit was parked in memory and never written; a crash would have taken it"
        );
    }

    /// Merely visiting a photograph is not editing it, and must not leave an
    /// autosave behind.
    ///
    /// An autosave beats a `.peproj` when the photograph is next opened —
    /// rightly, because it is by construction the later of the two. A write on
    /// every switch would make an untouched document the later one, and a crop
    /// somebody deliberately saved beside their photograph would be shadowed by
    /// a blank because they clicked past it.
    #[test]
    fn passing_through_a_photograph_does_not_write_over_what_is_saved_beside_it() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone(), b]).unwrap();
        s.focus(1).unwrap();

        assert!(
            autosave::load(&Support::at(&support), &a).is_none(),
            "a photograph nobody edited was autosaved on the way past"
        );
    }

    #[test]
    fn focusing_past_the_end_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = Session::new();
        s.open_paths(vec![a.clone(), b]).unwrap();
        assert!(matches!(
            s.focus(2),
            Err(SessionError::NoSuchPhoto { index: 2, count: 2 })
        ));
        // And the refusal left the session where it was rather than half-moved.
        assert_eq!(s.path(), Some(a.as_path()));
        assert_eq!(s.library().unwrap().current(), 0);
    }

    /// Clicking the thumbnail already showing is not a reason to throw the
    /// picture away and decode it again.
    #[test]
    fn focusing_the_photograph_already_showing_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = Session::new();
        s.open_paths(vec![a, b]).unwrap();
        let before = s.snapshot_version();
        s.focus(0).unwrap();
        assert_eq!(s.snapshot_version(), before, "the photograph was reloaded");
    }

    #[test]
    fn a_photograph_that_will_not_decode_leaves_the_set_where_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let shredded = tmp.path().join("shredded.png");
        std::fs::write(&shredded, b"not really a png").unwrap();

        let mut s = Session::new();
        s.open_paths(vec![a.clone(), shredded]).unwrap();
        assert!(matches!(s.focus(1), Err(SessionError::Read { .. })));
        assert_eq!(s.path(), Some(a.as_path()), "the session was half-swapped");
        assert_eq!(s.library().unwrap().current(), 0);
        assert_eq!(s.image_size(), (64, 64));
    }

    /// The one-photograph case is the case that existed before a session held a
    /// set, and it is the same call with one element in it.
    #[test]
    fn a_one_photograph_session_still_behaves_as_it_did() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = photo_at(tmp.path(), "only.png", 64, 64);
        // A sidecar beside it, which `open_path` has never read: pulling one
        // over the top is an explicit action. Routing the open through the
        // library would have quietly started honouring it.
        let mut sidecar = pe_effects::new_document(photo.to_string_lossy().to_string());
        sidecar.geometry.size = [0.3, 0.6];
        std::fs::write(photo.with_extension("peproj"), sidecar.to_json().unwrap()).unwrap();

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_path(&photo).unwrap();

        assert!(s.is_open());
        assert_eq!(s.path(), Some(photo.as_path()));
        assert_eq!(s.row_count(), pe_effects::PINNED_ROWS.len());
        assert_ne!(
            s.geometry().unwrap().size,
            [0.3, 0.6],
            "opening a photograph started reading the sidecar beside it"
        );
        // A set of one, so a shell asking for the strip gets a straight answer
        // rather than a special case.
        let library = s.library().expect("a set of one is still a set");
        assert_eq!(library.len(), 1);
        assert_eq!(library.current(), 0);
        assert!(matches!(s.focus(1), Err(SessionError::NoSuchPhoto { .. })));

        // And the autosave still comes back, which is the behaviour every
        // single-photograph test in this file is about.
        let row = s.add_effect("sharpen").unwrap();
        s.set_float(row, "amount", 1.5).unwrap();
        s.write_autosave().unwrap();

        let mut again = Session::new();
        again.set_support_dir(tmp.path().join("support"));
        again.open_path(&photo).unwrap();
        let restored = again
            .document()
            .unwrap()
            .stack
            .iter()
            .find(|r| r.effect == "sharpen")
            .and_then(|r| r.params.get("amount"))
            .and_then(pe_core::ParamValue::as_float);
        assert_eq!(restored, Some(1.5));
    }

    #[test]
    fn a_thumbnail_asked_for_through_the_session_arrives() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 320, 240);

        let mut s = Session::new();
        s.open_paths(vec![a]).unwrap();
        assert!(
            !s.collect_thumbnails(),
            "something arrived that was never asked for"
        );
        s.request_thumbnails(0..1);

        // The worker is a real thread, so this polls to a deadline rather than
        // sleeping for a guessed interval and hoping.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            s.collect_thumbnails();
            let entry = &s.library().unwrap().entries()[0];
            if entry.thumb.is_some() || entry.failed {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the thumbnail worker delivered nothing in thirty seconds"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let thumb = s.library().unwrap().entries()[0]
            .thumb
            .as_ref()
            .expect("the worker could not read a file it had just written");
        assert_eq!(thumb.width, crate::library::THUMB_EDGE);
    }

    /// Asking a session with nothing open, or with the built-in chart, for a
    /// set gets nothing rather than a set of one thing that is not a file.
    #[test]
    fn a_chart_is_not_a_set_of_photographs() {
        let mut s = Session::new();
        assert!(s.library().is_none());
        s.open_test_chart(64, 64).unwrap();
        assert!(s.library().is_none());
        assert!(matches!(s.focus(0), Err(SessionError::NothingOpen)));
        assert!(!s.collect_thumbnails());
        // And it does not panic when asked for thumbnails it has no set for.
        s.request_thumbnails(0..8);
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

    // ---- geometry -------------------------------------------------------

    #[test]
    fn a_fresh_document_has_no_crop_and_says_so() {
        let s = chart_session();
        let g = s.geometry().expect("something is open");
        assert!(g.is_identity(), "a fresh photograph is not cropped");
    }

    /// The engine corrects what it is handed. A crop that hangs off the edge is
    /// not a crop anyone can render, and the shell should not have to know the
    /// rules to avoid proposing one — that is what `shrink_to_fit` and
    /// `slide_to_fit` are for, and they live here.
    #[test]
    fn a_crop_that_hangs_off_the_edge_is_brought_back_inside() {
        let mut s = chart_session();
        let (w, h) = s.image_size();
        let want = pe_core::Geometry {
            centre: [0.9, 0.9],
            size: [0.5, 0.5],
            ..Default::default()
        };
        let got = s.set_geometry(want).unwrap();
        assert!(
            got.fits(w, h),
            "the engine returned a crop that is not inside the source: {got:?}"
        );
        assert_ne!(got.centre, want.centre, "nothing was corrected");
    }

    /// And a move is a move: it slides back to the edge rather than shrinking,
    /// which is the distinction `apps/windows` draws between Position and
    /// everything else. Shrinking here would let one control write another's
    /// value.
    #[test]
    fn a_crop_that_is_slid_back_keeps_its_size() {
        let mut s = chart_session();
        let want = pe_core::Geometry {
            centre: [0.9, 0.9],
            size: [0.5, 0.5],
            ..Default::default()
        };
        let got = s.set_geometry(want).unwrap();
        assert_eq!(got.size, want.size, "a move resized the crop");
    }

    /// And the corrected value is what the document holds — the caller is told
    /// the truth rather than being left holding what it asked for.
    #[test]
    fn what_comes_back_is_what_was_stored() {
        let mut s = chart_session();
        let want = pe_core::Geometry {
            angle: 12.0,
            size: [0.4, 0.4],
            ..Default::default()
        };
        let got = s.set_geometry(want).unwrap();
        assert_eq!(s.geometry().unwrap(), got);
    }

    #[test]
    fn a_locked_aspect_is_honoured() {
        let mut s = chart_session();
        let (w, h) = s.image_size();
        let want = pe_core::Geometry {
            aspect: pe_core::AspectLock::Ratio { w: 1.0, h: 1.0 },
            size: [0.8, 0.4],
            ..Default::default()
        };
        let got = s.set_geometry(want).unwrap();
        let (ow, oh) = got.output_size(w, h);
        assert!(
            (ow as f32 / oh as f32 - 1.0).abs() < 0.02,
            "a square lock produced {ow}x{oh}"
        );
    }

    /// Straightening is the case that must shrink: the rotated rectangle
    /// genuinely does not fit any more, wherever it is put.
    #[test]
    fn straightening_a_full_frame_cuts_it_in() {
        let mut s = chart_session();
        let (w, h) = s.image_size();
        let want = pe_core::Geometry {
            angle: 10.0,
            ..Default::default()
        };
        let got = s.set_geometry(want).unwrap();
        assert!(got.fits(w, h), "{got:?} still hangs off the edge");
        assert!(got.size[0] < 1.0, "a straightened crop was not cut in");
        assert_eq!(got.angle, 10.0, "the angle asked for was not kept");
    }

    #[test]
    fn setting_a_geometry_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(s.set_geometry(pe_core::Geometry::default()).is_err());
        assert!(s.geometry().is_none());
    }

    /// A crop is an edit like any other, so it takes its place in the history
    /// rather than changing the picture behind undo's back.
    #[test]
    fn a_crop_can_be_undone() {
        let mut s = chart_session();
        let want = pe_core::Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        };
        s.set_geometry(want).unwrap();
        assert!(s.can_undo());
        s.undo().unwrap();
        assert!(s.geometry().unwrap().is_identity());
    }

    // ---- the frame the viewer shows -------------------------------------

    /// The crop the tool is opened on below: half the frame, off to one side,
    /// and straightened, so the enclosing frame differs from it in size, in
    /// position and in shape at once.
    fn a_crop() -> pe_core::Geometry {
        pe_core::Geometry {
            centre: [0.1, -0.05],
            size: [0.5, 0.4],
            angle: 8.0,
            ..Default::default()
        }
    }

    #[test]
    fn the_viewer_shows_the_crop_until_the_tool_is_opened() {
        let mut s = chart_session();
        assert!(
            !s.cropping(),
            "the crop tool is open on a session nobody opened it on"
        );
        s.set_geometry(a_crop()).unwrap();
        assert_eq!(
            s.framing(),
            s.geometry(),
            "the viewer is showing something other than the document's crop"
        );
        // And with the crop as the frame, the crop fills it — which is what
        // makes one call answer both states.
        let r = s.crop_in_frame().unwrap();
        for (got, want) in r.iter().zip([0.0, 0.0, 1.0, 1.0]) {
            assert!(
                (got - want).abs() < 1e-4,
                "the crop does not fill its own frame: {r:?}"
            );
        }
    }

    /// The property the whole tool rests on: with the tool open the viewer
    /// shows the whole straightened source, so there is something outside the
    /// rectangle to see and to drag back into.
    #[test]
    fn opening_the_crop_tool_shows_the_whole_straightened_source() {
        let mut s = chart_session();
        let (w, h) = s.image_size();
        let stored = s.set_geometry(a_crop()).unwrap();
        let cropped = s.framing().unwrap().output_size(w, h);

        s.set_cropping(true);
        assert_eq!(s.framing().unwrap(), stored.enclosing(w, h));
        let showing = s.framing().unwrap().output_size(w, h);
        assert_eq!(showing, stored.enclosing(w, h).output_size(w, h));
        assert_ne!(showing, cropped, "the viewer is still framed on the crop");
        assert_eq!(
            s.geometry().unwrap(),
            stored,
            "opening the crop tool edited the document"
        );
    }

    #[test]
    fn closing_the_crop_tool_puts_the_crop_back() {
        let mut s = chart_session();
        let stored = s.set_geometry(a_crop()).unwrap();
        s.set_cropping(true);
        s.set_cropping(false);
        assert!(!s.cropping());
        assert_eq!(s.framing().unwrap(), stored);
    }

    /// The guard. The working texture is built for one frame, and a flag that
    /// changes the frame without invalidating it would leave the viewer showing
    /// the cropped picture with a rectangle drawn over it — which is the bug
    /// this whole task exists to fix, and it is invisible to anything that only
    /// reads `framing`.
    #[test]
    fn opening_the_crop_tool_repaints_the_viewer() {
        let mut s = chart_session();
        s.set_geometry(a_crop()).unwrap();
        let cropped = s.render_offscreen(64, 64).unwrap();
        assert!(!s.needs_render(), "a frame was just drawn");

        s.set_cropping(true);
        assert!(
            s.needs_render(),
            "opening the crop tool did not ask for a frame"
        );
        let whole = s.render_offscreen(64, 64).unwrap();
        assert_ne!(
            whole, cropped,
            "the viewer drew the cropped picture again: the working texture was not rebuilt"
        );

        s.set_cropping(false);
        assert!(
            s.needs_render(),
            "closing the crop tool did not ask for a frame"
        );
        assert_eq!(
            s.render_offscreen(64, 64).unwrap(),
            cropped,
            "closing the crop tool did not put the crop back on screen"
        );
    }

    /// Where the rectangle goes, on a case somebody can check by hand: half the
    /// frame, dead centre, unstraightened. The enclosing frame is then the whole
    /// source, so the crop is the middle half of it.
    #[test]
    fn a_centred_half_crop_is_the_middle_of_the_frame_it_is_drawn_in() {
        let mut s = chart_session();
        s.set_geometry(pe_core::Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        })
        .unwrap();
        s.set_cropping(true);
        let r = s.crop_in_frame().unwrap();
        for (got, want) in r.iter().zip([0.25, 0.25, 0.75, 0.75]) {
            assert!((got - want).abs() < 2e-3, "{r:?}");
        }
    }

    /// A drag that does not move the pointer must not move the crop. Read the
    /// rectangle out, write the same one back, and the document is where it
    /// was — through every turn and flip, which is the permutation that looks
    /// entirely plausible when it is wrong.
    #[test]
    fn putting_the_crop_back_where_it_is_moves_nothing() {
        for (name, want) in [
            ("plain", a_crop()),
            (
                "one quarter-turn",
                pe_core::Geometry {
                    turns: 1,
                    ..a_crop()
                },
            ),
            (
                "one quarter-turn, flipped horizontally",
                pe_core::Geometry {
                    turns: 1,
                    flip_h: true,
                    ..a_crop()
                },
            ),
            (
                "three quarter-turns, flipped vertically",
                pe_core::Geometry {
                    turns: 3,
                    flip_v: true,
                    ..a_crop()
                },
            ),
            (
                "two quarter-turns, both flips, the other way round",
                pe_core::Geometry {
                    angle: -20.0,
                    turns: 2,
                    flip_h: true,
                    flip_v: true,
                    ..a_crop()
                },
            ),
        ] {
            let mut s = chart_session();
            // The stored value rather than the proposal: it is legal by
            // construction, so anything that moves below was moved by the round
            // trip and not by the correction.
            let stored = s.set_geometry(want).unwrap();
            s.set_cropping(true);

            let rect = s.crop_in_frame().unwrap();
            let back = s.set_crop_in_frame(rect).unwrap();
            for i in 0..4 {
                assert!(
                    (back[i] - rect[i]).abs() < 2e-3,
                    "{name}: {rect:?} came back as {back:?}"
                );
            }
            let now = s.geometry().unwrap();
            assert!(
                (now.centre[0] - stored.centre[0]).abs() < 2e-3
                    && (now.centre[1] - stored.centre[1]).abs() < 2e-3
                    && (now.size[0] - stored.size[0]).abs() < 2e-3
                    && (now.size[1] - stored.size[1]).abs() < 2e-3,
                "{name}: the crop crawled from {stored:?} to {now:?}"
            );
            assert_eq!(now.angle, stored.angle, "{name}: the angle moved");
            assert_eq!(now.turns, stored.turns, "{name}: the turn moved");
            assert_eq!(now.flip_h, stored.flip_h, "{name}: a flip moved");
            assert_eq!(now.flip_v, stored.flip_v, "{name}: a flip moved");
        }
    }

    /// And a rectangle dragged off the frame is corrected, with the corrected
    /// one handed back — so the overlay has something true to draw rather than
    /// its own proposal.
    #[test]
    fn a_crop_dragged_off_the_frame_comes_back_inside_it() {
        let mut s = chart_session();
        s.set_geometry(pe_core::Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        })
        .unwrap();
        s.set_cropping(true);

        let asked = [-0.4, -0.4, 0.1, 0.1];
        let got = s.set_crop_in_frame(asked).unwrap();
        assert!(
            got[0] >= -1e-3 && got[1] >= -1e-3,
            "the crop still hangs off the frame: {got:?}"
        );
        assert_ne!(got, asked, "nothing was corrected");
        assert_eq!(
            got,
            s.crop_in_frame().unwrap(),
            "what came back is not what can be read back"
        );
        // Slid, not shrunk: a move does not resize.
        assert!(
            (got[2] - got[0] - 0.5).abs() < 2e-3 && (got[3] - got[1] - 0.5).abs() < 2e-3,
            "the move resized the crop: {got:?}"
        );
    }

    #[test]
    fn a_crop_in_the_frame_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(s.framing().is_none());
        assert!(s.crop_in_frame().is_err());
        assert!(s.set_crop_in_frame([0.0, 0.0, 1.0, 1.0]).is_err());
        // The flag is a property of the window, so it needs nothing open.
        s.set_cropping(true);
        assert!(s.cropping());
    }

    // ---- a batch of them ------------------------------------------------

    /// A session over a set, writing PNGs.
    ///
    /// PNG rather than JPEG because these tests read the written pixels back,
    /// and a lossy step between the render and the assertion is noise nobody
    /// needs.
    fn batch_session(paths: Vec<PathBuf>, support: &Path) -> Session {
        let mut s = Session::new();
        s.set_support_dir(support);
        s.open_paths(paths).unwrap();
        s.set_export(export::Format::Png, 95);
        s
    }

    fn out_dir(tmp: &Path) -> PathBuf {
        let out = tmp.join("out");
        std::fs::create_dir(&out).expect("the temporary directory is writable");
        out
    }

    /// Step until there is no more to do, and say how many steps it took.
    fn run_batch(s: &mut Session) -> usize {
        let mut steps = 0;
        loop {
            let more = s.step_batch().expect("the run had a device to draw with");
            steps += 1;
            assert!(steps < 64, "a batch that will not finish");
            if !more {
                return steps;
            }
        }
    }

    /// The mean of a file that has been written, which is the only way to tell
    /// one edit from another once the pixels have left the process.
    fn mean_of_file(path: &Path) -> f32 {
        let img = pe_io::load(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        mean(&img.pixels)
    }

    #[test]
    fn a_batch_writes_one_file_per_photograph() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);

        let mut s = batch_session(vec![a, b, c], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        assert_eq!(s.batch_progress(), Some((0, 0, 3)), "nothing has run yet");

        assert_eq!(run_batch(&mut s), 3, "one step per photograph, no more");
        assert_eq!(s.batch_progress(), Some((3, 0, 3)));
        for name in ["a_KROMA.png", "b_KROMA.png", "c_KROMA.png"] {
            assert!(out.join(name).exists(), "{name} was not written");
        }
    }

    /// The edit follows the photograph, whether it is the one in hand, one
    /// visited and parked, or one never opened with a sidecar beside it.
    ///
    /// The one that matters. Getting this wrong exports sixty photographs with
    /// the wrong sixty edits, and the files look right until somebody opens
    /// them — so this asserts on the written pixels rather than on a document,
    /// and gives the four photographs identical sources so that the *only*
    /// thing which can make the four files differ is the edit each was
    /// exported with.
    ///
    /// `d` is the control: nobody has touched it, so it is what a lookup that
    /// fell back to the defaults would produce. Every other file has to be on
    /// the correct side of it by a visible margin.
    #[test]
    fn each_photograph_is_exported_with_its_own_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let out = out_dir(tmp.path());
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);
        let d = photo_at(tmp.path(), "d.png", 64, 64);
        let parked_photo = b.clone();

        // c is never opened by the session that runs the batch. Its edit is in
        // a sidecar beside it, written by a session that has since gone away —
        // in its own support directory, so that nothing but the `.peproj` can
        // be what carries the edit across.
        {
            let mut earlier = Session::new();
            earlier.set_support_dir(tmp.path().join("elsewhere"));
            earlier.open_path(&c).unwrap();
            let row = earlier.add_effect("exposure").unwrap();
            earlier.set_float(row, "ev", 1.0).unwrap();
            earlier.save_sidecar().unwrap();
        }

        let mut s = batch_session(vec![a, b, c, d.clone()], &support);
        // a is in hand. Two stops up first — which switching away writes out
        // to its autosave — and then, once it is back in hand, one stop *down*
        // on top of that, which nothing has written anywhere.
        //
        // The second edit is the whole point of the live history being asked
        // first: the autosave is throttled, so at any moment the photograph on
        // screen is ahead of everything on disc about it, and a run that read
        // the disc would export the picture as it was a few seconds ago.
        let a_row = s.add_effect("exposure").unwrap();
        s.set_float(a_row, "ev", 2.0).unwrap();
        // b is visited and then left: three stops down, parked behind.
        s.focus(1).unwrap();
        let b_row = s.add_effect("exposure").unwrap();
        s.set_float(b_row, "ev", -3.0).unwrap();
        s.focus(0).unwrap();
        s.set_float(a_row, "ev", -1.0).unwrap();
        // And b's autosave is deleted behind its back, so that the parked
        // history in memory is the *only* place its three stops still exist.
        // Otherwise this branch proves nothing: switching away writes the
        // autosave, so a run that ignored the parked edit and read the disc
        // would get the same answer and look correct.
        autosave::forget(&Support::at(&support), &parked_photo);
        assert_eq!(
            autosave::load(&Support::at(&support), &d)
                .or_else(|| library::load_sidecar(&d))
                .map(|doc| doc.stack.len()),
            None,
            "the control photograph picked up an edit from somewhere"
        );

        s.start_batch(out.clone()).unwrap();
        run_batch(&mut s);
        assert_eq!(s.batch_progress(), Some((4, 0, 4)));

        let untouched = mean_of_file(&out.join("d_KROMA.png"));
        let in_hand = mean_of_file(&out.join("a_KROMA.png"));
        let parked = mean_of_file(&out.join("b_KROMA.png"));
        let sidecar = mean_of_file(&out.join("c_KROMA.png"));

        assert!(
            sidecar > untouched + 5.0,
            "the sidecar edit did not reach the file: {sidecar} against an untouched {untouched}"
        );
        // Darker than untouched, so neither the defaults nor a's own autosave
        // — which says two stops *up* — can produce this number.
        assert!(
            in_hand < untouched - 5.0,
            "the photograph in hand was exported with something other than the \
             edit in hand: {in_hand} against an untouched {untouched}, where the \
             autosave a fallback would have found is brighter than both"
        );
        assert!(
            parked < in_hand - 5.0,
            "the parked edit did not reach the file: three stops down read as \
             {parked}, one stop down as {in_hand}, untouched as {untouched}"
        );
    }

    /// Two sources called the same thing in different folders must not write
    /// over one another.
    #[test]
    fn two_photographs_with_the_same_name_get_different_files() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let holiday = tmp.path().join("holiday");
        let work = tmp.path().join("work");
        std::fs::create_dir(&holiday).unwrap();
        std::fs::create_dir(&work).unwrap();
        let one = photo_at(&holiday, "sunset.png", 64, 64);
        let two = photo_at(&work, "sunset.png", 64, 64);

        let mut s = batch_session(vec![one, two], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        run_batch(&mut s);

        assert_eq!(s.batch_progress(), Some((2, 0, 2)));
        assert!(out.join("sunset_KROMA.png").exists());
        assert!(
            out.join("sunset_KROMA_2.png").exists(),
            "the second sunset landed on the first: one file on disc, two successes reported"
        );
    }

    /// One collision does not abandon the run.
    #[test]
    fn a_photograph_that_would_land_on_an_original_is_counted_and_skipped() {
        // Contrived deliberately, and not far-fetched: a folder that has been
        // exported once already, exported into again.
        let tmp = tempfile::tempdir().unwrap();
        let sunset = photo_at(tmp.path(), "sunset.png", 64, 64);
        let already = photo_at(tmp.path(), "sunset_KROMA.png", 64, 64);
        let untouched = std::fs::read(&already).unwrap();

        let mut s = batch_session(vec![sunset, already.clone()], &tmp.path().join("support"));
        s.start_batch(tmp.path().to_path_buf()).unwrap();
        run_batch(&mut s);

        assert_eq!(
            std::fs::read(&already).unwrap(),
            untouched,
            "an original was written over"
        );
        assert_eq!(
            s.batch_progress(),
            Some((1, 1, 2)),
            "the collision was not counted, or it stopped the run"
        );
        assert!(
            tmp.path().join("sunset_KROMA_KROMA.png").exists(),
            "one collision abandoned the photograph after it"
        );
    }

    #[test]
    fn a_photograph_that_will_not_decode_is_counted_and_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let good = photo_at(tmp.path(), "a.png", 64, 64);
        // Second, not first: opening the set decodes the first one, and this
        // is about a file that fails when the *run* reaches it.
        let bad = tmp.path().join("b.png");
        std::fs::write(&bad, b"not a photograph").unwrap();

        let mut s = batch_session(vec![good, bad], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        run_batch(&mut s);

        assert_eq!(s.batch_progress(), Some((1, 1, 2)));
        assert!(out.join("a_KROMA.png").exists());
        assert!(!out.join("b_KROMA.png").exists());
    }

    /// Taken out of the set half way through, still exported.
    #[test]
    fn a_photograph_removed_mid_run_is_still_written() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = batch_session(vec![a.clone(), b], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        assert!(
            s.step_batch().unwrap(),
            "there is a second photograph to do"
        );

        // b leaves the set with the run half done. The session is re-opened on
        // a alone, which is what a removal amounts to from the run's side: the
        // photograph is no longer in the library, and is still on disc.
        s.open_paths(vec![a]).unwrap();
        assert_eq!(s.library().unwrap().len(), 1);

        assert!(!s.step_batch().unwrap());
        assert_eq!(s.batch_progress(), Some((2, 0, 2)));
        assert!(
            out.join("b_KROMA.png").exists(),
            "a photograph taken out of the set was abandoned, though it is still on disc"
        );
    }

    #[test]
    fn cancelling_keeps_what_was_already_written() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);

        let mut s = batch_session(vec![a, b, c], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        s.step_batch().unwrap();
        s.cancel_batch();

        assert!(s.batch_progress().is_none(), "the run is still going");
        assert!(
            !s.step_batch().unwrap(),
            "a cancelled run carried on when it was stepped again"
        );
        assert!(
            out.join("a_KROMA.png").exists(),
            "cancelling took back what had already been written"
        );
        assert!(!out.join("b_KROMA.png").exists());
        assert!(!out.join("c_KROMA.png").exists());
    }

    #[test]
    fn a_batch_with_no_set_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Session::new();
        assert!(matches!(
            s.start_batch(tmp.path().to_path_buf()),
            Err(SessionError::NothingOpen)
        ));
        assert!(s.batch_progress().is_none());

        // Nor is the built-in chart a set: there is no file for a run to be a
        // run over.
        s.open_test_chart(64, 64).unwrap();
        assert!(matches!(
            s.start_batch(tmp.path().to_path_buf()),
            Err(SessionError::NothingOpen)
        ));
        assert!(s.batch_progress().is_none());
        assert!(!s.step_batch().unwrap(), "a run that was never started ran");
    }

    /// The format is the run's, taken when it started.
    ///
    /// Changing it half way through would otherwise leave a folder half JPEG
    /// and half PNG with no record of where the line fell.
    #[test]
    fn a_run_writes_the_format_it_started_with() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let mut s = batch_session(vec![a, b], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        s.step_batch().unwrap();
        s.set_export(export::Format::Jpeg, 95);
        run_batch(&mut s);

        assert_eq!(s.batch_progress(), Some((2, 0, 2)));
        assert!(
            out.join("b_KROMA.png").exists(),
            "the format changed under a run that was already going"
        );
        assert!(!out.join("b_KROMA.jpg").exists());
    }

    /// The image is loaded before the document is chosen.
    ///
    /// A photograph that has never been opened has no document, and the file is
    /// the only thing that can say what colour space it is in. Choosing the
    /// document first means inventing one with nothing to tell it, and every
    /// wide-gamut photograph in the set comes out as though it had been sRGB
    /// all along — a subtle wrong answer rather than a crash, and one nobody
    /// sees until they put the export beside the original.
    #[test]
    fn a_photograph_never_opened_is_exported_in_the_space_its_file_declares() {
        let tmp = tempfile::tempdir().unwrap();
        let out = out_dir(tmp.path());
        // The same pixels three times. The first is there to be the one the
        // session opens, so that the other two are never opened at all; the
        // other two differ in nothing but what their file says about them.
        let chart = pe_io::test_chart(64, 64);
        let opened = tmp.path().join("opened.png");
        let narrow = tmp.path().join("narrow.png");
        let wide = tmp.path().join("wide.png");
        pe_io::save_png(&chart, &opened, &pe_color::space::SRGB).unwrap();
        pe_io::save_png(&chart, &narrow, &pe_color::space::SRGB).unwrap();
        pe_io::save_png(&chart, &wide, &pe_color::space::DISPLAY_P3).unwrap();
        assert_eq!(pe_io::load(&wide).unwrap().space, Some("Display P3"));

        let mut s = batch_session(vec![opened, narrow, wide], &tmp.path().join("support"));
        s.start_batch(out.clone()).unwrap();
        run_batch(&mut s);
        assert_eq!(s.batch_progress(), Some((3, 0, 3)));

        let narrow_out = pe_io::load(out.join("narrow_KROMA.png")).unwrap();
        let wide_out = pe_io::load(out.join("wide_KROMA.png")).unwrap();
        assert_ne!(
            narrow_out.pixels, wide_out.pixels,
            "the wide photograph was exported as though its file had said nothing"
        );
    }

    // ---- comparing ---------------------------------------------------------

    /// The frame the comparison tests read back. Small: every assertion below
    /// is about bands of it, not about detail.
    const W: u32 = 64;
    const H: u32 = 64;

    /// A chart with an edit that is obvious in bytes.
    fn graded() -> (Session, RowId) {
        let mut s = chart_session();
        let row = s
            .add_effect("exposure")
            .expect("exposure is a registered effect");
        s.set_float(row, "ev", 3.0).unwrap();
        (s, row)
    }

    /// Columns `range` of a `W`-wide RGBA8 frame, every row of them.
    fn cols(pixels: &[u8], range: Range<u32>) -> Vec<u8> {
        let stride = W as usize * 4;
        let (from, to) = (range.start as usize * 4, range.end as usize * 4);
        pixels
            .chunks_exact(stride)
            .flat_map(|row| row[from..to].iter().copied())
            .collect()
    }

    fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    /// The mean colour channel over one rectangle of a frame.
    fn rect_mean(pixels: &[u8], r: Rect) -> f32 {
        let mut sum = 0.0;
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                let p = pixel(pixels, x, y);
                sum += f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2]);
            }
        }
        sum / (r.width * r.height * 3) as f32
    }

    #[test]
    fn the_cycle_comes_back_to_off() {
        assert_eq!(Compare::default(), Compare::Off);
        // One button, three presses, back where it started. A comparison you
        // cannot turn off with the control that turned it on is a control that
        // works in one direction only.
        let mut mode = Compare::Off;
        let seen: Vec<Compare> = (0..3)
            .map(|_| {
                mode = mode.next();
                mode
            })
            .collect();
        assert_eq!(seen, vec![Compare::Wipe, Compare::Side, Compare::Off]);
        assert!(!Compare::Off.on());
        assert!(Compare::Wipe.on() && Compare::Side.on());
    }

    #[test]
    fn a_wipe_shows_the_ungraded_frame_on_the_left_and_the_graded_one_on_the_right() {
        let (mut s, _) = graded();
        let after = s.render_offscreen(W, H).unwrap();
        s.set_compare(Compare::Wipe, 1.0);
        let ungraded = s.render_offscreen(W, H).unwrap();

        s.set_compare(Compare::Wipe, 0.5);
        let wiped = s.render_offscreen(W, H).unwrap();

        // The two halves are one picture with a seam: each is exactly what the
        // whole of it would have been there. **No gap and no scaling
        // difference** — squeeze the before into half the target instead of
        // scissoring it and both of these fail, which is the difference between
        // this mode and the other one.
        assert_eq!(
            cols(&wiped, W / 2..W),
            cols(&after, W / 2..W),
            "the right of the seam is not the graded frame, where it was"
        );
        assert_eq!(
            cols(&wiped, 0..W / 2),
            cols(&ungraded, 0..W / 2),
            "the left of the seam is not the ungraded frame, where it was"
        );
        // And what that difference amounts to: three stops.
        let (left, was) = (mean(&cols(&wiped, 0..W / 2)), mean(&cols(&after, 0..W / 2)));
        assert!(
            left < was - 20.0,
            "the left of the seam is still graded: {left} against {was}"
        );
    }

    /// And the seam moves when the wipe does.
    #[test]
    fn the_seam_sits_where_the_wipe_says() {
        let (mut s, _) = graded();
        let after = s.render_offscreen(W, H).unwrap();

        for fraction in [0.25_f32, 0.4, 0.5, 0.75] {
            s.set_compare(Compare::Wipe, fraction);
            let wiped = s.render_offscreen(W, H).unwrap();
            let seam = (fraction * W as f32).round() as u32;
            assert_eq!(
                cols(&wiped, seam..W),
                cols(&after, seam..W),
                "at {fraction} the graded frame does not begin at column {seam}"
            );
            assert_ne!(
                cols(&wiped, seam - 1..seam),
                cols(&after, seam - 1..seam),
                "at {fraction} the column before the seam is already graded"
            );
        }
    }

    /// At either end a wipe is all of one picture, and neither end draws a
    /// scissor of nothing — 0.0 is somewhere a user will drag to.
    #[test]
    fn a_wipe_at_nothing_is_the_graded_frame_and_at_everything_is_the_ungraded_one() {
        let (mut s, row) = graded();
        let after = s.render_offscreen(W, H).unwrap();

        s.set_compare(Compare::Wipe, 0.0);
        assert_eq!(
            s.render_offscreen(W, H).unwrap(),
            after,
            "a wipe at nothing hid some of the grade"
        );

        s.set_compare(Compare::Wipe, 1.0);
        let all_before = s.render_offscreen(W, H).unwrap();
        assert_ne!(all_before, after, "a wipe at everything is still the grade");

        // And what it shows is the ungraded frame exactly: the working texture
        // through the display transform, which is what the stack renders when
        // every row of it is inert.
        s.set_compare(Compare::Off, 0.0);
        s.set_row_enabled(row, false).unwrap();
        assert_eq!(
            all_before,
            s.render_offscreen(W, H).unwrap(),
            "the before is not the frame the stack starts from"
        );
    }

    #[test]
    fn a_wipe_cannot_be_dragged_off_the_frame() {
        // Each of these renders as well as being read back: past either end the
        // scissor would be wider than the target or negative, and wgpu rejects
        // both rather than shrugging.
        let (mut s, _) = graded();
        for (asked, want) in [(-3.0, 0.0), (4.0, 1.0), (f32::NAN, 0.0)] {
            s.set_compare(Compare::Wipe, asked);
            assert_eq!(s.wipe(), want, "a wipe of {asked} landed at {}", s.wipe());
            s.render_offscreen(W, H).unwrap();
        }
    }

    #[test]
    fn side_by_side_puts_a_gap_between_two_half_size_pictures() {
        let (mut s, _) = graded();
        s.set_compare(Compare::Side, 0.5);
        let side = s.render_offscreen(W, H).unwrap();
        let (before, after) = side_rects(W, H);
        assert!(before.width * 2 < W, "the two pictures leave no gap");
        assert!(before.height < H, "the pictures are not half size");

        // The surround, taken from a corner no picture reaches rather than from
        // a constant, so what this asserts is "one colour behind both" and not
        // "this exact grey".
        let surround = pixel(&side, 0, 0);
        assert!(
            surround[0] < 60,
            "the surround is not the dark one the viewer paints: {surround:?}"
        );
        // The full-size after frame is not still showing behind the halves.
        for x in 0..W {
            assert_eq!(
                pixel(&side, x, 0),
                surround,
                "the top row still has the full-size frame in it at column {x}"
            );
        }
        for x in before.width..after.x {
            for y in 0..H {
                assert_eq!(
                    pixel(&side, x, y),
                    surround,
                    "the gap has a picture in it at {x}, {y}"
                );
            }
        }
        // Two pictures, and the ungraded one is on the left.
        let (l, r) = (rect_mean(&side, before), rect_mean(&side, after));
        assert!(
            l < r - 20.0,
            "the graded picture is not the one on the right: {l} against {r}"
        );
    }

    /// Off costs nothing: the before pass does not run.
    #[test]
    fn comparing_nothing_renders_exactly_what_it_did_before() {
        let (mut s, _) = graded();
        let plain = s.render_offscreen(W, H).unwrap();

        // What "the pass does not run" looks like from outside: no part of the
        // frame is the ungraded picture. Held against a wipe at nothing, which
        // draws no before either — by the same arithmetic that would make a
        // zero-width scissor a validation error — because a frame compared with
        // itself is the only reference an Off frame has.
        s.set_compare(Compare::Wipe, 0.0);
        assert_eq!(
            s.render_offscreen(W, H).unwrap(),
            plain,
            "the before pass ran with nothing comparing"
        );

        // And the mode gates it, not the seam: a position remembered from the
        // last time the user was in a wipe must still draw nothing.
        s.set_compare(Compare::Off, 0.5);
        assert_eq!(
            s.render_offscreen(W, H).unwrap(),
            plain,
            "the remembered seam drew a before with the comparison off"
        );
        assert_eq!(s.compare(), Compare::Off);
        assert_eq!(s.wipe(), 0.5, "the seam was not kept for next time");
    }

    /// The before is the frame before the *effects*, not before the crop.
    ///
    /// The one that catches the tempting wrong implementation — re-decoding the
    /// file — which would put the whole frame on one side of the seam and the
    /// crop on the other.
    #[test]
    fn the_ungraded_half_is_still_cropped() {
        let (mut s, row) = graded();
        // The right half of the chart. Its top band is a ramp from black to
        // white across the source, so which half is showing is a fact about
        // bytes: cropped, the left edge is halfway up the ramp; re-decoded from
        // the file, it is black.
        s.set_geometry(Geometry {
            centre: [0.25, 0.0],
            size: [0.5, 1.0],
            ..Default::default()
        })
        .unwrap();

        s.set_compare(Compare::Wipe, 1.0);
        let all_before = s.render_offscreen(W, H).unwrap();

        s.set_compare(Compare::Off, 0.0);
        s.set_row_enabled(row, false).unwrap();
        assert_eq!(
            all_before,
            s.render_offscreen(W, H).unwrap(),
            "the ungraded half is not the ungraded crop"
        );

        s.set_geometry(Geometry::default()).unwrap();
        let whole = s.render_offscreen(W, H).unwrap();
        assert_ne!(
            all_before, whole,
            "the ungraded half is the whole file: the crop was undone on the way"
        );
        let (cropped_edge, file_edge) = (mean(&cols(&all_before, 0..2)), mean(&cols(&whole, 0..2)));
        assert!(
            cropped_edge > file_edge + 20.0,
            "the ungraded half begins where the file begins, not where the crop does: \
             {cropped_edge} against {file_edge}"
        );
    }

    /// Where the arithmetic runs out. A one-pixel target leaves each half of a
    /// side by side no width at all, and wgpu hands a zero-sized viewport
    /// straight to the driver — Vulkan rejects one — rather than treating it as
    /// a draw of nothing. A window dragged to nothing is not a crash.
    #[test]
    fn a_comparison_survives_a_target_too_small_to_halve() {
        let (mut s, _) = graded();
        for mode in [Compare::Wipe, Compare::Side] {
            for seam in [0.0, 0.5, 1.0] {
                s.set_compare(mode, seam);
                for (w, h) in [(1, 1), (2, 2), (3, 7)] {
                    s.render_offscreen(w, h).unwrap();
                }
            }
        }
    }

    #[test]
    fn turning_a_comparison_on_asks_for_a_frame() {
        let (mut s, _) = graded();
        s.render_offscreen(W, H).unwrap();
        assert!(!s.needs_render(), "a frame was just drawn");

        s.set_compare(Compare::Wipe, 0.5);
        assert!(s.needs_render(), "a comparison did not ask for a frame");
        s.render_offscreen(W, H).unwrap();

        s.set_compare(Compare::Wipe, 0.5);
        assert!(!s.needs_render(), "saying it twice asked for another frame");
        s.set_compare(Compare::Wipe, 0.6);
        assert!(
            s.needs_render(),
            "dragging the seam did not ask for a frame"
        );
    }

    /// A comparison is a property of the window. It must not reach a file.
    #[test]
    fn an_export_is_not_a_comparison() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("sunset.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        s.set_export(export::Format::Png, 95);
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 3.0).unwrap();

        let plain = std::fs::read(s.export_current().unwrap()).unwrap();
        s.set_compare(Compare::Side, 0.5);
        let compared = std::fs::read(s.export_current().unwrap()).unwrap();
        assert_eq!(plain, compared, "the comparison was written to the file");
    }

    // ---- what is remembered between runs ---------------------------------

    /// The next launch, sharing the support directory and nothing else. What
    /// crossing between runs actually means.
    fn a_later_run(support: &Path) -> Session {
        let mut s = Session::new();
        s.set_support_dir(support);
        s
    }

    #[test]
    fn the_set_that_was_open_comes_back_next_run() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone(), b.clone(), c.clone()]).unwrap();

        // Opening is already enough to be remembered — nothing has been
        // focused and nothing has been closed.
        assert_eq!(
            a_later_run(&support).remembered_session(),
            (vec![a.clone(), b.clone(), c.clone()], 0),
            "the set was not written down until something else happened"
        );

        s.focus(2).unwrap();
        // The first session is still running. The point of writing on every
        // move rather than on the way out is that a crash here costs nothing.
        let (paths, index) = a_later_run(&support).remembered_session();
        assert_eq!(paths, vec![a, b, c.clone()]);
        assert_eq!(paths[index], c, "which one was showing was not remembered");
    }

    /// A photograph that has been moved, renamed, or left on a volume that is
    /// not mounted must not stop the rest of the set opening. There is nobody
    /// to tell at this point in a launch, so it is quietly left out.
    ///
    /// And dropping one renumbers the list, so the remembered *position* is a
    /// number from an older numbering: the one that was showing has to be
    /// found again by name or the application reopens confidently on whatever
    /// slid into its place.
    #[test]
    fn a_photograph_that_has_gone_does_not_stop_the_others_opening() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 96, 32);
        let c = photo_at(tmp.path(), "c.png", 32, 32);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone(), b.clone(), c.clone()]).unwrap();
        // The middle one, so that losing the first makes the remembered
        // position and the remembered photograph two different answers: after
        // the filtering, position 1 is `c` and the photograph is `b`.
        s.focus(1).unwrap();
        drop(s);

        // Between the two runs, somebody moves the first one out of the folder.
        std::fs::remove_file(&a).unwrap();

        let mut next = a_later_run(&support);
        let (paths, index) = next.remembered_session();
        assert_eq!(paths, vec![b.clone(), c], "the one that has gone came back");
        assert_eq!(
            paths[index], b,
            "reopened on the wrong photograph: the remembered number was taken \
             as a position in what survived rather than as which photograph it \
             named"
        );

        // And the whole of what a shell has to do with that answer works.
        next.open_paths(paths).unwrap();
        next.focus(index).unwrap();
        assert_eq!(next.path(), Some(b.as_path()));
        assert_eq!(next.image_size(), (96, 32), "a different photograph opened");
    }

    /// When the photograph you were on is itself the one that has gone there
    /// is no right answer, only a reasonable one: the remembered position,
    /// clamped to what is left.
    #[test]
    fn losing_the_photograph_you_were_on_lands_on_the_last_survivor() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone(), b.clone(), c.clone()]).unwrap();
        s.focus(2).unwrap();
        drop(s);

        std::fs::remove_file(&c).unwrap();

        let (paths, index) = a_later_run(&support).remembered_session();
        assert_eq!(paths, vec![a, b.clone()]);
        assert!(
            index < paths.len(),
            "index {index} is off the end of the set"
        );
        assert_eq!(paths[index], b);
    }

    /// The whole folder on a volume that is not mounted. An empty answer, and
    /// the shell's own refusal to open nothing — not a panic and not a crash
    /// before the window ever appears.
    #[test]
    fn a_set_that_has_entirely_gone_comes_back_empty_rather_than_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");
        let a = photo_at(tmp.path(), "a.png", 64, 64);

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.open_paths(vec![a.clone()]).unwrap();
        drop(s);

        std::fs::remove_file(&a).unwrap();

        let mut next = a_later_run(&support);
        let (paths, index) = next.remembered_session();
        assert!(paths.is_empty(), "a photograph that is not there came back");
        assert_eq!(index, 0);
        assert!(
            matches!(next.open_paths(paths), Err(SessionError::NothingOpen)),
            "opening the empty answer was not refused cleanly"
        );
        assert!(!next.is_open());
    }

    /// A star has to outlive the process that made it.
    #[test]
    fn a_star_crosses_from_one_run_to_the_next() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");

        let mut s = Session::new();
        s.set_support_dir(&support);
        assert!(!s.is_favourite("grain"), "nothing is starred to begin with");
        s.toggle_favourite("grain");
        assert!(s.is_favourite("grain"));

        assert!(
            a_later_run(&support).is_favourite("grain"),
            "the star did not survive the window closing"
        );

        // And the same gesture takes it away again.
        s.toggle_favourite("grain");
        assert!(!a_later_run(&support).is_favourite("grain"));
        assert_eq!(s.settings().favourites, Vec::<String>::new());
    }

    /// Exporting JPEGs at 92 is a decision about the work, not about one
    /// photograph. Asking again next time is asking somebody to answer a
    /// question they have already answered.
    #[test]
    fn how_you_export_is_remembered_between_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("support");

        let mut s = Session::new();
        s.set_support_dir(&support);
        s.set_export(export::Format::Jpeg, 92);
        drop(s);

        let next = a_later_run(&support);
        assert_eq!(
            next.export_settings(),
            export::Export {
                format: export::Format::Jpeg,
                quality: 92,
            },
            "the export choice was asked again"
        );
        // One home for the answer, so an export cannot use one figure while
        // the file on disc holds another.
        assert_eq!(next.settings().export, next.export_settings());
    }

    /// A host that has named no support directory has not agreed to anything
    /// being written. Everything still works; it simply does not outlive the
    /// run, and nothing is thrown.
    #[test]
    fn a_session_with_nowhere_to_write_still_remembers_within_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);

        let mut s = Session::new();
        s.toggle_favourite("grain");
        s.set_export(export::Format::Png, 95);
        s.open_paths(vec![a.clone()]).unwrap();

        assert!(s.is_favourite("grain"));
        assert_eq!(s.export_settings().format, export::Format::Png);
        assert_eq!(
            s.remembered_session(),
            (vec![a], 0),
            "the set in hand is still the set in hand"
        );
        // None of which reached a file, because there is no file to reach.
        assert!(Support::default().settings_path().is_none());
    }

    // ---- the grade in hand -------------------------------------------------

    #[test]
    fn a_grade_is_copied_whole_and_pasted_onto_another_photograph() {
        let mut a = Session::new();
        a.open_test_chart(32, 32).unwrap();
        let id = a.add_effect("sharpen").unwrap();
        a.copy_grade().unwrap();

        // A second session standing in for a second photograph: the clipboard
        // is the session's, so this is the honest way to show what travels.
        let stack = a.clipboard.clone().unwrap();
        let mut b = Session::new();
        b.open_test_chart(32, 32).unwrap();
        assert!(
            b.document()
                .unwrap()
                .stack
                .find_by_effect("sharpen")
                .is_none(),
            "the second document already had one"
        );
        b.clipboard = Some(stack);
        b.paste_grade().unwrap();

        let pasted = b.document().unwrap();
        assert!(
            pasted.stack.find_by_effect("sharpen").is_some(),
            "the added row did not travel"
        );
        // And the pinned rows came with it. A grade that left the exposure
        // behind is not the look that was copied.
        assert!(pasted.stack.find_by_effect("exposure").is_some());
        assert_eq!(pasted.stack.len(), a.document().unwrap().stack.len());
        let _ = id;
    }

    /// The mistake the paste invites, and the reason it re-seeds the generator.
    ///
    /// The pasted rows carry ids issued by *another* document. A generator that
    /// carried on from where this one had got to would hand the next added row
    /// an id a pasted row already holds — two rows with one id, and every
    /// lookup finding whichever comes first.
    ///
    /// In a debug build `Stack::push`'s own `debug_assert` fires first, at the
    /// point the duplicate is made rather than where it is felt, so that is
    /// what this test trips on. The explicit checks below are what would catch
    /// it in a release build, where that assertion is compiled out and the
    /// second row simply becomes one you can see and cannot touch.
    #[test]
    fn an_effect_added_after_a_paste_does_not_collide_with_a_pasted_row() {
        let mut source = Session::new();
        source.open_test_chart(32, 32).unwrap();
        // Several, so the source's ids run well past a fresh document's.
        for _ in 0..5 {
            source.add_effect("sharpen").unwrap();
        }
        source.copy_grade().unwrap();
        let stack = source.clipboard.clone().unwrap();

        let mut target = Session::new();
        target.open_test_chart(32, 32).unwrap();
        target.clipboard = Some(stack);
        target.paste_grade().unwrap();

        let fresh = target.add_effect("dehaze").unwrap();
        let doc = target.document().unwrap();
        let ids: Vec<_> = doc.stack.iter().map(|r| r.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            ids.len(),
            "two rows share an id after a paste"
        );
        assert_eq!(
            doc.stack.iter().filter(|r| r.id == fresh).count(),
            1,
            "the row just added has a pasted row's id"
        );
    }

    #[test]
    fn a_paste_is_one_undo_step_back_to_what_was_there() {
        let mut s = Session::new();
        s.open_test_chart(32, 32).unwrap();
        s.copy_grade().unwrap();
        let before = s.document().unwrap().stack.len();

        s.add_effect("sharpen").unwrap();
        s.paste_grade().unwrap();
        assert_eq!(
            s.document().unwrap().stack.len(),
            before,
            "the paste did not replace"
        );

        s.undo().unwrap();
        assert_eq!(
            s.document().unwrap().stack.len(),
            before + 1,
            "one undo did not put the added row back"
        );
        assert_eq!(s.undo_label().as_deref(), Some("Add Sharpen"));
    }

    #[test]
    fn pasting_with_nothing_copied_is_refused_and_says_so() {
        let mut s = Session::new();
        s.open_test_chart(32, 32).unwrap();
        assert!(!s.has_grade());
        let e = s.paste_grade().unwrap_err();
        assert!(matches!(e, SessionError::NothingCopied), "{e}");
        assert_eq!(e.to_string(), "no grade has been copied");

        s.copy_grade().unwrap();
        assert!(s.has_grade());
        assert!(s.paste_grade().is_ok());
    }

    #[test]
    fn copying_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(matches!(
            s.copy_grade().unwrap_err(),
            SessionError::NothingOpen
        ));
        assert!(!s.has_grade());
    }

    /// And pasting to a set that is not open is refused rather than silently
    /// doing nothing — a "pasted to 0 photos" would read as success.
    #[test]
    fn pasting_to_all_with_no_set_open_is_refused() {
        let mut s = Session::new();
        s.open_test_chart(32, 32).unwrap();
        s.copy_grade().unwrap();
        assert!(matches!(
            s.paste_grade_to_all().unwrap_err(),
            SessionError::NothingOpen
        ));
    }

    // ---- the zoom readout --------------------------------------------------

    /// A fitted view is not 100%: the frame's fraction is 1 either way, and
    /// what a person means by 100% is one image pixel to one screen pixel.
    #[test]
    fn a_fitted_view_reads_the_ratio_of_the_window_to_the_picture() {
        // A 4000-wide photograph fitted into an 800-wide window is a fifth.
        assert_eq!(
            view_scale_of((800, 600), (4000, 3000), [1.0, 1.0]),
            Some(0.2)
        );
        // And a small photograph in a big window is over 1, which is a viewer
        // showing it larger than life — a real answer, not an error.
        assert_eq!(view_scale_of((800, 600), (400, 300), [1.0, 1.0]), Some(2.0));
    }

    /// Zoomed in, the visible fraction shrinks and the scale grows with it.
    #[test]
    fn zooming_in_raises_the_scale_in_proportion() {
        let fit = view_scale_of((800, 600), (4000, 3000), [1.0, 1.0]).unwrap();
        let quarter = view_scale_of((800, 600), (4000, 3000), [0.25, 0.25]).unwrap();
        assert!(
            (quarter - fit * 4.0).abs() < 1e-5,
            "{quarter} is not four times {fit}"
        );
    }

    /// The axis that runs out first sets it. A wide window on a tall picture is
    /// limited by its height, and reading the width would say the picture is
    /// bigger on screen than it is.
    #[test]
    fn the_letterboxed_axis_is_the_one_that_decides() {
        // 1000x1000 window, 500x2000 picture: width would say 2, height says
        // 0.5, and 0.5 is what actually fits.
        assert_eq!(
            view_scale_of((1000, 1000), (500, 2000), [1.0, 1.0]),
            Some(0.5)
        );
        assert_eq!(
            view_scale_of((1000, 1000), (2000, 500), [1.0, 1.0]),
            Some(0.5)
        );
    }

    /// Nothing to measure is `None`, not a made-up 1.0 — a readout that looks
    /// right and is not is worse than one that is absent.
    #[test]
    fn a_frame_with_no_area_has_no_scale() {
        assert_eq!(view_scale_of((800, 600), (0, 0), [1.0, 1.0]), None);
        assert_eq!(view_scale_of((800, 600), (4000, 3000), [0.0, 0.0]), None);
    }

    /// A session that has not drawn has no device, and says so rather than
    /// acquiring one to answer. Reading a label must not be the most expensive
    /// thing in the frame.
    #[test]
    fn a_session_that_has_not_drawn_names_no_gpu() {
        let s = Session::new();
        assert_eq!(s.gpu_name(), None);
        // And opening a photograph is not drawing one: the chart goes through
        // the CPU decoder, and no device is needed until there is a layer.
        let mut s = s;
        s.open_test_chart(32, 32).unwrap();
        assert_eq!(s.gpu_name(), None, "opening a photograph acquired a device");
    }

    #[test]
    fn opening_a_folder_of_nothing_is_refused_and_names_it() {
        let dir = std::env::temp_dir().join("kroma-empty-folder-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new();
        let e = s.open_folder(&dir).unwrap_err();
        assert!(matches!(e, SessionError::NoPhotographs(_)), "{e}");
        assert!(e.to_string().contains("no photographs in"), "{e}");
        // And the session is untouched: a refusal is not a half-open set.
        assert!(!s.is_open());
        let _ = std::fs::remove_dir(&dir);
    }

    /// A folder with pictures in it opens all of them and says how many, and
    /// files this application cannot read are not counted.
    #[test]
    fn opening_a_folder_takes_the_photographs_and_leaves_the_rest() {
        let dir = std::env::temp_dir().join("kroma-folder-scan-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.png", "a.png"] {
            let img = pe_io::test_chart(8, 8);
            pe_io::save_png(&img, dir.join(name), &pe_color::space::SRGB).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), b"not a photograph").unwrap();

        let mut s = Session::new();
        let n = s.open_folder(&dir).unwrap();
        assert_eq!(n, 2, "the text file was counted as a photograph");
        assert_eq!(s.library().map(|l| l.len()), Some(2));
        // Sorted, so the set is in a predictable order rather than the
        // directory's own.
        assert!(
            s.path().unwrap().ends_with("a.png"),
            "the set did not open on the first"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "Never write over an original" is a rule, not a likelihood. Swapping the
    /// extension can only collide if a photograph is genuinely called
    /// `something.peproj`, and the check costs nothing.
    #[test]
    fn a_sidecar_is_refused_when_it_would_land_on_an_open_photograph() {
        let dir = std::env::temp_dir().join("kroma-sidecar-collision");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A photograph whose name already ends in the sidecar's extension, so
        // `with_extension` maps it onto itself.
        let photo = dir.join("odd.peproj");
        let img = pe_io::test_chart(8, 8);
        pe_io::save_png(&img, &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_paths(vec![photo.clone()]).unwrap();
        let e = s.save_sidecar().unwrap_err();
        assert!(matches!(e, SessionError::Write(_)), "{e}");
        assert!(e.to_string().contains("one of the photographs open"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And the ordinary case still writes, beside the photograph.
    #[test]
    fn a_sidecar_is_written_beside_the_photograph() {
        let dir = std::env::temp_dir().join("kroma-sidecar-write");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("shot.png");
        let img = pe_io::test_chart(8, 8);
        pe_io::save_png(&img, &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_paths(vec![photo.clone()]).unwrap();
        s.add_effect("sharpen").unwrap();
        let out = s.save_sidecar().unwrap();
        assert_eq!(out, dir.join("shot.peproj"));
        assert!(out.exists(), "the sidecar was not written");

        // And it reads back as the edit that was saved.
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains("sharpen"),
            "the sidecar does not carry the stack"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_the_stack_off_is_a_view_and_not_an_edit() {
        let mut s = Session::new();
        s.open_test_chart(32, 32).unwrap();
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 2.0).unwrap();
        let version = s.snapshot_version();
        let undo = s.undo_label();
        assert!(undo.is_some(), "nothing to undo, so this proves nothing");

        assert!(!s.bypass_all());
        s.set_bypass_all(true);
        assert!(s.bypass_all());
        // The document is untouched: same version, same row, and nothing new on
        // the undo stack to take the bypass back off with.
        assert_eq!(
            s.snapshot_version(),
            version,
            "bypassing edited the document"
        );
        assert!(
            s.document()
                .unwrap()
                .stack
                .find_by_effect("exposure")
                .is_some()
        );
        assert_eq!(s.undo_label(), undo, "bypassing put a step on the history");

        s.set_bypass_all(false);
        assert!(!s.bypass_all());
    }

    /// The one that matters: an export writes the *grade*, whatever the viewer
    /// happens to be showing. Somebody who switched the stack off to look at
    /// the original and then exported would otherwise write the original out
    /// over their work.
    ///
    /// The same shape as `an_export_is_not_a_comparison`, and for the same
    /// reason — a view property that reached the file would be silent.
    #[test]
    fn an_export_is_not_a_bypass() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("shot.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        s.set_export(export::Format::Png, 95);
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 3.0).unwrap();

        let graded = std::fs::read(s.export_current().unwrap()).unwrap();
        s.set_bypass_all(true);
        let bypassed = std::fs::read(s.export_current().unwrap()).unwrap();
        assert_eq!(graded, bypassed, "the bypass was written to the file");
    }
}
