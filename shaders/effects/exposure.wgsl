// Linear. Slot 0: ev.
//
// Exposure is multiplying light by a scalar. Run in any other space this is
// merely a brightness slider wearing the name exposure.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    return c * exp2(u.p[0].x);
}
