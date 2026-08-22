// The LUT texture holds ten rows: four tone curves, then the six secondaries
// in the order hue_vs_hue, hue_vs_sat, hue_vs_lum, lum_vs_sat, sat_vs_sat,
// sat_vs_lum. `upload_lut` writes them in that order, so it is load-bearing.
//
// Log. Slots: 0-3 the per-channel curve intensities (Y, R, G, B), 4-7 the
// four-part soft clip (low, low soft, high, high soft), 8-11 the parametric
// regions (shadows, darks, lights, highlights), 12-14 the splits between them.
// The point curves themselves live in the LUT texture.
//
// Slots are assigned by the order the parameters are declared in the registry,
// so that order is load-bearing and this comment has to move with it.
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

// Resolve's Soft Clip: where the knee starts at each end of the range, and
// how gradual it is. Exponential rather than a polynomial shoulder, so the
// approach to the limit is asymptotic and nothing ever actually clips — which
// is the point of soft clipping rather than hard.
fn apply_soft_clip(c: vec3<f32>) -> vec3<f32> {
    let low = slot(4u);
    let low_soft = slot(5u);
    let high = slot(6u);
    let high_soft = slot(7u);
    var o = c;

    if high > 0.0 {
        // Measured in from diffuse white towards grey, so "1" is a knee at
        // mid-grey rather than at some number with no meaning in the picture.
        let knee = CCT_WHITE - high * (CCT_WHITE - CCT_GREY);
        let room = max((CCT_WHITE - knee) * (0.35 + high_soft), 1e-4);
        let over = max(o - vec3<f32>(knee), vec3<f32>(0.0));
        o = min(o, vec3<f32>(knee)) + room * (vec3<f32>(1.0) - exp(-over / room));
    }
    if low > 0.0 {
        let knee = CCT_BLACK + low * (CCT_GREY - CCT_BLACK);
        let room = max((knee - CCT_BLACK) * (0.35 + low_soft), 1e-4);
        let under = max(vec3<f32>(knee) - o, vec3<f32>(0.0));
        o = max(o, vec3<f32>(knee)) - room * (vec3<f32>(1.0) - exp(-under / room));
    }
    return o;
}

fn parametric(c: vec3<f32>) -> vec3<f32> {
    let amounts = vec4<f32>(slot(8u), slot(9u), slot(10u), slot(11u));
    if all(amounts == vec4<f32>(0.0)) {
        return c;
    }

    // A user may drag one handle past another; that should squeeze a region,
    // not invert it.
    let a = slot(12u);
    let b = slot(13u);
    let d = slot(14u);
    let lo = min(a, min(b, d));
    let hi = max(a, max(b, d));
    let mid = clamp(a + b + d - lo - hi, lo, hi);

    // Position in the tonal range rather than in raw log, so the splits mean
    // the same thing they do on screen: 0 is black, 1 is diffuse white.
    let t = clamp((luma(c) - CCT_BLACK) / (CCT_WHITE - CCT_BLACK), 0.0, 1.0);
    let w = parametric_weights(t, lo, mid, hi);
    return c + vec3<f32>(dot(amounts, w) * PARAMETRIC_RANGE);
}

/// How far a full-travel Hue Vs Hue move rotates, either way.
///
/// A whole turn would let the curve send green to green the long way round,
/// which is a lot of travel for a control whose useful range is "nudge this
/// one hue". Ninety degrees each way is a strong move that still reads.
const SECONDARY_HUE_RANGE: f32 = 0.25;

/// Full travel of a Lum Gain, in stops.
const SECONDARY_LUM_STOPS: f32 = 2.0;

/// The six secondary curves.
///
/// Each is indexed by something about the pixel rather than by its level, and
/// each returns 0.5 when it should do nothing — which is why a missing one
/// bakes to a flat half rather than to a diagonal.
///
/// They run after the tone curves, on the result. A secondary asks "what
/// should happen to this hue", and the hue it should be asking about is the
/// one the picture ends up with, not the one it started with.
fn secondaries(c: vec3<f32>) -> vec3<f32> {
    var hsv = rgb_to_hsv(c);

    // Saturation is measured on linear light for the same reason Vibrance
    // measures it there: log compresses the gap between a colour's brightest
    // and dimmest channel, so a vivid colour only reaches about 0.47 on the
    // log axis and the right-hand half of every Sat curve would be empty.
    let lin = cct_decode(c);
    let top = max(max(lin.r, lin.g), lin.b);
    let bottom = min(min(lin.r, lin.g), lin.b);
    let sat_in = select(0.0, clamp(1.0 - bottom / top, 0.0, 1.0), top > 1e-5);
    let lum_in = clamp((luma(c) - CCT_BLACK) / (CCT_WHITE - CCT_BLACK), 0.0, 1.0);

    let hue = fract(hsv.x + 1.0);
    var sat_gain = 1.0;
    var lum_shift = 0.0;

    // Hue Vs Hue: rotate.
    hsv.x = fract(hsv.x + (lut(4, hue) - 0.5) * 2.0 * SECONDARY_HUE_RANGE + 1.0);
    // Hue Vs Sat, Lum Vs Sat, Sat Vs Sat: three ways of asking for the same
    // multiplier, so they multiply.
    sat_gain = sat_gain * (lut(5, hue) * 2.0);
    sat_gain = sat_gain * (lut(7, lum_in) * 2.0);
    sat_gain = sat_gain * (lut(8, sat_in) * 2.0);
    // Hue Vs Lum and Sat Vs Lum: a gain on the light, which in log is a shift.
    lum_shift = lum_shift + (lut(6, hue) - 0.5) * 2.0 * SECONDARY_LUM_STOPS;
    lum_shift = lum_shift + (lut(9, sat_in) - 0.5) * 2.0 * SECONDARY_LUM_STOPS;

    hsv.y = clamp(hsv.y * sat_gain, 0.0, 8.0);
    let out = hsv_to_rgb(hsv);
    // One stop is one log unit over the ACEScct scale factor.
    return out + vec3<f32>(lum_shift / 17.52);
}

/// A tone curve, over the range a photograph actually occupies.
///
/// The LUT is indexed 0..1 and the curve is drawn 0..1, but ACEScct runs from
/// well below black to well above diffuse white — an SDR frame sits between
/// about 0.073 and 0.555 of it. Handing the raw log value to the LUT would
/// squeeze the whole picture into the left half of the curve editor and leave
/// the right half addressing headroom nobody has.
///
/// So the curve spans black to diffuse white, and signal outside that range
/// carries the endpoint's offset rather than being clamped onto it. That is
/// what keeps a recovered highlight recoverable: pull the white point down and
/// everything above it comes with it, instead of fusing into one flat value.
fn tone_lut(row: i32, v: f32) -> f32 {
    let span = CCT_WHITE - CCT_BLACK;
    let u = (v - CCT_BLACK) / span;
    let inside = clamp(u, 0.0, 1.0);
    let overflow = (u - inside) * span;
    return CCT_BLACK + lut(row, inside) * span + overflow;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let p = parametric(c);

    // Per-channel first, then luma. The order matters: a luma curve applied
    // after per-channel work shapes the result the user just built, which is
    // what Resolve does and what people expect.
    //
    // The intensities mix each curve against the signal that went into it, so
    // dialling one back is dialling back that curve rather than the whole
    // chain — which is why there are four of them and not one.
    let per = vec3<f32>(tone_lut(1, p.r), tone_lut(2, p.g), tone_lut(3, p.b));
    var o = mix(p, per, vec3<f32>(slot(1u), slot(2u), slot(3u)) * 0.01);

    let luma_curved = vec3<f32>(tone_lut(0, o.r), tone_lut(0, o.g), tone_lut(0, o.b));
    o = mix(o, luma_curved, slot(0u) * 0.01);

    return apply_soft_clip(secondaries(o));
}
