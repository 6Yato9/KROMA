// Linear. Slots:
//   0 amount, 1 size, 2 anamorphism, 3 softness,
//   4 border_shape, 5 rotation, 6 center_x, 7 center_y, 8-10 colour
//
// Light falloff across the frame, so it belongs in linear.
//
// Follows Resolve's Vignette, which splits into a Basic set (Size,
// Anamorphism, Softness, Color) and an Advanced set (Border Shape, Rotation,
// Center). Two of Resolve's controls are deliberately absent:
//
//   * Composite Type — that is the row's blend mode, which every effect here
//     already has.
//   * Transparency — folded into Amount. Two controls for "how much vignette"
//     is one too many, and Amount is bipolar so it can also *brighten* the
//     corners, which Transparency cannot.

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p[0].x;
    if amount == 0.0 {
        return c;
    }
    let size = u.p[0].y;
    let anamorphism = u.p[0].z;
    let softness = u.p[0].w;
    let border_shape = u.p[1].x;
    let rotation = u.p[1].y;
    let centre = vec2<f32>(u.p[1].z, u.p[1].w);
    let colour = slot3(8u);

    // Frame coordinates, so the vignette stays anchored to the photograph
    // rather than following the viewport when the view is zoomed or panned.
    var d = frame_uv(uv) - centre;

    // Rotation first, so it turns the shape rather than the frame.
    if rotation != 0.0 {
        let a = radians(rotation);
        let ca = cos(a);
        let sa = sin(a);
        d = vec2<f32>(d.x * ca - d.y * sa, d.x * sa + d.y * ca);
    }

    // Anamorphism stretches the shape horizontally. At 0 the vignette follows
    // the frame; positive values widen it the way an anamorphic lens would.
    d.x = d.x / max(1.0 + anamorphism, 0.05);

    // Border Shape moves between an ellipse and a rectangle by raising the
    // superellipse exponent. p = 2 is an ellipse; large p approaches a box.
    let p = mix(2.0, 14.0, clamp(border_shape, 0.0, 1.0));
    let e = pow(pow(abs(d.x), p) + pow(abs(d.y), p), 1.0 / p) / 0.5;

    // Size is how far the vignette reaches into the frame: 0 hugs the very
    // edge, 1 reaches the centre.
    let inner = clamp(1.0 - size, 0.0, 0.999);
    let outer = inner + max(softness, 1e-3);
    let t = smoothstep(inner, outer, e);

    if amount > 0.0 {
        // Toward the vignette colour. Black is the classic darkening; any
        // other colour tints the border, which is what Resolve's Color is for.
        return mix(c, colour, clamp(t * amount, 0.0, 1.0));
    }
    // Negative amount brightens the corners instead, which is occasionally
    // what a portrait wants.
    return c * (1.0 + t * (-amount));
}
