// Log. Slots 0: hue (degrees), 1: saturation, 2: luminance.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var hsv = rgb_to_hsv(c);
    hsv.x = fract(hsv.x + u.p[0].x / 360.0);
    hsv.y = clamp(hsv.y * (1.0 + u.p[0].y), 0.0, 8.0);
    var o = hsv_to_rgb(hsv);
    // Luminance is additive, not multiplicative: the signal is already
    // log-encoded, so adding a constant is a uniform exposure shift.
    o = o + vec3<f32>(u.p[0].z * 0.2);
    return o;
}
