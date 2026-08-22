// Log. Slots follow the registry's declaration order, which is Resolve's panel
// order, so that order is load-bearing and this list moves when it does:
//
//    0 preset             1 color_blend        2 effects_blend   3 lut_compatible
//    4 film_look_blend    5 core_look          6 skin_bias
//    7 exposure           8 contrast           9 highlights     10 fade
//   11 white_balance     12 tint              13 subtractive_sat
//   14 richness          15 bleach_bypass
//   16 split_tone_enable 17 split_tone_mode   18 split_tone_amount
//   19 split_tone_hue    20 split_tone_pivot
//   21 vignette_enable   22 vignette_amount   23 vignette_size
//   24 halation_enable   25 halation_highlights_only
//   26 halation_amount   27 halation_radius   28 halation_saturation
//   29 halation_hue
//   30 bloom_enable      31 bloom_amount      32 bloom_radius
//   33 grain_enable      34 grain_preset      35 grain_amount    36 grain_size
//   37 grain_softness    38 grain_saturation  39 image_defocus
//   40 gate_enable       41 gate_preset       42 gate_ratio_h    43 gate_ratio_v
//   44 gate_curvature    45 gate_padding
//
// Resolve's Film Look Creator: a film response, then the five things a print
// does to the light on its way to the screen. Every one of those five is also
// a row of its own here — which is exactly why none of them is written twice.
// The gathers, the vignette falloff and the grain lattice all live in
// common.wgsl, and the standalone effects call the same functions this does.
//
// **On the two-space rule.** This effect declares Log, because the bulk of it
// is perception: a film response curve is the shape of the print, not a
// photometric measurement. But halation and bloom are light scattering, and
// light scattering averaged in a log signal reads as fog rather than glow.
// So the two glow sections decode to linear, do their arithmetic there, and
// encode back — explicitly, in the open, with the shared cct_ helpers.
//
// That is the one place in this application where an effect touches its own
// encoding, and it is deliberate: this single row genuinely spans both halves
// of the pipeline, because Resolve's does. The rule it does not break is the
// important one — the *renderer* still decides what arrives here, and nothing
// downstream has to guess what leaves.

const FILM_SPAN: f32 = CCT_WHITE - CCT_BLACK;

/// What the top of a radius slider means, as a fraction of the frame.
///
/// Resolve's Halation Radius runs to 10 and its Bloom Radius to 100. Neither
/// is a distance in any unit the sampler can use, so these are the conversion.
const HALATION_REACH: f32 = 0.02;
const BLOOM_REACH: f32 = 0.0016;

struct CoreLook {
    /// How hard the shoulder and toe bend, before the sliders scale them.
    shoulder: f32,
    toe: f32,
    /// Contrast through the middle, and how much colour the stock holds.
    contrast: f32,
    saturation: f32,
    /// How far the stock pushes the shadows cool and the highlights warm.
    tone: f32,
}

/// The five looks, as the numbers that separate them.
///
/// Not presets that write the sliders: choosing one changes the *base* the
/// sliders modulate, so a look is still that look after you have adjusted it.
/// Presets that overwrite your work are a worse control than a menu.
fn core_look_of(index: f32) -> CoreLook {
    var s: CoreLook;
    let i = i32(round(index));
    switch i {
        // A modern colour negative: long shoulder, gentle toe, restrained.
        case 0: {
            s.shoulder = 0.55; s.toe = 0.35; s.contrast = 1.06;
            s.saturation = 1.02; s.tone = 1.0;
        }
        // Vintage: a shorter shoulder, more contrast, more colour, and the
        // cool-shadow warm-highlight split an old print drifts into.
        case 1: {
            s.shoulder = 0.75; s.toe = 0.50; s.contrast = 1.18;
            s.saturation = 1.16; s.tone = 1.8;
        }
        // Modern: cleaner than either, closer to a digital intermediate.
        case 2: {
            s.shoulder = 0.40; s.toe = 0.25; s.contrast = 1.02;
            s.saturation = 1.00; s.tone = 0.5;
        }
        // Bleach: silver left in the print. Hard, contrasty, desaturated.
        case 3: {
            s.shoulder = 1.05; s.toe = 0.85; s.contrast = 1.30;
            s.saturation = 0.72; s.tone = 0.3;
        }
        // Neutral: the response with nothing added, for building from.
        default: {
            s.shoulder = 0.30; s.toe = 0.20; s.contrast = 1.0;
            s.saturation = 1.0; s.tone = 0.0;
        }
    }
    return s;
}

/// Roll one end of the range off asymptotically.
///
/// Exponential, so nothing ever reaches the limit. That is the property that
/// matters: a shoulder that *arrives* somewhere is a clip with extra steps,
/// and the whole reason film highlights look the way they do is that they
/// never quite get there.
fn film_rolloff(v: f32, knee: f32, amount: f32, upward: bool) -> f32 {
    if amount <= 0.0 {
        return v;
    }
    let room = max((1.0 - knee) * (0.35 + amount * 0.9), 1e-4);
    if upward {
        let over = max(v - knee, 0.0);
        return min(v, knee) + room * (1.0 - exp(-over / room));
    }
    let under = max(knee - v, 0.0);
    return max(v, knee) - room * (1.0 - exp(-under / room));
}

/// How much a colour looks like skin, 0 to 1.
///
/// A window on the hue wheel around orange, narrowed by saturation: skin is a
/// fairly desaturated orange, and a fire engine is not skin.
fn skin_weight(c: vec3<f32>) -> f32 {
    let hsv = rgb_to_hsv(max(c, vec3<f32>(0.0)));
    let h = hsv.x * 360.0;
    let d = abs(h - 28.0);
    let near = 1.0 - smoothstep(8.0, 30.0, min(d, 360.0 - d));
    let plausible = 1.0 - smoothstep(0.45, 0.8, hsv.y);
    return near * plausible;
}

/// The film gate's aperture: 1 inside the hole, 0 outside it.
fn gate_mask(uv: vec2<f32>, ratio_h: f32, ratio_v: f32, curvature: bool, padding: f32) -> f32 {
    let f = frame_uv(uv) - vec2<f32>(0.5);
    let frame = frame_size();
    let picture = frame.x / max(frame.y, 1.0);
    let gate = max(ratio_h, 0.05) / max(ratio_v, 0.05);
    // Fit the gate inside the picture, whichever way round the two are.
    var half = vec2<f32>(0.5, 0.5);
    if gate > picture {
        half.y = 0.5 * picture / gate;
    } else {
        half.x = 0.5 * gate / picture;
    }
    half = half * (1.0 - clamp(padding, 0.0, 0.95));

    // A real gate is a stamped hole, so its corners are rounded because the
    // punch was. p = 2 is an ellipse, large p a rectangle; a gate is between.
    let p = select(20.0, 5.0, curvature);
    let d = pow(
        pow(abs(f.x) / max(half.x, 1e-3), p) + pow(abs(f.y) / max(half.y, 1e-3), p),
        1.0 / p,
    );
    return 1.0 - smoothstep(0.985, 1.0, d);
}

/// How much the format magnifies its own imperfections.
///
/// Relative to 65mm. A Super 8 frame is about six times smaller, so by the
/// time it reaches the same screen its grain is six times bigger and its
/// halation six times wider — which is the whole reason anyone can tell the
/// formats apart. Presets scales the spatial half by this rather than writing
/// the sliders, so the numbers you set stay the numbers you set.
fn format_scale(preset: f32) -> f32 {
    let i = i32(round(preset));
    switch i {
        case 1: { return 1.46; }   // 35mm
        case 2: { return 2.90; }   // 16mm
        case 3: { return 5.80; }   // Super 8
        default: { return 1.0; }   // 65mm
    }
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let format = format_scale(slot(0u));
    let color_blend = clamp(slot(1u), 0.0, 1.0);
    let effects_blend = clamp(slot(2u), 0.0, 1.0);
    let lut_compatible = slot(3u) > 0.5;
    let film_look_blend = clamp(slot(4u), 0.0, 1.0);
    let look = core_look_of(slot(5u));
    let skin_bias = slot(6u);

    // ---- The colour half -------------------------------------------------
    var t = (c - vec3<f32>(CCT_BLACK)) / FILM_SPAN;
    let mid = (CCT_GREY - CCT_BLACK) / FILM_SPAN;

    // Exposure is a stop count, and a stop is a doubling of light — so it is
    // an offset here, where the signal is already logarithmic.
    t = t + vec3<f32>(slot(7u) * (0.301 / FILM_SPAN) * 0.5);

    t = vec3<f32>(mid) + (t - vec3<f32>(mid)) * (slot(8u) * look.contrast);

    // The shoulder, per channel rather than on luminance — that is what film
    // does, because each dye layer saturates on its own, and it is why a red
    // that clips on film goes orange rather than white.
    let shoulder = max(slot(9u), 0.0) * look.shoulder * 2.0;
    t = vec3<f32>(
        film_rolloff(t.r, 0.72, shoulder, true),
        film_rolloff(t.g, 0.72, shoulder, true),
        film_rolloff(t.b, 0.72, shoulder, true),
    );

    // Fade lifts the toe: the milky black of a print that has been through a
    // projector a few hundred times.
    let fade = clamp(slot(10u), 0.0, 1.0);
    if fade > 0.0 {
        let lift = fade * 0.18;
        t = vec3<f32>(lift) + t * (1.0 - lift);
        let toe = fade * look.toe * 2.0;
        t = vec3<f32>(
            film_rolloff(t.r, 0.14 + lift, toe, false),
            film_rolloff(t.g, 0.14 + lift, toe, false),
            film_rolloff(t.b, 0.14 + lift, toe, false),
        );
    }

    var out = vec3<f32>(CCT_BLACK) + t * FILM_SPAN;

    // White Balance and Tint. Both are offsets on a log signal, which is what
    // makes them hold their strength across the whole range instead of only
    // biting in the highlights.
    let kelvin = clamp(slot(11u), 2000.0, 20000.0);
    let warmth = (1.0 / 6500.0 - 1.0 / kelvin) * 8000.0;
    let green = slot(12u) * 0.0008;
    out = out + vec3<f32>(warmth * 0.03, 0.0, -warmth * 0.03)
        + vec3<f32>(-green, green * 2.0, -green);

    // Subtractive saturation: dye removed rather than chroma pushed. A print
    // saturates by taking light away, which is why film reds go deep instead
    // of going electric.
    let sub_sat = slot(13u) * look.saturation;
    if abs(sub_sat - 1.0) > 1e-4 {
        let lin = cct_decode(out);
        let grey = vec3<f32>(dot(lin, AP1_LUMA));
        // Each channel moves away from grey *multiplicatively*, so the
        // saturated result is darker than the neutral one — which is the whole
        // difference between this and an ordinary saturation control.
        let ratio = max(lin, vec3<f32>(1e-5)) / max(grey, vec3<f32>(1e-5));
        out = cct_encode(grey * pow(ratio, vec3<f32>(sub_sat)));
    }

    // Richness: density in the midtones, where a print carries its weight.
    let richness = slot(14u);
    if abs(richness - 1.0) > 1e-4 {
        let l = luma(out);
        let w = 1.0 - abs((l - CCT_GREY) / max(CCT_WHITE - CCT_BLACK, 1e-4)) * 2.0;
        out = out * (1.0 + (richness - 1.0) * 0.12 * clamp(w, 0.0, 1.0));
    }

    // Bleach bypass: silver left in the print alongside the dye. Contrast goes
    // up and colour goes out, because the silver is neutral and dense.
    let bleach = clamp(slot(15u), 0.0, 1.0);
    if bleach > 0.0 {
        let l = vec3<f32>(luma(out));
        let hard = vec3<f32>(CCT_GREY) + (l - vec3<f32>(CCT_GREY)) * 1.5;
        out = mix(out, mix(hard, out * 0.35 + hard * 0.65, 0.35), bleach);
    }

    // The stock's own split: cool shadows, warm highlights.
    if look.tone > 0.0 {
        let l = luma(out);
        let shadow_w = 1.0 - smoothstep(CCT_BLACK, CCT_GREY, l);
        let highlight_w = smoothstep(CCT_GREY, CCT_WHITE, l);
        out = out
            + vec3<f32>(-0.004, 0.0, 0.006) * look.tone * shadow_w
            + vec3<f32>(0.006, 0.001, -0.005) * look.tone * highlight_w;
    }

    // Split Tone, the same shape as the standalone effect: opposing hues
    // either side of a pivot, rolled off towards the extremes in Natural so
    // the brightest point stays white.
    if slot(16u) > 0.5 && slot(18u) > 0.0 {
        let mode = round(slot(17u));
        let amount = slot(18u);
        let hue_angle = slot(19u);
        let pivot = clamp(slot(20u), 0.0, 1.0);
        let l = clamp((luma(out) - CCT_BLACK) / FILM_SPAN, 0.0, 1.0);
        var s: f32;
        if l < pivot {
            s = (l - pivot) / max(pivot, 1e-4);
        } else {
            s = (l - pivot) / max(1.0 - pivot, 1e-4);
        }
        s = clamp(s, -1.0, 1.0);
        let hue = select(fract((hue_angle + 180.0) / 360.0), fract(hue_angle / 360.0), s > 0.0);
        var a = amount * abs(s);
        if mode < 0.5 {
            a = a * (1.0 - abs(s) * 0.55);
        }
        let tint = hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0));
        let tinted = out * (vec3<f32>(1.0) + tint - vec3<f32>(luma(tint)));
        out = mix(out, tinted, clamp(a * 0.6, 0.0, 1.0));
    }

    // Skin Bias pulls the whole colour half back towards the original wherever
    // the picture looks like a face. Applied to the *result* rather than to
    // each control, so it holds skin still no matter which slider moved it.
    if skin_bias != 0.0 {
        let w = skin_weight(c) * clamp(-skin_bias, 0.0, 1.0)
            + (1.0 - skin_weight(c)) * clamp(skin_bias, 0.0, 1.0);
        out = mix(out, c, w * 0.8);
    }

    out = mix(c, out, film_look_blend);
    let coloured = mix(c, out, color_blend);

    // ---- The effects half ------------------------------------------------
    // Nothing below here can be expressed as a function of one colour, which
    // is exactly what 3D LUT Compatible switches off.
    if lut_compatible || effects_blend <= 0.0 {
        return coloured;
    }
    out = coloured;

    // Halation and bloom are light, so they are done in light. Sampled from
    // the log source and decoded, because that is what the renderer handed us.
    let halation_on = slot(24u) > 0.5 && slot(26u) > 0.0 && slot(27u) > 0.0;
    let bloom_on = slot(30u) > 0.5 && slot(31u) > 0.0 && slot(32u) > 0.0;
    if halation_on || bloom_on {
        var lin = cct_decode(out);
        if halation_on {
            let highlights_only = slot(25u) > 0.5;
            // Threshold zero means the whole frame contributes; the usual case
            // is that only what is already bright does.
            let threshold = select(0.0, 0.45, highlights_only);
            let glow = film_halo(
                uv,
                slot(27u) * HALATION_REACH * format,
                1.0,
                threshold,
                1.0,
                1.0,
            );
            let pure = hsv_to_rgb(vec3<f32>(fract(slot(29u) * 0.05), 1.0, 1.0));
            let tint = mix(vec3<f32>(1.0), pure, clamp(slot(28u), 0.0, 1.0));
            lin = lin + glow * tint * slot(26u);
        }
        if bloom_on {
            lin = lin + film_bloom(uv, slot(32u) * BLOOM_REACH * format, 0.5) * slot(31u);
        }
        out = cct_encode(max(lin, vec3<f32>(0.0)));
    }

    // Vignette. Light falloff, so it multiplies light rather than levels.
    if slot(21u) > 0.5 && slot(22u) > 0.0 {
        let t_v = film_vignette_t(uv, slot(23u), 0.45, 1.0, 0.0, 0.0, vec2<f32>(0.5));
        let lin = cct_decode(out) * (1.0 - clamp(t_v * slot(22u), 0.0, 1.0));
        out = cct_encode(max(lin, vec3<f32>(0.0)));
    }

    // Grain, on a picture that has already given up a little of its finest
    // detail — that is what puts the grain *in* the image rather than on it.
    if slot(33u) > 0.5 && slot(35u) > 0.0 {
        let defocus = clamp(slot(39u), 0.0, 1.0);
        if defocus > 0.0 {
            let soft = film_halo_blur(uv, frame_to_uv(0.0012 * defocus), 1.0);
            out = mix(out, soft, defocus * 0.5);
        }
        // The format's magnification lands on the grain as a size shift: the
        // same emulsion on a smaller negative reads coarser on the print.
        let size_t = clamp(slot(36u) - log2(format) / 6.0, 0.0, 1.0);
        let n = film_grain_field(uv, slot(34u), size_t, 1.0, 0.0, slot(37u), slot(38u));
        out = out + n * slot(35u) * 0.36;
    }

    // The gate is last: it is the hole the light came through, so everything
    // else happened inside it.
    if slot(40u) > 0.5 {
        let m = gate_mask(uv, slot(42u), slot(43u), slot(44u) > 0.5, slot(45u));
        out = mix(vec3<f32>(CCT_BLACK), out, m);
    }

    return mix(coloured, out, effects_blend);
}
