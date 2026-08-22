// Linear. Slots:
//   0 strength, 1-3 haze_colour, 4 display_depth, 5 shadow, 6 highlight
//
// Slots are assigned by the order the parameters are *declared* in the
// registry, so that order is load-bearing and this comment has to move with
// it. Reordering the panel without reordering this reads every control off
// its neighbour.
//
// Aerial perspective: distant objects lose contrast and drift toward the
// colour of the intervening air. The physical model is
//
//     observed = scene * t + haze * (1 - t)
//
// where t is transmission — 1 at the camera, falling toward 0 with distance.
// That is a statement about *light*, so this runs in linear. In a
// gamma-encoded space the recovery over-brightens the shadows.
//
// t is estimated with a dark channel prior: in a haze-free patch of a natural
// image at least one channel is almost always very dark, so whatever floor a
// patch does have is mostly haze. We approximate the patch minimum with a
// golden-angle disc rather than a true min-filter, and skip the guided-filter
// refinement the literature uses — so edges are softer than a full
// implementation. Both are multi-pass work, and belong with the rest of M3.

const DEHAZE_SAMPLES: i32 = 16;
// Leaving a little haze reads as natural; removing all of it looks synthetic.
const DEHAZE_OMEGA: f32 = 0.92;
// Transmission floor. Without it the recovery divides by ~0 in the densest
// haze and the sky explodes into noise.
const DEHAZE_MIN_T: f32 = 0.08;

fn transmission(uv: vec2<f32>, haze: vec3<f32>) -> f32 {
    let aspect = frame_aspect();
    // Patch radius as a fraction of the frame, so the estimate covers the same
    // real area in a preview, a zoomed view, and a full-resolution export.
    let radius = frame_to_uv(0.012);

    var dark = 1e9;
    for (var i = 0; i < DEHAZE_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(DEHAZE_SAMPLES)) * radius;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb / haze;
        dark = min(dark, min(s.r, min(s.g, s.b)));
    }
    return 1.0 - DEHAZE_OMEGA * clamp(dark, 0.0, 1.0);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = slot(0u);
    let display_depth = slot(4u) > 0.5;
    let shadow = slot(5u);
    let highlight = slot(6u);

    if strength == 0.0 && !display_depth {
        return c;
    }

    // Guard against a black haze colour: it is a divisor.
    let haze = max(slot3(1u), vec3<f32>(1e-3));

    var t = transmission(uv, haze);
    // Shadow lifts the far end of the depth matte, Highlight scales the near
    // end — Resolve's two controls for guiding the estimate rather than the
    // image. Display Depth exists so you can see what you are adjusting.
    t = t * (1.0 + highlight) + shadow * (1.0 - t);
    t = clamp(t, DEHAZE_MIN_T, 1.0);

    if display_depth {
        return vec3<f32>(t);
    }

    if strength > 0.0 {
        // Invert the scattering model to recover the scene.
        let recovered = (c - haze) / t + haze;
        return mix(c, recovered, clamp(strength, 0.0, 1.0));
    }
    // Negative strength runs the model forwards and *adds* haze, which is what
    // Resolve's bipolar slider does below zero.
    let hazed = mix(c, haze, (1.0 - t) * 0.7);
    return mix(c, hazed, clamp(-strength, 0.0, 1.0));
}
