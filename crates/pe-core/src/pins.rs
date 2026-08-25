//! Pins on the chromaticity diagram — the Colour Warper's third view.
//!
//! The other two views are one object seen along different axes: a grid, with
//! a control point at every intersection whether you wanted one there or not.
//! This is the opposite arrangement. You put a pin *where the colour you care
//! about actually is*, drag it to where you want that colour to go, and say
//! how far around it the pull reaches and which tones it applies to.
//!
//! That difference is the whole reason it is a separate view rather than a
//! third set of axes. A grid asks "what happens to every colour"; a pin asks
//! "what happens to this one", and a picture usually has two or three colours
//! anybody has an opinion about.

use serde::{Deserialize, Serialize};

/// How many pins one warper may carry.
///
/// Bounded because they travel to the GPU inside the curve LUT, and because
/// the honest number is small: past a handful of pins you are describing a
/// field rather than a few opinions, and the grid views already do fields.
pub const MAX_PINS: usize = 8;

/// How many floats one pin occupies in the LUT row. Two spare, so a later
/// control does not move every pin's offset.
pub const PIN_STRIDE: usize = 12;

/// How far the chromaticity plot reaches, in xy.
///
/// The locus reaches 0.7347 in x and **0.8338** in y — that second number is
/// the one that matters, because a span of 0.8 quietly cut the top off the
/// curve, and the part it cut was the greenest colour there is. 0.88 clears it
/// with a little air, which is also how Resolve draws it: the shape sits in the
/// plot rather than running out of it.
pub const PLOT_SPAN: f32 = 0.88;

/// And where it starts. A hair below zero, so the locus has air around it
/// rather than sitting hard against the frame — at exactly zero the curve
/// touches the left edge at 500 nm and reads as though it had been cut off.
pub const PLOT_MIN: f32 = -0.03;

/// A chromaticity as a fraction across the plot.
pub fn plot_fraction(v: f32) -> f32 {
    ((v - PLOT_MIN) / (PLOT_SPAN - PLOT_MIN)).clamp(0.0, 1.0)
}

/// And back. Clamped, so a pin dragged past the frame stops at it rather than
/// acquiring a chromaticity no colour has.
pub fn plot_value(t: f32) -> f32 {
    PLOT_MIN + t.clamp(0.0, 1.0) * (PLOT_SPAN - PLOT_MIN)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    /// Where the pin was placed, as a **CIE xy chromaticity** — not a fraction
    /// of the plot. `[0.33, 0.35]` is a point near the white point, and it is
    /// [`plot_fraction`] that turns one of these into a fraction across the
    /// plot, whose range is [`PLOT_MIN`]..[`PLOT_SPAN`]. Read as a fraction
    /// instead, 0.33 lands somewhere entirely different.
    pub at: [f32; 2],
    /// Where it has been dragged to, in the same CIE xy chromaticities. Equal
    /// to `at` on a pin that has been placed and not yet moved, which is
    /// exactly the pin that should do nothing.
    pub to: [f32; 2],
    /// How far around `at` the pull reaches, as a distance in xy — the same
    /// units as `at` and `to`, so [`plot_fraction`] does not apply to it;
    /// dividing by `PLOT_SPAN - PLOT_MIN` is what turns it into a fraction of
    /// the plot's width.
    pub chroma_range: f32,
    /// How much of the pull the shadows and the highlights take, and where the
    /// boundary between them sits. Both at one is every tone equally, which is
    /// why both default to one.
    pub tonal_low: f32,
    pub tonal_high: f32,
    pub tonal_pivot: f32,
    /// Stops of light, applied within the pin's reach.
    pub exposure: f32,
}

impl Pin {
    /// A pin placed at a point and not yet moved.
    pub fn placed(at: [f32; 2]) -> Self {
        Self {
            at,
            to: at,
            chroma_range: 0.04,
            tonal_low: 1.0,
            tonal_high: 1.0,
            tonal_pivot: 0.5,
            exposure: 0.0,
        }
    }

    /// Whether this pin leaves the picture alone.
    ///
    /// A pin that has been placed but not dragged is *not* a no-op waiting to
    /// happen — it is a pin the user has put somewhere deliberately and is
    /// about to move. It reads as neutral so the row costs nothing until it
    /// does something, which is the same rule every other effect follows.
    pub fn is_neutral(&self) -> bool {
        self.at == self.to && self.exposure == 0.0
    }

    /// The twelve floats the shader reads.
    pub fn pack(&self) -> [f32; PIN_STRIDE] {
        [
            self.at[0],
            self.at[1],
            self.to[0],
            self.to[1],
            self.chroma_range,
            self.tonal_low,
            self.tonal_high,
            self.tonal_pivot,
            self.exposure,
            0.0,
            0.0,
            0.0,
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pins(pub Vec<Pin>);

impl Pins {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Pin> {
        self.0.iter()
    }

    pub fn get(&self, i: usize) -> Option<&Pin> {
        self.0.get(i)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Pin> {
        self.0.get_mut(i)
    }

    /// Add a pin, if there is room. Returns its index.
    pub fn add(&mut self, pin: Pin) -> Option<usize> {
        (self.0.len() < MAX_PINS).then(|| {
            self.0.push(pin);
            self.0.len() - 1
        })
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.0.len() {
            self.0.remove(i);
        }
    }

    /// Whether the whole set leaves the picture alone.
    pub fn is_neutral(&self) -> bool {
        self.0.iter().all(Pin::is_neutral)
    }

    /// The nearest pin to a point, and how far away it is.
    pub fn nearest(&self, to: [f32; 2]) -> Option<(usize, f32)> {
        self.0
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let d = ((p.to[0] - to[0]).powi(2) + (p.to[1] - to[1]).powi(2)).sqrt();
                (i, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placed_pin_does_nothing_until_it_is_moved() {
        let mut p = Pin::placed([0.4, 0.5]);
        assert!(
            p.is_neutral(),
            "placing a pin should not change the picture"
        );
        p.to = [0.45, 0.5];
        assert!(!p.is_neutral());
    }

    /// Exposure counts too. A pin can be dead centre and still be doing
    /// something, and a neutrality check that only looked at the drag would
    /// let the renderer skip a row that is brightening the picture.
    #[test]
    fn a_pin_with_exposure_is_not_neutral() {
        let mut p = Pin::placed([0.4, 0.5]);
        p.exposure = 0.5;
        assert!(!p.is_neutral());
    }

    #[test]
    fn the_set_is_bounded() {
        let mut pins = Pins::default();
        for i in 0..MAX_PINS {
            assert_eq!(pins.add(Pin::placed([0.1 * i as f32, 0.5])), Some(i));
        }
        assert_eq!(pins.add(Pin::placed([0.9, 0.9])), None, "took a ninth pin");
        assert_eq!(pins.len(), MAX_PINS);
    }

    #[test]
    fn the_nearest_pin_is_found_by_where_it_was_dragged_to() {
        let mut pins = Pins::default();
        pins.add(Pin::placed([0.2, 0.2]));
        let mut second = Pin::placed([0.8, 0.8]);
        // Dragged across the plot: it should be found where it *is*, not where
        // it was placed, because that is where it is drawn.
        second.to = [0.25, 0.25];
        pins.add(second);
        let (index, _) = pins.nearest([0.26, 0.26]).unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn removing_a_pin_leaves_the_others() {
        let mut pins = Pins::default();
        pins.add(Pin::placed([0.1, 0.1]));
        pins.add(Pin::placed([0.2, 0.2]));
        pins.remove(0);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins.get(0).unwrap().at, [0.2, 0.2]);
        // And removing past the end is not a panic.
        pins.remove(9);
        assert_eq!(pins.len(), 1);
    }

    /// The plot has to clear the spectral locus, which reaches 0.8338 in y.
    /// A span of 0.8 quietly cut the top off the curve, and the part it cut
    /// was the greenest colour there is.
    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "constant is the point: these two are the values under test"
    )]
    fn the_plot_clears_the_locus() {
        assert!(PLOT_SPAN > 0.8338, "the plot cuts off the green corner");
        assert!(PLOT_MIN < 0.0, "the locus sits hard against the left frame");
    }

    /// A chromaticity as a fraction across the plot, and back.
    #[test]
    fn the_two_plot_mappings_are_inverses() {
        for v in [0.0_f32, 0.15, 0.3333, 0.5, 0.8338] {
            assert!(
                (plot_value(plot_fraction(v)) - v).abs() < 1e-5,
                "{v} came back as {}",
                plot_value(plot_fraction(v))
            );
        }
    }

    /// Outside the plot is clamped rather than extrapolated — a pin dragged
    /// past the frame stops at it rather than acquiring a chromaticity no
    /// colour has.
    #[test]
    fn the_plot_clamps_at_its_edges() {
        assert_eq!(plot_fraction(-99.0), 0.0);
        assert_eq!(plot_fraction(99.0), 1.0);
        assert_eq!(plot_value(-99.0), PLOT_MIN);
        assert_eq!(plot_value(99.0), PLOT_SPAN);
    }

    /// The white point is where a fresh pin goes, and it must land inside the
    /// plot rather than on its edge.
    #[test]
    fn a_fresh_pin_lands_well_inside_the_plot() {
        let p = Pin::placed([0.33, 0.35]);
        for v in p.at {
            let f = plot_fraction(v);
            assert!(f > 0.1 && f < 0.9, "{v} maps to {f}, which is at the frame");
        }
    }
}
