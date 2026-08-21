// Linear. Slots 0: temperature, 1: tint, 2-4: derived rgb gains.
//
// All the colour science is in pe_color::white_balance, which derives the
// gains from the Planckian locus and normalises them to preserve luminance.
// The shader is one multiply. That division of labour is deliberate: the
// gains stay testable on the CPU, and the GPU does no reasoning.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let gains = vec3<f32>(u.p[0].z, u.p[0].w, u.p[1].x);
    return c * gains;
}
