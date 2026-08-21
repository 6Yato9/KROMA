//! Full-resolution export.
//!
//! Deliberately a separate path from the interactive renderer, for one reason:
//! **memory**. The preview keeps a cached texture per row so that editing row 9
//! of 12 re-runs four rows. At 24 megapixels an `Rgba16Float` texture is 192 MB,
//! so a twelve-row stack would want 2.3 GB of VRAM. Export does not need
//! caching — it runs once — so it ping-pongs between two textures and submits
//! one pass at a time.
//!
//! Submitting per pass is what lets a single uniform buffer and LUT be reused:
//! several passes recorded into one encoder would all observe the last write.
//!
//! Both paths run identical shaders. Only size and caching differ, which is
//! what makes "what I see is what I export" true rather than aspirational.

use pe_color::space;
use pe_core::Document;

use crate::device::GpuContext;
use crate::effect::EffectRenderer;
use crate::texture::{ImageTexture, SOURCE_FORMAT};
use crate::transform::TransformPass;
use crate::{RenderError, read_rgba8};

/// Render a document at the source image's full resolution.
///
/// `pixels` is tightly packed 8-bit RGBA in the document's input colour space.
/// Returns the same layout in its output space, ready to be written to a file.
pub fn render_full(
    gpu: &GpuContext,
    renderer: &EffectRenderer,
    width: u32,
    height: u32,
    pixels: &[u8],
    doc: &Document,
) -> Result<Vec<u8>, RenderError> {
    let pipeline = doc.color.pipeline();

    let source = ImageTexture::upload_rgba8(
        &gpu.device,
        &gpu.queue,
        width,
        height,
        pixels,
        "export-source",
    )?;

    let to_working = TransformPass::new(&gpu.device, crate::WORKING_FORMAT);
    let mut front = to_working.to_working(gpu, &source, &pipeline.input);
    let mut back = ImageTexture::new_working(&gpu.device, width, height, "export-back");

    let scratch = renderer.scratch(&gpu.device);

    for row in doc.stack.iter() {
        if row.is_noop() {
            continue;
        }
        let Some(effect) = pe_effects::by_key(&row.effect) else {
            // Unknown to this build. The row survives in the document; there is
            // simply nothing to draw.
            continue;
        };

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export-row"),
            });
        renderer.encode_into(
            gpu,
            &mut encoder,
            &scratch,
            &front.view,
            &back.view,
            (width, height),
            // Export is full resolution by definition, so spatial effects get
            // a scale of 1. This is the number that makes grain and halation
            // match the preview instead of shrinking to nothing.
            1.0,
            row,
            effect,
        );
        // One submit per pass: the shared uniform and LUT are rewritten each
        // time, so the passes must not be batched.
        gpu.queue.submit([encoder.finish()]);
        std::mem::swap(&mut front, &mut back);
    }

    // Working space out to the document's output space.
    let out = ImageTexture::new(
        &gpu.device,
        width,
        height,
        SOURCE_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        "export-out",
    );
    let to_display = TransformPass::new(&gpu.device, SOURCE_FORMAT);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("export-display"),
        });
    to_display.encode(
        gpu,
        &mut encoder,
        &front.view,
        &out.view,
        &space::ACESCG,
        &pipeline.output,
    );
    gpu.queue.submit([encoder.finish()]);

    read_rgba8(gpu, &out)
}

/// Peak VRAM the export path needs, in bytes, for reporting and for deciding
/// whether to warn before a very large export.
///
/// Two working textures plus the source and the output, independent of stack
/// depth — which is the whole point of ping-ponging. A 24MP export comes to
/// about 576 MB, which is a real cost worth surfacing before a batch run on a
/// small card, but it does not grow as the user adds rows.
pub fn estimated_vram(width: u32, height: u32) -> u64 {
    let px = width as u64 * height as u64;
    px * 8 * 2 + px * 4 * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_memory_does_not_grow_with_stack_depth() {
        // The property the ping-pong exists to provide: a 24MP export costs
        // about 576 MB whether the stack has one row or fifty. Caching a
        // texture per row the way the preview does would need over 1.5 GB for
        // nine rows, and keep climbing.
        let ping_pong = estimated_vram(6000, 4000);
        assert!(
            ping_pong < 700 * 1024 * 1024,
            "24MP export wants {ping_pong} bytes"
        );

        let per_row_cached = 6000u64 * 4000 * 8 * 9;
        assert!(per_row_cached > ping_pong * 2);
    }

    #[test]
    fn preview_sized_export_is_cheap() {
        // 1080p comes to roughly 50 MB, which is what makes a full-resolution
        // render of a preview-sized image a non-event.
        assert!(estimated_vram(1920, 1080) < 60 * 1024 * 1024);
    }
}
