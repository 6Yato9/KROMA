// Log. Slots 0-3 lift, 4-7 gamma, 8-11 gain, 12-15 offset.
// Each wheel is (r, g, b, master); master applies to all three channels.
//
// Four wheels, not three. Offset is the one colourists reach for first, and
// omitting it is the most common way a clone of these controls feels wrong.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let lift = u.p[0].xyz + vec3<f32>(u.p[0].w);
    let gamma = u.p[1].xyz + vec3<f32>(u.p[1].w);
    let gain = u.p[2].xyz + vec3<f32>(u.p[2].w);
    let offset = u.p[3].xyz + vec3<f32>(u.p[3].w);

    var o = c + offset;
    // Lift raises blacks while white stays anchored; gain scales whites while
    // black stays anchored. Between them they hinge the transfer curve at each
    // end, which is what makes the wheels feel independent of one another.
    o = o + lift * (vec3<f32>(1.0) - o);
    o = o * (vec3<f32>(1.0) + gain);
    o = max(o, vec3<f32>(0.0));
    o = pow(o, vec3<f32>(1.0) / max(vec3<f32>(1.0) + gamma, vec3<f32>(0.05)));
    return o;
}
