// Prelude prepended to every effect shader.
//
// WGSL has no #include, so pe-render concatenates:
//     common.wgsl  +  effects/<name>.wgsl  +  epilogue.wgsl
// An effect file therefore defines exactly one function:
//
//     fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32>
//
// `c` arrives already in the effect's declared working space. It never
// converts its own input — the epilogue does that, driven by the registry.

// Must match pe_core::BlendMode's declaration order. Asserted by
// pe_core::stack::tests::blend_mode_indices_match_the_shader.
const BLEND_NORMAL: u32 = 0u;
const BLEND_ADD: u32 = 1u;
const BLEND_MULTIPLY: u32 = 2u;
const BLEND_SCREEN: u32 = 3u;
const BLEND_OVERLAY: u32 = 4u;
const BLEND_SOFT_LIGHT: u32 = 5u;
const BLEND_HARD_LIGHT: u32 = 6u;
const BLEND_DARKEN: u32 = 7u;
const BLEND_LIGHTEN: u32 = 8u;
const BLEND_DIFFERENCE: u32 = 9u;
const BLEND_EXCLUSION: u32 = 10u;
const BLEND_COLOR: u32 = 11u;
const BLEND_LUMINOSITY: u32 = 12u;

struct EffectUniform {
    image_size: vec2<f32>,
    inv_size: vec2<f32>,
    opacity: f32,
    blend_mode: u32,
    // 1 when the effect declared WorkingSpace::Log.
    space_is_log: u32,
    // Preview size as a fraction of full resolution. Spatial effects multiply
    // pixel-space work by this so a 1200px preview and a 6000px export agree.
    scale: f32,
    seed: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    // Parameters, packed by pe_effects::pack. Slot n is p[n / 4][n % 4].
    p: array<vec4<f32>, 12>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: EffectUniform;
// 256x4 R16Float: row 0 luma, 1 red, 2 green, 3 blue. Bound for every effect;
// only Curves reads it.
@group(0) @binding(3) var lut_texture: texture_2d<f32>;

// AP1 luminance weights, from primaries::AP1.rgb_to_xyz() row 1. Guarded by
// pe_color::tests::ap1_luma_matches_the_shader_constant — do not edit by hand.
const AP1_LUMA = vec3<f32>(0.2722287, 0.6740818, 0.0536895);

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, AP1_LUMA);
}

// Read parameter slot `i` by its flat index, matching pe_effects::pack.
//
// Most effects index `u.p[0].x` directly and read fine that way. Film Damage
// has 33 parameters, where hand-written vec4 lanes stop being readable and
// start being a place for off-by-one errors to hide.
fn slot(i: u32) -> f32 {
    let v = u.p[i / 4u];
    return v[i % 4u];
}

fn slot3(i: u32) -> vec3<f32> {
    return vec3<f32>(slot(i), slot(i + 1u), slot(i + 2u));
}

// NOT `sign()`. WGSL's sign(0.0) is 0.0, which would make cct_encode(0.0)
// return 0 instead of 0.0729 and crush every true black. Rust's f32::signum
// returns 1.0 for +0.0, and the CPU reference the golden tests compare against
// relies on that.
fn sgn(x: f32) -> f32 {
    return select(-1.0, 1.0, x >= 0.0);
}

const CCT_A: f32 = 10.5402377416545;
const CCT_B: f32 = 0.0729055341958355;

// Tonal anchors *in ACEScct*, for effects that split the image into shadows,
// midtones and highlights.
//
// These are not 0.0 / 0.5 / 1.0, and reaching for those is the mistake this
// block exists to prevent: an SDR image occupies roughly 0.073 to 0.555 in
// log, so a threshold at 0.6 never fires at all. Guarded by
// pe_color::tests::acescct_anchors_match_the_shader.
const CCT_BLACK: f32 = 0.0729055341958355;   // linear 0.0
const CCT_GREY: f32 = 0.4135886669;          // linear 0.18
const CCT_WHITE: f32 = 0.5547945205;         // linear 1.0

fn cct_encode1(v: f32) -> f32 {
    let s = sgn(v);
    let a = abs(v);
    if a <= 0.0078125 {
        return s * (CCT_A * a + CCT_B);
    }
    return s * ((log2(a) + 9.72) / 17.52);
}

fn cct_decode1(v: f32) -> f32 {
    let s = sgn(v);
    let a = abs(v);
    if a <= 0.155251141552511 {
        return s * ((a - CCT_B) / CCT_A);
    }
    if a < 1.468 {
        return s * exp2(a * 17.52 - 9.72);
    }
    return s * 65504.0;
}

fn cct_encode(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(cct_encode1(c.r), cct_encode1(c.g), cct_encode1(c.b));
}

fn cct_decode(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(cct_decode1(c.r), cct_decode1(c.g), cct_decode1(c.b));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let r = max(c.r, 0.0);
    let g = max(c.g, 0.0);
    let b = max(c.b, 0.0);
    let hi = max(r, max(g, b));
    let lo = min(r, min(g, b));
    let d = hi - lo;

    var h = 0.0;
    if d > 1e-8 {
        if hi == r {
            h = (g - b) / d;
            if h < 0.0 { h = h + 6.0; }
        } else if hi == g {
            h = (b - r) / d + 2.0;
        } else {
            h = (r - g) / d + 4.0;
        }
        h = h / 6.0;
    }
    let s = select(0.0, d / hi, hi > 1e-8);
    return vec3<f32>(h, s, hi);
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let h = fract(hsv.x) * 6.0;
    let s = clamp(hsv.y, 0.0, 8.0);
    let v = hsv.z;
    let i = floor(h);
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let idx = i32(i) % 6;
    if idx == 0 { return vec3<f32>(v, t, p); }
    if idx == 1 { return vec3<f32>(q, v, p); }
    if idx == 2 { return vec3<f32>(p, v, t); }
    if idx == 3 { return vec3<f32>(p, q, v); }
    if idx == 4 { return vec3<f32>(t, p, v); }
    return vec3<f32>(v, p, q);
}

// Cheap hash noise. Not cryptographic and not meant to be — grain only needs
// to be uncorrelated and stable for a given pixel.
fn hash21(p: vec2<f32>) -> f32 {
    var v = fract(p * vec2<f32>(123.34, 456.21));
    v = v + dot(v, v + 45.32);
    return fract(v.x * v.y);
}

// Smooth rolloff approaching 1.0, so a curve pushed into clipping compresses
// rather than flattening to a hard edge.
fn soft_clip(c: vec3<f32>, amount: f32) -> vec3<f32> {
    let knee = mix(1.0, 0.7, clamp(amount, 0.0, 1.0));
    let over = max(c - vec3<f32>(knee), vec3<f32>(0.0));
    let room = max(1.0 - knee, 1e-4);
    return min(c, vec3<f32>(knee)) + room * (vec3<f32>(1.0) - exp(-over / room));
}

fn blend_channels(base: vec3<f32>, top: vec3<f32>, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return base + top; }
        case 2u: { return base * top; }
        case 3u: { return vec3<f32>(1.0) - (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - top); }
        case 4u: {
            return select(
                vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - top),
                2.0 * base * top,
                base < vec3<f32>(0.5)
            );
        }
        case 5u: {
            return select(
                base + (2.0 * top - vec3<f32>(1.0)) * (sqrt(max(base, vec3<f32>(0.0))) - base),
                base - (vec3<f32>(1.0) - 2.0 * top) * base * (vec3<f32>(1.0) - base),
                top < vec3<f32>(0.5)
            );
        }
        case 6u: {
            return select(
                vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - top),
                2.0 * base * top,
                top < vec3<f32>(0.5)
            );
        }
        case 7u: { return min(base, top); }
        case 8u: { return max(base, top); }
        case 9u: { return abs(base - top); }
        case 10u: { return base + top - 2.0 * base * top; }
        case 11u: {
            // Hue and saturation from the effect, luminance from the input.
            let hsv = rgb_to_hsv(top);
            let recoloured = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y, max(luma(base), 0.0)));
            return recoloured;
        }
        case 12u: {
            // Luminance from the effect, colour from the input.
            let l = luma(base);
            return select(base * (luma(top) / l), top, l < 1e-6);
        }
        default: { return top; }
    }
}

/// True for blend modes that model light arriving at a sensor, which must be
/// evaluated in linear space regardless of the effect's own working space.
/// This is why Screen-mode halation reads as glow rather than haze.
fn blend_is_light_like(mode: u32) -> bool {
    return mode == BLEND_ADD || mode == BLEND_SCREEN;
}

fn blend(base: vec3<f32>, top: vec3<f32>, mode: u32, opacity: f32) -> vec3<f32> {
    return mix(base, blend_channels(base, top, mode), clamp(opacity, 0.0, 1.0));
}
