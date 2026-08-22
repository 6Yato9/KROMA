// Linear. Slots follow the registry's declaration order:
//   0 amount  1 radius  2 display_edges  3 pre_denoise
//   4 edge_threshold  5 edge_strength  6 edge_blur
//
// The same unsharp mask as Sharpen, applied only where the picture has an
// edge. What separates the two is what this one *leaves alone*: sky, skin and
// shadow have no edges in them, so they keep their noise instead of having it
// amplified — which is the whole reason Resolve ships both.

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = slot(0u);
    let radius = frame_to_uv(max(slot(1u), 1e-4));
    let display_edges = slot(2u) > 0.5;
    let pre_denoise = slot(3u);
    let threshold = slot(4u);
    let strength = slot(5u);
    let blur = slot(6u);

    // Softening before detection, not after. An edge detector run on noise
    // finds edges in the noise, and the sharpener then amplifies exactly the
    // thing this effect exists to avoid.
    var detect_radius = radius;
    if pre_denoise > 0.0 {
        detect_radius = radius * (1.0 + pre_denoise * 3.0);
    }
    var mask = edge_strength(uv, detect_radius);
    mask = clamp((mask - threshold) * strength, 0.0, 1.0);

    // Feathered, so the sharpening arrives at an edge rather than switching
    // on at it. Sampling the mask's own neighbourhood would mean a second
    // detector pass; widening the falloff is the same shape for one lerp.
    if blur > 0.0 {
        let wide = clamp(
            (edge_strength(uv, detect_radius * (1.0 + blur * 4.0)) - threshold) * strength,
            0.0,
            1.0,
        );
        mask = mix(mask, min(mask, wide), blur);
        mask = mix(mask, smoothstep(0.0, 1.0, mask), blur);
    }

    if display_edges {
        // The mask on its own. Not a nicety: an edge mask you cannot see is a
        // mask you are tuning by guessing at its effect through the sharpening.
        return vec3<f32>(mask);
    }
    if amount <= 0.0 || mask <= 0.0 {
        return c;
    }

    let detail = c - film_halo_blur(uv, radius, 1.0);
    return c + detail * amount * mask * 0.5;
}
