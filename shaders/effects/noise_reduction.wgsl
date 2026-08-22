// Linear. Slots:
//   0 mode      1 radius     2 luma_threshold   3 chroma_threshold
//   4 blend
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// Resolve's Noise Reduction has a temporal half and a spatial half. The
// temporal half compares a frame against its neighbours, and there is no next
// frame here, so this is the spatial half — which is the one that works on a
// photograph anyway.
//
// It is an edge-preserving average: each sample is weighted both by how far
// away it is and by how different it is, so a neighbour on the other side of
// an edge counts for almost nothing. That is what separates noise reduction
// from blur. A plain average removes the noise *and* the picture; this removes
// what does not agree with its surroundings and leaves what does.
//
// Luma and chroma get separate thresholds because they carry different noise.
// Chroma noise is coarse, ugly, and almost free to remove — the eye has little
// colour acuity, so it can be smoothed hard before anything is lost. Luma
// noise is fine and sits on top of real detail, so the same treatment would
// take the detail with it. One threshold for both would mean choosing which of
// those two mistakes to make.

const NR_SAMPLES: i32 = 16;

/// Radius as a fraction of the frame, per setting.
///
/// A fraction rather than pixels, for the same reason grain is measured in
/// microns: a 1200px preview and a 6000px export have to smooth the same real
/// detail, or what you approve is not what you get.
fn nr_radius(mode: f32) -> f32 {
    let i = i32(round(mode));
    switch i {
        case 0: { return 0.0016; }  // Faster
        case 2: { return 0.0055; }  // Enhanced
        default: { return 0.0030; } // Better
    }
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let mode = slot(0u);
    let radius_scale = max(slot(1u), 0.0);
    let luma_threshold = slot(2u);
    let chroma_threshold = slot(3u);
    let blend = slot(4u);

    if blend <= 0.0 || (luma_threshold <= 0.0 && chroma_threshold <= 0.0) {
        return c;
    }

    let aspect = frame_aspect();
    let r_uv = frame_to_uv(nr_radius(mode) * radius_scale);
    let here_luma = luma(c);

    // Two separate averages. They share the sampling — reading the texture
    // twice would double the cost of the effect for nothing — but they weight
    // it differently, which is the whole point.
    var luma_sum = 0.0;
    var luma_weight = 0.0;
    var chroma_sum = vec3<f32>(0.0);
    var chroma_weight = 0.0;

    for (var i = 0; i < NR_SAMPLES; i = i + 1) {
        let fi = f32(i);
        // Golden angle, radius by sqrt: even coverage at any sample count.
        let angle = fi * 2.39996323;
        let rad = sqrt((fi + 0.5) / f32(NR_SAMPLES)) * r_uv;
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
    return mix(c, cleaned, clamp(blend, 0.0, 1.0));
}
