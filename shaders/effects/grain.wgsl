// Log. Slots:
//   0 strength, 1 size (microns), 2 softness, 3 saturation,
//   4 offset, 5 shadow_gain, 6 midtone_gain, 7 highlight_gain
//
// Grain lives in log because film grain is a density fluctuation in the
// negative, not a light phenomenon. Applied in linear it vanishes from the
// shadows, which is exactly backwards from how film behaves.
//
// Size is in microns on a 35mm frame, never pixels. That is what makes a
// 1200px preview and a 6000px export show the same grain rather than
// invisible fizz.
//
// Parameter set follows Resolve's Film Grain: separate Shadow, Midtone and
// Highlight Gain rather than one slider sliding a peak around, which is how a
// colourist actually wants to put grain in the midtones only.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = u.p[0].x;
    if strength <= 0.0 {
        return c;
    }
    let size_um = max(u.p[0].y, 0.01);
    let softness = u.p[0].z;
    let saturation = u.p[0].w;
    let offset = u.p[1].x;
    let shadow_gain = u.p[1].y;
    let midtone_gain = u.p[1].z;
    let highlight_gain = u.p[1].w;

    // 36mm of film width, expressed as grains across the frame.
    let across = 36000.0 / size_um;
    let aspect = u.image_size.y / max(u.image_size.x, 1.0);
    let scaled = uv * vec2<f32>(across, across * aspect) + vec2<f32>(u.seed);
    let cell = floor(scaled);

    var mono = hash21(cell) - 0.5;
    var n = vec3<f32>(
        mono,
        hash21(cell + vec2<f32>(17.0, 3.0)) - 0.5,
        hash21(cell + vec2<f32>(5.0, 29.0)) - 0.5,
    );

    // Softness blurs the grain layer by mixing each cell toward the average of
    // its neighbours — cheaper than a real blur and enough at grain scale.
    if softness > 0.0 {
        var neighbours = 0.0;
        neighbours = neighbours + hash21(cell + vec2<f32>(1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(-1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, 1.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, -1.0));
        let smoothed = neighbours * 0.25 - 0.5;
        n = mix(n, vec3<f32>(smoothed), clamp(softness, 0.0, 1.0));
        mono = mix(mono, smoothed, clamp(softness, 0.0, 1.0));
    }

    // Saturation 0 is monochrome grain, matching Resolve.
    n = mix(vec3<f32>(mono), n, clamp(saturation, 0.0, 2.0));

    // Offset lightens or darkens the whole grain layer, so lower values
    // emphasise the light grains and higher values the dark ones.
    n = n + vec3<f32>(offset * 0.25);

    // Three independent tonal gains rather than one peak position.
    //
    // Weighted against the ACEScct anchors, not 0/0.5/1. The signal here is
    // log-encoded: an SDR image spans about 0.073 to 0.555, so a highlight
    // threshold at 0.6 would never fire.
    let l = luma(c);
    let shadow_w = clamp((CCT_GREY - l) / max(CCT_GREY - CCT_BLACK, 1e-4), 0.0, 1.0);
    let highlight_w = clamp((l - CCT_GREY) / max(CCT_WHITE - CCT_GREY, 1e-4), 0.0, 1.0);
    let midtone_w = clamp(1.0 - shadow_w - highlight_w, 0.0, 1.0);
    let gain = shadow_w * shadow_gain + midtone_w * midtone_gain + highlight_w * highlight_gain;

    return c + n * strength * 0.18 * gain;
}
