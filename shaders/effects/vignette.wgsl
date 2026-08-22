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

    // The falloff lives in common.wgsl, shared with Film Look Creator's own
    // Vignette section.
    let t = film_vignette_t(uv, size, softness, anamorphism, border_shape, rotation, centre);

    if amount > 0.0 {
        // Toward the vignette colour. Black is the classic darkening; any
        // other colour tints the border, which is what Resolve's Color is for.
        return mix(c, colour, clamp(t * amount, 0.0, 1.0));
    }
    // Negative amount brightens the corners instead, which is occasionally
    // what a portrait wants.
    return c * (1.0 + t * (-amount));
}
