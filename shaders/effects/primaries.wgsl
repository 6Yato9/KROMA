// Log. Slots 0-3 lift, 4-7 gamma, 8-11 gain, 12-15 offset.
// Each wheel is (r, g, b, master).
//
// Four wheels, not three. Offset is the one colourists reach for first, and
// omitting it is the most common way a clone of these controls feels wrong.
//
// The numbers here are the ones the panel shows, which is not the same as the
// ones the arithmetic wants. Resolve's Gain reads 1.00 when it is doing
// nothing and its Offset reads 25.00, so those are what the document stores —
// the value in the box is the thing a colourist checks against a reference,
// and a panel that read 0.00 where Resolve reads 1.00 would be lying about
// what it is. The conversion belongs here, once.

/// What Offset reads when it is doing nothing, and how much of the range a
/// unit of it is worth.
const OFFSET_NEUTRAL: f32 = 25.0;
const OFFSET_SCALE: f32 = 500.0;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    // Lift and gamma are nudges about zero, so the master adds.
    let lift = u.p[0].xyz + vec3<f32>(u.p[0].w);
    let gamma = u.p[1].xyz + vec3<f32>(u.p[1].w);
    // Gain is a multiplier about one, so the master multiplies. Adding them
    // would make a neutral wheel double the picture.
    let gain = u.p[2].xyz * vec3<f32>(u.p[2].w);
    let gain_is_neutral = all(abs(gain - vec3<f32>(1.0)) < vec3<f32>(1e-5));
    // Offset has no master ring; the fourth slot is along for the ride.
    let offset = (u.p[3].xyz - vec3<f32>(OFFSET_NEUTRAL)) / OFFSET_SCALE;

    var o = c + offset;
    // Lift raises blacks while white stays anchored; gain scales whites while
    // black stays anchored. Between them they hinge the transfer curve at each
    // end, which is what makes the wheels feel independent of one another.
    o = o + lift * (vec3<f32>(1.0) - o);
    // Gain multiplies *light*, which is the one operation in this effect
    // that is not about perception: Resolve's Gain of 2.0 is one stop and its
    // 16.0 is four, and those are statements about how much light there was.
    // Done on the log signal instead it would scale the encoding, which looks
    // like a gain for small pushes and like nothing recognisable at the top of
    // the range.
    //
    // The same deliberate exception as Film Look Creator's glow sections, for
    // the same reason, and in the open for the same reason.
    if !gain_is_neutral {
        o = cct_encode(max(cct_decode(o) * gain, vec3<f32>(0.0)));
    }
    o = max(o, vec3<f32>(0.0));
    o = pow(o, vec3<f32>(1.0) / max(vec3<f32>(1.0) + gamma, vec3<f32>(0.05)));
    return o;
}
