// Linear. Slots follow the registry's declaration order:
//   0 small  1 medium  2 large  3 small_size
//
// The same three bands as Sharpen, except each one is bipolar. Positive
// sharpens, negative softens, and doing both at once is the entire point:
// medium at -0.8 with small at +0.3 is skin that keeps its pores and loses
// its blotches, which is not something a sharpener or a blur can do alone.

const BAND_STEP: f32 = 4.0;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let small = slot(0u);
    let medium = slot(1u);
    let large = slot(2u);
    if small == 0.0 && medium == 0.0 && large == 0.0 {
        return c;
    }
    let r0 = frame_to_uv(max(slot(3u), 1e-4));
    let r1 = r0 * BAND_STEP;
    let r2 = r1 * BAND_STEP;

    let b0 = c - film_halo_blur(uv, r0, 1.0);
    let b1 = detail_band(uv, r0, r1);
    let b2 = detail_band(uv, r1, r2);

    // A gain of -1 removes the band completely, which is what makes the
    // negative end a real softening rather than a weaker sharpen.
    return c + b0 * small + b1 * medium + b2 * large;
}
