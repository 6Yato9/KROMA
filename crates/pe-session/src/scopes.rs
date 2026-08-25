//! Measuring the graded frame.
//!
//! `pe-scopes` does the counting and is tested on its own. What lives here is
//! the *when*: which frame is measured, at what size, and how a shell knows
//! the numbers it is holding still describe the picture on screen.
//!
//! The measurement is dropped by any edit. The alternative — keeping it and
//! letting each shell decide whether it is still true — puts that question in
//! every caller, and the caller that forgets to ask draws a scope of a
//! photograph that is no longer there.

use pe_scopes::{ColourSpread, Histogram, Vectorscope, warper::Distribution, waveform::Waveform};

/// Everything measured from one frame.
///
/// One struct rather than six, because they are all binned in a single pass
/// over the same pixels and are all invalidated at the same moment. Splitting
/// them would mean six copies of the "has this changed" question.
#[derive(Clone, Debug)]
pub struct Scopes {
    pub histogram: Histogram,
    /// The same frame binned in the curve's own domain, for drawing behind the
    /// curve editor.
    pub log_histogram: Histogram,
    /// Where the frame's hues and saturations sit, for the secondary curves. A
    /// tone histogram behind a Hue Vs Sat curve would put every peak in the
    /// wrong place.
    pub colour: ColourSpread,
    pub waveform: Waveform,
    pub vectorscope: Vectorscope,
    /// Where the frame's colours sit on each of the Colour Warper's three
    /// plots. Without it the warper is a diagram of colour in general rather
    /// than a tool aimed at the photograph in front of you.
    pub warper: Distribution,
}

impl Scopes {
    /// Bin one frame of display-referred 8-bit RGBA.
    pub fn measure(pixels: &[u8], width: usize, height: usize) -> Self {
        Self {
            histogram: Histogram::from_display(pixels),
            log_histogram: Histogram::from_display_log(pixels),
            colour: ColourSpread::from_display(pixels),
            waveform: Waveform::from_display(pixels, width, height),
            vectorscope: Vectorscope::from_display(pixels),
            warper: Distribution::from_display(pixels),
        }
    }
}
