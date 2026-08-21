// Linear. Slots: 0 texture, 1 clarity.
//
// Both are local contrast — the difference between a pixel and its
// neighbourhood, added back to itself. What separates them is the size of that
// neighbourhood: Texture works at a fine scale and brings out grain, skin and
// foliage detail, while Clarity works broadly and gives the picture "punch".
//
// Linear, because it is adding light back to a region rather than reshaping
// the perceptual response. Done in log, the halo around a hard edge picks up a
// grey cast instead of reading as extra light.
//
// The classic failure of Clarity is the dark halo around a bright sky. Two
// things keep it in check: the broad term is weighted toward the midtones, so
// it lets go at both ends of the range; and the difference is soft-limited, so
// a hard edge cannot produce an unbounded overshoot.

const PRESENCE_SAMPLES: i32 = 12;
// Radii as a fraction of the frame, so a 1200px preview and a 6000px export
// sharpen the same real detail.
const TEXTURE_RADIUS: f32 = 0.0035;
const CLARITY_RADIUS: f32 = 0.022;

fn local_average(uv: vec2<f32>, radius: f32, aspect: f32) -> vec3<f32> {
    let r_uv = frame_to_uv(radius);
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < PRESENCE_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(PRESENCE_SAMPLES)) * r_uv;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        sum = sum + textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
    }
    return sum / f32(PRESENCE_SAMPLES);
}

// Soft-limit the detail we add back, so a hard edge rolls off rather than
// spiking into a halo.
fn limit(d: vec3<f32>, ceiling: f32) -> vec3<f32> {
    return ceiling * tanh(d / max(ceiling, 1e-4));
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let texture_amount = u.p[0].x;
    let clarity_amount = u.p[0].y;

    if texture_amount == 0.0 && clarity_amount == 0.0 {
        return c;
    }

    let aspect = frame_aspect();
    var out = c;

    if texture_amount != 0.0 {
        let fine = local_average(uv, TEXTURE_RADIUS, aspect);
        out = out + limit(c - fine, 0.25) * texture_amount;
    }

    if clarity_amount != 0.0 {
        let broad = local_average(uv, CLARITY_RADIUS, aspect);
        // Midtone weighting: Clarity should not carve into a bright sky or
        // block up the shadows, which is exactly where its halos show.
        let l = clamp(luma(c), 0.0, 1.0);
        let midtone = 1.0 - abs(l - 0.35) * 2.0;
        let weight = clamp(midtone, 0.15, 1.0);
        out = out + limit(c - broad, 0.35) * clarity_amount * weight;
    }

    return out;
}
