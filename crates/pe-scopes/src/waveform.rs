//! Waveform, parade and vectorscope.
//!
//! All three read the same display-referred pixels the histogram does, and for
//! the same reason: what a colourist is asking a scope is "what will this look
//! like on the output", so the signal being measured has to be the one going
//! to the output.
//!
//! They are counters, not pictures. Turning a grid of counts into something on
//! screen — the brightness curve, the graticule, the colours — belongs to the
//! panel that draws them, so that the numbers stay testable without a display.

use crate::BINS;

/// Vertical resolution of a waveform. Matches [`crate::BINS`], because both
/// are answering the same question about an 8-bit output and it would be odd
/// for the histogram and the waveform to disagree about where 50% is.
pub const LEVELS: usize = BINS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Red,
    Green,
    Blue,
    Luma,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Red, Channel::Green, Channel::Blue, Channel::Luma];

    fn index(self) -> usize {
        match self {
            Channel::Red => 0,
            Channel::Green => 1,
            Channel::Blue => 2,
            Channel::Luma => 3,
        }
    }
}

/// A per-column histogram: how many pixels of each column sit at each level.
///
/// This is the scope that tells you *where* in the frame something is
/// happening, which is the one thing a histogram can never do. A blown sky and
/// a blown highlight on a face look identical in a histogram; on a waveform one
/// is a plateau at the top of the left third and the other is a spike.
///
/// A parade is this, drawn as three panels instead of one — same counts, and
/// the reason both are offered is the reading, not the data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waveform {
    columns: usize,
    /// How many image rows fed each column. The natural scale for brightness:
    /// a cell holding every pixel of its column is as bright as a cell can be,
    /// and unlike the observed peak it does not change from frame to frame, so
    /// the display does not flicker as the picture is graded.
    rows: usize,
    /// `bins[channel][column * LEVELS + level]`.
    bins: [Vec<u32>; 4],
}

impl Waveform {
    /// Measure 8-bit RGBA display pixels.
    pub fn from_display(pixels: &[u8], width: usize, height: usize) -> Self {
        let columns = width.max(1);
        let mut bins = std::array::from_fn(|_| vec![0u32; columns * LEVELS]);

        for (i, px) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let column = i % columns;
            let base = column * LEVELS;
            for (c, v) in px[..3].iter().enumerate() {
                bins[c][base + *v as usize] += 1;
            }
            // Rec.709 weights: the signal being measured is display-referred,
            // so the display primaries are the right ones here, not AP1.
            let l = 0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32;
            bins[3][base + (l.round() as usize).min(LEVELS - 1)] += 1;
        }

        Self {
            columns,
            rows: height.max(1),
            bins,
        }
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// One channel's grid, `column * LEVELS + level`.
    pub fn channel(&self, channel: Channel) -> &[u32] {
        &self.bins[channel.index()]
    }

    pub fn at(&self, channel: Channel, column: usize, level: usize) -> u32 {
        if column >= self.columns || level >= LEVELS {
            return 0;
        }
        self.bins[channel.index()][column * LEVELS + level]
    }
}

/// Resolution of the vectorscope grid.
///
/// 128 was enough to read a hue to within a couple of degrees, which is the
/// question a vectorscope answers — but it is drawn at around 250 points and
/// upscaling by two put a visible softness on every trace. 256 is one grid cell
/// per point at the size the panel actually is, and a quarter of a megabyte.
pub const VECTOR_SIZE: usize = 256;

/// The six colour bar targets a vectorscope is read against, at 75%.
///
/// 75% rather than 100% because that is what the boxes on every hardware
/// vectorscope mark, and a colourist reading ours should be able to use what
/// they already know.
pub const TARGETS: [(&str, [u8; 3]); 6] = [
    ("R", [191, 0, 0]),
    ("Yl", [191, 191, 0]),
    ("G", [0, 191, 0]),
    ("Cy", [0, 191, 191]),
    ("B", [0, 0, 191]),
    ("Mg", [191, 0, 191]),
];

/// A skin tone, for the line every vectorscope draws.
///
/// One sample is enough, which is the whole reason the line is useful: skin of
/// every shade sits along the same hue axis and differs in how far out and how
/// bright it is, not in which direction it points. Guarded by
/// `skin_tones_of_every_shade_share_a_hue_axis`.
pub const SKIN: [u8; 3] = [198, 134, 102];

/// Where a display colour lands on the vectorscope, in -1..1 with the centre
/// at the origin and Cr pointing up.
///
/// Public because the graticule is drawn by running the colour bar targets
/// through this same function. Two copies of the projection would be two
/// chances for the boxes to end up somewhere the pixels never reach.
pub fn position(rgb: [u8; 3]) -> [f32; 2] {
    let (r, g, b) = (
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    );
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    // Rec.709 chroma, scaled so the primaries land inside the unit circle the
    // way they do on a hardware scope.
    let cb = (b - y) / 1.8556;
    let cr = (r - y) / 1.5748;
    [cb * 2.0, cr * 2.0]
}

/// A two-dimensional histogram of chroma: hue is the angle, saturation the
/// distance from the middle.
///
/// The scope that makes a colour cast obvious. A histogram and a waveform both
/// show three channels drifting apart; only this shows them drifting apart *in
/// a direction*, which is what tells you whether to reach for temperature or
/// tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vectorscope {
    /// `bins[y * VECTOR_SIZE + x]`, with y increasing downwards for drawing.
    bins: Vec<u32>,
    peak: u32,
    total: u32,
}

impl Default for Vectorscope {
    fn default() -> Self {
        Self {
            bins: vec![0; VECTOR_SIZE * VECTOR_SIZE],
            peak: 0,
            total: 0,
        }
    }
}

impl Vectorscope {
    pub fn from_display(pixels: &[u8]) -> Self {
        let mut v = Vectorscope::default();
        for px in pixels.as_chunks::<4>().0 {
            let p = position([px[0], px[1], px[2]]);
            // Clamp rather than drop: a colour outside the plot is still a
            // colour that is there, and losing it would understate saturation
            // exactly when it matters most.
            let x = (((p[0] + 1.0) * 0.5 * VECTOR_SIZE as f32) as usize).min(VECTOR_SIZE - 1);
            let y = (((1.0 - p[1]) * 0.5 * VECTOR_SIZE as f32) as usize).min(VECTOR_SIZE - 1);
            let cell = &mut v.bins[y * VECTOR_SIZE + x];
            *cell += 1;
            v.peak = v.peak.max(*cell);
            v.total += 1;
        }
        v
    }

    pub fn bins(&self) -> &[u32] {
        &self.bins
    }

    /// The busiest cell, which is what the display normalises against.
    ///
    /// Unlike a waveform there is no natural ceiling here — a flat grey frame
    /// puts every pixel in one cell and a rainbow spreads them over thousands —
    /// so the peak is the only scale that works for both.
    pub fn peak(&self) -> u32 {
        self.peak
    }

    pub fn total(&self) -> u32 {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame that is black on the left and white on the right.
    fn split(width: usize, height: usize) -> Vec<u8> {
        let mut px = Vec::new();
        for _ in 0..height {
            for x in 0..width {
                let v = if x < width / 2 { 0u8 } else { 255 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        px
    }

    #[test]
    fn a_waveform_puts_each_column_where_it_belongs() {
        let w = Waveform::from_display(&split(64, 16), 64, 16);
        assert_eq!(w.columns(), 64);
        assert_eq!(w.rows(), 16);

        // Every pixel of a left-hand column is black, so the whole column
        // stacks in the bottom bin and nowhere else.
        assert_eq!(w.at(Channel::Luma, 10, 0), 16);
        assert_eq!(w.at(Channel::Luma, 10, LEVELS - 1), 0);
        // And the mirror on the right.
        assert_eq!(w.at(Channel::Luma, 50, LEVELS - 1), 16);
        assert_eq!(w.at(Channel::Luma, 50, 0), 0);
    }

    /// The property that separates a waveform from a histogram: move the
    /// content across the frame and the scope moves with it.
    #[test]
    fn a_waveform_knows_where_in_the_frame_something_is() {
        let mut px = Vec::new();
        for _ in 0..8 {
            for x in 0..32 {
                let v = if (8..12).contains(&x) { 255u8 } else { 40 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let w = Waveform::from_display(&px, 32, 8);
        for column in 0..32 {
            let bright = w.at(Channel::Luma, column, LEVELS - 1);
            if (8..12).contains(&column) {
                assert_eq!(bright, 8, "column {column} should be the bright band");
            } else {
                assert_eq!(bright, 0, "column {column} should not be");
            }
        }
    }

    #[test]
    fn every_pixel_is_counted_once_per_channel() {
        let w = Waveform::from_display(&split(20, 5), 20, 5);
        for channel in Channel::ALL {
            let total: u32 = w.channel(channel).iter().sum();
            assert_eq!(total, 100, "{channel:?} counted {total} of 100 pixels");
        }
    }

    #[test]
    fn reading_outside_the_grid_is_zero_rather_than_a_panic() {
        let w = Waveform::from_display(&split(8, 2), 8, 2);
        assert_eq!(w.at(Channel::Red, 99, 0), 0);
        assert_eq!(w.at(Channel::Red, 0, LEVELS + 10), 0);
    }

    /// Grey has no hue, so it belongs in the middle. If it did not, every
    /// neutral frame would read as a colour cast.
    #[test]
    fn neutrals_sit_at_the_centre() {
        for v in [0u8, 64, 128, 200, 255] {
            let p = position([v, v, v]);
            assert!(
                p[0].abs() < 1e-5 && p[1].abs() < 1e-5,
                "grey {v} landed at {p:?}"
            );
        }
    }

    /// The six targets have to be spread around the circle in the order a
    /// colourist expects, or the graticule is decoration.
    #[test]
    fn the_colour_bar_targets_go_round_the_circle_in_order() {
        let angles: Vec<f32> = TARGETS
            .iter()
            .map(|(_, rgb)| {
                let p = position(*rgb);
                p[1].atan2(p[0]).to_degrees().rem_euclid(360.0)
            })
            .collect();

        // Red and cyan are opposite, as are green/magenta and blue/yellow.
        for (a, b) in [(0, 3), (2, 5), (4, 1)] {
            let apart = (angles[a] - angles[b]).abs();
            assert!(
                (apart - 180.0).abs() < 1.0,
                "{} and {} are {apart} degrees apart, not 180",
                TARGETS[a].0,
                TARGETS[b].0
            );
        }
        // And every target is well clear of the middle, or the boxes would
        // pile up on top of each other.
        for (name, rgb) in TARGETS {
            let p = position(rgb);
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!(r > 0.3, "{name} is only {r} from the centre");
        }
    }

    /// The reason a single skin sample is enough to draw the line.
    ///
    /// Skin of every shade points the same way out of the middle; what changes
    /// is how far and how bright, not the direction. If that were not true the
    /// skin line would be useless, and drawing it from one sample would be
    /// worse than useless.
    #[test]
    fn skin_tones_of_every_shade_share_a_hue_axis() {
        let angle = |rgb: [u8; 3]| {
            let p = position(rgb);
            p[1].atan2(p[0]).to_degrees()
        };
        let reference = angle(SKIN);
        for tone in [
            [245, 205, 180],
            [231, 180, 144],
            [198, 134, 102],
            [141, 85, 58],
            [92, 56, 38],
            [61, 38, 28],
        ] {
            let d = (angle(tone) - reference).abs();
            assert!(
                d < 12.0,
                "{tone:?} is {d} degrees off the skin line at {reference}"
            );
        }
    }

    #[test]
    fn a_flat_frame_puts_everything_in_one_vectorscope_cell() {
        let px: Vec<u8> = std::iter::repeat_n([200u8, 60, 60, 255], 500)
            .flatten()
            .collect();
        let v = Vectorscope::from_display(&px);
        assert_eq!(v.total(), 500);
        assert_eq!(v.peak(), 500);
        assert_eq!(v.bins().iter().filter(|c| **c > 0).count(), 1);
    }

    #[test]
    fn a_neutral_frame_lands_in_the_middle_of_the_vectorscope() {
        let px: Vec<u8> = std::iter::repeat_n([128u8, 128, 128, 255], 64)
            .flatten()
            .collect();
        let v = Vectorscope::from_display(&px);
        let middle = (VECTOR_SIZE / 2) * VECTOR_SIZE + VECTOR_SIZE / 2;
        assert_eq!(v.bins()[middle], 64);
    }

    /// A colour outside the plot is still a colour that is in the picture.
    /// Dropping it would understate saturation exactly when it matters.
    #[test]
    fn colours_beyond_the_plot_are_kept_rather_than_dropped() {
        let px: Vec<u8> = std::iter::repeat_n([255u8, 0, 0, 255], 30)
            .flatten()
            .collect();
        let v = Vectorscope::from_display(&px);
        assert_eq!(v.total(), 30, "a fully saturated red was lost");
        assert_eq!(v.bins().iter().sum::<u32>(), 30);
    }
}
