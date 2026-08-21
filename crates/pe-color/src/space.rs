//! A colour space is a gamut plus a transfer function, and conversion between
//! two of them is: decode to linear, rotate the gamut (adapting the white
//! point), re-encode.

use crate::matrix::Mat3;
use crate::primaries::{self, Primaries};
use crate::transfer::TransferFn;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSpace {
    pub name: &'static str,
    pub primaries: Primaries,
    pub transfer: TransferFn,
}

impl ColorSpace {
    pub const fn new(name: &'static str, primaries: Primaries, transfer: TransferFn) -> Self {
        Self {
            name,
            primaries,
            transfer,
        }
    }
}

/// Display-referred sRGB. What almost every JPEG on disk actually is.
pub const SRGB: ColorSpace = ColorSpace::new("sRGB", primaries::SRGB, TransferFn::Srgb);

/// sRGB gamut, linear light. Useful as an intermediate and for debugging.
pub const LINEAR_SRGB: ColorSpace =
    ColorSpace::new("Linear sRGB", primaries::SRGB, TransferFn::Linear);

pub const DISPLAY_P3: ColorSpace =
    ColorSpace::new("Display P3", primaries::DISPLAY_P3, TransferFn::Srgb);

pub const REC2020: ColorSpace = ColorSpace::new(
    "Rec.2020",
    primaries::REC2020,
    // Rec.2020's own transfer is close enough to a 2.4 power law for our
    // purposes; true PQ/HLG belongs with HDR output, which is not M0's problem.
    TransferFn::Gamma(2.4),
);

/// ACES2065-1 — the archival interchange space.
pub const ACES2065_1: ColorSpace =
    ColorSpace::new("ACES2065-1", primaries::AP0, TransferFn::Linear);

/// **The working linear space.** Every effect that simulates light runs here.
pub const ACESCG: ColorSpace = ColorSpace::new("ACEScg", primaries::AP1, TransferFn::Linear);

/// **The working log space.** Every effect that shapes perception runs here.
pub const ACESCCT: ColorSpace = ColorSpace::new("ACEScct", primaries::AP1, TransferFn::AcesCct);

/// All spaces the colour-management panel can offer.
pub const ALL: &[ColorSpace] = &[
    SRGB,
    LINEAR_SRGB,
    DISPLAY_P3,
    REC2020,
    ACES2065_1,
    ACESCG,
    ACESCCT,
];

/// Look up a colour space by its display name.
///
/// Documents store spaces by name rather than by matrix, so this is the
/// resolution step on load. Returns `None` for unknown names; callers decide
/// whether that is an error or a fallback.
pub fn by_name(name: &str) -> Option<ColorSpace> {
    ALL.iter()
        .find(|cs| cs.name.eq_ignore_ascii_case(name))
        .copied()
}

/// The 3x3 that rotates linear RGB from one gamut into another, including
/// Bradford white-point adaptation.
///
/// This is the matrix that gets uploaded to the GPU, so it is derived on the
/// CPU in `f64` exactly once per document rather than per frame.
pub fn gamut_matrix(src: &Primaries, dst: &Primaries) -> Mat3 {
    let adapt = primaries::bradford_adaptation(src.white, dst.white);
    dst.xyz_to_rgb().mul(&adapt).mul(&src.rgb_to_xyz())
}

/// Full conversion of a single pixel between two colour spaces.
///
/// The CPU reference path. The GPU does the same thing in a shader; the golden
/// tests assert they agree.
pub fn convert(src: &ColorSpace, dst: &ColorSpace, rgb: [f64; 3]) -> [f64; 3] {
    let linear = src.transfer.decode_rgb(rgb);
    let rotated = if src.primaries == dst.primaries {
        linear
    } else {
        gamut_matrix(&src.primaries, &dst.primaries).mul_vec(linear)
    };
    dst.transfer.encode_rgb(rotated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(got: [f64; 3], want: [f64; 3], eps: f64, ctx: &str) {
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() < eps,
                "{ctx}: channel {i} got {} want {} (delta {})",
                got[i],
                want[i],
                (got[i] - want[i]).abs()
            );
        }
    }

    #[test]
    fn identity_conversion_is_lossless() {
        for cs in ALL {
            let rgb = [0.2, 0.5, 0.8];
            assert_close(convert(cs, cs, rgb), rgb, 1e-12, cs.name);
        }
    }

    #[test]
    fn conversion_round_trips_through_every_space() {
        let samples = [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.18, 0.18, 0.18],
            [0.9, 0.2, 0.05],
            [0.05, 0.6, 0.95],
            [0.42, 0.42, 0.01],
        ];
        for dst in ALL {
            for rgb in samples {
                let there = convert(&SRGB, dst, rgb);
                let back = convert(dst, &SRGB, there);
                assert_close(back, rgb, 1e-9, &format!("sRGB <-> {}", dst.name));
            }
        }
    }

    #[test]
    fn neutral_grey_stays_neutral() {
        // A grey in one space must be a grey in every other, or the white point
        // adaptation is wrong. This is the single most sensitive check here:
        // AP1 is D60 and sRGB is D65, so a missing Bradford step shows up
        // immediately as a warm cast.
        for dst in ALL {
            for level in [0.05, 0.18, 0.5, 0.9] {
                let out = convert(&SRGB, dst, [level, level, level]);
                assert!(
                    (out[0] - out[1]).abs() < 1e-9 && (out[1] - out[2]).abs() < 1e-9,
                    "{}: grey {level} became {out:?}",
                    dst.name
                );
            }
        }
    }

    #[test]
    fn srgb_white_maps_to_acescg_white() {
        let out = convert(&SRGB, &ACESCG, [1.0, 1.0, 1.0]);
        assert_close(out, [1.0, 1.0, 1.0], 1e-9, "white");
    }

    #[test]
    fn gamut_matrix_round_trips() {
        let fwd = gamut_matrix(&primaries::SRGB, &primaries::AP1);
        let back = gamut_matrix(&primaries::AP1, &primaries::SRGB);
        assert!(fwd.mul(&back).approx_eq(&Mat3::IDENTITY, 1e-12));
    }

    #[test]
    fn ap1_encloses_srgb() {
        // AP1 is wide enough to contain all of Rec.709, so a legal sRGB colour
        // never needs negative channels to be represented in ACEScg. Worth
        // asserting explicitly: it is the reason importing a JPEG is lossless.
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
        ] {
            let out = convert(&SRGB, &ACESCG, rgb);
            assert!(
                out.iter().all(|c| *c > -1e-6),
                "sRGB {rgb:?} needed negatives in AP1: {out:?}"
            );
        }
    }

    #[test]
    fn saturated_ap1_goes_negative_in_srgb_rather_than_clamping() {
        // The other direction does not fit: AP1's green lies well outside
        // Rec.709. The correct behaviour is a negative channel, preserved so
        // that gamut mapping can deal with it — not a clamp buried in the
        // transfer function, which would bake in a hue shift.
        let out = convert(&ACESCG, &SRGB, [0.0, 1.0, 0.0]);
        assert!(
            out.iter().any(|c| *c < 0.0),
            "expected a negative channel, got {out:?}"
        );
    }

    #[test]
    fn every_named_space_is_findable() {
        for cs in ALL {
            assert_eq!(by_name(cs.name).map(|c| c.name), Some(cs.name));
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(by_name("srgb").map(|c| c.name), Some("sRGB"));
        assert_eq!(by_name("ACEScg").map(|c| c.name), Some("ACEScg"));
    }

    #[test]
    fn unknown_names_return_none() {
        assert!(by_name("Definitely Not A Colour Space").is_none());
    }
}
