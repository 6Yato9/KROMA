// Linear. Slots follow the registry's declaration order:
//   0 split_channels  1 distortion  2 fine_adjustment
//   3 center_x  4 center_y  5 edge_behaviour
//
// Barrel and pincushion, added or taken away. Negative pulls the corners in,
// which is what corrects the barrel a wide lens gives you; positive pushes
// them out.
//
// Linear because it is a resample, and a resample averages light. It is also
// the only reason this needs to be an effect rather than part of the geometry:
// the distortion is not affine, so it cannot fold into the one sampling map
// the crop and straighten share.

/// How much of the radius the control commands.
///
/// A lens's distortion is a small number — a strong barrel is a few per cent —
/// so a slider running to a whole radius would spend its first tenth on
/// everything useful. Fine Adjustment divides this again by ten for the cases
/// where even that is coarse.
const DISTORT_REACH: f32 = 0.35;

fn distort(p: vec2<f32>, k: f32) -> vec2<f32> {
    let r2 = dot(p, p);
    // The standard radial model, first term only. The second term matters for
    // a fisheye and for nothing else a photograph is likely to have been shot
    // on, and it costs a control that would sit at zero.
    return p * (1.0 + k * r2);
}

fn sample_edge(uv: vec2<f32>, behaviour: i32) -> vec3<f32> {
    let outside = uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
    if outside && behaviour == 0 {
        return vec3<f32>(0.0);
    }
    var p = uv;
    if behaviour == 2 {
        // Mirror: fold back at the edge, so the invented pixels at least
        // belong to this photograph.
        let f = fract(p * 0.5) * 2.0;
        p = vec2<f32>(
            select(f.x, 2.0 - f.x, f.x > 1.0),
            select(f.y, 2.0 - f.y, f.y > 1.0),
        );
    } else if behaviour == 3 {
        p = fract(p);
    } else {
        p = clamp(p, vec2<f32>(0.0), vec2<f32>(1.0));
    }
    return textureSampleLevel(src_texture, src_sampler, p, 0.0).rgb;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var k = slot(1u) * DISTORT_REACH;
    if slot(2u) > 0.5 {
        k = k * 0.1;
    }
    if k == 0.0 {
        return c;
    }
    let behaviour = i32(round(slot(5u)));
    let centre = vec2<f32>(slot(3u), slot(4u));
    let aspect = frame_aspect();

    // Square coordinates about the centre, or the distortion comes out
    // elliptical on a frame that is not square — which is every frame.
    let here = frame_uv(uv);
    let p = vec2<f32>((here.x - centre.x) * aspect, here.y - centre.y);

    if slot(0u) > 0.5 {
        // Split Channels: each channel distorted by a slightly different
        // amount, which *is* lateral chromatic aberration — the same optical
        // failure, so the same control undoes it.
        var out = vec3<f32>(0.0);
        let scale = vec3<f32>(1.03, 1.0, 0.97);
        for (var i = 0; i < 3; i = i + 1) {
            let q = distort(p, k * scale[i]);
            let frame_point = vec2<f32>(q.x / max(aspect, 1e-4) + centre.x, q.y + centre.y);
            out[i] = sample_edge(uv_from_frame(frame_point), behaviour)[i];
        }
        return out;
    }

    let q = distort(p, k);
    let frame_point = vec2<f32>(q.x / max(aspect, 1e-4) + centre.x, q.y + centre.y);
    return sample_edge(uv_from_frame(frame_point), behaviour);
}
