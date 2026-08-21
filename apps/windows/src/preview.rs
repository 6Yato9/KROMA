//! Getting the graded image onto the screen.
//!
//! Still no image processing here — this file owns *sizing and registration*,
//! not pixels. It decides how big the preview is, asks `pe-render` for the
//! result, and hands the texture to egui.

use std::sync::Arc;

use egui::mutex::RwLock;
use egui_wgpu::wgpu;
use pe_color::space;
use pe_core::Document;
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext, ImageTexture, RenderError, TransformPass};

/// Upper bound on preview dimensions.
///
/// The stage cache keeps one working texture per row, so preview memory is
/// `rows x width x height x 8` bytes. At 2560 that is about 21 MB per row —
/// a twelve-row stack costs 250 MB, which is fine. Letting it follow a 5K
/// window unbounded would not be.
const MAX_PREVIEW: u32 = 2560;

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
        }
    }

    /// Swap in a different photograph.
    ///
    /// Zeroing `size` is what forces `resize` to rebuild the working and
    /// display textures on the next frame — the new image may be a different
    /// shape, so every cached stage and both intermediates are stale.
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

    /// Render the document and return the egui texture plus the number of GPU
    /// passes the frame actually cost.
    pub fn render(
        &mut self,
        image: &DecodedImage,
        doc: &Document,
        available: egui::Vec2,
    ) -> Result<(egui::TextureId, usize), RenderError> {
        let target = fit(
            image.width,
            image.height,
            available.x.max(1.0) as u32,
            available.y.max(1.0) as u32,
        );
        if self.size != target {
            self.resize(doc, target)?;
        }

        let working = self.working.as_ref().expect("resize built it");
        let display = self.display.as_ref().expect("resize built it");

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

        Ok((self.texture_id.expect("registered on resize"), passes))
    }

    /// Rebuild the working and display textures for a new preview size.
    fn resize(&mut self, doc: &Document, target: (u32, u32)) -> Result<(), RenderError> {
        let (w, h) = target;

        // Downsample into the working space in one pass. The fullscreen
        // triangle plus a linear sampler gives a bilinear reduction for free.
        self.working = Some(self.to_working.to_working_sized(
            &self.gpu,
            &self.source,
            &doc.color.pipeline().input,
            w,
            h,
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
        self.size = target;
        // The stack must re-run: every cached stage was the old size.
        self.renderer.invalidate();
        Ok(())
    }

    /// Render at full resolution for export. Separate path, no caching — see
    /// `pe_render::export`.
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

/// Largest size within `(max_w, max_h)` that preserves the image aspect ratio
/// and does not exceed [`MAX_PREVIEW`].
fn fit(img_w: u32, img_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let img_w = img_w.max(1);
    let img_h = img_h.max(1);
    let scale = (max_w as f32 / img_w as f32)
        .min(max_h as f32 / img_h as f32)
        // Never upsample: rendering more pixels than the source has buys
        // nothing but memory and time.
        .min(1.0);
    let w = ((img_w as f32 * scale) as u32).clamp(1, MAX_PREVIEW);
    let h = ((img_h as f32 * scale) as u32).clamp(1, MAX_PREVIEW);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_never_upsamples() {
        // A small image in a big window renders at its own size. Rendering
        // more pixels than exist buys nothing and costs cache memory.
        assert_eq!(fit(800, 600, 4000, 3000), (800, 600));
    }

    #[test]
    fn the_preview_fits_the_viewport() {
        let (w, h) = fit(6000, 4000, 1200, 900);
        assert!(w <= 1200 && h <= 900);
        // Aspect preserved within a pixel of rounding.
        let ratio = w as f32 / h as f32;
        assert!((ratio - 1.5).abs() < 0.01, "aspect became {ratio}");
    }

    #[test]
    fn the_preview_is_capped() {
        let (w, h) = fit(12000, 9000, 20000, 20000);
        assert!(w <= MAX_PREVIEW && h <= MAX_PREVIEW, "got {w}x{h}");
    }

    #[test]
    fn degenerate_sizes_do_not_produce_a_zero_texture() {
        for (iw, ih, mw, mh) in [(1, 1, 1, 1), (0, 0, 10, 10), (100, 1, 50, 50)] {
            let (w, h) = fit(iw, ih, mw, mh);
            assert!(w >= 1 && h >= 1, "{iw}x{ih} in {mw}x{mh} gave {w}x{h}");
        }
    }
}
