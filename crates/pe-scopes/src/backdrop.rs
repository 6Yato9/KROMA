//! Which measurement belongs behind which control.
//!
//! A curve editor with nothing behind it is a diagram of a function: you can
//! see the shape you drew and not the thing you drew it for. What goes behind
//! it has to be counted in the same units its x-axis is indexed by — a tone
//! histogram behind a Hue Vs Sat curve puts every peak in the wrong place,
//! which is worse than drawing nothing, because it aims the user at colours
//! that are not there.
//!
//! Shared rather than written per shell because it is the same question in
//! both, and because the mode that got it wrong got it wrong in the one shell
//! that had an answer at all.

/// What a control wants drawn behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// The three channel histograms, read through the SDR window.
    Tones,
    /// The luma histogram alone, for a curve indexed by luminance.
    Luma,
    /// Hue counts, running once round the circle from red.
    Hue,
    /// Saturation counts.
    Saturation,
    /// Nothing is known to belong there.
    Nothing,
}

impl Backdrop {
    /// What belongs behind the curve at `key`.
    ///
    /// Decided by what the curve's x-axis is indexed by, which is not always
    /// what its name leads with: `lum_vs_sat` reads an input *luminance* and
    /// outputs a saturation, so it takes a luma backdrop and not a saturation
    /// one.
    pub fn behind(key: &str) -> Self {
        match key {
            "luma" | "red" | "green" | "blue" => Backdrop::Tones,
            "hue_vs_hue" | "hue_vs_sat" | "hue_vs_lum" => Backdrop::Hue,
            "sat_vs_sat" | "sat_vs_lum" => Backdrop::Saturation,
            "lum_vs_sat" => Backdrop::Luma,
            _ => Backdrop::Nothing,
        }
    }
}

/// How far either side of a bin the smoothing reaches.
///
/// A histogram of a photograph is spiky — real images have runs of identical
/// values, and every one of them is a bin standing alone. Drawn raw that reads
/// as a bar chart, which is a picture of the sampling rather than of the
/// photograph. Three bins either side is enough to make it a curve and short
/// enough that a genuine spike is still a spike.
pub const SMOOTH: usize = 3;

/// Smooth and normalise one channel into 0..1 heights.
///
/// Here rather than in a shell for the same reason [`Backdrop::behind`] is:
/// both shells draw this trace, and two hand-written copies of a seven-tap
/// filter agree until the day one of them is tidied. The Swift copy is checked
/// against this one bin for bin, from the fixture `pe-session` writes.
///
/// A bin near either end has fewer neighbours, and the weight it divides by
/// shrinks with the window rather than the window being clamped or wrapped.
/// That matters: clamping would repeat the end bin and pull a peak outward,
/// and wrapping would fold the shadows into the highlights. All three look
/// identical in the middle, which is why the fixture puts a value hard against
/// each end.
pub fn trace(bins: &[u32], peak: f32) -> Vec<f32> {
    let count = bins.len();
    // An empty frame has no scale to draw against, and dividing by its peak
    // would make every bin NaN — which draws as a hole rather than as the
    // nothing it is. Counts are whole numbers, so a peak below one is no peak.
    let full = peak.max(1.0);
    (0..count)
        .map(|i| {
            let mut sum = 0.0;
            let mut weight = 0.0;
            for d in -(SMOOTH as i32)..=(SMOOTH as i32) {
                let j = i as i32 + d;
                if !(0..count as i32).contains(&j) {
                    continue;
                }
                // Triangular, which is a box filter applied twice and quite
                // smooth enough for something drawn a few hundred pixels wide.
                let w = 1.0 - (d.abs() as f32 / (SMOOTH as f32 + 1.0));
                sum += bins[j as usize] as f32 * w;
                weight += w;
            }
            let v = sum / weight.max(1e-4) / full;
            // The same compression the panel histogram used: one flat area of
            // sky can hold a fifth of the frame in a single bin, and against
            // that everything else would be a pixel high.
            v.clamp(0.0, 1.0).powf(0.42)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BINS;

    /// The one mode nobody checked. `LumVsSat` reads "Input Lum" and was given
    /// the saturation spread, so every peak behind it counted a quantity its
    /// x-axis was not measuring.
    #[test]
    fn a_curve_indexed_by_luminance_gets_luminance() {
        assert_eq!(Backdrop::behind("lum_vs_sat"), Backdrop::Luma);
    }

    #[test]
    fn a_tone_curve_gets_tones_and_a_hue_curve_gets_hues() {
        for key in ["luma", "red", "green", "blue"] {
            assert_eq!(Backdrop::behind(key), Backdrop::Tones, "{key}");
        }
        for key in ["hue_vs_hue", "hue_vs_sat", "hue_vs_lum"] {
            assert_eq!(Backdrop::behind(key), Backdrop::Hue, "{key}");
        }
        for key in ["sat_vs_sat", "sat_vs_lum"] {
            assert_eq!(Backdrop::behind(key), Backdrop::Saturation, "{key}");
        }
    }

    /// Anything that is not a curve of this effect has no answer, and saying so
    /// is what lets a caller draw nothing rather than draw the wrong thing.
    ///
    /// That every curve the registry declares *does* have an answer is checked
    /// in `pe-session`'s `fixtures` suite, which can read the registry; this
    /// crate cannot see it.
    #[test]
    fn something_that_is_not_a_curve_has_no_backdrop() {
        assert_eq!(Backdrop::behind("not_a_curve"), Backdrop::Nothing);
    }

    /// The point of smoothing at all: a run of identical values in a
    /// photograph is one bin standing alone, and drawn raw that reads as a bar
    /// chart rather than as the picture it was measured from.
    #[test]
    fn a_lone_spike_becomes_a_bump_without_moving() {
        let mut bins = [0u32; BINS];
        bins[128] = 1000;
        let t = trace(&bins, 1000.0);
        // Still highest where the spike was.
        let peak_at = t
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak_at, 128);
        // And its neighbours have picked something up, which is the point.
        assert!(t[127] > 0.0 && t[129] > 0.0);
        assert_eq!(t[100], 0.0, "smoothing reached further than it should");
    }

    #[test]
    fn a_flat_histogram_traces_flat() {
        let bins = [10u32; BINS];
        let t = trace(&bins, 10.0);
        let first = t[BINS / 2];
        for v in &t[SMOOTH..BINS - SMOOTH] {
            assert!((v - first).abs() < 1e-5, "a flat input rippled");
        }
    }

    #[test]
    fn nothing_traces_to_nothing_rather_than_dividing_by_zero() {
        assert!(trace(&[0u32; BINS], 0.0).iter().all(|v| *v == 0.0));
    }

    /// Moved here with the function it tests, from the Windows shell that used
    /// to own both.
    #[test]
    fn smoothing_spreads_a_spike_into_a_curve() {
        let mut bins = [0u32; BINS];
        bins[100] = 1000;
        let t = trace(&bins, 1000.0);
        assert!(t[100] > 0.0, "the spike vanished");
        for d in 1..=SMOOTH {
            assert!(
                t[100 - d] > 0.0 && t[100 + d] > 0.0,
                "the spike did not reach {d} bins out"
            );
            assert!(
                t[100 - d] < t[100 - d + 1],
                "the shoulder should fall away from the peak"
            );
        }
        assert!(
            t[100 - SMOOTH - 1] == 0.0,
            "the smoothing reached further than it should"
        );
    }

    /// Heights are what a shell multiplies its plot height by, so one above
    /// 1.0 is a trace drawn outside the plot it belongs to.
    #[test]
    fn a_trace_never_leaves_the_plot() {
        let mut bins = [0u32; BINS];
        // Everything in one bin, which is what a flat frame gives.
        bins[10] = u32::MAX / 2;
        let t = trace(&bins, 1.0);
        assert!(t.iter().all(|v| (0.0..=1.0).contains(v)), "{:?}", t[10]);
    }

    /// An end bin has half a window, and what it does with the missing half is
    /// the thing three plausible implementations disagree about.
    #[test]
    fn the_ends_shorten_their_window_rather_than_inventing_neighbours() {
        let mut bins = [0u32; BINS];
        bins[0] = 1000;
        let t = trace(&bins, 1000.0);
        // Clamping would repeat bin zero across the missing taps and make the
        // end read as full height; the real answer is the weight that is
        // actually there, 1.0 out of 2.5.
        let interior = {
            let mut b = [0u32; BINS];
            b[128] = 1000;
            trace(&b, 1000.0)[128]
        };
        assert!(
            t[0] > interior,
            "an end bin lost weight it kept in the middle"
        );
        assert!(t[0] < 1.0, "the end bin was clamped up to full height");
        // And nothing wrapped round to the far end.
        assert_eq!(t[BINS - 1], 0.0, "the smoothing wrapped");
    }
}
