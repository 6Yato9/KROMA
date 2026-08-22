// Linear. Slots follow the registry's declaration order, which is Resolve's
// panel order, so that order is load-bearing and this list moves with it:
//
//   0 mode  1 radius  2 split_luma_chroma  3 threshold
//   4 luma_threshold  5 chroma_threshold   6 blend
//
// Resolve's Noise Reduction has a temporal half and a spatial half. The
// temporal half compares a frame against its neighbours, and a photograph has
// no neighbours, so this is the spatial half — which is the one that works on
// a still anyway.
//
// It is an edge-preserving average: each sample is weighted both by how far
// away it is and by how different it is, so a neighbour on the other side of
// an edge counts for almost nothing. That is what separates noise reduction
// from blur. A plain average removes the noise *and* the picture; this removes
// what does not agree with its surroundings and leaves what does.
//
// Luma and chroma get separate thresholds when Split Luma Chroma is on,
// because they carry different noise. Chroma noise is coarse, ugly, and almost
// free to remove — the eye has little colour acuity, so it can be smoothed
// hard before anything is lost. Luma noise is fine and sits on top of real
// detail, so the same treatment would take the detail with it. One threshold
// for both means choosing which of those two mistakes to make.

/// Radius as a fraction of the frame, per setting.
///
/// A fraction rather than pixels, for the same reason grain is measured in
/// microns: a 1200px preview and a 6000px export have to smooth the same real
/// detail, or what you approve is not what you get.
fn nr_radius(radius: f32) -> f32 {
    let i = i32(round(radius));
    switch i {
        case 1: { return 0.0030; }  // Medium
        case 2: { return 0.0055; }  // Large
        default: { return 0.0016; } // Small
    }
}

/// Samples per Mode. The cost of this effect is entirely here.
fn nr_samples(mode: f32) -> i32 {
    let i = i32(round(mode));
    switch i {
        case 1: { return 16; }  // Better
        case 2: { return 28; }  // Enhanced
        default: { return 8; }  // Faster
    }
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let mode = slot(0u);
    let radius = slot(1u);
    let split = slot(2u) > 0.5;
    // The controls read 0 to 100, as Resolve's do; the maths wants 0 to 1.
    let single = slot(3u) * 0.01;
    let luma_threshold = select(single, slot(4u) * 0.01, split);
    let chroma_threshold = select(single, slot(5u) * 0.01, split);
    // How much of the original comes back over the cleaned picture. Zero is
    // the full effect, which is why zero is Resolve's default.
    let restore = clamp(slot(6u) * 0.01, 0.0, 1.0);

    if restore >= 1.0 || (luma_threshold <= 0.0 && chroma_threshold <= 0.0) {
        return c;
    }

    let aspect = frame_aspect();
    let r_uv = frame_to_uv(nr_radius(radius));
    let count = nr_samples(mode);
    let here_luma = luma(c);

    // Two separate averages. They share the sampling — reading the texture
    // twice would double the cost of the effect for nothing — but they weight
    // it differently, which is the whole point.
    var luma_sum = 0.0;
    var luma_weight = 0.0;
    var chroma_sum = vec3<f32>(0.0);
    var chroma_weight = 0.0;

    for (var i = 0; i < count; i = i + 1) {
        let fi = f32(i);
        // Golden angle, radius by sqrt: even coverage at any sample count.
        let angle = fi * 2.39996323;
        let rad = sqrt((fi + 0.5) / f32(count)) * r_uv;
        let offset = vec2<f32>(cos(angle) * rad / max(aspect, 1e-4), sin(angle) * rad);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;

        let s_luma = luma(s);
        // The range weight. A neighbour that disagrees by more than the
        // threshold is on the other side of an edge and counts for almost
        // nothing; one that agrees counts fully.
        if luma_threshold > 0.0 {
            let d = abs(s_luma - here_luma) / max(luma_threshold * 0.5, 1e-4);
            let w = exp(-d * d);
            luma_sum = luma_sum + s_luma * w;
            luma_weight = luma_weight + w;
        }
        if chroma_threshold > 0.0 {
            // Chroma is measured with the luminance taken out, so an edge in
            // brightness does not stop colour noise being smoothed across it.
            let s_chroma = s - vec3<f32>(s_luma);
            let here_chroma = c - vec3<f32>(here_luma);
            let d = length(s_chroma - here_chroma) / max(chroma_threshold * 0.5, 1e-4);
            let w = exp(-d * d);
            chroma_sum = chroma_sum + s_chroma * w;
            chroma_weight = chroma_weight + w;
        }
    }

    var out_luma = here_luma;
    if luma_weight > 0.0 {
        out_luma = luma_sum / luma_weight;
    }
    var out_chroma = c - vec3<f32>(here_luma);
    if chroma_weight > 0.0 {
        out_chroma = chroma_sum / chroma_weight;
    }

    let cleaned = vec3<f32>(out_luma) + out_chroma;
    return mix(cleaned, c, restore);
}
