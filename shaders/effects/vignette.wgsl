// Linear. Slots follow the registry's declaration order, which is Resolve's
// panel order, so that order is load-bearing and this list moves with it:
//
//   0 operating_mode  1 size  2 anamorphism  3 softness  4-6 color
//   7 border_shape    8 rotation  9 center_x  10 center_y
//
// Light falloff across the frame, so it belongs in linear.
//
// Resolve's Basic set is Size, Anamorphism, Softness and Color; Operating Mode
// reveals the rest. Three of its controls are deliberately absent: Composite
// Type is the row's blend mode, Use Alpha needs an alpha channel we do not
// carry, and Global Blend is the row's own Blend.
//
// There is no Amount. Resolve has none either — a subtle vignette is a lower
// Blend, not a lower amount — and the falloff below runs to the full colour.

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let advanced = i32(round(slot(0u))) == 1;
    let size = slot(1u);
    let anamorphism = slot(2u);
    let softness = slot(3u);
    let colour = slot3(4u);

    if size <= 0.0 {
        return c;
    }

    // Basic mode is not a subset of the panel — it is a subset of the
    // *effect*. Reading the advanced slots anyway would mean a rotation set
    // once and then switched away from went on quietly turning the vignette.
    var border_shape = 0.0;
    var rotation = 0.0;
    var centre = vec2<f32>(0.5, 0.5);
    if advanced {
        border_shape = slot(7u);
        rotation = slot(8u);
        centre = vec2<f32>(slot(9u), slot(10u));
    }

    // The falloff lives in common.wgsl, shared with Film Look Creator's own
    // Vignette section.
    let t = film_vignette_t(uv, size, softness, anamorphism, border_shape, rotation, centre);

    // Toward the vignette colour. Black is the classic darkening; any other
    // colour tints the border, which is what Resolve's Color is for.
    return mix(c, colour, clamp(t, 0.0, 1.0));
}
