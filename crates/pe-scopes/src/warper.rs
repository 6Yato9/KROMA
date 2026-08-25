//! Where a picture's colours actually sit, on each of the Colour Warper's
//! three plots.
//!
//! This is what turns the warper from a diagram into a tool. A grid you can
//! drag, over a plot of the whole colour space, tells you nothing about the
//! photograph in front of you — you would be aiming at where greens are *in
//! general*. Resolve draws the frame's own colours over every one of its three
//! plots for exactly this reason, and without it you are placing pins blind.
//!
//! Three grids rather than one, because the three views are three different
//! projections and a cloud measured for one is meaningless on another. They
//! are counted in a single pass over the pixels, which is the only part of
//! this worth optimising.

use crate::srgb_decode;
use pe_core::pins::plot_fraction;

/// Resolution of each grid.
///
/// These are drawn as a translucent haze behind a control, not read for
/// values, so they want enough cells to show the shape of a distribution and
/// no more. 128 squared is 64 KB a grid.
pub const GRID: usize = 128;

/// The frame's colours, projected three ways.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distribution {
    /// CIE xy, binned over the chromaticity plot's own range —
    /// [`pe_core::pins::PLOT_MIN`]..[`pe_core::pins::PLOT_SPAN`] on each
    /// axis, as [`pe_core::pins::plot_fraction`] measures it. The Chroma
    /// Warp plot.
    pub chromaticity: Vec<u32>,
    /// Hue and saturation as a square over −1..1, which is the polar plot the
    /// Hue - Saturation view draws laid out in cartesian storage: a colour at
    /// hue *h* and saturation *s* lands at (s·cos h, s·sin h).
    pub hue_sat: Vec<u32>,
    /// Saturation across, luma up, both 0..1. The Chroma - Luma view.
    pub chroma_luma: Vec<u32>,
    /// The busiest cell in each, for scaling the haze. Kept per grid because
    /// a chromaticity cloud and a luma spread concentrate very differently.
    pub peaks: [u32; 3],
}

/// The published sRGB D65 primaries, as a matrix to XYZ.
///
/// The pixels being measured are display-referred — they are what is going to
/// the screen — so the display's own primaries are the right ones here, the
/// same argument the waveform makes for using Rec.709 luma weights.
const SRGB_TO_XYZ: [[f64; 3]; 3] = [
    [0.412_390_8, 0.357_584_3, 0.180_480_8],
    [0.212_639_0, 0.715_168_7, 0.072_192_3],
    [0.019_330_8, 0.119_194_8, 0.950_532_2],
];

impl Distribution {
    /// Measure 8-bit RGBA display pixels.
    pub fn from_display(pixels: &[u8]) -> Self {
        let table = srgb_decode();
        let mut chromaticity = vec![0u32; GRID * GRID];
        let mut hue_sat = vec![0u32; GRID * GRID];
        let mut chroma_luma = vec![0u32; GRID * GRID];

        for px in pixels.as_chunks::<4>().0 {
            let lin = [
                table[px[0] as usize],
                table[px[1] as usize],
                table[px[2] as usize],
            ];

            // ---- chromaticity ----
            let xyz: Vec<f64> = SRGB_TO_XYZ
                .iter()
                .map(|row| row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2])
                .collect();
            let sum = xyz[0] + xyz[1] + xyz[2];
            if sum > 1e-6 {
                let x = xyz[0] / sum;
                let y = xyz[1] / sum;
                // Binned in the plot's own coordinates, because the drawing
                // reads this grid at plot fractions: two ranges read as one
                // put every colour in the wrong cell, and they agreed only at
                // x = 0.218.
                bump(
                    &mut chromaticity,
                    plot_fraction(x as f32) as f64,
                    plot_fraction(y as f32) as f64,
                );
            }

            // ---- hue and saturation ----
            // From the 8-bit values rather than the linear ones: hue and
            // saturation are perceptual, and the plot they land on is drawn in
            // those terms too.
            let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let chroma = max - min;
            if max > 0.0 {
                let sat = chroma / max;
                let hue = if chroma <= 0.0 {
                    0.0
                } else if max == r {
                    ((g - b) / chroma).rem_euclid(6.0)
                } else if max == g {
                    (b - r) / chroma + 2.0
                } else {
                    (r - g) / chroma + 4.0
                } * std::f32::consts::FRAC_PI_3;
                // Polar to the square the grid stores, −1..1 mapped to 0..1.
                let (sx, sy) = (sat * hue.cos(), sat * hue.sin());
                bump(
                    &mut hue_sat,
                    (sx as f64 + 1.0) * 0.5,
                    (sy as f64 + 1.0) * 0.5,
                );

                // ---- saturation against luma ----
                let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                bump(&mut chroma_luma, sat as f64, (l / 255.0) as f64);
            }
        }

        let peak = |g: &[u32]| g.iter().copied().max().unwrap_or(0);
        Self {
            peaks: [peak(&chromaticity), peak(&hue_sat), peak(&chroma_luma)],
            chromaticity,
            hue_sat,
            chroma_luma,
        }
    }
}

/// Count one sample into a grid, given its position in 0..1 on each axis.
///
/// Silently drops anything outside, which is the right answer: a chromaticity
/// past the edge of the plot is a colour the plot does not claim to show, and
/// clamping it would pile every out-of-range sample onto the border and invent
/// a bright line that is not there.
///
/// The range is closed at *both* ends, which matters more than it looks. A
/// fully saturated red lands at exactly 1.0 on the hue/saturation plot, so a
/// half-open range drops the one colour most likely to be the reason somebody
/// opened the tool.
fn bump(grid: &mut [u32], u: f64, v: f64) {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return;
    }
    let col = ((u * GRID as f64) as usize).min(GRID - 1);
    // Stored with v increasing upwards, which is how all three plots draw it.
    let row = (GRID - 1) - ((v * GRID as f64) as usize).min(GRID - 1);
    grid[row * GRID + col] += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(rgb: [u8; 3], count: usize) -> Vec<u8> {
        std::iter::repeat_n([rgb[0], rgb[1], rgb[2], 255], count)
            .flatten()
            .collect()
    }

    /// The centre of mass of a grid, in cells.
    ///
    /// Used instead of naming a cell, because "the middle" of an even grid is
    /// a boundary between two cells rather than a cell — and an assertion that
    /// picks one of them is an assertion about rounding.
    fn centroid(grid: &[u32]) -> (f64, f64) {
        let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
        for row in 0..GRID {
            for col in 0..GRID {
                let n = grid[row * GRID + col] as f64;
                sx += col as f64 * n;
                sy += row as f64 * n;
                total += n;
            }
        }
        (sx / total.max(1.0), sy / total.max(1.0))
    }

    /// A grey frame has no chroma at all: it should pile up in the middle of
    /// the hue/saturation plot and at the left of the chroma/luma one.
    #[test]
    fn grey_lands_where_there_is_no_colour() {
        let d = Distribution::from_display(&pixels([128, 128, 128], 100));
        assert_eq!(d.hue_sat.iter().sum::<u32>(), 100, "grey was not counted");
        let (x, y) = centroid(&d.hue_sat);
        let middle = GRID as f64 / 2.0;
        assert!(
            (x - middle).abs() <= 1.0 && (y - middle).abs() <= 1.0,
            "grey is not at the centre: ({x}, {y})"
        );
        // Saturation zero is the left-hand column.
        let column: u32 = (0..GRID).map(|r| d.chroma_luma[r * GRID]).sum();
        assert_eq!(column, 100);
    }

    /// And a saturated colour lands away from it, on the side its hue points.
    #[test]
    fn red_lands_to_the_right_of_the_centre() {
        let d = Distribution::from_display(&pixels([255, 0, 0], 50));
        assert_eq!(
            d.hue_sat.iter().sum::<u32>(),
            50,
            "a fully saturated red was dropped — it lands on the plot's very              edge, which is exactly where a half-open range loses it"
        );
        let (x, _) = centroid(&d.hue_sat);
        assert!(
            x > GRID as f64 * 0.75,
            "red should sit at the right edge, landed at {x}"
        );
    }

    /// Black has no chromaticity — there is no direction to a colour with no
    /// light in it — and counting it would put a spike at the plot's origin
    /// that means nothing.
    #[test]
    fn black_is_not_counted_as_a_chromaticity() {
        let d = Distribution::from_display(&pixels([0, 0, 0], 40));
        assert_eq!(d.chromaticity.iter().sum::<u32>(), 0);
        assert_eq!(d.peaks[0], 0);
    }

    #[test]
    fn a_white_frame_sits_near_the_white_point() {
        let d = Distribution::from_display(&pixels([255, 255, 255], 10));
        let (mut sx, mut sy, mut total) = (0.0f64, 0.0f64, 0.0f64);
        for row in 0..GRID {
            for col in 0..GRID {
                let n = d.chromaticity[row * GRID + col] as f64;
                sx += col as f64 * n;
                sy += ((GRID - 1) - row) as f64 * n;
                total += n;
            }
        }
        assert_eq!(total, 10.0);
        // D65 is x = 0.3127, y = 0.3290, a little under four tenths of the way
        // along each axis of a plot running PLOT_MIN..PLOT_SPAN — which is the
        // range the grid is binned over, so a cell reads back through
        // `plot_value`.
        let x = pe_core::pins::plot_value((sx / total / GRID as f64) as f32) as f64;
        let y = pe_core::pins::plot_value((sy / total / GRID as f64) as f32) as f64;
        assert!((x - 0.3127).abs() < 0.02, "x came out {x}");
        assert!((y - 0.3290).abs() < 0.02, "y came out {y}");
    }

    /// The chromaticity grid is drawn on a plot spanning `PLOT_MIN` to
    /// `PLOT_SPAN`, and the drawing reads it at plot fractions. Binning it over
    /// a different range put every colour in the wrong cell — they agreed only
    /// at x = 0.218, and at a saturated red the cloud sat about six per cent of
    /// the plot away from the pin a colourist would put on it.
    ///
    /// A cloud that disagrees with the coordinates drawn over it is worse than
    /// no cloud: it is confidently wrong, and it exists precisely so you are
    /// aiming at this photograph's colours rather than colours in general.
    #[test]
    fn the_chromaticity_grid_is_binned_over_the_plot_it_is_drawn_on() {
        use pe_core::pins::{PLOT_MIN, PLOT_SPAN, plot_fraction};

        // A pixel of a known chromaticity, and where it lands.
        // sRGB pure red has xy = (0.64, 0.33).
        let pixels = [255u8, 0, 0, 255];
        let d = Distribution::from_display(&pixels);

        let cell = |grid: &[u32]| -> Option<(usize, usize)> {
            grid.iter()
                .position(|c| *c > 0)
                .map(|i| (i % GRID, i / GRID))
        };
        let (col, row) = cell(&d.chromaticity).expect("red was not counted at all");

        let want_col = (plot_fraction(0.64) * GRID as f32) as usize;
        assert!(
            col.abs_diff(want_col) <= 1,
            "red binned at column {col}, but the plot draws x = 0.64 at {want_col}"
        );

        // And the row, which is the half of this most easily got wrong: `bump`
        // stores v *downwards* — row zero is the top of the plot — and the
        // drawing's `sample_grid` reads it back upwards. Getting x right and y
        // upside down would draw the cloud mirrored about the plot's middle
        // and still pass an assertion about columns alone.
        let want_row = (GRID - 1) - (plot_fraction(0.33) * GRID as f32) as usize;
        assert!(
            row.abs_diff(want_row) <= 1,
            "red binned at row {row}, but the plot draws y = 0.33 at row \
             {want_row} — rows count down from the top"
        );

        // And the two ends of the plot are reachable, which they are not if the
        // grid covers a narrower range than the plot: reading the cell back
        // through the plot's own range has to return the chromaticity that was
        // measured.
        let back = |i: usize| PLOT_MIN + (i as f32 + 0.5) / GRID as f32 * (PLOT_SPAN - PLOT_MIN);
        let (got_x, got_y) = (back(col), back((GRID - 1) - row));
        assert!(
            (got_x - 0.64).abs() < 0.01 && (got_y - 0.33).abs() < 0.01,
            "the cell red landed in reads back as ({got_x}, {got_y}), not (0.64, 0.33)"
        );
    }
}
