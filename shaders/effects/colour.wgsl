// Log. Slots: 0 vibrance, 1 saturation.
//
// Saturation is uniform. Vibrance is the interesting one: it scales by how
// unsaturated a colour already is, so muted colours lift while the ones that
// are already vivid barely move. That is what stops a saturation push turning
// skin orange and a sunset into a poster.
//
// Perceptual, so it runs in log — saturation applied to linear light pushes
// bright colours far harder than dark ones for the same slider travel.

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let vibrance = u.p[0].x;
    let saturation = u.p[0].y;

    if vibrance == 0.0 && saturation == 0.0 {
        return c;
    }

    var hsv = rgb_to_hsv(c);
    let s = hsv.y;

    // The falloff is measured on *linear* saturation, not on the log-encoded
    // signal we are about to change. That distinction matters more than it
    // looks: log compresses the gap between a colour's brightest and dimmest
    // channel, so even an sRGB colour with a channel at zero only reaches
    // about 0.47 on the log saturation axis. Weighting by that would hand a
    // fully vivid colour nearly a third of a full push — precisely the thing
    // Vibrance exists to prevent.
    let lin = cct_decode(c);
    let top = max(max(lin.r, lin.g), lin.b);
    let bottom = min(min(lin.r, lin.g), lin.b);
    // A negative channel means the colour is outside the working gamut, which
    // is as saturated as anything gets — clamping lands it at full protection.
    let s_linear = select(0.0, clamp(1.0 - bottom / top, 0.0, 1.0), top > 1e-5);

    // Squared rather than linear so the protection holds up longer as a colour
    // approaches full saturation.
    let headroom = 1.0 - s_linear;
    let vib = vibrance * headroom * headroom;

    hsv.y = clamp(s * (1.0 + saturation + vib), 0.0, 8.0);
    return hsv_to_rgb(hsv);
}
