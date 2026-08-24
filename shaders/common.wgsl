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
    // Which part of the whole frame this pass renders, in frame uv:
    // xy = top-left offset, zw = size. (0, 0, 1, 1) is the whole image.
    //
    // The preview renders only the visible rectangle when zoomed in, so that
    // 100% is genuinely 1:1 rather than an upscaled thumbnail. Anything that
    // reasons about the *frame* rather than the texture has to go through the
    // helpers below, or it will drift as you pan.
    region: vec4<f32>,
    // Parameters, packed by pe_effects::pack. Slot n is p[n / 4][n % 4].
    p: array<vec4<f32>, 16>,
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

// Position within the whole frame, for effects anchored to the image rather
// than to the viewport — a vignette's centre, a grain lattice.
fn frame_uv(uv: vec2<f32>) -> vec2<f32> {
    return u.region.xy + uv * u.region.zw;
}

// The inverse of frame_uv: a point in the whole frame, back to this pass's uv.
//
// Needed by any effect that samples somewhere it has *named* in frame terms —
// a blur sweeping about a centre point, a region being averaged. The effects
// that only offset from where they already are get by with frame_to_uv on the
// distance alone.
fn uv_from_frame(f: vec2<f32>) -> vec2<f32> {
    return (f - u.region.xy) / max(u.region.zw, vec2<f32>(1e-6));
}

// A distance expressed as a fraction of the frame, converted to this pass's uv.
// Without it a halation radius would shrink as you zoom in, because the texture
// covers less of the frame.
fn frame_to_uv(d: f32) -> f32 {
    return d / max(u.region.z, 1e-6);
}

// Pixel dimensions of the whole frame, not of this pass's texture.
fn frame_size() -> vec2<f32> {
    return u.image_size / max(u.region.zw, vec2<f32>(1e-6));
}

fn frame_aspect() -> f32 {
    let f = frame_size();
    return f.x / max(f.y, 1.0);
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
//
// Integer, and that is the whole point. The float version this replaces ended
// in `fract()` of a product, and `fract()` of a product is chaotic in its last
// bit: backends disagree about whether to fuse the multiply-add that feeds it,
// so a one-ulp difference did not perturb the result, it replaced it. Grain
// survived that because it *adds* the hash and a tiny difference stayed tiny.
// Film damage did not, because it *thresholds* the hash, and a tiny difference
// decided whether a speck of dirt existed at all. Two machines rendered the
// same photograph with different dirt on it, and the document could not record
// which.
//
// WGSL's integer arithmetic is exactly defined and wraps, so this is identical
// on every backend by construction rather than by luck.
//
// Bit patterns rather than truncated integers, because callers pass a seed that
// need not be whole — `hash21(cell + vec2<f32>(dirt_seed, 13.0))` — and
// `u32(i32(x))` would throw the fraction away and collapse seeds onto each
// other.
fn hash21(p: vec2<f32>) -> f32 {
    var h = bitcast<u32>(p.x) * 0x9E3779B9u;
    h = h ^ (bitcast<u32>(p.y) * 0x85EBCA6Bu);
    // Without this an all-zero input stays all-zero through every step below,
    // and cell (0, 0) would score zero on a threshold that means "always".
    h = h ^ 0xC2B2AE35u;
    h = h ^ (h >> 16u);
    h = h * 0x7FEB352Du;
    h = h ^ (h >> 15u);
    h = h * 0x846CA68Bu;
    h = h ^ (h >> 16u);
    // Twenty-four bits over 2^24 lands exactly in [0, 1), which is the range
    // the old `fract()` promised. Using all thirty-two would round to 1.0 at
    // the top, and a noise value of exactly one is a speck nobody asked for.
    return f32(h >> 8u) * (1.0 / 16777216.0);
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

// ===========================================================================
// Film primitives, shared.
// ===========================================================================
//
// Halation, Bloom, Vignette and Film Grain exist twice over in this
// application: as rows of their own, and as sections inside Film Look Creator,
// because that is how Resolve ships them. Written twice they would be two
// implementations that drift apart the first time one of them is fixed, and
// the second one would be the one nobody remembers to fix.
//
// So they are written once, here, and both callers reach for the same
// function. The standalone effects pass their full parameter set; Film Look
// passes the handful of controls Resolve exposes inside it and leaves the rest
// at neutral.

const FILM_SAMPLES: i32 = 24;

/// Width of the negative in microns, by format.
///
/// This is the number grain size is measured against, and it is what makes
/// 16mm look like 16mm: the same emulsion on a smaller frame is magnified
/// further by the time it reaches the same print.
fn film_width_um(preset: f32) -> f32 {
    let i = i32(round(preset));
    switch i {
        case 0: { return 12500.0; }  // Super 16
        case 2: { return 52500.0; }  // 65mm
        default: { return 36000.0; } // 35mm still, and Custom
    }
}

/// How much a colour's own saturation counts towards being isolated.
///
/// A saturated highlight halates harder than a neutral one of the same
/// brightness, because the dye layer it came through is denser. At a level of
/// one this does nothing at all, which is why one is the default.
fn film_isolate_weight(c: vec3<f32>, level: f32) -> f32 {
    let s = rgb_to_hsv(max(c, vec3<f32>(0.0))).y;
    return clamp(1.0 + s * (level - 1.0), 0.0, 8.0);
}

/// What glows, per channel: the band between Threshold and Normalization.
///
/// A band rather than everything above one level, which is what stops a bright
/// sky glowing as hard as a specular highlight.
fn film_isolate3(c: vec3<f32>, threshold: f32, normalization: f32, level: f32) -> vec3<f32> {
    let band = max(normalization - threshold, 1e-3);
    let over = clamp((c - vec3<f32>(threshold)) / band, vec3<f32>(0.0), vec3<f32>(1.0));
    return over * film_isolate_weight(c, level);
}

/// The same, as one number, for the isolation preview.
fn film_isolate(c: vec3<f32>, threshold: f32, normalization: f32, level: f32) -> f32 {
    return dot(film_isolate3(c, threshold, normalization, level), AP1_LUMA);
}

/// One radius for all three channels: a disc of golden-angle samples, each
/// weighted down with distance so the core stays brighter than the tail.
///
/// `spread` is a fraction of the frame, never pixels — a pixel radius would
/// shrink to a rim on export. `stretch` is Resolve's Aspect Ratio, which makes
/// the glow oval the way an anamorphic one is.
fn film_halo(
    uv: vec2<f32>,
    spread: f32,
    stretch: f32,
    threshold: f32,
    normalization: f32,
    level: f32,
) -> vec3<f32> {
    let aspect = frame_aspect();
    let radius = frame_to_uv(spread);

    var glow = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < FILM_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(FILM_SAMPLES)) * radius;
        let offset = vec2<f32>(cos(angle) * r * stretch / aspect, sin(angle) * r / stretch);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        let w = 1.0 / (1.0 + r * 12.0);
        glow = glow + film_isolate3(s, threshold, normalization, level) * w;
        total = total + w;
    }
    return glow / max(total, 1e-4);
}

/// A separate radius per channel.
///
/// Each channel is read from its own sample position, so the three glows
/// genuinely differ in extent rather than being one glow that was tinted.
fn film_halo_rgb(
    uv: vec2<f32>,
    spread: f32,
    stretch: f32,
    threshold: f32,
    normalization: f32,
    level: f32,
    relative: vec3<f32>,
) -> vec3<f32> {
    let aspect = frame_aspect();
    let radius = frame_to_uv(spread);

    var glow = vec3<f32>(0.0);
    var total = vec3<f32>(0.0);
    for (var i = 0; i < FILM_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let base = sqrt((fi + 0.5) / f32(FILM_SAMPLES)) * radius;
        let dir = vec2<f32>(cos(angle) * stretch / aspect, sin(angle) / stretch);

        for (var ch = 0; ch < 3; ch = ch + 1) {
            let r = base * relative[ch];
            let s = textureSampleLevel(src_texture, src_sampler, uv + dir * r, 0.0).rgb;
            let over = film_isolate3(s, threshold, normalization, level);
            // Weighted against each channel's own radius, so a narrow channel
            // keeps a tight core rather than being diluted by the widest one.
            let w = 1.0 / (1.0 + r * 12.0);
            glow[ch] = glow[ch] + over[ch] * w;
            total[ch] = total[ch] + w;
        }
    }
    return glow / max(total, vec3<f32>(1e-4));
}

/// A plain average over the same disc — no isolation, no falloff weighting.
/// For where something wants softening rather than glowing.
fn film_halo_blur(uv: vec2<f32>, radius_uv: f32, stretch: f32) -> vec3<f32> {
    let aspect = frame_aspect();
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < FILM_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(FILM_SAMPLES)) * radius_uv;
        let offset = vec2<f32>(cos(angle) * r * stretch / aspect, sin(angle) * r / stretch);
        sum = sum + textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
    }
    return sum / f32(FILM_SAMPLES);
}

/// Light spilling out of the highlights: lens scatter rather than emulsion.
///
/// Only what is above the threshold spills — everything below it is not a
/// highlight and has no business glowing.
fn film_bloom(uv: vec2<f32>, radius: f32, threshold: f32) -> vec3<f32> {
    let aspect = frame_aspect();
    let uv_radius = frame_to_uv(radius);
    var glow = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < FILM_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(FILM_SAMPLES)) * uv_radius;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        let over = max(s - vec3<f32>(threshold), vec3<f32>(0.0));
        let w = 1.0 / (1.0 + r * 10.0);
        glow = glow + over * w;
        total = total + w;
    }
    return glow / max(total, 1e-4);
}

/// How far into the vignette a point is: 0 clear of it, 1 fully in it.
///
/// Frame coordinates, so the vignette stays anchored to the photograph rather
/// than following the viewport when the view is zoomed or panned.
fn film_vignette_t(
    uv: vec2<f32>,
    size: f32,
    softness: f32,
    /// The shape of the frame the vignette is cut for, as a ratio: 1.0 is a
    /// circle and 1.78 is 16:9. Resolve's Anamorphism, in Resolve's units.
    anamorphism: f32,
    border_shape: f32,
    rotation: f32,
    centre: vec2<f32>,
) -> f32 {
    var d = frame_uv(uv) - centre;

    // Rotation first, so it turns the shape rather than the frame.
    if rotation != 0.0 {
        let a = radians(rotation);
        let ca = cos(a);
        let sa = sin(a);
        d = vec2<f32>(d.x * ca - d.y * sa, d.x * sa + d.y * ca);
    }

    // Anamorphism widens the shape the way an anamorphic lens would.
    d.x = d.x / max(anamorphism, 0.05);

    // Border Shape moves between an ellipse and a rectangle by raising the
    // superellipse exponent. p = 2 is an ellipse; large p approaches a box.
    let p = mix(2.0, 14.0, clamp(border_shape, 0.0, 1.0));
    let e = pow(pow(abs(d.x), p) + pow(abs(d.y), p), 1.0 / p) / 0.5;

    // Size is how far the vignette reaches in: 0 hugs the very edge, 1 reaches
    // the centre.
    let inner = clamp(1.0 - size, 0.0, 0.999);
    let outer = inner + max(softness, 1e-3);
    return smoothstep(inner, outer, e);
}

/// The grain layer itself: zero-mean noise on a lattice sized in microns.
///
/// Everything that decides what the grain *looks* like lives here. What each
/// caller does with it afterwards — tonal weighting, per-channel gain, which
/// blend it is composited with — is the caller's business.
fn film_grain_field(
    uv: vec2<f32>,
    preset: f32,
    size_t: f32,
    aspect_ratio: f32,
    texture: f32,
    softness: f32,
    saturation: f32,
) -> vec3<f32> {
    // 0 is coarse and 1 is fine, over six stops of grain size. Exponential,
    // because grain size is: halfway along the slider should look halfway
    // between fine and coarse, and the eye reads size logarithmically.
    let size_um = 4.0 * exp2((1.0 - clamp(size_t, 0.0, 1.0)) * 6.0);
    let across = film_width_um(preset) / size_um;
    let f = frame_size();
    let frame_ratio = f.y / max(f.x, 1.0);
    let ar = max(aspect_ratio, 0.05);
    // Real emulsion grains are not round, and an anamorphic squeeze makes them
    // less so.
    let lattice = vec2<f32>(across / ar, across * frame_ratio * ar);
    // Frame coordinates: grain belongs to the negative, so it must not crawl
    // across the picture when the view is panned.
    let scaled = frame_uv(uv) * lattice + vec2<f32>(u.seed);
    let cell = floor(scaled);

    var mono = hash21(cell) - 0.5;
    var n = vec3<f32>(
        mono,
        hash21(cell + vec2<f32>(17.0, 3.0)) - 0.5,
        hash21(cell + vec2<f32>(5.0, 29.0)) - 0.5,
    );

    // Texture: a second, coarser octave mixed in. Fine emulsions read as even
    // fizz and coarse ones clump, and one lattice can only do the first — the
    // clumping is what makes a fast stock look fast.
    if texture > 0.0 {
        let coarse_cell = floor(scaled * 0.4);
        let coarse = vec3<f32>(
            hash21(coarse_cell + vec2<f32>(3.0, 11.0)) - 0.5,
            hash21(coarse_cell + vec2<f32>(23.0, 7.0)) - 0.5,
            hash21(coarse_cell + vec2<f32>(13.0, 41.0)) - 0.5,
        );
        n = mix(n, n * 0.55 + coarse * 0.85, texture);
        mono = mix(mono, mono * 0.55 + coarse.r * 0.85, texture);
    }

    // Softness blurs the grain layer by mixing each cell toward the average of
    // its neighbours — cheaper than a real blur and enough at grain scale.
    if softness > 0.0 {
        var neighbours = 0.0;
        neighbours = neighbours + hash21(cell + vec2<f32>(1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(-1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, 1.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, -1.0));
        let smoothed = neighbours * 0.25 - 0.5;
        n = mix(n, vec3<f32>(smoothed), clamp(softness, 0.0, 1.0));
        mono = mix(mono, smoothed, clamp(softness, 0.0, 1.0));
    }

    // Saturation 0 is monochrome grain, matching Resolve.
    return mix(vec3<f32>(mono), n, clamp(saturation, 0.0, 2.0));
}

// ---------------------------------------------------------------------------
// Detail bands, shared.
// ---------------------------------------------------------------------------
//
// Sharpen, Sharpen Edges and Soften and Sharpen are all the same idea: take
// the picture apart into scales, scale each band, put it back. Written once
// here because the *decomposition* is the part that has to agree between them
// — three implementations would put "medium detail" at three different sizes,
// and a user moving between the effects would find the same slider meaning
// something different in each.

/// One band: what is in the picture at this scale and not at the next one up.
///
/// A difference of blurs, which is the cheap and correct way to say it. The
/// sum of every band plus the coarsest blur is the original picture exactly,
/// so scaling the bands and adding them back can never invent or lose light.
fn detail_band(uv: vec2<f32>, inner: f32, outer: f32) -> vec3<f32> {
    return film_halo_blur(uv, inner, 1.0) - film_halo_blur(uv, outer, 1.0);
}

/// The strength of the local edge, 0 where the picture is flat.
///
/// The spread of a small neighbourhood rather than a gradient: a gradient has
/// a direction and picks a favourite, and an edge mask wants to say "there is
/// structure here" whichever way it runs.
fn edge_strength(uv: vec2<f32>, radius: f32) -> f32 {
    let aspect = frame_aspect();
    let here = luma(textureSampleLevel(src_texture, src_sampler, uv, 0.0).rgb);
    var lo = here;
    var hi = here;
    for (var i = 0; i < 8; i = i + 1) {
        let a = f32(i) * 0.785398;
        let offset = vec2<f32>(cos(a) * radius / max(aspect, 1e-4), sin(a) * radius);
        let s = luma(textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb);
        lo = min(lo, s);
        hi = max(hi, s);
    }
    return hi - lo;
}
