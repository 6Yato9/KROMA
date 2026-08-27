//! Getting the graded image onto the screen.
//!
//! Still no image processing here — this file owns *framing*: how much of the
//! photograph is on screen, at what resolution, and how that rectangle is
//! handed to egui. The pixels are `pe-render`'s business.

use std::sync::Arc;

use egui::mutex::RwLock;
use egui_wgpu::wgpu;
use pe_color::space;
use pe_core::{Document, Geometry};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext, ImageTexture, Region, RenderError, TransformPass};
use pe_scopes::{ColourSpread, Distribution, Histogram, Vectorscope, Waveform};

/// Everything measured from one readback of the graded frame.
///
/// Bundled because they all come from the same pixels and are all invalidated
/// at the same moment; splitting them would mean three copies of the "has this
/// changed" question.
pub struct Scopes {
    pub histogram: Histogram,
    /// The same frame binned in the curve's own domain, for drawing behind
    /// the curve editor. See `Histogram::from_display_log`.
    pub log_histogram: Histogram,
    /// Where the frame's hues and saturations sit, for the secondary curves.
    /// A tone histogram behind a Hue Vs Sat curve would put every peak in the
    /// wrong place.
    pub colour: ColourSpread,
    pub waveform: Waveform,
    pub vectorscope: Vectorscope,
    /// Where the frame's colours sit on each of the Colour Warper's three
    /// plots. Without it the warper is a diagram of colour in general rather
    /// than a tool aimed at the photograph in front of you.
    pub warper: Distribution,
    /// Bumped on every fresh measurement, so the panel knows when to re-upload
    /// its textures instead of doing it every frame.
    pub generation: u64,
}

/// Size of the off-screen render every scope is measured from.
///
/// The scopes describe the whole photograph, not the part currently on screen
/// — that is the point of them, and they must not change when you zoom in. So
/// they get their own small full-frame render rather than reading the
/// viewport. It has its own stage cache, so it only re-runs when the edit
/// actually changes.
///
/// Both dimensions matter, and they matter for different reasons.
///
/// *Columns* are horizontal resolution: a waveform draws one column of the
/// picture per column of the panel, and a panel is six or seven hundred points
/// wide. *Rows* are how many samples each of those columns gets — and with 256
/// levels to spread them over, 240 rows meant most levels held one sample or
/// none, so a smooth gradient came out as a comb rather than a sweep. That was
/// the visible mess: not blur, sparseness.
///
/// 640 by 480 is three hundred thousand pixels — a 1.2 MB readback, a couple
/// of milliseconds to bin, and it only happens when the edit changes.
const SCOPE_SIZE: (u32, u32) = (640, 480);

/// Upper bound on preview dimensions.
///
/// The stage cache keeps a working texture per row, so preview memory is
/// `rows x width x height x 8` bytes. At 2560 that is about 21 MB a row — a
/// twelve-row stack costs 250 MB, which is fine. Letting it follow a 5K window
/// unbounded would not be.
const MAX_PREVIEW: u32 = 2560;

/// How much extra frame to render around the visible rectangle when zoomed in.
///
/// Spatial effects read neighbouring pixels. If the rendered texture stopped
/// exactly at the edge of the viewport, halation and blur would sample the
/// clamped border and leave a visible seam along it. Rendering a margin and
/// displaying only the inner part gives them real neighbours to read.
///
/// 6% covers the default spreads comfortably. A very large halation radius at
/// high zoom can still reach past it, which is a known and bounded softness at
/// the very edge rather than a correctness problem.
const MARGIN: f32 = 0.06;

/// How far out the view can go, as a fraction of fit.
///
/// Below fit is not a nonsense request: it is how you see a photograph
/// *as an object* — with room around it, against the surround, the way it
/// would sit on a wall or a page. A viewer that stops at fit can only ever
/// show the picture filling something.
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;

/// Where the viewer is looking.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// 1.0 fits the whole photograph in the viewport. 2.0 shows half of it,
    /// and 0.5 draws it at half the size it would fit at, with surround
    /// around it.
    pub zoom: f32,
    /// Centre of the view, in frame uv. (0.5, 0.5) is the middle of the image.
    pub centre: egui::Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            centre: egui::vec2(0.5, 0.5),
        }
    }
}

impl View {
    pub fn fit(&mut self) {
        *self = View::default();
    }

    pub fn is_fit(&self) -> bool {
        (self.zoom - 1.0).abs() < 1e-3
    }
}

/// What a frame of rendering worked out to. Returned so the UI can draw the
/// image in the right place without recomputing any of it.
pub struct Framing {
    pub texture: egui::TextureId,
    /// The part of the rendered texture to actually show — the margin is
    /// rendered but not displayed.
    pub uv: egui::Rect,
    /// Size on screen, in points.
    pub size: egui::Vec2,
    /// Screen pixels per image pixel, for the zoom readout.
    pub scale: f32,
    /// The visible rectangle in frame uv. The viewer needs it to turn a drag
    /// or a scroll at the cursor back into a move in image space.
    pub visible: egui::Rect,
    /// The same frame with the stack switched off, for the comparison views.
    /// `None` unless one of them asked for it.
    pub before: Option<egui::TextureId>,
    /// Pixel size of the frame that was drawn. Not the source's size: the crop
    /// decides what the frame is, and while the crop tool is open the frame is
    /// bigger than either.
    pub frame: (u32, u32),
    pub passes: usize,
}

pub struct Preview {
    gpu: GpuContext,
    egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
    renderer: EffectRenderer,
    to_working: TransformPass,
    to_display: TransformPass,

    source: ImageTexture,
    working: Option<ImageTexture>,
    display: Option<ImageTexture>,
    texture_id: Option<egui::TextureId>,
    size: (u32, u32),
    region: Region,
    geometry: Geometry,

    /// The ungraded frame, for the comparison views. Kept beside the graded
    /// one rather than re-rendered on demand: it is the same working texture
    /// the stack starts from, so producing it is a single transform pass with
    /// no effects at all.
    before: Option<ImageTexture>,
    before_id: Option<egui::TextureId>,

    scope_renderer: EffectRenderer,
    scope_working: Option<ImageTexture>,
    scope_display: ImageTexture,
    scope_geometry: Option<Geometry>,
    scopes: Option<Scopes>,
    generation: u64,
}

impl Preview {
    pub fn new(
        gpu: GpuContext,
        egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
        image: &DecodedImage,
    ) -> Result<Self, RenderError> {
        let source = ImageTexture::upload_rgba8(
            &gpu.device,
            &gpu.queue,
            image.width,
            image.height,
            &image.pixels,
            "source",
        )?;

        let to_working = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
        let to_display = TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT);
        let renderer = EffectRenderer::new(&gpu.device);
        let scope_renderer = EffectRenderer::new(&gpu.device);
        let scope_display = ImageTexture::new(
            &gpu.device,
            SCOPE_SIZE.0,
            SCOPE_SIZE.1,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            "histogram",
        );

        Ok(Self {
            gpu,
            egui_renderer,
            renderer,
            to_working,
            to_display,
            source,
            working: None,
            display: None,
            texture_id: None,
            size: (0, 0),
            region: Region::FULL,
            geometry: Geometry::default(),
            before: None,
            before_id: None,
            scope_renderer,
            scope_working: None,
            scope_display,
            scope_geometry: None,
            scopes: None,
            generation: 0,
        })
    }

    /// Every scope, measured on the whole photograph as currently graded.
    pub fn scopes(&self) -> Option<&Scopes> {
        self.scopes.as_ref()
    }

    /// Re-measure the scopes, but only when the edit has actually moved.
    ///
    /// The dedicated renderer has its own stage cache, so an unchanged
    /// document costs zero passes — and when it does, there is nothing new to
    /// read back and the GPU stall is skipped entirely. During a slider drag
    /// it is one 300 KB readback a frame, which is affordable; on an idle
    /// frame it is nothing at all.
    fn update_scopes(&mut self, doc: &Document, source: (u32, u32)) {
        // The histogram describes the photograph the user is making, so it
        // reads through the crop. A rejected corner should stop counting
        // towards the clipping warning the moment it is cropped away.
        if self.scope_working.is_none() || self.scope_geometry != Some(doc.geometry) {
            self.scope_working = Some(self.to_working.to_working_mapped(
                &self.gpu,
                &self.source,
                &doc.color.pipeline().input,
                SCOPE_SIZE.0,
                SCOPE_SIZE.1,
                pe_render::Sampling::of(&doc.geometry, source.0, source.1),
            ));
            self.scope_geometry = Some(doc.geometry);
            self.scope_renderer.invalidate();
            self.scopes = None;
        }
        let working = self.scope_working.as_ref().expect("built above");

        self.scope_renderer.set_region(Region::FULL);
        let graded = self.scope_renderer.render(&self.gpu, working, doc, 1);
        let graded_view = graded.view.clone();
        let changed = self.scope_renderer.last_pass_count() > 0;
        if !changed && self.scopes.is_some() {
            return;
        }

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scopes"),
            });
        self.to_display.encode(
            &self.gpu,
            &mut encoder,
            &graded_view,
            &self.scope_display.view,
            &space::ACESCG,
            &doc.color.pipeline().output,
            pe_render::Placement::WHOLE,
        );
        self.gpu.queue.submit([encoder.finish()]);

        if let Ok(pixels) = pe_render::read_rgba8(&self.gpu, &self.scope_display) {
            let (w, h) = (SCOPE_SIZE.0 as usize, SCOPE_SIZE.1 as usize);
            self.generation += 1;
            self.scopes = Some(Scopes {
                histogram: Histogram::from_display(&pixels),
                log_histogram: Histogram::from_display_log(&pixels),
                colour: ColourSpread::from_display(&pixels),
                waveform: Waveform::from_display(&pixels, w, h),
                vectorscope: Vectorscope::from_display(&pixels),
                warper: Distribution::from_display(&pixels),
                generation: self.generation,
            });
        }
    }

    /// Swap in a different photograph.
    ///
    /// Zeroing `size` forces the working and display textures to be rebuilt on
    /// the next frame: the new image may be a different shape, so every cached
    /// stage and both intermediates are stale.
    pub fn set_source(&mut self, image: &DecodedImage) -> Result<(), RenderError> {
        self.source = ImageTexture::upload_rgba8(
            &self.gpu.device,
            &self.gpu.queue,
            image.width,
            image.height,
            &image.pixels,
            "source",
        )?;
        self.size = (0, 0);
        self.renderer.invalidate();
        self.scope_renderer.invalidate();
        self.scope_working = None;
        self.scope_geometry = None;
        self.scopes = None;
        Ok(())
    }

    /// Render the document for the current view.
    ///
    /// `framing` is the geometry the *viewer* is showing, which is normally
    /// the document's own. While the crop tool is open it is the enclosing
    /// frame instead, so the user can see what is outside the crop; the
    /// document is untouched either way.
    pub fn render(
        &mut self,
        image: &DecodedImage,
        doc: &Document,
        framing: Geometry,
        view: View,
        viewport: egui::Vec2,
        compare: bool,
    ) -> Result<Framing, RenderError> {
        let (fw, fh) = framing.output_size(image.width, image.height);
        let plan = frame_plan(fw, fh, view, viewport);

        if self.size != plan.render_size || self.region != plan.rendered || self.geometry != framing
        {
            self.rebuild(doc, image, framing, plan.render_size, plan.rendered)?;
        }

        let working = self.working.as_ref().expect("rebuilt above");
        let display = self.display.as_ref().expect("rebuilt above");

        self.renderer.set_region(plan.rendered);
        let graded = self.renderer.render(&self.gpu, working, doc, 1);
        let graded_view = graded.view.clone();
        let passes = self.renderer.last_pass_count();

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview-display"),
            });
        self.to_display.encode(
            &self.gpu,
            &mut encoder,
            &graded_view,
            &display.view,
            &space::ACESCG,
            &doc.color.pipeline().output,
            pe_render::Placement::WHOLE,
        );
        self.gpu.queue.submit([encoder.finish()]);

        // The ungraded frame, for a wipe or a side-by-side. It is the working
        // texture the stack starts from, so this is one transform pass and no
        // effects — cheap enough not to bother caching, and skipped entirely
        // when nothing is comparing.
        if compare {
            let before = self.before.as_ref().expect("rebuilt above");
            let mut encoder =
                self.gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("preview-before"),
                    });
            self.to_display.encode(
                &self.gpu,
                &mut encoder,
                &working.view,
                &before.view,
                &space::ACESCG,
                &doc.color.pipeline().output,
                pe_render::Placement::WHOLE,
            );
            self.gpu.queue.submit([encoder.finish()]);
        }

        self.update_scopes(doc, (image.width, image.height));

        Ok(Framing {
            texture: self.texture_id.expect("registered on rebuild"),
            uv: plan.uv,
            size: plan.on_screen,
            scale: plan.scale,
            visible: plan.visible,
            before: compare.then_some(self.before_id).flatten(),
            frame: (fw, fh),
            passes,
        })
    }

    fn rebuild(
        &mut self,
        doc: &Document,
        image: &DecodedImage,
        framing: Geometry,
        size: (u32, u32),
        region: Region,
    ) -> Result<(), RenderError> {
        let (w, h) = size;

        // Decode the region straight into the working space, reading through
        // the crop on the way. Both are affine, so they compose into one map
        // and the source is still sampled exactly once — zooming into a
        // straightened crop costs no more resampling than zooming into a
        // plain one.
        let sampling = pe_render::Sampling::of(&framing, image.width, image.height).within(region);
        self.working = Some(self.to_working.to_working_mapped(
            &self.gpu,
            &self.source,
            &doc.color.pipeline().input,
            w,
            h,
            sampling,
        ));

        let display = ImageTexture::new(
            &self.gpu.device,
            w,
            h,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            "preview-display",
        );

        // Handed to egui through a view that does *not* decode sRGB on the way
        // out, because egui decodes it itself. Through the ordinary view the
        // picture goes through the transfer function twice and arrives
        // noticeably dark — dark enough to see beside the filmstrip, which
        // goes to egui as plain bytes and gets it right.
        let display_view = display.raw_view();
        let mut renderer = self.egui_renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &self.gpu.device,
                &display_view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id = Some(renderer.register_native_texture(
                    &self.gpu.device,
                    &display_view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        let before = ImageTexture::new(
            &self.gpu.device,
            w,
            h,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            "preview-before",
        );
        let before_view = before.raw_view();
        match self.before_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &self.gpu.device,
                &before_view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.before_id = Some(renderer.register_native_texture(
                    &self.gpu.device,
                    &before_view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        drop(renderer);

        self.before = Some(before);
        self.display = Some(display);
        self.size = size;
        self.region = region;
        self.geometry = framing;
        // Every cached stage was rendered for the old size or rectangle.
        self.renderer.invalidate();
        Ok(())
    }

    /// Render at full resolution for export. Separate path, no caching, always
    /// the whole frame — see `pe_render::export`.
    pub fn export(&self, image: &DecodedImage, doc: &Document) -> Result<Vec<u8>, RenderError> {
        pe_render::render_full(
            &self.gpu,
            &self.renderer,
            image.width,
            image.height,
            &image.pixels,
            doc,
        )
    }

    /// The same render, read back at full depth.
    pub fn export_16(&self, image: &DecodedImage, doc: &Document) -> Result<Vec<u16>, RenderError> {
        pe_render::export::render_full_16(
            &self.gpu,
            &self.renderer,
            image.width,
            image.height,
            &image.pixels,
            doc,
        )
    }
}

/// Everything the framing decision produces, worked out without touching a GPU
/// so it can be tested directly.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FramePlan {
    /// Region actually rendered, including the margin.
    rendered: Region,
    /// Pixel size of the render target.
    render_size: (u32, u32),
    /// Portion of that texture to display — the margin trimmed off.
    uv: egui::Rect,
    /// Size of the drawn image on screen, in points.
    on_screen: egui::Vec2,
    /// Screen pixels per image pixel.
    scale: f32,
    /// The visible rectangle in frame uv, after clamping.
    visible: egui::Rect,
}

fn frame_plan(img_w: u32, img_h: u32, view: View, viewport: egui::Vec2) -> FramePlan {
    let iw = img_w.max(1) as f32;
    let ih = img_h.max(1) as f32;
    let vw = viewport.x.max(1.0);
    let vh = viewport.y.max(1.0);

    let fit = (vw / iw).min(vh / ih);
    let zoom = view.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let scale = fit * zoom;

    // How much of the image is visible, as a fraction of the frame.
    let visible = egui::vec2((vw / (iw * scale)).min(1.0), (vh / (ih * scale)).min(1.0));

    // Clamp the centre so the view cannot wander off the picture.
    let half = visible * 0.5;
    let centre = egui::vec2(
        view.centre.x.clamp(half.x, 1.0 - half.x),
        view.centre.y.clamp(half.y, 1.0 - half.y),
    );

    let inner = egui::Rect::from_min_max(
        egui::pos2(centre.x - half.x, centre.y - half.y),
        egui::pos2(centre.x + half.x, centre.y + half.y),
    );

    // At fit there is nothing outside the viewport to bleed in from, so the
    // margin is skipped and the whole frame is rendered as before.
    let full = visible.x >= 0.999 && visible.y >= 0.999;
    let rendered_rect = if full {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
    } else {
        let m = egui::vec2(visible.x * MARGIN, visible.y * MARGIN);
        egui::Rect::from_min_max(
            egui::pos2((inner.min.x - m.x).max(0.0), (inner.min.y - m.y).max(0.0)),
            egui::pos2((inner.max.x + m.x).min(1.0), (inner.max.y + m.y).min(1.0)),
        )
    };

    let rendered = Region {
        offset: [rendered_rect.min.x, rendered_rect.min.y],
        size: [
            rendered_rect.width().max(1e-4),
            rendered_rect.height().max(1e-4),
        ],
    };

    // What it will occupy on screen. Below fit that is smaller than the
    // viewport, and above fit the visible part fills it.
    let on_screen = egui::vec2(
        (iw * visible.x * scale).min(vw),
        (ih * visible.y * scale).min(vh),
    );

    // Enough pixels for the screen, but never more than the source actually
    // has for that rectangle, and never past the memory cap.
    //
    // Measured against what is drawn rather than against the viewport. At half
    // zoom those differ by a factor of two in each direction, and rendering
    // four times the pixels egui is going to sample bilinearly does not look
    // better — it aliases, the same way any 2:1 downscale does without a
    // mip chain.
    let want_w = on_screen.x * (rendered.size[0] / visible.x.max(1e-4));
    let want_h = on_screen.y * (rendered.size[1] / visible.y.max(1e-4));
    let source_w = iw * rendered.size[0];
    let source_h = ih * rendered.size[1];
    let render_size = (
        (want_w.min(source_w) as u32).clamp(1, MAX_PREVIEW),
        (want_h.min(source_h) as u32).clamp(1, MAX_PREVIEW),
    );

    // Where the visible rectangle sits inside the rendered one.
    let uv = egui::Rect::from_min_max(
        egui::pos2(
            (inner.min.x - rendered_rect.min.x) / rendered_rect.width().max(1e-6),
            (inner.min.y - rendered_rect.min.y) / rendered_rect.height().max(1e-6),
        ),
        egui::pos2(
            (inner.max.x - rendered_rect.min.x) / rendered_rect.width().max(1e-6),
            (inner.max.y - rendered_rect.min.y) / rendered_rect.height().max(1e-6),
        ),
    );

    FramePlan {
        rendered,
        render_size,
        uv,
        on_screen,
        scale,
        visible: inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(iw: u32, ih: u32, zoom: f32, viewport: (f32, f32)) -> FramePlan {
        frame_plan(
            iw,
            ih,
            View {
                zoom,
                ..Default::default()
            },
            egui::vec2(viewport.0, viewport.1),
        )
    }

    #[test]
    fn fitting_renders_the_whole_frame() {
        let p = plan(6000, 4000, 1.0, (1200.0, 900.0));
        assert_eq!(p.rendered, Region::FULL);
        assert_eq!(
            p.uv,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }

    /// Below fit, the picture is drawn smaller with surround around it.
    ///
    /// The viewer used to clamp at fit, which meant a photograph could only
    /// ever be shown filling something. Seeing it *as an object* — with room
    /// around it, the way it would sit on a page — needs the other direction.
    #[test]
    fn zooming_out_below_fit_draws_the_picture_smaller() {
        let fit = plan(6000, 4000, 1.0, (1200.0, 900.0));
        let out = plan(6000, 4000, 0.5, (1200.0, 900.0));

        assert!(
            (out.on_screen.x - fit.on_screen.x * 0.5).abs() < 1.0,
            "half zoom should draw at half the size: {} against {}",
            out.on_screen.x,
            fit.on_screen.x
        );
        // Still the whole photograph, and still one region.
        assert_eq!(out.rendered, Region::FULL);
        assert!(out.visible.width() >= 0.999 && out.visible.height() >= 0.999);
    }

    /// And it renders for the size it will be drawn at.
    ///
    /// Rendering the full viewport and letting egui sample it down aliases,
    /// the same way any 2:1 downscale does without a mip chain — and it costs
    /// four times the pixels to look worse.
    #[test]
    fn zooming_out_renders_fewer_pixels_rather_than_downscaling() {
        let fit = plan(6000, 4000, 1.0, (1200.0, 900.0));
        let out = plan(6000, 4000, 0.5, (1200.0, 900.0));
        assert!(
            out.render_size.0 < fit.render_size.0,
            "{:?} against {:?}",
            out.render_size,
            fit.render_size
        );
    }

    #[test]
    fn fitting_never_renders_more_than_the_viewport() {
        let p = plan(6000, 4000, 1.0, (1200.0, 900.0));
        assert!(
            p.render_size.0 <= 1200 && p.render_size.1 <= 900,
            "{:?}",
            p.render_size
        );
    }

    /// The reason region rendering exists.
    ///
    /// Zoomed to 100% on a 24MP photo, the old approach rendered the whole
    /// frame capped at 2560px and then magnified it — you would be judging
    /// focus on an upscaled thumbnail. Rendering only the visible rectangle
    /// gives it real pixels at the same memory cost.
    #[test]
    fn zooming_in_narrows_the_region_rather_than_magnifying() {
        let fit = plan(6000, 4000, 1.0, (1200.0, 900.0));
        let close = plan(6000, 4000, 5.0, (1200.0, 900.0));

        assert!(
            close.rendered.size[0] < fit.rendered.size[0] * 0.5,
            "zooming should shrink the rendered region: {:?}",
            close.rendered
        );
        // Same pixel budget, spent on a smaller piece of the photograph.
        assert!(close.render_size.0 <= 1500, "{:?}", close.render_size);
    }

    #[test]
    fn a_zoomed_region_carries_a_margin_for_spatial_effects() {
        let p = plan(6000, 4000, 4.0, (1200.0, 900.0));
        // The displayed rectangle sits strictly inside the rendered one, so
        // halation and blur have neighbours to read at the viewport edge.
        assert!(
            p.uv.min.x > 0.001 && p.uv.max.x < 0.999,
            "uv was {:?}",
            p.uv
        );
    }

    #[test]
    fn the_view_cannot_wander_off_the_picture() {
        let p = frame_plan(
            4000,
            3000,
            View {
                zoom: 3.0,
                centre: egui::vec2(5.0, -2.0),
            },
            egui::vec2(1000.0, 800.0),
        );
        assert!(p.rendered.offset[0] >= 0.0 && p.rendered.offset[1] >= 0.0);
        assert!(p.rendered.offset[0] + p.rendered.size[0] <= 1.0001);
        assert!(p.rendered.offset[1] + p.rendered.size[1] <= 1.0001);
    }

    #[test]
    fn a_small_image_is_never_rendered_larger_than_it_is() {
        // Rendering more pixels than the source has buys nothing but memory.
        let p = plan(800, 600, 1.0, (4000.0, 3000.0));
        assert!(
            p.render_size.0 <= 800 && p.render_size.1 <= 600,
            "{:?}",
            p.render_size
        );
    }

    #[test]
    fn the_render_is_always_capped() {
        let p = plan(12000, 9000, 20.0, (8000.0, 6000.0));
        assert!(
            p.render_size.0 <= MAX_PREVIEW && p.render_size.1 <= MAX_PREVIEW,
            "{:?}",
            p.render_size
        );
    }

    #[test]
    fn degenerate_inputs_do_not_produce_a_zero_texture() {
        for (iw, ih, vw, vh) in [
            (1u32, 1u32, 1.0f32, 1.0f32),
            (0, 0, 10.0, 10.0),
            (100, 1, 50.0, 50.0),
        ] {
            let p = frame_plan(iw, ih, View::default(), egui::vec2(vw, vh));
            assert!(p.render_size.0 >= 1 && p.render_size.1 >= 1, "{p:?}");
        }
    }
}
