// Linear. Slots:
//   0 strength   1 blur_type   2 symmetry   3 quality
//   4 border     5 centre_x    6 centre_y
//   7 red        8 green       9 blue
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// A rotational blur: every pixel is smeared along the arc it would sweep if
// the picture spun about a point. Zoom Blur is the same idea along the radius
// instead, and the two share everything but that one line — which is why the
// sampling, the weighting, the border handling and the channel gains all read
// the same in both files.
//
// Linear, because a blur is an average of light. Averaging in a log or gamma
// encoding darkens the result, which is the same mistake as downscaling in the
// wrong space and just as invisible until you compare.

/// How far a full-strength blur sweeps, in radians. Twenty degrees is a strong
/// spin without turning the frame into a smear of nothing.
const RADIAL_MAX_ANGLE: f32 = 0.35;

/// Samples per quality setting. The cost of this effect is entirely here.
fn blur_samples(quality: f32) -> i32 {
    let i = i32(round(quality));
    switch i {
        case 0: { return 9; }   // Faster
        case 2: { return 33; }  // Best
        default: { return 17; } // Better
    }
}

/// What to read where a sample lands outside the picture.
///
/// The sampler clamps on its own, which is Replicate. The others have to be
/// done here, and they matter more than they look: a rotational blur near a
/// corner reaches past the edge on every sample, so whatever this returns is
/// most of what the corner ends up looking like.
fn border_uv(uv: vec2<f32>, mode: f32) -> vec2<f32> {
    let i = i32(round(mode));
    switch i {
        // Mirror: fold back at the edge, so the invented pixels at least
        // belong to this photograph.
        case 1: {
            let f = fract(uv * 0.5) * 2.0;
            return vec2<f32>(
                select(f.x, 2.0 - f.x, f.x > 1.0),
                select(f.y, 2.0 - f.y, f.y > 1.0),
            );
        }
        // Wrap.
        case 2: { return fract(uv); }
        // Replicate, and Black, which is handled by the caller.
        default: { return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)); }
    }
}

fn outside(uv: vec2<f32>) -> bool {
    return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

/// Where one sample sits along the sweep, and what it is worth.
///
/// Symmetric spreads the samples either side of the pixel, which reads as
/// motion with no direction. Asymmetric puts them all on one side, which reads
/// as a trail — the difference between a spinning object and one that has just
/// stopped.
fn sweep_position(i: i32, count: i32, symmetry: f32) -> f32 {
    let t = f32(i) / f32(max(count - 1, 1));
    if i32(round(symmetry)) == 1 {
        return t;
    }
    return t * 2.0 - 1.0;
}

/// Realistic falls off toward the ends of the sweep, so the streak fades the
/// way a real exposure does. Even weights every sample the same, which is
/// harsher and occasionally what you want.
fn sweep_weight(t: f32, blur_type: f32) -> f32 {
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

    // The centre belongs to the photograph, not to the viewport, so it stays
    // put when the view is panned or zoomed.
    let aspect = frame_aspect();
    let here = frame_uv(uv);
    // Rotate in square coordinates or a spin on a wide frame comes out as a
    // shear.
    let offset = vec2<f32>((here.x - centre.x) * aspect, here.y - centre.y);

    let count = blur_samples(quality);
    let angle = strength * RADIAL_MAX_ANGLE;
    var sum = vec3<f32>(0.0);
    var total = 0.0;

    for (var i = 0; i < count; i = i + 1) {
        let t = sweep_position(i, count, symmetry);
        let w = sweep_weight(t, blur_type);
        let a = t * angle;
        let s = sin(a);
        let co = cos(a);
        let turned = vec2<f32>(
            offset.x * co - offset.y * s,
            offset.x * s + offset.y * co,
        );
        let frame_point = vec2<f32>(turned.x / max(aspect, 1e-4) + centre.x, turned.y + centre.y);

        if outside(frame_point) && i32(round(border)) == 3 {
            // Black: the sample contributes nothing but still costs its
            // weight, which is what makes an edge darken rather than smear.
            total = total + w;
            continue;
        }
        let sample_uv = uv_from_frame(border_uv(frame_point, border));
        sum = sum + textureSampleLevel(src_texture, src_sampler, sample_uv, 0.0).rgb * w;
        total = total + w;
    }

    let blurred = sum / max(total, 1e-4);
    // Channel Adjustment mixes each channel between sharp and blurred, so one
    // channel can be smeared while the others stay put — which is how this
    // effect is used for chromatic streaks rather than plain motion.
    return mix(c, blurred, clamp(channels, vec3<f32>(0.0), vec3<f32>(1.0)));
}
