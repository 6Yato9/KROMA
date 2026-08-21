//! The parametric tone curve's four regions.
//!
//! A point curve lets you put a control point anywhere; a parametric curve
//! gives you four sliders and three movable boundaries and refuses to let you
//! make a curve that isn't smooth. That constraint is the feature — it is why
//! people reach for it when they want a shape they can trust rather than one
//! they have to keep checking.
//!
//! The weights live here rather than only in WGSL because the editor has to
//! draw the resulting curve, and drawing an approximation of what the shader
//! does is how a curve widget starts lying to its user.
//!
//! `shaders/effects/curves.wgsl` mirrors [`weights`] exactly. If one changes,
//! the other has to.

/// Where the boundaries sit by default, as fractions of the tonal range.
pub const DEFAULT_SPLITS: [f32; 3] = [0.25, 0.5, 0.75];

/// Smoothstep, matching WGSL's.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much each of the four regions owns of a tone at `t`, in the order
/// shadows, darks, lights, highlights.
///
/// `t` is a position in the tonal range, 0 at black and 1 at white. The three
/// splits are the boundaries the user drags.
///
/// The weights always sum to one. That is what keeps the curve smooth: at
/// every tone the four sliders are dividing a single unit of influence between
/// them rather than each contributing an independent bump that could stack
/// into a kink where two overlap.
pub fn weights(t: f32, splits: [f32; 3]) -> [f32; 4] {
    // A user is allowed to drag one split past another; the result should be a
    // squeezed region, not an inverted one.
    let mut s = splits;
    s.sort_by(f32::total_cmp);
    let (lo, mid, hi) = (
        s[0].clamp(0.0, 1.0),
        s[1].clamp(0.0, 1.0),
        s[2].clamp(0.0, 1.0),
    );

    // Each region's centre of gravity: halfway between the splits that bound
    // it. Interpolating between centres — rather than switching at the splits
    // themselves — is what makes a split feel like it moves a boundary instead
    // of teleporting one.
    let c = [
        lo * 0.5,
        (lo + mid) * 0.5,
        (mid + hi) * 0.5,
        (hi + 1.0) * 0.5,
    ];

    let t = t.clamp(0.0, 1.0);
    let mut w = [0.0f32; 4];
    if t <= c[0] {
        w[0] = 1.0;
    } else if t <= c[1] {
        let k = smoothstep(c[0], c[1], t);
        w[0] = 1.0 - k;
        w[1] = k;
    } else if t <= c[2] {
        let k = smoothstep(c[1], c[2], t);
        w[1] = 1.0 - k;
        w[2] = k;
    } else if t <= c[3] {
        let k = smoothstep(c[2], c[3], t);
        w[2] = 1.0 - k;
        w[3] = k;
    } else {
        w[3] = 1.0;
    }
    w
}

/// Full travel of a region slider, in ACEScct log units. Mirrors
/// `PARAMETRIC_RANGE` in `shaders/effects/curves.wgsl`.
pub const RANGE_IN_LOG: f32 = 0.12;

/// How much log signal an SDR image occupies: CCT_WHITE minus CCT_BLACK.
///
/// Spelled out rather than imported because this crate deliberately has no
/// colour-science dependency; `pe_color::tests::acescct_anchors_match_the_shader`
/// is what keeps these numbers honest.
// The shader's CCT_WHITE and CCT_BLACK, at the precision an f32 can hold.
pub const SDR_SPAN_IN_LOG: f32 = 0.554_794_5 - 0.072_905_53;

/// The shift a set of region amounts produces at tone `t`, in the same units
/// the sliders use (-1 to 1).
pub fn shift(t: f32, amounts: [f32; 4], splits: [f32; 3]) -> f32 {
    let w = weights(t, splits);
    (0..4).map(|i| amounts[i] * w[i]).sum()
}

/// Where the tone at `t` ends up, as a position in the same 0..1 range.
///
/// This is what the curve editor draws. It has to agree with the shader, or
/// the line on screen is a picture of a curve nobody is applying.
pub fn tone_out(t: f32, amounts: [f32; 4], splits: [f32; 3]) -> f32 {
    (t + shift(t, amounts, splits) * RANGE_IN_LOG / SDR_SPAN_IN_LOG).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(t: f32, splits: [f32; 3]) -> f32 {
        weights(t, splits).iter().sum()
    }

    /// The property the whole construction exists for. Anything else and two
    /// sliders at the same value would not produce a flat result.
    #[test]
    fn the_weights_always_sum_to_one() {
        for splits in [
            DEFAULT_SPLITS,
            [0.1, 0.2, 0.9],
            [0.5, 0.5, 0.5],
            [0.0, 0.5, 1.0],
        ] {
            for i in 0..=100 {
                let t = i as f32 / 100.0;
                let sum = total(t, splits);
                assert!(
                    (sum - 1.0).abs() < 1e-4,
                    "weights summed to {sum} at t={t} with splits {splits:?}"
                );
            }
        }
    }

    #[test]
    fn each_region_owns_its_own_end_of_the_range() {
        let w = weights(0.0, DEFAULT_SPLITS);
        assert_eq!(w[0], 1.0, "black should be entirely shadows");
        let w = weights(1.0, DEFAULT_SPLITS);
        assert_eq!(w[3], 1.0, "white should be entirely highlights");
    }

    /// Dragging a split is supposed to move a boundary. If the weight at a
    /// fixed tone did not change, the control would be decorative.
    #[test]
    fn moving_a_split_moves_the_boundary() {
        let low = weights(0.35, [0.2, 0.5, 0.75])[0];
        let high = weights(0.35, [0.6, 0.8, 0.9])[0];
        assert!(
            high > low + 0.2,
            "raising the first split should give shadows more of t=0.35 \
             ({high} vs {low})"
        );
    }

    /// Splits arrive from a document file and from a user dragging one handle
    /// past another, so out-of-order input is normal rather than exceptional.
    #[test]
    fn splits_out_of_order_still_give_valid_weights() {
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let w = weights(t, [0.9, 0.1, 0.5]);
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "sum was {sum} at t={t}");
            assert!(w.iter().all(|v| (0.0..=1.0).contains(v)), "{w:?}");
        }
    }

    #[test]
    fn nothing_set_is_the_identity() {
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            assert!((tone_out(t, [0.0; 4], DEFAULT_SPLITS) - t).abs() < 1e-6);
        }
    }

    #[test]
    fn a_uniform_push_shifts_every_tone_equally() {
        // Four equal amounts and weights that sum to one means the same shift
        // everywhere — the parametric equivalent of an exposure change.
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            let s = shift(t, [0.5; 4], DEFAULT_SPLITS);
            assert!((s - 0.5).abs() < 1e-4, "shift was {s} at t={t}");
        }
    }
}
