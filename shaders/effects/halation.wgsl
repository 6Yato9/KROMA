// Linear. Slots:
//   0 strength, 1 threshold, 2 normalization, 3 spread,
//   4 saturation, 5 hue (degrees), 6 secondary_strength, 7 secondary_spread,
//   8 fine_tune_spread, 9 relative_red, 10 relative_green, 11 relative_blue
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
// Fine Tune Relative Spread is the interesting control. With it off, the glow
// is one radius and gets its colour from the Hue tint — a colour applied
// *after* the fact. With it on, each channel scatters its own distance, and
// the red-orange fringe emerges from the physics instead: longer wavelengths
// penetrate the emulsion further and scatter wider, so red spreads past green,
// which spreads past blue. The rim goes red without anyone tinting it.
//
// M1 uses single-pass golden-angle disc samples. That is a real approximation
// and it shows at large radii; a separable multi-pass blur is M2 work. Spread
// is a fraction of the frame, never pixels.

const HALATION_SAMPLES: i32 = 24;

// One radius for all three channels. The cheap path, used when Fine Tune is
// off — a third of the texture fetches of the per-channel version.
fn gather_glow(uv: vec2<f32>, radius: f32, threshold: f32, normalization: f32) -> vec3<f32> {
    let aspect = u.image_size.x / max(u.image_size.y, 1.0);
    let band = max(normalization - threshold, 1e-3);

    var glow = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < HALATION_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let r = sqrt((fi + 0.5) / f32(HALATION_SAMPLES)) * radius;
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

// A separate radius per channel. Each channel is read from its own sample
// position, so the three glows genuinely differ in extent rather than being
// one glow that was tinted.
fn gather_glow_rgb(
    uv: vec2<f32>,
    radius: f32,
    threshold: f32,
    normalization: f32,
    relative: vec3<f32>,
) -> vec3<f32> {
    let aspect = u.image_size.x / max(u.image_size.y, 1.0);
    let band = max(normalization - threshold, 1e-3);

    var glow = vec3<f32>(0.0);
    var total = vec3<f32>(0.0);
    for (var i = 0; i < HALATION_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let base = sqrt((fi + 0.5) / f32(HALATION_SAMPLES)) * radius;
        let dir = vec2<f32>(cos(angle) / aspect, sin(angle));

        for (var ch = 0; ch < 3; ch = ch + 1) {
            let r = base * relative[ch];
            let s = textureSampleLevel(src_texture, src_sampler, uv + dir * r, 0.0).rgb;
            let over = clamp((s[ch] - threshold) / band, 0.0, 1.0);
            // Weight against each channel's own radius, so a narrow channel
            // keeps a tight core rather than being diluted by the widest one.
            let w = 1.0 / (1.0 + r * 12.0);
            glow[ch] = glow[ch] + over * w;
            total[ch] = total[ch] + w;
        }
    }
    return glow / max(total, vec3<f32>(1e-4));
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
    let fine_tune = u.p[2].x > 0.5;
    let relative = max(vec3<f32>(u.p[2].y, u.p[2].z, u.p[2].w), vec3<f32>(0.0));

    if strength <= 0.0 && secondary_strength <= 0.0 {
        return c;
    }

    // With per-channel spread doing the colouring, the Hue tint would be a
    // second bite at the same apple, so it stands down to neutral.
    var tint = vec3<f32>(1.0);
    if !fine_tune {
        let pure = hsv_to_rgb(vec3<f32>(fract(hue / 360.0), 1.0, 1.0));
        tint = mix(vec3<f32>(1.0), pure, clamp(saturation, 0.0, 2.0));
    }

    var out = c;
    if strength > 0.0 && spread > 0.0 {
        var glow: vec3<f32>;
        if fine_tune {
            glow = gather_glow_rgb(uv, spread, threshold, normalization, relative);
            // Saturation still applies, pulling the naturally-separated glow
            // toward or away from neutral.
            glow = mix(vec3<f32>(dot(glow, AP1_LUMA)), glow, clamp(saturation, 0.0, 2.0));
        } else {
            glow = gather_glow(uv, spread, threshold, normalization);
        }
        out = out + glow * tint * strength;
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
