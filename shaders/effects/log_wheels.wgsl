// Log. Slots:
//   0-3 shadow, 4-7 midtone, 8-11 highlight, 12-15 offset,
//   16 low_range, 17 high_range
//
// Resolve's log wheels. The primaries wheels hinge the transfer curve at its
// ends — lift holds white, gain holds black — which makes them interact: pull
// lift up and the midtones follow. The log wheels instead address three tonal
// *bands* whose boundaries you set yourself, so a shadow push genuinely leaves
// the highlights alone.
//
// That is the whole reason both sets exist, and why Low Range and High Range
// are controls rather than constants: the point of the tool is deciding where
// "shadow" stops.
//
// Weighted against the ACEScct anchors, not 0/0.5/1 — an SDR image only spans
// about 0.073 to 0.555 in log.

/// What Offset reads when it is doing nothing, and how much of the range a
/// unit of it is worth. Shared with primaries.wgsl by convention rather than
/// by inclusion, because WGSL has no way to say it once — if one moves, so
/// must the other.
const OFFSET_NEUTRAL: f32 = 25.0;
const OFFSET_SCALE: f32 = 500.0;

/// What one unit on a wheel is worth, in log.
///
/// The panel reads in Resolve's numbers — shadow runs to -8 and highlight to
/// +8 — and an SDR picture spans about 0.48 of the log range in total, so a
/// unit cannot be a unit. At this scale a full push of one is about a third of
/// the range, which is decisive, and the long tails run out to a shadow fully
/// crushed or a highlight fully blown. Which is what a tail is for.
const WHEEL_SCALE: f32 = 0.15;

/// A log wheel's three channels.
///
/// No master *slot* on any of them: these wheels show three readouts, and the
/// bar under each one writes into the three channels together rather than into
/// a fourth component. So the fourth slot is unused, not ignored.
fn wheel_offset(base: u32) -> vec3<f32> {
    return slot3(base) * WHEEL_SCALE;
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let shadow = wheel_offset(0u);
    let midtone = wheel_offset(4u);
    let highlight = wheel_offset(8u);
    // The numbers here are the ones the panel shows. See primaries.wgsl.
    let offset = (slot3(12u) - vec3<f32>(OFFSET_NEUTRAL)) / OFFSET_SCALE;
    let low = slot(16u);
    let high = slot(17u);

    let l = luma(c);

    // Guard the ordering: a user dragging High Range below Low Range should
    // get a narrow midtone band, not an inverted one.
    let lo = min(low, high);
    let hi = max(low, high);

    let shadow_w = 1.0 - smoothstep(CCT_BLACK, lo, l);
    let highlight_w = smoothstep(hi, CCT_WHITE, l);
    let midtone_w = clamp(1.0 - shadow_w - highlight_w, 0.0, 1.0);

    // Offset is unweighted — it moves the whole image, which is exactly why
    // colourists reach for it first.
    return c
        + shadow * shadow_w
        + midtone * midtone_w
        + highlight * highlight_w
        + offset;
}
