// Linear. Slots:
//   0 region      1 source_x    2 source_y   3 source_width  4 source_height
//   5 mode         6 stabilize   7 strength
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// In Resolve this removes flicker and colour drift *between frames*: it
// measures a region on each one and corrects it back to a reference. There is
// no next frame here, so what is left is the half that still means something
// on a still — measure a region and correct it to neutral.
//
// That half is worth having on its own. Point it at something that should be
// grey and it computes the white balance for you; point it at something that
// should be mid and it sets the exposure. It is the eyedropper every editor
// has, with the region and the strength exposed instead of hidden.
//
// Linear, because both corrections are about light: channel gains and an
// exposure scale.

/// Taps used to measure the region.
///
/// A proper average needs a reduction pass over the whole rectangle. This
/// samples it instead, on a golden-angle spiral so the taps land evenly at any
/// count — the same trick the dark-channel prior in Dehaze uses, and accurate
/// enough for a statistic that is then used as a single gain.
const STABILIZER_TAPS: i32 = 48;

/// What the corrected region is aimed at: 18% grey, the anchor every exposure
/// meter in the world is built around.
const STABILIZER_TARGET: f32 = 0.18;

struct RegionStats {
    mean: vec3<f32>,
    spread: f32,
}

fn measure(centre: vec2<f32>, size: vec2<f32>) -> RegionStats {
    var sum = vec3<f32>(0.0);
    var sum_sq = 0.0;
    for (var i = 0; i < STABILIZER_TAPS; i = i + 1) {
        let fi = f32(i);
        // Golden angle, radius by sqrt so the density is even rather than
        // piling up in the middle.
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(STABILIZER_TAPS)) * 0.5;
        let at = centre + vec2<f32>(cos(angle), sin(angle)) * r * size;
        let s = textureSampleLevel(
            src_texture,
            src_sampler,
            uv_from_frame(clamp(at, vec2<f32>(0.0), vec2<f32>(1.0))),
            0.0,
        ).rgb;
        sum = sum + s;
        let l = luma(s);
        sum_sq = sum_sq + l * l;
    }
    let n = f32(STABILIZER_TAPS);
    let mean = sum / n;
    let mean_l = luma(mean);
    var out: RegionStats;
    out.mean = mean;
    // Standard deviation of luminance across the region, for the contrast
    // half of "Levels and Contrast".
    out.spread = sqrt(max(sum_sq / n - mean_l * mean_l, 0.0));
    return out;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let region = slot(0u);
    // Mode says which of the two corrections to make. Resolve shows a pair of
    // checkboxes beside it, greyed out — they read back what Mode has chosen
    // rather than offering a second way to set it, so Mode is the control.
    let mode = i32(round(slot(5u)));
    let stabilize_wb = mode != 2;
    let stabilize_brightness = mode != 1;
    let with_contrast = i32(round(slot(6u))) == 1;
    let strength = slot(7u);

    if strength <= 0.0 || (!stabilize_wb && !stabilize_brightness) {
        return c;
    }

    // Entire Frame is the whole picture; Selected Area is the rectangle the
    // four Analysis Region controls describe.
    var centre = vec2<f32>(0.5, 0.5);
    var size = vec2<f32>(1.0, 1.0);
    if i32(round(region)) == 0 {
        centre = vec2<f32>(slot(1u), slot(2u));
        size = vec2<f32>(max(slot(3u), 1e-3), max(slot(4u), 1e-3));
    }

    let stats = measure(centre, size);
    let mean = max(stats.mean, vec3<f32>(1e-5));
    let mean_l = max(luma(mean), 1e-5);

    var out = c;

    if stabilize_wb {
        // Gains that take the region's own colour to neutral. Normalised by
        // its luminance so the correction changes the balance and not the
        // exposure — those are two controls, and doing both here would make
        // the second one impossible to turn off.
        let gains = vec3<f32>(mean_l) / mean;
        out = out * gains;
    }

    if stabilize_brightness {
        out = out * (STABILIZER_TARGET / mean_l);
        if with_contrast {
            // "Levels and Contrast": also normalise how far the region
            // spreads, so a flat measurement is opened up and a harsh one is
            // pulled in. Around the target rather than around zero, or this
            // would be a gain again rather than a contrast.
            let target_spread = STABILIZER_TARGET * 0.5;
            let scale = clamp(target_spread / max(stats.spread, 1e-4), 0.25, 4.0);
            out = vec3<f32>(STABILIZER_TARGET) + (out - vec3<f32>(STABILIZER_TARGET)) * scale;
        }
    }

    return mix(c, out, clamp(strength, 0.0, 1.0));
}
