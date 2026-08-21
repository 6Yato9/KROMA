// Log. Slots 0: strength, 1: size (microns), 2: shadow_bias, 3: monochrome.
//
// Grain lives in log because film grain is a density fluctuation in the
// negative, not a light phenomenon. Applied in linear it vanishes from the
// shadows, which is exactly backwards from how film behaves.
//
// Size is in microns on a 35mm frame, never pixels. That is what makes a
// 1200px preview and a 6000px export show the same grain rather than
// invisible fizz.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = u.p[0].x;
    if strength <= 0.0 {
        return c;
    }
    let size_um = max(u.p[0].y, 0.01);
    let bias = u.p[0].z;
    let monochrome = u.p[0].w > 0.5;

    // 36mm of film width, expressed as grains across the frame.
    let across = 36000.0 / size_um;
    let aspect = u.image_size.y / max(u.image_size.x, 1.0);
    let p = floor(uv * vec2<f32>(across, across * aspect) + vec2<f32>(u.seed));

    var n: vec3<f32>;
    if monochrome {
        n = vec3<f32>(hash21(p) - 0.5);
    } else {
        n = vec3<f32>(
            hash21(p) - 0.5,
            hash21(p + vec2<f32>(17.0, 3.0)) - 0.5,
            hash21(p + vec2<f32>(5.0, 29.0)) - 0.5,
        );
    }

    // Real grain peaks in the midtones and falls away at both ends.
    // shadow_bias slides where that peak sits, for a pushed-film look.
    let l = clamp(luma(c), 0.0, 1.0);
    let peak = mix(0.30, 0.55, bias);
    let falloff = 1.0 - abs(l - peak) / max(max(peak, 1.0 - peak), 1e-4);
    let amplitude = max(falloff, 0.0);

    return c + n * strength * 0.18 * amplitude;
}
