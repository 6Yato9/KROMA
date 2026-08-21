// Log. Slot 0: soft_clip. The curves themselves live in the LUT texture.
//
// Per-channel first, then luma. The order matters: a luma curve applied after
// per-channel work shapes the result the user just built, which is what
// Resolve does and what people expect.
// Linear interpolation between LUT entries, not nearest-neighbour.
//
// Nearest-neighbour quantises to 1/255 in *log* space, and log values for an
// SDR image only span roughly 0.07..0.55, so barely half the table is in use.
// Worse, converting a saturated colour back out of the wide working gamut
// amplifies small errors — large matrix terms cancel to a near-zero channel —
// so that quantisation showed up as a 42-level shift on saturated cyan. With
// interpolation an identity curve is exact, which is what makes a freshly
// added Curves layer invisible until the user drags something.
fn lut(row: i32, x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0) * 255.0;
    let i0 = i32(floor(t));
    let i1 = min(i0 + 1, 255);
    let f = t - floor(t);
    let a = textureLoad(lut_texture, vec2<i32>(i0, row), 0).r;
    let b = textureLoad(lut_texture, vec2<i32>(i1, row), 0).r;
    return mix(a, b, f);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var o = vec3<f32>(lut(1, c.r), lut(2, c.g), lut(3, c.b));
    o = vec3<f32>(lut(0, o.r), lut(0, o.g), lut(0, o.b));

    let amount = u.p[0].x;
    if amount > 0.0 {
        o = soft_clip(o, amount);
    }
    return o;
}
