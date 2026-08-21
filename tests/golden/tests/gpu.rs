//! Does the GPU agree with the CPU?
//!
//! `pe-color` is the oracle: a complete `f64` implementation of the pipeline.
//! These tests push the same image through the real shaders and check the
//! results match. Without this, "the window showed something" is the only
//! evidence the shaders are correct, which is no evidence at all.
//!
//! Skipped with a message when no GPU is available, so CI runners without one
//! stay green rather than failing misleadingly.

use pe_color::{Pipeline, space};
use pe_io::DecodedImage;
use pe_render::{GpuContext, ImageTexture, TransformPass};

/// Vendors round the last bit differently, and the GPU works in `f32` where the
/// reference works in `f64`. One level out of 255 is the honest tolerance; more
/// than that is a bug, not rounding.
const GPU_TOLERANCE: u8 = 1;

fn gpu() -> Option<GpuContext> {
    match GpuContext::new_blocking() {
        Ok(g) => {
            eprintln!("GPU: {}", g.describe());
            Some(g)
        }
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

/// Run source → ACEScg (16-bit float) → sRGB on the GPU and read the result
/// back. This is exactly what the app does every frame.
fn round_trip_on_gpu(gpu: &GpuContext, src: &DecodedImage) -> DecodedImage {
    let source = ImageTexture::upload_rgba8(
        &gpu.device,
        &gpu.queue,
        src.width,
        src.height,
        &src.pixels,
        "test-source",
    )
    .expect("upload");

    let to_working = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
    let working = to_working.to_working(gpu, &source, &space::SRGB);

    // Back out to an sRGB-encoded 8-bit target, the same format the swapchain
    // uses, so the hardware applies the OETF exactly as it does on screen.
    let out = ImageTexture::new(
        &gpu.device,
        src.width,
        src.height,
        pe_render::SOURCE_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        "test-output",
    );
    let to_display = TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    to_display.encode(
        gpu,
        &mut encoder,
        &working.view,
        &out.view,
        &space::ACESCG,
        &space::SRGB,
    );
    gpu.queue.submit([encoder.finish()]);

    let pixels = pe_render::read_rgba8(gpu, &out).expect("readback");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

#[test]
fn the_gpu_pipeline_is_lossless_for_a_no_op_stack() {
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(256, 192);
    let out = round_trip_on_gpu(&gpu, &src);

    let delta = out.max_channel_delta(&src).expect("same size");
    assert!(
        delta <= GPU_TOLERANCE,
        "GPU round trip drifted by {delta} levels; the shader or a texture \
         format is wrong, not rounding"
    );
}

#[test]
fn the_gpu_agrees_with_the_cpu_reference() {
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(256, 192);
    let on_gpu = round_trip_on_gpu(&gpu, &src);
    let on_cpu = pe_golden::render_reference(&src, &Pipeline::default(), &[]);

    let delta = on_gpu.max_channel_delta(&on_cpu).expect("same size");
    assert!(
        delta <= GPU_TOLERANCE,
        "GPU and CPU reference disagree by {delta} levels"
    );
}

#[test]
fn a_neutral_ramp_stays_neutral_on_the_gpu() {
    // The GPU counterpart of the CPU test. If the gamut matrix reaches the
    // shader with the wrong memory layout — the classic `mat3x3` padding bug —
    // greys pick up a tint and this catches it, where a whole-image delta
    // might not.
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(256, 8);
    let out = round_trip_on_gpu(&gpu, &src);

    for x in 0..out.width {
        let [r, g, b, _] = out.pixel(x, 0);
        let spread = r.max(g).max(b) - r.min(g).min(b);
        assert!(
            spread <= GPU_TOLERANCE,
            "column {x} picked up a tint on the GPU: {r},{g},{b}"
        );
    }
}

#[test]
fn the_working_texture_really_is_16_bit_float() {
    // Guards the invariant by observation rather than by reading the constant:
    // if someone "optimises" the intermediate to 8-bit, the format reported by
    // the live texture changes and this fails.
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(64, 64);
    let source =
        ImageTexture::upload_rgba8(&gpu.device, &gpu.queue, 64, 64, &src.pixels, "src").unwrap();
    let pass = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
    let working = pass.to_working(&gpu, &source, &space::SRGB);

    assert_eq!(working.texture.format(), wgpu::TextureFormat::Rgba16Float);
}
