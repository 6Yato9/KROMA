// Linear. Slots follow the registry's declaration order, which is Resolve's
// panel order, so that order is load-bearing and this list moves with it:
//
//   0 strength   1 blur_type   2 center_exclusion
//   3 red        4 green       5 blue
//   6 centre_x   7 centre_y    8 quality   9 move_with_sizing
//
// Radial Blur smears along the arc; this smears along the radius. Everything
// else about the two is the same, which is why the helpers below are named and
// commented the same way — the one line that differs is the one that matters,
// and it is easier to see that when nothing else is dressed up differently.
//
// Linear, because a blur is an average of light.

/// How far a full-strength blur pushes, as a fraction of the distance from the
/// centre. A third is a hard zoom without dissolving the frame.
const ZOOM_MAX_SCALE: f32 = 0.33;

fn zoom_samples(quality: f32) -> i32 {
    let i = i32(round(quality));
    switch i {
        case 0: { return 9; }   // Faster
        case 2: { return 33; }  // Best
        default: { return 17; } // Better
    }
}

fn zoom_border_uv(uv: vec2<f32>, mode: f32) -> vec2<f32> {
    let i = i32(round(mode));
    switch i {
        case 1: {
            let f = fract(uv * 0.5) * 2.0;
            return vec2<f32>(
                select(f.x, 2.0 - f.x, f.x > 1.0),
                select(f.y, 2.0 - f.y, f.y > 1.0),
            );
        }
        case 2: { return fract(uv); }
        default: { return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)); }
    }
}

fn zoom_outside(uv: vec2<f32>) -> bool {
    return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

/// Symmetric streaks both inward and outward, which reads as a lens pulling
/// focus. Asymmetric streaks one way only, which reads as a push.
/// Realistic falls off toward the ends of the sweep, so the streak fades the
/// way a real exposure does. Even weights every sample the same, which is
/// harsher and occasionally what you want.
fn zoom_weight(t: f32, blur_type: f32) -> f32 {
    if i32(round(blur_type)) == 1 {
        return 1.0;
    }
    return 1.0 - abs(t) * 0.85;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = slot(0u);
    let blur_type = slot(1u);
    let exclusion = clamp(slot(2u), 0.0, 1.0);
    let channels = vec3<f32>(slot(3u), slot(4u), slot(5u));
    let centre = vec2<f32>(slot(6u), slot(7u));
    let quality = slot(8u);
    let anchored = slot(9u) > 0.5;

    if strength <= 0.0 {
        return c;
    }

    // Move With Sizing: the centre belongs to the photograph, so it stays put
    // when the picture is cropped, panned or zoomed. Off, it belongs to the
    // *output* instead — the blur stays where it is on screen while the
    // picture moves under it.
    let here = select(uv, frame_uv(uv), anchored);
    let offset = here - centre;

    let count = zoom_samples(quality);
    // Center Exclusion holds a disc around the centre sharp. The classic use
    // of a zoom blur is speed behind a subject that is still readable, and
    // without this the subject sits at the one point where the blur is
    // weakest rather than at a point where it is absent.
    var reach = strength * ZOOM_MAX_SCALE;
    if exclusion > 0.0 {
        let d = length(offset) * 2.0;
        reach = reach * smoothstep(exclusion, exclusion + 0.15, d);
        if reach <= 0.0 {
            return c;
        }
    }
    var sum = vec3<f32>(0.0);
    var total = 0.0;

    for (var i = 0; i < count; i = i + 1) {
        // Spread either side of the pixel: a zoom blur has no Symmetry
        // control in Resolve, and a one-sided zoom reads as a scale change
        // rather than as motion.
        let t = f32(i) / f32(max(count - 1, 1)) * 2.0 - 1.0;
        let w = zoom_weight(t, blur_type);
        // The one line that is not Radial Blur: scale the offset instead of
        // turning it. A pixel far from the centre travels further for the same
        // strength, which is exactly what a zoom does and why the streaks fan
        // out from the middle.
        let frame_point = centre + offset * (1.0 + t * reach);

        // Border Type is greyed out in Resolve's Zoom Blur — permanently,
        // not conditionally — so there is no control here and the edge is
        // simply held, which is what its Replicate would have done anyway.
        let bounded = clamp(frame_point, vec2<f32>(0.0), vec2<f32>(1.0));
        let sample_uv = select(bounded, uv_from_frame(bounded), anchored);
        sum = sum + textureSampleLevel(src_texture, src_sampler, sample_uv, 0.0).rgb * w;
        total = total + w;
    }

    let blurred = sum / max(total, 1e-4);
    // Channel Adjustment mixes each channel between sharp and blurred, so one
    // channel can be smeared while the others stay put — which is how this
    // effect makes a chromatic streak rather than plain motion.
    //
    // The mix is not clamped at one. Resolve's sliders run to two, and past
    // one the mix extrapolates: the channel is pushed further from the
    // original than the blur itself went, which is a stronger streak rather
    // than a dead half of the control.
    return mix(c, blurred, clamp(channels, vec3<f32>(0.0), vec3<f32>(2.0)));
}
