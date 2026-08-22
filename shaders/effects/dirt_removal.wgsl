// Linear. Slots follow the registry's declaration order:
//   0 strength  1 size_threshold  2 show_mask  3 edge_ignore
//
// Resolve's Automatic Dirt Removal, made single-frame — and that is not a
// trim, it is a weaker test, so it is worth saying plainly what changed.
//
// Theirs finds dirt by *motion*: a speck is something present in this frame
// and absent from its neighbours, which is close to proof. Motion Est. Type,
// Neighbor Frames and Motion Thr. are that test, and a photograph has no
// neighbours to run it against.
//
// What a still can test is weaker: a speck is a small spot that disagrees with
// everything around it. That finds sensor dust and scanning dirt well, and it
// will also find a distant bird. Show Repair Mask exists for that reason — it
// is how you check the weaker test did not take something you wanted.
//
// Linear, because the repair is an average of the surroundings, and an average
// of light belongs in light.

const DIRT_RING: i32 = 12;

/// The ring of pixels around a point, and how far the point sits outside it.
///
/// A ring rather than a disc: dirt is *filled from its surroundings*, so the
/// samples that decide whether it is dirt must be the ones that will replace
/// it. Sampling the middle would mean the speck votes on its own removal.
fn ring_at(uv: vec2<f32>, radius: f32) -> vec3<f32> {
    let aspect = frame_aspect();
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < DIRT_RING; i = i + 1) {
        let a = f32(i) * 0.5235988;
        let offset = vec2<f32>(cos(a) * radius / max(aspect, 1e-4), sin(a) * radius);
        sum = sum + textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
    }
    return sum / f32(DIRT_RING);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let strength = slot(0u);
    let show_mask = slot(2u) > 0.5;
    if strength <= 0.0 && !show_mask {
        return c;
    }
    // Size threshold is a fraction of the frame, and a speck is small: the
    // top of the slider is about a fiftieth of the long edge, which is a
    // generous dust mote and a very small bird.
    let radius = frame_to_uv(max(slot(1u), 1e-3) * 0.02);
    let edge_ignore = slot(3u);

    let around = ring_at(uv, radius);
    let here = luma(c);
    let neighbourhood = luma(around);
    let difference = here - neighbourhood;

    // Dirt is a *local extreme*, light or dark, so the sign does not matter.
    // How far outside its surroundings it has to sit before it counts is the
    // spread of those surroundings — a busy area has to disagree more.
    let spread = max(edge_strength(uv, radius * 2.0), 1e-4);
    var mask = clamp(abs(difference) / max(spread * 1.5, 1e-3) - 1.0, 0.0, 1.0);

    // Edges are where a single-frame detector goes wrong: a corner is a small
    // thing that disagrees with its surroundings, which is the definition it
    // is working from.
    if edge_ignore > 0.0 {
        let structure = clamp(edge_strength(uv, radius * 4.0) * 6.0, 0.0, 1.0);
        mask = mask * (1.0 - structure * edge_ignore);
    }

    if show_mask {
        return vec3<f32>(mask);
    }
    // Filled from the ring that decided it was dirt, which is the only honest
    // thing to fill it from — those are the pixels the picture actually has.
    return mix(c, around, mask * strength);
}
