// Linear. Slots follow the registry's declaration order, which is Resolve's
// panel order, so this list is load-bearing and moves when that does:
//
//   Isolation              0 threshold   1 normalization
//                          2 film_saturation_level        3 view_isolated
//   Dye Layer Reflections  4 strength    5 gamma   6 saturation   7 spread
//                          8 fine_tune_spread
//                          9 relative_red  10 relative_green  11 relative_blue
//   Secondary Glow        12 strength   13 gamma  14 spread  15-17 filter
//   Basic Grain           18 append    19 strength  20 size  21 softness
//                         22 saturation
//   Global Adjustments    23 view_glow_alone  24 reduce_highlights
//                         25 aspect_ratio     26 detail_loss
//
// Halation is light passing through the emulsion, scattering off the film
// base and re-exposing from behind. A linear-light phenomenon: done anywhere
// else it reads as fog rather than glow.
//
// There is no Hue control, and Resolve is right not to have one. The
// red-orange is not a tint anybody chose — it is what light coming back
// through the dye layers *is*. It is the constant below, and Saturation says
// how much of it reaches the picture.
//
// M1 uses single-pass golden-angle disc samples. That is a real approximation
// and it shows at large radii; a separable multi-pass blur is M2 work. Spread
// is a fraction of the frame, never pixels.

/// The colour of the dye layer reflection, as a hue on the wheel.
///
/// Around 12 degrees: the red-orange of light that has passed down through
/// cyan, magenta and yellow dye, bounced off the base, and come back up
/// through all three again.
const DYE_HUE: f32 = 12.0;

/// What the top of the Spread slider means, as a fraction of the frame.
///
/// The control runs 0 to 1 like Resolve's, but a glow whose radius is a whole
/// frame width is not a glow, so the slider has to mean something narrower.
/// The map is *squared*: a radius control needs its fine resolution at the
/// bottom, where the difference between a rim and a halo lives, and a linear
/// slider spends half its travel on sizes that all read as "large".
///
/// The secondary reaches further by construction. That is the entire reason
/// there are two: a tight core with a long falloff is not something one
/// radius can do.
const PRIMARY_REACH: f32 = 0.25;
const SECONDARY_REACH: f32 = 0.35;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let threshold = slot(0u);
    let normalization = slot(1u);
    let film_saturation_level = slot(2u);
    let view_isolated = slot(3u) > 0.5;
    let strength = slot(4u);
    let gamma = max(slot(5u), 1e-3);
    let saturation = clamp(slot(6u), 0.0, 3.0);
    let spread_t = clamp(slot(7u), 0.0, 1.0);
    let spread = spread_t * spread_t * PRIMARY_REACH;
    let fine_tune = slot(8u) > 0.5;
    let relative = max(vec3<f32>(slot(9u), slot(10u), slot(11u)), vec3<f32>(0.0));
    let secondary_strength = slot(12u);
    let secondary_gamma = max(slot(13u), 1e-3);
    let secondary_t = clamp(slot(14u), 0.0, 1.0);
    let secondary_spread = secondary_t * secondary_t * SECONDARY_REACH;
    let secondary_filter = slot3(15u);
    let append_grain = slot(18u) > 0.5;
    let grain_strength = slot(19u);
    let grain_size = slot(20u);
    let grain_softness = slot(21u);
    let grain_saturation = slot(22u);
    let view_glow_alone = slot(23u) > 0.5;
    let reduce_highlights = slot(24u);
    let aspect_ratio = max(slot(25u), 0.05);
    let detail_loss = slot(26u);

    // View Isolated Regions answers the question the Isolation group exists to
    // answer — what is going to glow — without making you infer it from the
    // result. Drawn before the early-out, so it still works at strength zero.
    if view_isolated {
        return vec3<f32>(film_isolate(c, threshold, normalization, film_saturation_level));
    }

    if strength <= 0.0 && secondary_strength <= 0.0 {
        return c;
    }

    // With per-channel spread doing the colouring, the dye tint would be a
    // second bite at the same apple, so it stands down to neutral.
    var tint = vec3<f32>(1.0);
    if !fine_tune {
        let pure = hsv_to_rgb(vec3<f32>(fract(DYE_HUE / 360.0), 1.0, 1.0));
        tint = mix(vec3<f32>(1.0), pure, saturation);
    }

    var glow = vec3<f32>(0.0);
    if strength > 0.0 && spread > 0.0 {
        var g: vec3<f32>;
        if fine_tune {
            g = film_halo_rgb(
                uv,
                spread,
                aspect_ratio,
                threshold,
                normalization,
                film_saturation_level,
                relative,
            );
            // Saturation still applies, pulling the naturally-separated glow
            // toward or away from neutral.
            g = mix(vec3<f32>(dot(g, AP1_LUMA)), g, saturation);
        } else {
            g = film_halo(
                uv,
                spread,
                aspect_ratio,
                threshold,
                normalization,
                film_saturation_level,
            );
        }
        // Gamma shapes the falloff: below one the glow reaches further out at
        // low levels, above one it stays tight around the source.
        g = pow(max(g, vec3<f32>(0.0)), vec3<f32>(gamma));
        glow = glow + g * tint * strength;
    }

    // The secondary glow is wider and weaker, and takes its colour from its
    // own Filter rather than the dye — it is lens and gate scatter, not
    // emulsion, so it has no reason to be red.
    if secondary_strength > 0.0 && secondary_spread > 0.0 {
        var g = film_halo(
            uv,
            secondary_spread,
            aspect_ratio,
            threshold,
            normalization,
            film_saturation_level,
        );
        g = pow(max(g, vec3<f32>(0.0)), vec3<f32>(secondary_gamma));
        glow = glow + g * secondary_filter * 2.0 * secondary_strength * 0.5;
    }

    // Grain inside the halation rather than over it. The order is the whole
    // point: grain laid on top sits on the glow like dust on glass, where
    // grain applied here is in the emulsion the glow happened in.
    if append_grain && grain_strength > 0.0 {
        let n = film_grain_field(uv, 1.0, grain_size, 1.0, 0.0, grain_softness, grain_saturation);
        glow = glow * (vec3<f32>(1.0) + n * grain_strength);
    }

    if view_glow_alone {
        return glow;
    }

    var out = c;

    // Detail loss: the picture softens under the glow, the way a halated frame
    // gives up its fine detail to the scattered light.
    if detail_loss > 0.0 {
        let soft = film_halo_blur(uv, frame_to_uv(0.004 + detail_loss * 0.012), aspect_ratio);
        out = mix(out, soft, clamp(detail_loss, 0.0, 1.0));
    }

    out = out + glow;

    // The glow adds light, so without this the picture gets brighter as well
    // as glowier. Pulling the top back down is what makes it read as
    // scattering rather than as exposure.
    if reduce_highlights > 0.0 {
        let added = max(dot(glow, AP1_LUMA), 0.0);
        out = out / (vec3<f32>(1.0) + vec3<f32>(added * reduce_highlights));
    }
    return out;
}
