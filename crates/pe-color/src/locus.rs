//! The spectral locus, and what colour sits at a chromaticity.
//!
//! The horseshoe is the boundary of colour itself: every real colour is a
//! mixture of spectral lights, so every real colour sits inside the curve those
//! lights trace on the CIE diagram. A chromaticity plot that draws a triangle
//! instead is drawing one display's gamut and calling it the world.
//!
//! Kept apart from the widget because it is *data plus a couple of geometric
//! questions*, and both are testable without a screen. Here rather than in a
//! shell because it is the same question in both, and because the matrix it
//! needs to answer the second question is one this crate already derives.
//!
//! Deliberately `f32`, unlike the rest of the crate. This is drawing data — a
//! polyline and a per-texel colour, both of which end up in a vertex buffer or
//! a texture — and carrying it in `f64` would mean rounding on the way out and
//! a fixture that no longer says what either shell actually draws. The one
//! piece of colour science in it, [`colour_at`]'s matrix, is derived in `f64`
//! and narrowed once.

use crate::primaries;

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

/// How many points the curve is drawn and tested against.
///
/// The table is sampled every 10 nm, and joining those with straight lines
/// gives a boundary you can count the corners on — which is the difference
/// between our horseshoe and Resolve's. Sixteen steps between each pair is
/// smooth at any size this plot is drawn.
pub const SUBDIVISIONS: usize = 16;

/// The locus as a smooth closed curve.
///
/// Catmull-Rom through the tabulated points: it passes *through* every one of
/// them, which matters when the points are measurements rather than handles —
/// a spline that merely approaches them would be drawing a different curve
/// from the one the CIE published.
///
/// The curve editor rejected Catmull-Rom for exactly the property that is
/// harmless here: it overshoots between control points, and a tone curve that
/// bulges is a bright halo nobody asked for. This is a smooth closed curve
/// that no pixel is looked up in, so the overshoot is a fraction of a
/// thousandth of a chromaticity and invisible. The choice is deliberate on
/// both sides; neither should be changed to match the other.
///
/// The line of purples stays straight. It is a chord, not a spectral colour:
/// there is no wavelength anywhere along it, and rounding it off would claim
/// colours that do not exist.
pub fn curve() -> &'static [[f32; 2]] {
    static CURVE: std::sync::OnceLock<Vec<[f32; 2]>> = std::sync::OnceLock::new();
    CURVE.get_or_init(|| {
        let n = LOCUS.len();
        let at = |i: isize| LOCUS[i.clamp(0, n as isize - 1) as usize];
        let mut out = Vec::with_capacity(n * SUBDIVISIONS + 1);
        for i in 0..n - 1 {
            let (p0, p1, p2, p3) = (
                at(i as isize - 1),
                at(i as isize),
                at(i as isize + 1),
                at(i as isize + 2),
            );
            for step in 0..SUBDIVISIONS {
                let t = step as f32 / SUBDIVISIONS as f32;
                out.push(catmull_rom(p0, p1, p2, p3, t));
            }
        }
        out.push(LOCUS[n - 1]);
        out
    })
}

fn catmull_rom(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let (t2, t3) = (t * t, t * t * t);
    let axis = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * ((2.0 * b)
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    [
        axis(p0[0], p1[0], p2[0], p3[0]),
        axis(p0[1], p1[1], p2[1], p3[1]),
    ]
}

/// How finely the span table divides the y axis.
const SPAN_ROWS: usize = 1024;
/// The top of the table. The locus reaches 0.8338.
const SPAN_TOP: f32 = 0.84;

/// For each row of y, where the curve starts and stops.
///
/// The region is convex, so a horizontal line crosses its boundary exactly
/// twice and "is this a colour" becomes a lookup and two comparisons. Built
/// because [`inside`] is asked once per texel of the plot — a hundred and
/// fifty thousand times per rebuild — and walking five hundred segments each
/// time is a third of a second of somebody's slider drag.
fn spans() -> &'static [[f32; 2]; SPAN_ROWS] {
    static SPANS: std::sync::OnceLock<Box<[[f32; 2]; SPAN_ROWS]>> = std::sync::OnceLock::new();
    SPANS.get_or_init(|| {
        let mut table = Box::new([[1.0f32, -1.0f32]; SPAN_ROWS]);
        let points = curve();
        let n = points.len();
        for row in 0..SPAN_ROWS {
            let y = (row as f32 + 0.5) / SPAN_ROWS as f32 * SPAN_TOP;
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for i in 0..n {
                // Closed: the last point joins the first along the purple line.
                let a = points[i];
                let b = points[(i + 1) % n];
                if (a[1] > y) != (b[1] > y) {
                    let t = (y - a[1]) / (b[1] - a[1]);
                    let x = a[0] + t * (b[0] - a[0]);
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            table[row] = if lo <= hi { [lo, hi] } else { [1.0, -1.0] };
        }
        table
    })
}

/// Whether a chromaticity is a colour at all.
pub fn inside(x: f32, y: f32) -> bool {
    if !(0.0..SPAN_TOP).contains(&y) {
        return false;
    }
    let row = ((y / SPAN_TOP) * SPAN_ROWS as f32) as usize;
    let [lo, hi] = spans()[row.min(SPAN_ROWS - 1)];
    x >= lo && x <= hi
}

/// The sRGB primaries as a matrix from XYZ, for asking what a chromaticity
/// looks like on this screen.
///
/// Derived from [`primaries::SRGB`] rather than written out. The nine numbers
/// used to be a literal here, which is one more place for a matrix to drift
/// from the four chromaticities it is supposed to be a consequence of; the
/// numbers themselves are now the assertion in
/// `the_derived_matrix_is_the_published_one`, which is worth more than the
/// constant was because it checks this crate's derivation against the standard
/// instead of assuming it.
///
/// Narrowed to `f32` once, here. The derivation runs in `f64` for the reason
/// [`crate::matrix`] gives — inverting in `f32` loses about three decimal
/// digits — and a plot texel does not need the other five.
fn xyz_to_srgb() -> &'static [[f32; 3]; 3] {
    static M: std::sync::OnceLock<[[f32; 3]; 3]> = std::sync::OnceLock::new();
    M.get_or_init(|| {
        let m = primaries::SRGB.xyz_to_rgb();
        std::array::from_fn(|r| std::array::from_fn(|c| m.0[r][c] as f32))
    })
}

/// The colour at a chromaticity, as near as a display can put it.
///
/// Answers for the *whole plane*, not only for real colours — the caller asks
/// [`inside`] separately and dims what is outside. Resolve fills its whole
/// diagram this way and it is the better drawing: a black surround makes the
/// plot a shape floating in nothing, where a dimmed one makes it a bright
/// region of a continuous field, which is what a gamut actually is.
///
/// `None` only where the arithmetic has nothing to say: at y of zero there is
/// no colour to normalise, however the plot would like to draw it.
///
/// Most of the diagram is outside what any monitor can produce, and there are
/// two honest things to do about that: darken it, or show the nearest colour
/// the screen has. Resolve does the second, and it is the better answer for a
/// plot you are going to place a pin on — a region drawn black is a region you
/// will not aim at, and those colours are perfectly real.
///
/// Normalised to full brightness rather than scaled by luminance, because this
/// is a map of *chromaticity*: how bright a colour is has its own axes
/// elsewhere, and shading the plot by luminance would make yellow look like the
/// only colour worth having.
pub fn colour_at(x: f32, y: f32) -> Option<[f32; 3]> {
    if y <= 1e-4 {
        return None;
    }
    let xyz = [x / y, 1.0, (1.0 - x - y) / y];
    let mut rgb = [0.0f32; 3];
    for (i, row) in xyz_to_srgb().iter().enumerate() {
        rgb[i] = row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2];
    }
    // Clipped towards white rather than per channel: taking a negative to zero
    // on its own shifts the hue, and a plot that changes the colour of a
    // wavelength is worse than one that shows it pale.
    //
    // The deep greens are where it is worst, and worth measuring rather than
    // guessing at: at 510 nm a per-channel clamp moves the hue angle 26°, from
    // 150° towards pure green at 124°, because the negative it flattens is the
    // blue. So the failure is a green that loses its cyan lean, not one that
    // gains it. Clipping towards white holds the angle exactly and pays for it
    // in saturation, which is the trade this plot wants: a chromaticity
    // diagram whose whole job is to say *which* colour is where.
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
    use crate::matrix::Mat3;

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

    /// The nine numbers that used to be a literal in this file.
    ///
    /// Two published forms, and the gap between them is the whole reason the
    /// literal was worth deleting. The full-precision inverse — the one every
    /// colour-science table prints — the derivation reproduces to about 5e-8,
    /// the same agreement `srgb_matrix_matches_published_values` gets in the
    /// forward direction. The four-decimal form printed in IEC 61966-2-1 needs
    /// 5e-4, three and a half orders of magnitude looser, because it was
    /// obtained by inverting the *rounded* four-decimal forward matrix rather
    /// than by rounding the exact inverse: its first entry is 3.2406 where the
    /// exact value is 3.24045, which is not a rounding of it at all. Inversion
    /// magnifies a rounding in the fourth decimal into an error of 3.7e-4, and
    /// the largest is on the green row.
    ///
    /// So the literal this file used to carry was not the sRGB matrix; it was
    /// the sRGB matrix seen through two roundings. Deriving it is the fix, and
    /// this is the check that the derivation lands where the standard means.
    #[test]
    fn the_derived_matrix_is_the_published_one() {
        let derived = primaries::SRGB.xyz_to_rgb();

        let published = Mat3([
            [3.2404542, -1.5371385, -0.4985314],
            [-0.9692660, 1.8760108, 0.0415560],
            [0.0556434, -0.2040259, 1.0572252],
        ]);
        assert!(
            derived.approx_eq(&published, 1e-6),
            "derived {:?}",
            derived.0
        );

        // And the rounded form that was hardcoded here, which is all a texel of
        // the plot ever needed and which f32 narrowing does not disturb.
        let rounded = Mat3([
            [3.2406, -1.5372, -0.4986],
            [-0.9689, 1.8758, 0.0415],
            [0.0557, -0.2040, 1.0570],
        ]);
        assert!(derived.approx_eq(&rounded, 5e-4), "derived {:?}", derived.0);
        let narrowed = xyz_to_srgb();
        for (r, (got, want)) in narrowed.iter().zip(rounded.0.iter()).enumerate() {
            for (c, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert!(
                    (*g as f64 - *w).abs() < 5e-4,
                    "row {r} column {c}: the f32 matrix has {g}, the literal had {w}"
                );
            }
        }
    }

    /// Inside is inside and outside is outside, at points that are not close.
    #[test]
    fn a_real_colour_is_inside_and_an_impossible_one_is_not() {
        assert!(inside(0.3127, 0.3290), "D65 is not a colour");
        assert!(inside(0.33, 0.33), "equal energy white is not a colour");
        // The sRGB primaries themselves: three real colours, well clear of the
        // boundary on the inside.
        assert!(inside(0.640, 0.330), "the sRGB red primary is not a colour");
        assert!(
            inside(0.300, 0.600),
            "the sRGB green primary is not a colour"
        );
        assert!(
            inside(0.150, 0.060),
            "the sRGB blue primary is not a colour"
        );
        assert!(!inside(0.8, 0.8), "the far corner is not a colour");
        assert!(!inside(0.05, 0.05), "below the purple line is not a colour");
        assert!(!inside(0.6, 0.05), "under the locus is not a colour");
        // x + y > 1 puts Z below zero, which is nowhere at all.
        assert!(!inside(0.5, 0.6), "negative Z is not a colour");
    }

    /// The line of purples closes the polygon: a colour below the ends and
    /// between them in x is real, and one below the line is not.
    ///
    /// The chord runs from 380 nm at (0.1741, 0.0050) to 700 nm at
    /// (0.7347, 0.2653), so at x = 0.45 it sits at y = 0.133. These are the
    /// magentas: real colours with no wavelength, and the half of the boundary
    /// that a horseshoe drawn from the table alone would leave open.
    #[test]
    fn the_line_of_purples_closes_the_horseshoe() {
        assert!(
            inside(0.45, 0.20),
            "a magenta above the purple line is real"
        );
        assert!(
            !inside(0.45, 0.10),
            "below the line of purples there is nothing"
        );
        // And it is a straight chord, not a bulge: walking it from end to end,
        // a hair above is inside and a hair below is not.
        let (a, b) = (LOCUS[0], LOCUS[32]);
        for i in 1..20 {
            let t = i as f32 / 20.0;
            let x = a[0] + t * (b[0] - a[0]);
            let y = a[1] + t * (b[1] - a[1]);
            assert!(inside(x, y + 0.01), "({x}, {y}) should be inside the chord");
            assert!(!inside(x, y - 0.01), "({x}, {y}) should be below the chord");
        }
    }

    /// The curve has to pass through the points it was built from. A spline
    /// that only approached them would be drawing a different boundary from
    /// the one the CIE published.
    #[test]
    fn the_smooth_curve_passes_through_every_tabulated_point() {
        let curve = curve();
        for p in LOCUS {
            let near = curve
                .iter()
                .map(|q| ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt())
                .fold(f32::MAX, f32::min);
            assert!(near < 1e-4, "{p:?} is not on the curve, nearest {near}");
        }
    }

    /// And it has to actually be smoother — that is the whole point of it.
    /// Measured as the longest step, which is what shows as a facet.
    #[test]
    fn the_smooth_curve_has_no_visible_corners() {
        let curve = curve();
        let longest = curve
            .windows(2)
            .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        let tabulated = LOCUS
            .windows(2)
            .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        assert!(
            longest < tabulated / 8.0,
            "steps of {longest} against the table's {tabulated} is not smoother"
        );
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

    /// Answered for the whole plane, so a plot can dim rather than blacken.
    #[test]
    fn a_colour_outside_the_horseshoe_still_has_something_to_draw() {
        for (x, y) in [(0.9, 0.9), (0.8, 0.8), (0.05, 0.05), (0.6, 0.05)] {
            assert!(!inside(x, y), "({x}, {y}) was supposed to be impossible");
            let c = colour_at(x, y).unwrap_or_else(|| panic!("({x}, {y}) had nothing to draw"));
            assert!(
                c.iter().all(|v| (0.0..=1.0).contains(v)),
                "({x}, {y}) drew {c:?}"
            );
            // Not black: a dimmed colour is what the caller wants to multiply
            // down, and a black one gives it nothing to dim.
            assert!(c.iter().cloned().fold(0.0f32, f32::max) > 0.5, "{c:?}");
        }
        assert!(colour_at(0.3127, 0.3290).is_some());
    }

    /// Except where the arithmetic has nothing to say.
    #[test]
    fn there_is_no_colour_at_no_luminance() {
        assert!(colour_at(0.4, 0.0).is_none(), "y of zero has no colour");
        assert!(colour_at(0.0, 0.0).is_none());
        // And not a step function at the boundary either: just above it there
        // is an answer, so the plot has one row of texels and not a seam.
        assert!(colour_at(0.4, 1e-3).is_some());
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

    /// Clipped towards white, so a green at the edge goes pale rather than
    /// cyan.
    ///
    /// Hue is measured as the angle in the chromatic plane of the RGB cube:
    /// `atan2(√3·(g − b), 2r − g − b)`, the projection along the grey axis that
    /// HSV's hue is defined by. It is the right instrument for this question
    /// because it is invariant under exactly the two things
    /// [`colour_at`] does after the matrix — adding the same amount to all
    /// three channels, and scaling all three by the peak — and under nothing
    /// else. So the clip-towards-white result must land on the *same* angle as
    /// the unrepresentable colour it came from, to the last bit of `f32`,
    /// while a per-channel clamp has no reason to.
    #[test]
    fn an_out_of_gamut_green_goes_pale_rather_than_changing_hue() {
        // Well outside sRGB's triangle — the green primary is at (0.30, 0.60)
        // and this is far past it — and well inside the horseshoe, whose 520 nm
        // vertex is at (0.0743, 0.8338).
        let (x, y) = (0.10, 0.75);
        assert!(inside(x, y), "the test point is not a real colour");

        let xyz = [x / y, 1.0, (1.0 - x - y) / y];
        let raw: [f32; 3] = std::array::from_fn(|i| {
            let row = xyz_to_srgb()[i];
            row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2]
        });
        assert!(
            raw.iter().any(|c| *c < 0.0),
            "the test point is inside sRGB after all: {raw:?}"
        );

        let hue = |c: [f32; 3]| {
            (3.0f32.sqrt() * (c[1] - c[2]))
                .atan2(2.0 * c[0] - c[1] - c[2])
                .to_degrees()
                .rem_euclid(360.0)
        };
        let wanted = hue(raw);

        let drawn = colour_at(x, y).unwrap();
        assert!(
            (hue(drawn) - wanted).abs() < 0.1,
            "the drawn colour moved from {wanted}° to {}°: {drawn:?}",
            hue(drawn)
        );

        // What the other rule would have done with the same colour: take each
        // negative to zero, normalise the same way.
        let mut clamped: [f32; 3] = std::array::from_fn(|i| raw[i].max(0.0));
        let peak = clamped.iter().cloned().fold(1e-4f32, f32::max);
        for c in &mut clamped {
            *c /= peak;
        }
        assert!(
            (hue(clamped) - wanted).abs() > 10.0,
            "a per-channel clamp was supposed to shift the hue, {wanted}° to {}°",
            hue(clamped)
        );

        // Pale, which is the price: still green, and no longer a green a
        // monitor would refuse to show.
        assert!(
            drawn[1] >= drawn[0] && drawn[1] >= drawn[2],
            "it stopped being green: {drawn:?}"
        );
        // Pale is measured against the other rule on the same colour: the
        // channel the clamp threw away comes back, and that is what the eye
        // reads as a green washed towards white instead of a vivid one.
        assert!(
            drawn[2] > clamped[2] + 0.3,
            "the blue the clamp discarded did not come back: {drawn:?} against {clamped:?}"
        );
    }
}
