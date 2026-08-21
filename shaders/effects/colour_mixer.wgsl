// Log. Slots: three per band — hue, saturation, luminance — for eight bands
// in the order red, orange, yellow, green, aqua, blue, purple, magenta.
//
// Lightroom's colour mixer. The global HSL effect moves every hue at once,
// which is fine for a wholesale shift but useless for the job people actually
// have: the sky is too cyan, the foliage too yellow, leave everything else
// alone.
//
// Two things decide whether a mixer feels precise or blotchy. The first is how
// the bands overlap: adjacent bands are interpolated so their weights sum to
// exactly one everywhere, which means a hue sitting between orange and yellow
// gets a blend of the two rather than a seam or a double dose. The second is
// what happens to near-neutral pixels — their hue is essentially noise, so
// rotating it would speckle the greys. They are faded out by saturation
// instead.

// Band centres, in hue units of 0..1.
fn band_hue(i: i32) -> f32 {
    switch i {
        case 0: { return 0.0; }            // red
        case 1: { return 30.0 / 360.0; }   // orange
        case 2: { return 60.0 / 360.0; }   // yellow
        case 3: { return 120.0 / 360.0; }  // green
        case 4: { return 180.0 / 360.0; }  // aqua
        case 5: { return 240.0 / 360.0; }  // blue
        case 6: { return 285.0 / 360.0; }  // purple
        default: { return 330.0 / 360.0; } // magenta
    }
}

// Signed distance from `b` to `a` around the hue circle, in -0.5..0.5.
fn hue_delta(a: f32, b: f32) -> f32 {
    return fract(a - b + 0.5) - 0.5;
}

// One band's share of a hue. Adjacent bands sum to one because smoothstep is
// symmetric about its midpoint: 1 - S(1 - t) is S(t).
fn band_weight(h: f32, i: i32) -> f32 {
    let centre = band_hue(i);
    let d = hue_delta(h, centre);
    var gap: f32;
    if d < 0.0 {
        gap = hue_delta(centre, band_hue((i + 7) % 8));
    } else {
        gap = hue_delta(band_hue((i + 1) % 8), centre);
    }
    return 1.0 - smoothstep(0.0, max(gap, 1e-5), abs(d));
}

// Full travel on a band's hue slider, in degrees. Wide enough to move foliage
// from yellow-green to green, narrow enough that the band stays recognisable.
const MIXER_HUE_RANGE: f32 = 30.0 / 360.0;
// Full travel on a band's luminance slider, in log units.
const MIXER_LUMA_RANGE: f32 = 0.2;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var hsv = rgb_to_hsv(c);

    // How much hue there is to speak of, measured on linear saturation for the
    // same reason Vibrance does: log compresses the gap between a colour's
    // brightest and dimmest channel, so a log-space threshold would treat a
    // vivid colour as half neutral.
    let lin = cct_decode(c);
    let top = max(max(lin.r, lin.g), lin.b);
    let bottom = min(min(lin.r, lin.g), lin.b);
    let s_linear = select(0.0, clamp(1.0 - bottom / top, 0.0, 1.0), top > 1e-5);
    let presence = smoothstep(0.0, 0.08, s_linear);
    if presence <= 0.0 {
        return c;
    }

    var hue_shift = 0.0;
    var saturation = 0.0;
    var luminance = 0.0;
    for (var i = 0; i < 8; i = i + 1) {
        let w = band_weight(hsv.x, i);
        if w > 0.0 {
            let base = u32(i) * 3u;
            hue_shift = hue_shift + slot(base) * w;
            saturation = saturation + slot(base + 1u) * w;
            luminance = luminance + slot(base + 2u) * w;
        }
    }

    hsv.x = fract(hsv.x + hue_shift * MIXER_HUE_RANGE * presence);
    hsv.y = clamp(hsv.y * (1.0 + saturation * presence), 0.0, 8.0);
    let out = hsv_to_rgb(hsv);

    // Additive in log is a uniform exposure change on that band, which is what
    // a luminance slider should be — not a lift that would grey it out.
    return out + vec3<f32>(luminance * MIXER_LUMA_RANGE * presence);
}
