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

#[cfg(test)]
mod tests {
    use super::*;

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
}
