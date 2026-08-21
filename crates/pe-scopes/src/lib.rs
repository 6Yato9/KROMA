//! Scopes — waveform, parade, vectorscope, histogram.
//!
//! Full implementation is M2, where these become live GPU instruments. What
//! exists now is the **CPU reference**, for the same reason `pe-color` has one:
//! it is the oracle the compute-shader version gets tested against, and it is
//! cheap to write while the shape of the data is still being decided.
//!
//! Scopes are the highest ratio of "feels like Resolve" to implementation cost
//! in the project. A histogram is binning; a waveform is a per-column
//! histogram; a parade is three waveforms. None of it is hard — it is just
//! rarely done properly outside grading tools.

/// Number of bins. 256 matches an 8-bit display and is what every grading tool
/// uses; finer bins do not survive being drawn at panel width.
pub const BINS: usize = 256;

/// A per-channel histogram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    pub red: [u32; BINS],
    pub green: [u32; BINS],
    pub blue: [u32; BINS],
    /// Rec.709-weighted luma, which is what the luma histogram shows.
    pub luma: [u32; BINS],
    pub total: u32,
    /// Pixels with any channel above diffuse white (linear 1.0).
    ///
    /// Counted separately from the bins on purpose. In a scene-linear pipeline
    /// the top histogram bin means "near the top of the *log encoding*", which
    /// is around linear 223 — far above anything a photograph contains. What a
    /// colourist actually wants flagged is detail that will be lost on an SDR
    /// output, and that threshold is diffuse white.
    pub over_white: u32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            red: [0; BINS],
            green: [0; BINS],
            blue: [0; BINS],
            luma: [0; BINS],
            total: 0,
            over_white: 0,
        }
    }
}

impl Histogram {
    /// Bin an image given as linear-light RGB triples.
    ///
    /// Binning is done on the *log-encoded* value, not the linear one. This is
    /// not cosmetic: a linear histogram piles almost every pixel into the
    /// leftmost few bins and is useless for grading. Resolve's scopes are all
    /// display- or log-referred for the same reason.
    pub fn from_linear<I>(pixels: I) -> Self
    where
        I: IntoIterator<Item = [f64; 3]>,
    {
        use pe_color::TransferFn;
        let mut h = Histogram::default();
        for rgb in pixels {
            let enc = TransferFn::AcesCct.encode_rgb(rgb);
            let luma = pe_color::REC709_LUMA[0] * rgb[0]
                + pe_color::REC709_LUMA[1] * rgb[1]
                + pe_color::REC709_LUMA[2] * rgb[2];
            h.red[bin(enc[0])] += 1;
            h.green[bin(enc[1])] += 1;
            h.blue[bin(enc[2])] += 1;
            h.luma[bin(TransferFn::AcesCct.encode(luma))] += 1;
            if rgb.iter().any(|c| *c > 1.0) {
                h.over_white += 1;
            }
            h.total += 1;
        }
        h
    }

    /// The tallest bin across all channels — the scale a scope draws against.
    pub fn peak(&self) -> u32 {
        [&self.red, &self.green, &self.blue, &self.luma]
            .iter()
            .flat_map(|c| c.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// Fraction of pixels with any channel above diffuse white.
    ///
    /// Drives the highlight warning. Not the same as "clipped": these values
    /// are still present and recoverable in the working space, which is the
    /// whole point of a linear pipeline. They are only lost if the output
    /// transform throws them away.
    pub fn over_white_fraction(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.over_white as f64 / self.total as f64
    }
}

fn bin(encoded: f64) -> usize {
    ((encoded.clamp(0.0, 1.0) * (BINS - 1) as f64).round() as usize).min(BINS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_image_produces_an_empty_histogram() {
        let h = Histogram::from_linear(std::iter::empty());
        assert_eq!(h.total, 0);
        assert_eq!(h.peak(), 0);
        assert_eq!(h.over_white_fraction(), 0.0);
    }

    #[test]
    fn every_pixel_lands_in_exactly_one_bin() {
        let pixels: Vec<[f64; 3]> = (0..100).map(|i| [i as f64 / 100.0; 3]).collect();
        let h = Histogram::from_linear(pixels);
        assert_eq!(h.total, 100);
        assert_eq!(h.red.iter().sum::<u32>(), 100);
        assert_eq!(h.luma.iter().sum::<u32>(), 100);
    }

    #[test]
    fn a_flat_grey_image_produces_one_spike() {
        let h = Histogram::from_linear((0..50).map(|_| [0.18, 0.18, 0.18]));
        assert_eq!(h.peak(), 50);
        assert_eq!(h.red.iter().filter(|c| **c > 0).count(), 1);
    }

    /// The reason binning happens in log.
    #[test]
    fn log_binning_spreads_shadows_that_linear_binning_would_pile_up() {
        // Eight values spanning the bottom 3% of linear range — where most of
        // a photograph's shadow detail actually lives.
        let pixels: Vec<[f64; 3]> = (1..=8).map(|i| [i as f64 * 0.004; 3]).collect();
        let h = Histogram::from_linear(pixels);
        let occupied = h.red.iter().filter(|c| **c > 0).count();
        assert!(
            occupied >= 6,
            "log binning collapsed shadow detail into {occupied} bins"
        );
    }

    #[test]
    fn channels_are_binned_independently() {
        let h = Histogram::from_linear([[1.0, 0.0, 0.0]]);
        assert_eq!(h.red[BINS - 1] + h.red[bin_of(1.0)], 1);
        assert_ne!(
            h.red.iter().position(|c| *c > 0),
            h.green.iter().position(|c| *c > 0),
            "red and green landed in the same bin for a pure red pixel"
        );
    }

    fn bin_of(linear: f64) -> usize {
        bin(pe_color::TransferFn::AcesCct.encode(linear))
    }

    #[test]
    fn highlights_above_diffuse_white_are_reported() {
        let mut pixels = vec![[0.18, 0.18, 0.18]; 99];
        pixels.push([100.0, 100.0, 100.0]);
        let h = Histogram::from_linear(pixels);
        assert_eq!(h.over_white, 1);
        assert!((h.over_white_fraction() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn a_normally_exposed_image_reports_no_highlights() {
        let h = Histogram::from_linear((0..100).map(|i| [i as f64 / 100.0; 3]));
        assert_eq!(h.over_white, 0);
    }

    #[test]
    fn a_single_hot_channel_counts_as_a_highlight() {
        // Blown red on an otherwise normal pixel is exactly the case a
        // per-channel check catches and a luma check misses.
        let h = Histogram::from_linear([[4.0, 0.2, 0.2]]);
        assert_eq!(h.over_white, 1);
    }
}
