// Linear. Slots: 0 amount, 1 radius, 2 threshold.
//
// A neutral glow spilling out of the highlights — light scattering inside the
// lens rather than inside the film. Linear, like every light-simulating
// effect: blur a highlight in a gamma-encoded space and it turns grey instead
// of glowing.
//
// Bloom and Halation are deliberately separate effects rather than one with a
// tint control, matching Resolve. They are different physical phenomena with
// different falloffs, and stacking both is a normal thing to want.

const BLOOM_SAMPLES: i32 = 24;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p[0].x;
    let radius = u.p[0].y;
    let threshold = u.p[0].z;

    if amount <= 0.0 || radius <= 0.0 {
        return c;
    }

    let aspect = frame_aspect();
    // Radius is frame-relative; convert it into this pass's uv.
    let uv_radius = frame_to_uv(radius);
    var glow = vec3<f32>(0.0);
    var total = 0.0;

    for (var i = 0; i < BLOOM_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(BLOOM_SAMPLES)) * uv_radius;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        // Only what is above the threshold spills. Everything below it is not
        // a highlight and should not glow.
        let over = max(s - vec3<f32>(threshold), vec3<f32>(0.0));
        // Weight falls with distance, so the core stays brighter than the tail.
        let w = 1.0 / (1.0 + r * 10.0);
        glow = glow + over * w;
        total = total + w;
    }

    return c + (glow / max(total, 1e-4)) * amount;
}
