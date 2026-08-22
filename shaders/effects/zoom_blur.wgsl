// Linear. Slots:
//   0 strength   1 blur_type   2 symmetry   3 quality
//   4 border     5 centre_x    6 centre_y
//   7 red        8 green       9 blue
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
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
fn zoom_position(i: i32, count: i32, symmetry: f32) -> f32 {
    let t = f32(i) / f32(max(count - 1, 1));
    if i32(round(symmetry)) == 1 {
        return t;
    }
    return t * 2.0 - 1.0;
}

fn zoom_weight(t: f32, blur_type: f32) -> f32 {
    if i32(round(blur_type)) == 1 {
        return 1.0;
    }
    return 1.0 - abs(t) * 0.85;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = slot(0u);
    let blur_type = slot(1u);
    let symmetry = slot(2u);
    let quality = slot(3u);
    let border = slot(4u);
    let centre = vec2<f32>(slot(5u), slot(6u));
    let channels = vec3<f32>(slot(7u), slot(8u), slot(9u));

    if strength <= 0.0 {
        return c;
    }

    // The centre belongs to the photograph, so it stays put when the view is
    // panned or zoomed.
    let here = frame_uv(uv);
    let offset = here - centre;

    let count = zoom_samples(quality);
    let reach = strength * ZOOM_MAX_SCALE;
    var sum = vec3<f32>(0.0);
    var total = 0.0;

    for (var i = 0; i < count; i = i + 1) {
        let t = zoom_position(i, count, symmetry);
        let w = zoom_weight(t, blur_type);
        // The one line that is not Radial Blur: scale the offset instead of
        // turning it. A pixel far from the centre travels further for the same
        // strength, which is exactly what a zoom does and why the streaks fan
        // out from the middle.
        let frame_point = centre + offset * (1.0 + t * reach);

        if zoom_outside(frame_point) && i32(round(border)) == 3 {
            total = total + w;
            continue;
        }
        let sample_uv = uv_from_frame(zoom_border_uv(frame_point, border));
        sum = sum + textureSampleLevel(src_texture, src_sampler, sample_uv, 0.0).rgb * w;
        total = total + w;
    }

    let blurred = sum / max(total, 1e-4);
    return mix(c, blurred, clamp(channels, vec3<f32>(0.0), vec3<f32>(1.0)));
}
