//! Colour primaries, white points, and the derivation of RGB↔XYZ matrices.
//!
//! Nothing here is hardcoded from a spec table. The matrices are *derived* from
//! chromaticity coordinates at runtime, which means a new colour space is four
//! xy pairs rather than nine copied constants — and the tests below check the
//! derivation against the published sRGB matrix, so a typo in the primaries
//! fails loudly instead of shifting every image by a percent.

use crate::matrix::Mat3;

/// A CIE xy chromaticity coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chromaticity {
    pub x: f64,
    pub y: f64,
}

impl Chromaticity {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Convert to XYZ, normalised so Y = 1.
    pub fn to_xyz(self) -> [f64; 3] {
        [self.x / self.y, 1.0, (1.0 - self.x - self.y) / self.y]
    }

    /// Derive a chromaticity from XYZ tristimulus values.
    ///
    /// Illuminants are *defined* by their tristimulus values; the xy pairs you
    /// see quoted are rounded projections of them. Deriving here rather than
    /// pasting a rounded xy is worth ~7e-5 of accuracy in the resulting
    /// matrices — the difference between agreeing with every published matrix
    /// and quietly disagreeing in the fourth decimal.
    pub const fn from_xyz(xyz: [f64; 3]) -> Self {
        let sum = xyz[0] + xyz[1] + xyz[2];
        Self {
            x: xyz[0] / sum,
            y: xyz[1] / sum,
        }
    }
}

/// The four chromaticities that define an RGB colour space's gamut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Primaries {
    pub red: Chromaticity,
    pub green: Chromaticity,
    pub blue: Chromaticity,
    pub white: Chromaticity,
}

impl Primaries {
    /// Derive the RGB→XYZ matrix.
    ///
    /// Standard construction: build a matrix `M` whose columns are the primaries
    /// in XYZ at unit luminance, solve `M · s = white_xyz` for the per-channel
    /// scale factors `s`, then scale the columns by `s` so that RGB(1,1,1) maps
    /// exactly to the white point.
    pub fn rgb_to_xyz(&self) -> Mat3 {
        let m = Mat3::from_cols(self.red.to_xyz(), self.green.to_xyz(), self.blue.to_xyz());
        let scale = m
            .inverse()
            .expect("primaries are linearly independent")
            .mul_vec(self.white.to_xyz());
        m.mul(&Mat3::diag(scale))
    }

    pub fn xyz_to_rgb(&self) -> Mat3 {
        self.rgb_to_xyz()
            .inverse()
            .expect("rgb_to_xyz is invertible for any valid primaries")
    }
}

// ---------------------------------------------------------------------------
// White points
// ---------------------------------------------------------------------------

/// D65, the white point of sRGB, Display P3 and Rec.2020.
///
/// Built from the CIE 2-degree tristimulus values rather than the rounded
/// xy = (0.3127, 0.3290) that IEC 61966-2-1 prints. Using the rounded pair puts
/// the derived sRGB matrix ~2e-4 away from every published one; the tristimulus
/// derivation lands within 5e-8. See `srgb_matrix_matches_published_values`.
pub const D65: Chromaticity = Chromaticity::from_xyz([0.95047, 1.0, 1.08883]);

/// D60 as used by ACES.
///
/// Unlike D65 and D50, the ACES specification defines this one *as* an xy pair,
/// so it is written out rather than derived. Note it is the ACES white point,
/// which is close to but not identical to CIE D60.
pub const ACES_WHITE: Chromaticity = Chromaticity::new(0.32168, 0.33767);

/// D50, the ICC profile connection space white point.
pub const D50: Chromaticity = Chromaticity::from_xyz([0.96422, 1.0, 0.82521]);

// ---------------------------------------------------------------------------
// Gamuts
// ---------------------------------------------------------------------------

/// IEC 61966-2-1 sRGB / Rec.709 primaries.
pub const SRGB: Primaries = Primaries {
    red: Chromaticity::new(0.640, 0.330),
    green: Chromaticity::new(0.300, 0.600),
    blue: Chromaticity::new(0.150, 0.060),
    white: D65,
};

/// SMPTE RP 431-2 / Display P3.
pub const DISPLAY_P3: Primaries = Primaries {
    red: Chromaticity::new(0.680, 0.320),
    green: Chromaticity::new(0.265, 0.690),
    blue: Chromaticity::new(0.150, 0.060),
    white: D65,
};

/// ITU-R BT.2020.
pub const REC2020: Primaries = Primaries {
    red: Chromaticity::new(0.708, 0.292),
    green: Chromaticity::new(0.170, 0.797),
    blue: Chromaticity::new(0.131, 0.046),
    white: D65,
};

/// ACES AP0 — the ACES2065-1 archival gamut. Encloses the entire visible
/// spectrum, which is why two of its primaries are imaginary.
pub const AP0: Primaries = Primaries {
    red: Chromaticity::new(0.7347, 0.2653),
    green: Chromaticity::new(0.0000, 1.0000),
    blue: Chromaticity::new(0.0001, -0.0770),
    white: ACES_WHITE,
};

/// ACES AP1 — the working gamut used by ACEScg and ACEScct. This is the gamut
/// the whole editor grades in.
pub const AP1: Primaries = Primaries {
    red: Chromaticity::new(0.713, 0.293),
    green: Chromaticity::new(0.165, 0.830),
    blue: Chromaticity::new(0.128, 0.044),
    white: ACES_WHITE,
};

// ---------------------------------------------------------------------------
// Chromatic adaptation
// ---------------------------------------------------------------------------

/// The Bradford cone response matrix.
const BRADFORD: Mat3 = Mat3([
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
]);

/// Build a von Kries–style chromatic adaptation matrix in XYZ, using Bradford
/// cone responses.
///
/// Needed because AP1 is D60 and sRGB is D65: without this, converting between
/// them leaves a visible warm cast. It is a small effect and easy to forget,
/// which is precisely why it is applied automatically in [`crate::space`]
/// rather than left to callers.
pub fn bradford_adaptation(src_white: Chromaticity, dst_white: Chromaticity) -> Mat3 {
    if src_white == dst_white {
        return Mat3::IDENTITY;
    }
    let src_cone = BRADFORD.mul_vec(src_white.to_xyz());
    let dst_cone = BRADFORD.mul_vec(dst_white.to_xyz());
    let scale = Mat3::diag([
        dst_cone[0] / src_cone[0],
        dst_cone[1] / src_cone[1],
        dst_cone[2] / src_cone[2],
    ]);
    let inv_bradford = BRADFORD.inverse().expect("Bradford matrix is invertible");
    inv_bradford.mul(&scale).mul(&BRADFORD)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published sRGB D65 RGB→XYZ matrix. If the derivation drifts, this is
    /// the test that catches it.
    #[test]
    fn srgb_matrix_matches_published_values() {
        let expected = Mat3([
            [0.4124564, 0.3575761, 0.1804375],
            [0.2126729, 0.7151522, 0.0721750],
            [0.0193339, 0.1191920, 0.9503041],
        ]);
        assert!(
            SRGB.rgb_to_xyz().approx_eq(&expected, 1e-6),
            "derived {:?}",
            SRGB.rgb_to_xyz()
        );
    }

    #[test]
    fn white_maps_to_white_point() {
        for (name, p) in [
            ("sRGB", SRGB),
            ("P3", DISPLAY_P3),
            ("Rec2020", REC2020),
            ("AP0", AP0),
            ("AP1", AP1),
        ] {
            let got = p.rgb_to_xyz().mul_vec([1.0, 1.0, 1.0]);
            let want = p.white.to_xyz();
            for i in 0..3 {
                assert!(
                    (got[i] - want[i]).abs() < 1e-10,
                    "{name}: channel {i} got {} want {}",
                    got[i],
                    want[i]
                );
            }
        }
    }

    #[test]
    fn rgb_to_xyz_round_trips() {
        for p in [SRGB, DISPLAY_P3, REC2020, AP0, AP1] {
            let round = p.xyz_to_rgb().mul(&p.rgb_to_xyz());
            assert!(round.approx_eq(&Mat3::IDENTITY, 1e-10));
        }
    }

    #[test]
    fn luminance_row_sums_to_one() {
        // Row 1 of RGB→XYZ is the luminance weighting; it must sum to the white
        // point's Y, which is 1 by our normalisation.
        for p in [SRGB, DISPLAY_P3, REC2020, AP1] {
            let m = p.rgb_to_xyz();
            let sum: f64 = m.0[1].iter().sum();
            assert!((sum - 1.0).abs() < 1e-10, "luminance row summed to {sum}");
        }
    }

    #[test]
    fn adaptation_maps_source_white_onto_destination_white() {
        let m = bradford_adaptation(ACES_WHITE, D65);
        let got = m.mul_vec(ACES_WHITE.to_xyz());
        let want = D65.to_xyz();
        for i in 0..3 {
            assert!((got[i] - want[i]).abs() < 1e-10, "channel {i}");
        }
    }

    #[test]
    fn adaptation_is_identity_for_equal_whites() {
        assert_eq!(bradford_adaptation(D65, D65), Mat3::IDENTITY);
    }

    #[test]
    fn adaptation_inverts() {
        let fwd = bradford_adaptation(ACES_WHITE, D65);
        let back = bradford_adaptation(D65, ACES_WHITE);
        assert!(fwd.mul(&back).approx_eq(&Mat3::IDENTITY, 1e-12));
    }
}
