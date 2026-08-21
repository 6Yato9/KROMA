// Linear. Slots 0: strength, 1: radius, 2: threshold, 3: tint index.
//
// Halation is light passing through the emulsion, scattering off the film
// base and re-exposing from behind. A linear-light phenomenon: done anywhere
// else it reads as fog rather than glow.
//
// M1 uses a single-pass golden-angle disc sample. That is a real
// approximation and it shows at large radii; a separable multi-pass blur is
// M2 work. Radius is a fraction of the frame, never pixels.
fn tint_colour(idx: f32) -> vec3<f32> {
    let i = i32(idx + 0.5);
    if i == 0 { return vec3<f32>(1.00, 0.15, 0.08); }
    if i == 1 { return vec3<f32>(1.00, 0.42, 0.16); }
    if i == 2 { return vec3<f32>(1.00, 0.66, 0.42); }
    return vec3<f32>(1.0, 1.0, 1.0);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = u.p[0].x;
    let radius = u.p[0].y;
    if strength <= 0.0 || radius <= 0.0 {
        return c;
    }
    let threshold = u.p[0].z;
    let aspect = u.image_size.x / max(u.image_size.y, 1.0);

    var glow = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 24; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / 24.0) * radius;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        let over = max(s - vec3<f32>(threshold), vec3<f32>(0.0));
        let w = 1.0 / (1.0 + r * 12.0);
        glow = glow + over * w;
        total = total + w;
    }

    return c + (glow / max(total, 1e-4)) * tint_colour(u.p[0].w) * strength;
}
