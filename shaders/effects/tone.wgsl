// Log. Slots: 0 highlights, 1 shadows, 2 whites, 3 blacks.
//
// Lightroom's four tonal sliders. Perceptual — the user is reshaping how the
// picture reads, not how much light fell on the sensor — so this runs in log.
//
// Highlights and Shadows act on the broad upper and lower halves; Whites and
// Blacks act at the very ends, which is what makes them useful *after* the
// other two rather than duplicating them. The weights are built from the
// ACEScct anchors rather than 0/0.5/1, because in log an SDR image only spans
// about 0.073 to 0.555 and display-referred thresholds would miss entirely.

// How far a slider at full travel moves the signal, in log units. Chosen so
// that +1 is a strong but recoverable move rather than a cliff.
const TONE_RANGE: f32 = 0.16;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let highlights = u.p[0].x;
    let shadows = u.p[0].y;
    let whites = u.p[0].z;
    let blacks = u.p[0].w;

    let l = luma(c);

    // Broad halves, meeting at mid-grey.
    let hi = smoothstep(CCT_GREY, CCT_WHITE, l);
    let sh = 1.0 - smoothstep(CCT_BLACK, CCT_GREY, l);

    // The extremes. Deliberately narrower and pushed further out, so Whites
    // still has somewhere to work after Highlights has been pulled down.
    let upper = mix(CCT_GREY, CCT_WHITE, 0.55);
    let lower = mix(CCT_BLACK, CCT_GREY, 0.45);
    let wh = smoothstep(upper, CCT_WHITE + 0.08, l);
    let bl = 1.0 - smoothstep(CCT_BLACK - 0.04, lower, l);

    let shift = (highlights * hi + shadows * sh + whites * wh + blacks * bl) * TONE_RANGE;

    // Additive in log is multiplicative in linear, so this is a tonally
    // weighted exposure change rather than a lift that would grey the blacks.
    return c + vec3<f32>(shift);
}
