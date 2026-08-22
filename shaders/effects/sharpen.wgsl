// Linear. Slots follow the registry's declaration order:
//   0 amount  1 fine_size  2 fine  3 medium  4 large  5 chroma
//
// Unsharp masking split across three scales. One radius sharpens everything
// at one size, which is exactly what an over-sharpened photograph looks like;
// three bands let the grain of a surface and the shape of an edge take
// different amounts.
//
// Linear, because sharpening adds and subtracts light. Done in log it pulls
// harder in the shadows than the highlights for no reason anyone asked for.

/// How the three bands are spaced.
///
/// Each is four times the last, which is a little over two octaves apart —
/// far enough that the bands do not simply repeat each other, close enough
/// that nothing in the picture falls between them.
const BAND_STEP: f32 = 4.0;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = slot(0u);
    if amount <= 0.0 {
        return c;
    }
    let fine_size = max(slot(1u), 1e-4);
    let fine_gain = slot(2u);
    let medium_gain = slot(3u);
    let large_gain = slot(4u);
    let chroma_gain = clamp(slot(5u), 0.0, 10.0);

    let r0 = frame_to_uv(fine_size);
    let r1 = r0 * BAND_STEP;
    let r2 = r1 * BAND_STEP;

    // Three differences of blurs. Their sum plus the coarsest blur is the
    // original exactly, so scaling them can neither invent nor lose light.
    let b0 = c - film_halo_blur(uv, r0, 1.0);
    let b1 = detail_band(uv, r0, r1);
    let b2 = detail_band(uv, r1, r2);

    let detail = b0 * fine_gain + b1 * medium_gain + b2 * large_gain;

    // Luma and chroma separately. Sharpening colour as hard as brightness is
    // how a sharpened photograph gets coloured fringes — chroma detail is
    // coarser than the thing you are trying to bring out, so it comes up
    // first.
    let luma_part = vec3<f32>(luma(detail));
    let chroma_part = (detail - luma_part) * chroma_gain;

    return c + (luma_part + chroma_part) * amount * 0.5;
}
