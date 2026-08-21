// Log. Slots: 0 soft_clip, 1-4 the parametric regions (shadows, darks,
// lights, highlights), 5-7 the splits between them. The point curves
// themselves live in the LUT texture.
//
// Parametric first, then the point curves. A parametric curve is a shape you
// cannot make un-smooth; a point curve is one you can make do anything. Doing
// the constrained one first means the freehand curve shapes the result the
// user has already settled on, rather than being quietly reshaped by it.
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

// Full travel on a region slider, in log units. Gentler than the Basic
// panel's tonal sliders: this curve is for shaping, not for rescuing.
const PARAMETRIC_RANGE: f32 = 0.12;

// Mirrors pe_core::parametric::weights. If one changes, so must the other —
// the editor draws the curve from the Rust version and this applies it.
fn parametric_weights(t: f32, lo: f32, mid: f32, hi: f32) -> vec4<f32> {
    let c = vec4<f32>(lo * 0.5, (lo + mid) * 0.5, (mid + hi) * 0.5, (hi + 1.0) * 0.5);
    if t <= c.x {
        return vec4<f32>(1.0, 0.0, 0.0, 0.0);
    } else if t <= c.y {
        let k = smoothstep(c.x, max(c.y, c.x + 1e-5), t);
        return vec4<f32>(1.0 - k, k, 0.0, 0.0);
    } else if t <= c.z {
        let k = smoothstep(c.y, max(c.z, c.y + 1e-5), t);
        return vec4<f32>(0.0, 1.0 - k, k, 0.0);
    } else if t <= c.w {
        let k = smoothstep(c.z, max(c.w, c.z + 1e-5), t);
        return vec4<f32>(0.0, 0.0, 1.0 - k, k);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

fn parametric(c: vec3<f32>) -> vec3<f32> {
    let amounts = vec4<f32>(slot(1u), slot(2u), slot(3u), slot(4u));
    if all(amounts == vec4<f32>(0.0)) {
        return c;
    }

    // A user may drag one handle past another; that should squeeze a region,
    // not invert it.
    let a = slot(5u);
    let b = slot(6u);
    let d = slot(7u);
    let lo = min(a, min(b, d));
    let hi = max(a, max(b, d));
    let mid = clamp(a + b + d - lo - hi, lo, hi);

    // Position in the tonal range rather than in raw log, so the splits mean
    // the same thing they do on screen: 0 is black, 1 is diffuse white.
    let t = clamp((luma(c) - CCT_BLACK) / (CCT_WHITE - CCT_BLACK), 0.0, 1.0);
    let w = parametric_weights(t, lo, mid, hi);
    return c + vec3<f32>(dot(amounts, w) * PARAMETRIC_RANGE);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let p = parametric(c);
    var o = vec3<f32>(lut(1, p.r), lut(2, p.g), lut(3, p.b));
    o = vec3<f32>(lut(0, o.r), lut(0, o.g), lut(0, o.b));

    let amount = u.p[0].x;
    if amount > 0.0 {
        o = soft_clip(o, amount);
    }
    return o;
}
