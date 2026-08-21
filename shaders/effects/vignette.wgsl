// Linear. Slots 0: amount, 1: midpoint, 2: roundness, 3: feather.
//
// Light falloff across the frame, so it belongs in linear. Negative amounts
// brighten the corners, which is occasionally what a portrait wants.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p[0].x;
    if amount == 0.0 {
        return c;
    }
    let midpoint = u.p[0].y;
    let roundness = u.p[0].z;
    let feather = u.p[0].w;

    let aspect = u.image_size.x / max(u.image_size.y, 1.0);
    var d = uv - vec2<f32>(0.5);
    // roundness 0 follows the frame, giving an oval matching the aspect ratio;
    // 1 is circular regardless of crop.
    d.x = d.x * mix(1.0, aspect, roundness);
    let r = length(d) / 0.70710678;

    let half_feather = feather * 0.5;
    let inner = clamp(midpoint - half_feather, 0.0, 1.0);
    let outer = max(midpoint + half_feather, inner + 1e-3);
    let t = smoothstep(inner, outer, r);

    return c * max(1.0 - t * amount, 0.0);
}
