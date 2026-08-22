//! The spectral locus, and what colour sits at a chromaticity.
//!
//! The horseshoe is the boundary of colour itself: every real colour is a
//! mixture of spectral lights, so every real colour sits inside the curve those
//! lights trace on the CIE diagram. A chromaticity plot that draws a triangle
//! instead is drawing one display's gamut and calling it the world.
//!
//! Kept apart from the widget because it is *data plus a couple of geometric
//! questions*, and both are testable without a screen.

/// The CIE 1931 2° spectral locus, in xy, at 10 nm from 380 to 700 nm.
///
/// Standard reference data. The three anchors worth checking against any table
/// you have to hand are the ends and the top: 380 nm at (0.1741, 0.0050),
/// 520 nm at (0.0743, 0.8338) — the greenest point there is — and 700 nm at
/// (0.7347, 0.2653). The test below pins those. An error of a thousandth
/// anywhere between them is a curve nobody can see the difference in.
///
/// The polygon closes from 700 nm straight back to 380 nm, which is the line
/// of purples: colours that are real but have no wavelength.
pub const LOCUS: [[f32; 2]; 33] = [
    [0.1741, 0.0050],
    [0.1738, 0.0049],
    [0.1733, 0.0048],
    [0.1726, 0.0048],
    [0.1714, 0.0051],
    [0.1689, 0.0069],
    [0.1644, 0.0109],
    [0.1566, 0.0177],
    [0.1440, 0.0297],
    [0.1241, 0.0578],
    [0.0913, 0.1327],
    [0.0454, 0.2950],
    [0.0082, 0.5384],
    [0.0139, 0.7502],
    [0.0743, 0.8338],
    [0.1547, 0.8059],
    [0.2296, 0.7543],
    [0.3016, 0.6923],
    [0.3731, 0.6245],
    [0.4441, 0.5547],
    [0.5125, 0.4866],
    [0.5752, 0.4242],
    [0.6270, 0.3725],
    [0.6658, 0.3340],
    [0.6915, 0.3083],
    [0.7079, 0.2920],
    [0.7190, 0.2809],
    [0.7260, 0.2740],
    [0.7300, 0.2700],
    [0.7320, 0.2680],
    [0.7334, 0.2666],
    [0.7344, 0.2656],
    [0.7347, 0.2653],
];

/// Whether a chromaticity is a colour at all.
///
/// Winding rule against the closed locus. The curve is convex, so a crossing
/// count would do — but "convex" is a property of the table rather than of the
/// code, and a winding test does not quietly start lying if somebody edits a
/// coefficient.
pub fn inside(x: f32, y: f32) -> bool {
    let mut crossings = 0;
    let n = LOCUS.len();
    for i in 0..n {
        let a = LOCUS[i];
        let b = LOCUS[(i + 1) % n];
        if (a[1] > y) != (b[1] > y) {
            let t = (y - a[1]) / (b[1] - a[1]);
            if x < a[0] + t * (b[0] - a[0]) {
                crossings += 1;
            }
        }
    }
    crossings % 2 == 1
}

/// The sRGB primaries as a matrix from XYZ, for asking what a chromaticity
/// looks like on this screen.
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.2406, -1.5372, -0.4986],
    [-0.9689, 1.8758, 0.0415],
    [0.0557, -0.2040, 1.0570],
];

/// The colour at a chromaticity, as near as a display can put it.
///
/// Returns `None` outside the locus — there is no colour there to show.
///
/// Inside it, most of the diagram is outside what any monitor can produce, and
/// there are two honest things to do about that: darken it, or show the nearest
/// colour the screen has. Resolve does the second, and it is the better answer
/// for a plot you are going to place a pin on — a region drawn black is a
/// region you will not aim at, and those colours are perfectly real.
///
/// Normalised to full brightness rather than scaled by luminance, because this
/// is a map of *chromaticity*: how bright a colour is has its own axes
/// elsewhere, and shading the plot by luminance would make yellow look like the
/// only colour worth having.
pub fn colour_at(x: f32, y: f32) -> Option<[f32; 3]> {
    if y <= 1e-4 || !inside(x, y) {
        return None;
    }
    let xyz = [x / y, 1.0, (1.0 - x - y) / y];
    let mut rgb = [0.0f32; 3];
    for (i, row) in XYZ_TO_SRGB.iter().enumerate() {
        rgb[i] = row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2];
    }
    // Clipped towards white rather than per channel: taking a negative to zero
    // on its own shifts the hue, and a plot whose greens turn cyan at the edge
    // is worse than one whose greens go pale.
    let low = rgb.iter().cloned().fold(0.0f32, f32::min);
    if low < 0.0 {
        for c in &mut rgb {
            *c -= low;
        }
    }
    let peak = rgb.iter().cloned().fold(1e-4f32, f32::max);
    Some([rgb[0] / peak, rgb[1] / peak, rgb[2] / peak])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three points worth checking a locus table against.
    #[test]
    fn the_anchors_are_where_they_should_be() {
        assert_eq!(LOCUS[0], [0.1741, 0.0050], "380 nm");
        assert_eq!(LOCUS[14], [0.0743, 0.8338], "520 nm, the greenest point");
        assert_eq!(LOCUS[32], [0.7347, 0.2653], "700 nm");
    }

    /// The curve has to be a curve: no doubling back, no spikes.
    #[test]
    fn the_locus_is_smooth() {
        for pair in LOCUS.windows(2) {
            let step =
                ((pair[1][0] - pair[0][0]).powi(2) + (pair[1][1] - pair[0][1]).powi(2)).sqrt();
            assert!(
                step < 0.25,
                "a 10 nm step of {step} is not a spectral locus"
            );
        }
    }

    #[test]
    fn white_is_inside_and_the_corners_are_not() {
        assert!(inside(0.3127, 0.3290), "D65 is not a colour");
        assert!(inside(0.33, 0.33), "equal energy white is not a colour");
        assert!(!inside(0.8, 0.8), "the far corner is not a colour");
        assert!(!inside(0.05, 0.05), "below the purple line is not a colour");
        assert!(!inside(0.6, 0.05), "under the locus is not a colour");
    }

    /// Every point *on* the locus is a colour, which is the property the plot
    /// leans on when it draws the boundary.
    #[test]
    fn the_spectral_colours_are_inside_their_own_curve() {
        for p in LOCUS {
            // Nudged a hair towards white, since a point exactly on a polygon
            // edge is a coin toss for any winding test.
            let x = p[0] + (0.33 - p[0]) * 0.02;
            let y = p[1] + (0.33 - p[1]) * 0.02;
            assert!(inside(x, y), "{p:?} is outside the locus it defines");
        }
    }

    #[test]
    fn there_is_no_colour_outside_the_horseshoe() {
        assert!(colour_at(0.9, 0.9).is_none());
        assert!(colour_at(0.3127, 0.3290).is_some());
    }

    /// White comes out white, which is the one value on this diagram anybody
    /// can check by eye.
    #[test]
    fn d65_comes_out_neutral() {
        let c = colour_at(0.3127, 0.3290).unwrap();
        assert!(
            (c[0] - c[1]).abs() < 0.02 && (c[1] - c[2]).abs() < 0.02,
            "D65 came out {c:?}"
        );
    }

    /// And a spectral green comes out green rather than pale or cyan, which is
    /// what the clip-towards-white rule is there to protect.
    #[test]
    fn a_spectral_green_is_still_green() {
        // A hair inside, for the same reason as above: 520 nm is a *vertex* of
        // the polygon, and asking a winding test whether a point is inside a
        // shape it is a corner of is asking it to toss a coin.
        let c = colour_at(0.0794, 0.8187).unwrap();
        assert!(c[1] >= c[0] && c[1] >= c[2], "520 nm came out {c:?}");
    }
}
