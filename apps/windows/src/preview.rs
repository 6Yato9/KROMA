//! Getting the graded image onto the screen.
//!
//! Still no image processing here — this file owns *framing*: how much of the
//! photograph is on screen, at what resolution, and how that rectangle is
//! handed to egui. The pixels are `pe-render`'s business.

use std::sync::Arc;

use egui::mutex::RwLock;
use egui_wgpu::wgpu;
use pe_color::space;
use pe_core::Document;
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext, ImageTexture, Region, RenderError, TransformPass};

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

/// Where the viewer is looking.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// 1.0 fits the whole photograph in the viewport. 2.0 shows half of it.
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
}

impl Preview {
    pub fn new(
        gpu: GpuContext,
        egui_renderer: Arc<RwLock<egui_wgpu::Renderer>>,
        image: &DecodedImage,
    ) -> Self {
        let source = ImageTexture::upload_rgba8(
            &gpu.device,
            &gpu.queue,
            image.width,
            image.height,
            &image.pixels,
            "source",
        )
        .expect("source upload");

        let to_working = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
        let to_display = TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT);
        let renderer = EffectRenderer::new(&gpu.device);

        Self {
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
        Ok(())
    }

    /// Render the document for the current view.
    pub fn render(
        &mut self,
        image: &DecodedImage,
        doc: &Document,
        view: View,
        viewport: egui::Vec2,
    ) -> Result<Framing, RenderError> {
        let plan = frame_plan(image.width, image.height, view, viewport);

        if self.size != plan.render_size || self.region != plan.rendered {
            self.rebuild(doc, plan.render_size, plan.rendered)?;
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
        );
        self.gpu.queue.submit([encoder.finish()]);

        Ok(Framing {
            texture: self.texture_id.expect("registered on rebuild"),
            uv: plan.uv,
            size: plan.on_screen,
            scale: plan.scale,
            visible: plan.visible,
            passes,
        })
    }

    fn rebuild(
        &mut self,
        doc: &Document,
        size: (u32, u32),
        region: Region,
    ) -> Result<(), RenderError> {
        let (w, h) = size;

        // Decode the region straight into the working space. The fullscreen
        // triangle plus a linear sampler gives a bilinear reduction for free.
        self.working = Some(self.to_working.to_working_in(
            &self.gpu,
            &self.source,
            &doc.color.pipeline().input,
            w,
            h,
            region,
        ));

        let display = ImageTexture::new(
            &self.gpu.device,
            w,
            h,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            "preview-display",
        );

        let mut renderer = self.egui_renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &self.gpu.device,
                &display.view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id = Some(renderer.register_native_texture(
                    &self.gpu.device,
                    &display.view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        drop(renderer);

        self.display = Some(display);
        self.size = size;
        self.region = region;
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
    let zoom = view.zoom.clamp(1.0, 32.0);
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

    // Enough pixels for the screen, but never more than the source actually
    // has for that rectangle, and never past the memory cap.
    let want_w = vw * (rendered.size[0] / visible.x.max(1e-4));
    let want_h = vh * (rendered.size[1] / visible.y.max(1e-4));
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

    let on_screen = egui::vec2(
        (iw * visible.x * scale).min(vw),
        (ih * visible.y * scale).min(vh),
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
