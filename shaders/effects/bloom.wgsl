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

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p[0].x;
    let radius = u.p[0].y;
    let threshold = u.p[0].z;

    if amount <= 0.0 || radius <= 0.0 {
        return c;
    }
    // The gather lives in common.wgsl, because Film Look Creator has a Bloom
    // section of its own and two implementations of one glow is one too many.
    return c + film_bloom(uv, radius, threshold) * amount;
}
