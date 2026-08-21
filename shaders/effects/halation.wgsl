// Linear. Slots:
//   0 strength, 1 threshold, 2 normalization, 3 spread,
//   4 saturation, 5 hue (degrees), 6 secondary_strength, 7 secondary_spread
//
// Halation is light passing through the emulsion, scattering off the film
// base and re-exposing from behind. A linear-light phenomenon: done anywhere
// else it reads as fog rather than glow.
//
// Isolation follows Resolve: Threshold is the low clip and Normalization the
// high clip, so the source of the glow is a *band* rather than everything
// above one level. That is what stops a bright sky glowing as hard as a
// specular highlight.
//
// M1 uses single-pass golden-angle disc samples. That is a real approximation
// and it shows at large radii; a separable multi-pass blur is M2 work. Spread
// is a fraction of the frame, never pixels.

fn gather_glow(uv: vec2<f32>, radius: f32, threshold: f32, normalization: f32) -> vec3<f32> {
    let aspect = u.image_size.x / max(u.image_size.y, 1.0);
    let band = max(normalization - threshold, 1e-3);

    var glow = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 24; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / 24.0) * radius;
        let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        // Clip low at Threshold, normalise against the band, so the brightest
        // sources saturate rather than dominating without limit.
        let over = clamp((s - vec3<f32>(threshold)) / band, vec3<f32>(0.0), vec3<f32>(1.0));
        let w = 1.0 / (1.0 + r * 12.0);
        glow = glow + over * w;
        total = total + w;
    }
    return glow / max(total, 1e-4);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = u.p[0].x;
    let threshold = u.p[0].y;
    let normalization = u.p[0].z;
    let spread = u.p[0].w;
    let saturation = u.p[1].x;
    let hue = u.p[1].y;
    let secondary_strength = u.p[1].z;
    let secondary_spread = u.p[1].w;

    if strength <= 0.0 && secondary_strength <= 0.0 {
        return c;
    }

    // Full-saturation tint desaturated toward white by the Saturation control,
    // so 0 gives a colourless bloom and 1 the characteristic red-orange.
    let pure = hsv_to_rgb(vec3<f32>(fract(hue / 360.0), 1.0, 1.0));
    let tint = mix(vec3<f32>(1.0), pure, clamp(saturation, 0.0, 2.0));

    var out = c;
    if strength > 0.0 && spread > 0.0 {
        out = out + gather_glow(uv, spread, threshold, normalization) * tint * strength;
    }
    // The secondary glow is wider and weaker: together with the tight primary
    // it gives a bright core with a long falloff, which a single radius cannot.
    if secondary_strength > 0.0 && secondary_spread > 0.0 {
        out = out
            + gather_glow(uv, secondary_spread, threshold, normalization)
                * tint
                * secondary_strength
                * 0.5;
    }
    return out;
}
