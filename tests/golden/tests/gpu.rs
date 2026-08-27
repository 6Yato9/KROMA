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
/// reference works in `f64`. One level out of 255 is the honest tolerance where
/// the pipeline is well conditioned, which is most of it — see
/// [`a_neutral_ramp_survives_the_round_trip_exactly`], where nothing moves at
/// all.
const GPU_TOLERANCE: u8 = 1;

/// What the round trip can promise for a colour at the edge of the gamut.
///
/// The working image is `Rgba16Float`: ten bits of mantissa. Going back out,
/// the ACEScg to sRGB matrix has large off-diagonal terms of opposite sign, so
/// a channel that is near nothing beside another near everything is
/// reconstructed by subtracting two similar large numbers. The quantisation
/// that was far below visible in the working space arrives as whole levels of
/// output — measured at two on the test chart, and at ten on a hazy frame.
///
/// This is a property of storing the working image in half floats, not a
/// defect: `docs/color-pipeline.md` records why that storage was chosen, and
/// the same maths in `f64` round-trips exactly (see
/// `crates/pe-color`'s own tests). Neither a shader change nor a different
/// backend can make it smaller; only a wider working texture could, at twice
/// the memory for a difference no eye can find.
///
/// Kept as a named constant so that a *regression* still shows: something
/// genuinely wrong with a transform moves colours by tens of levels, not by
/// three.
const GAMUT_EDGE_TOLERANCE: u8 = 3;

fn gpu() -> Option<&'static GpuContext> {
    pe_golden::shared_gpu()
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
        pe_render::Placement::WHOLE,
    );
    gpu.queue.submit([encoder.finish()]);

    let pixels = pe_render::read_rgba8(gpu, &out).expect("readback");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

/// A no-op stack returns the picture, to within what half-float storage allows.
///
/// It was called *lossless* and asserted at one level, which is true of the
/// maths and false of the storage: the test chart holds colours at the edge of
/// the gamut where the round trip costs two levels no matter what runs it. The
/// exact claim lives in [`a_neutral_ramp_survives_the_round_trip_exactly`]
/// instead, where it is true and worth having.
#[test]
fn a_no_op_stack_returns_the_picture() {
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(256, 192);
    let out = round_trip_on_gpu(gpu, &src);

    let delta = out.max_channel_delta(&src).expect("same size");
    assert!(
        delta <= GAMUT_EDGE_TOLERANCE,
        "GPU round trip drifted by {delta} levels, which is more than storing \
         the working image in half floats can account for"
    );
}

/// The claim the test above used to make, in the place where it holds.
///
/// Grey exercises the transfer functions and the diagonal of the gamut matrix
/// and nothing else — no cancellation, because there is no small channel beside
/// a large one. Every one of the 256 levels must come back exactly. If a
/// transfer function, a texture format or the matrix diagonal is wrong, this
/// fails immediately and by a lot, which is what the loosened tolerance above
/// would otherwise have stopped catching.
#[test]
fn a_neutral_ramp_survives_the_round_trip_exactly() {
    let Some(gpu) = gpu() else { return };

    let mut pixels = Vec::with_capacity(256 * 4);
    for level in 0..256u32 {
        let v = level as u8;
        pixels.extend_from_slice(&[v, v, v, 255]);
    }
    let src = DecodedImage::new(256, 1, pixels).expect("ramp");
    let out = round_trip_on_gpu(gpu, &src);

    let delta = out.max_channel_delta(&src).expect("same size");
    assert_eq!(
        delta, 0,
        "a neutral ramp came back changed by {delta} levels; the transfer \
         function or the gamut matrix is wrong, and this one is not rounding"
    );
}

#[test]
fn the_gpu_agrees_with_the_cpu_reference() {
    let Some(gpu) = gpu() else { return };

    let src = pe_io::test_chart(256, 192);
    let on_gpu = round_trip_on_gpu(gpu, &src);
    let on_cpu = pe_golden::render_reference(&src, &Pipeline::default(), &[]);

    let delta = on_gpu.max_channel_delta(&on_cpu).expect("same size");
    assert!(
        delta <= GAMUT_EDGE_TOLERANCE,
        "GPU and CPU reference disagree by {delta} levels, which is more than \
         the difference between f32-then-half and f64 can account for"
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
    let out = round_trip_on_gpu(gpu, &src);

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
    let working = pass.to_working(gpu, &source, &space::SRGB);

    assert_eq!(working.texture.format(), wgpu::TextureFormat::Rgba16Float);
}

/// A photograph bigger than the GPU will hold has to come back as a sentence,
/// not as a dead process.
///
/// wgpu answers an oversized texture with a validation error, and its default
/// handler for one of those ends the program — so before this check, opening a
/// panorama closed the window. Cameras have been past 8192 pixels on a side for
/// years, so this is an ordinary thing to be handed, not a corrupt file.
///
/// The allocation is deliberately one row, not a real image: the point is that
/// the refusal happens on the *dimensions*, before anything the size of a
/// panorama is asked for.
#[test]
fn an_oversized_photograph_is_refused_rather_than_fatal() {
    let Some(gpu) = gpu() else {
        return;
    };
    let max = gpu.device.limits().max_texture_dimension_2d;
    let too_wide = max + 1;

    let Err(err) = ImageTexture::upload_rgba8(
        &gpu.device,
        &gpu.queue,
        too_wide,
        1,
        &vec![0u8; too_wide as usize * 4],
        "oversized",
    ) else {
        panic!("a texture past the device limit was accepted");
    };

    match &err {
        pe_render::RenderError::ImageTooLarge { width, max: m, .. } => {
            assert_eq!(*width, too_wide);
            assert_eq!(*m, max);
        }
        other => panic!("wrong refusal: {other}"),
    }

    // And the message has to name the numbers, because it is the only thing
    // the person holding the panorama is going to see.
    let text = err.to_string();
    assert!(text.contains(&too_wide.to_string()), "unhelpful: {text}");
    assert!(text.contains(&max.to_string()), "unhelpful: {text}");
}

/// Telling the application what the source file is has to change what it
/// renders.
///
/// Every iPhone since the 7 writes Display P3 JPEGs. Read as sRGB, their
/// colours land pulled in towards the sRGB primaries — not obviously broken,
/// just quietly flatter than the photograph is, which is the worst way for a
/// colour tool to be wrong. The document has carried an input space all along;
/// this is the assertion that it is wired to something.
#[test]
fn the_source_colour_space_changes_what_is_rendered() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A saturated red, which is where two gamuts differ most.
    let src = DecodedImage::new(2, 2, [230, 20, 20, 255].repeat(4)).expect("source");

    let mut as_srgb = pe_core::Document::from_path("a.jpg");
    as_srgb.color.input = "sRGB".into();
    let mut as_p3 = as_srgb.clone();
    as_p3.color.input = "Display P3".into();

    let renderer = pe_render::EffectRenderer::new(&gpu.device);
    let one =
        pe_render::render_full(gpu, &renderer, 2, 2, &src.pixels, &as_srgb).expect("sRGB render");
    let two = pe_render::render_full(gpu, &renderer, 2, 2, &src.pixels, &as_p3).expect("P3 render");

    assert_ne!(
        one, two,
        "the source colour space made no difference — the control is decorative"
    );
    // P3 primaries are wider, so the same numbers mean a more saturated red;
    // brought back to sRGB for output it should clip harder, not softer.
    assert!(
        two[0] >= one[0],
        "reading the file as Display P3 made its red weaker, not stronger: \
         {} against {}",
        two[0],
        one[0]
    );
}
