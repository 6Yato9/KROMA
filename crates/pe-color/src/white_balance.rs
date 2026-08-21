//! White balance: temperature and tint to per-channel gains.
//!
//! Lives here rather than in a shader because it is colour science, not
//! rendering. The shader receives three multipliers and does one multiply; all
//! the reasoning is on this side, where it can be tested against known
//! illuminants.

use crate::matrix::Mat3;
use crate::primaries::{self, Chromaticity, Primaries};

/// The temperature the UI treats as neutral. An image tagged 6500 K needs no
/// correction.
pub const NEUTRAL_TEMPERATURE: f64 = 6500.0;

/// Chromaticity of a Planckian (black-body) radiator at `temp_k`.
///
/// Kim et al.'s cubic approximation, valid 1667–25000 K — which covers the
/// whole slider including tungsten at ~2800 K. The CIE daylight locus is the
/// other common choice but is only defined above 4000 K, so it cannot express
/// the warm end photographers actually use.
pub fn planckian_chromaticity(temp_k: f64) -> Chromaticity {
    let t = temp_k.clamp(1667.0, 25000.0);
    let (t1, t2, t3) = (1.0e3 / t, 1.0e6 / (t * t), 1.0e9 / (t * t * t));

    let x = if t <= 4000.0 {
        -0.266_123_9 * t3 - 0.234_358_9 * t2 + 0.877_695_6 * t1 + 0.179_910
    } else {
        -3.025_846_9 * t3 + 2.107_037_9 * t2 + 0.222_634_7 * t1 + 0.240_390
    };

    let (a, b, c, d) = if t <= 2222.0 {
        (-1.106_381_4, -1.348_110_20, 2.185_558_32, -0.202_196_83)
    } else if t <= 4000.0 {
        (-0.954_947_6, -1.374_185_93, 2.091_370_15, -0.167_488_67)
    } else {
        (3.081_758_0, -5.873_386_70, 3.751_129_97, -0.370_014_83)
    };
    let y = a * x * x * x + b * x * x + c * x + d;

    Chromaticity::new(x, y)
}

/// Per-channel gains that neutralise an image shot under `temp_k` with a
/// green/magenta `tint`, expressed in `working`'s primaries.
///
/// `tint` runs -100..100. Positive is magenta (less green), matching the
/// convention every other editor uses; getting the sign backwards is the kind
/// of thing nobody notices until a user complains their tint slider is
/// inverted.
///
/// Gains are normalised to preserve luminance, so white balance changes colour
/// without changing exposure. Without that, dragging temperature also drags
/// brightness and the two controls fight each other.
pub fn gains(temp_k: f64, tint: f64, working: &Primaries) -> [f64; 3] {
    let to_rgb = working.xyz_to_rgb();
    let scene = illuminant_rgb(&to_rgb, planckian_chromaticity(temp_k));
    let reference = illuminant_rgb(&to_rgb, planckian_chromaticity(NEUTRAL_TEMPERATURE));

    let mut g = [
        reference[0] / scene[0],
        reference[1] / scene[1],
        reference[2] / scene[2],
    ];

    // Tint moves along green-magenta, which is perpendicular to the
    // temperature axis and is what the Planckian locus cannot express.
    let green = 1.0 - tint.clamp(-100.0, 100.0) * 0.002;
    g[1] *= green;

    normalise_luminance(g, working)
}

fn illuminant_rgb(xyz_to_rgb: &Mat3, white: Chromaticity) -> [f64; 3] {
    let rgb = xyz_to_rgb.mul_vec(white.to_xyz());
    // A Planckian radiator has no zero or negative components in any sane
    // working gamut, but guard anyway: a division by zero here would blow the
    // whole image out rather than fail visibly.
    [rgb[0].max(1e-6), rgb[1].max(1e-6), rgb[2].max(1e-6)]
}

/// Scale gains so that applying them leaves a neutral's luminance unchanged.
fn normalise_luminance(g: [f64; 3], working: &Primaries) -> [f64; 3] {
    let luma = working.rgb_to_xyz().0[1];
    let y: f64 = (0..3).map(|i| luma[i] * g[i]).sum();
    if y <= 1e-9 {
        return [1.0; 3];
    }
    [g[0] / y, g[1] / y, g[2] / y]
}

/// Convenience for the working gamut.
pub fn working_gains(temp_k: f64, tint: f64) -> [f64; 3] {
    gains(temp_k, tint, &primaries::AP1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma_of(g: [f64; 3]) -> f64 {
        let w = primaries::AP1.rgb_to_xyz().0[1];
        (0..3).map(|i| w[i] * g[i]).sum()
    }

    #[test]
    fn the_neutral_temperature_is_a_no_op() {
        let g = working_gains(NEUTRAL_TEMPERATURE, 0.0);
        for (i, v) in g.iter().enumerate() {
            assert!((v - 1.0).abs() < 1e-9, "channel {i} gain is {v}");
        }
    }

    #[test]
    fn gains_always_preserve_luminance() {
        // The property that stops white balance and exposure fighting.
        for temp in [2000.0, 2800.0, 4000.0, 5500.0, 6500.0, 9000.0, 15000.0] {
            for tint in [-80.0, 0.0, 80.0] {
                let y = luma_of(working_gains(temp, tint));
                assert!(
                    (y - 1.0).abs() < 1e-9,
                    "T={temp} tint={tint} changed luminance to {y}"
                );
            }
        }
    }

    #[test]
    fn a_warmer_setting_cools_the_image() {
        // Telling the app "this was shot at 2800 K" means correcting a warm
        // cast, so blue is boosted relative to red.
        let g = working_gains(2800.0, 0.0);
        assert!(
            g[2] > g[0],
            "correcting tungsten should raise blue above red, got {g:?}"
        );
    }

    #[test]
    fn a_cooler_setting_warms_the_image() {
        let g = working_gains(12000.0, 0.0);
        assert!(
            g[0] > g[2],
            "correcting shade should raise red above blue, got {g:?}"
        );
    }

    #[test]
    fn gains_move_monotonically_with_temperature() {
        let ratios: Vec<f64> = [2000.0, 3000.0, 4500.0, 6500.0, 9000.0, 15000.0, 25000.0]
            .iter()
            .map(|t| {
                let g = working_gains(*t, 0.0);
                g[0] / g[2]
            })
            .collect();
        assert!(
            ratios.windows(2).all(|w| w[0] < w[1]),
            "red/blue ratio is not monotonic: {ratios:?}"
        );
    }

    #[test]
    fn positive_tint_is_magenta() {
        // Positive tint reduces green. Every other editor works this way and an
        // inverted slider is a bug users notice immediately.
        let neutral = working_gains(6500.0, 0.0);
        let magenta = working_gains(6500.0, 50.0);
        assert!(
            magenta[1] < neutral[1],
            "positive tint should reduce green: {magenta:?} vs {neutral:?}"
        );
    }

    #[test]
    fn negative_tint_is_green() {
        let neutral = working_gains(6500.0, 0.0);
        let green = working_gains(6500.0, -50.0);
        assert!(green[1] > neutral[1]);
    }

    #[test]
    fn extreme_temperatures_are_clamped_not_wild() {
        for temp in [0.0, 1.0, 500.0, 1e9] {
            let g = working_gains(temp, 0.0);
            assert!(
                g.iter().all(|v| v.is_finite() && *v > 0.0 && *v < 100.0),
                "T={temp} produced {g:?}"
            );
        }
    }

    #[test]
    fn the_planckian_locus_matches_known_illuminants() {
        // D65's Planckian neighbour sits near (0.3135, 0.3237); the locus is
        // close to but not identical to the daylight locus, which is expected.
        let p = planckian_chromaticity(6500.0);
        assert!((p.x - 0.3135).abs() < 0.005, "x = {}", p.x);
        assert!((p.y - 0.3237).abs() < 0.005, "y = {}", p.y);

        // Tungsten, ~2856 K (CIE illuminant A) is near (0.4476, 0.4074).
        let a = planckian_chromaticity(2856.0);
        assert!((a.x - 0.4476).abs() < 0.005, "x = {}", a.x);
        assert!((a.y - 0.4074).abs() < 0.005, "y = {}", a.y);
    }

    #[test]
    fn the_locus_is_continuous_across_its_piecewise_boundaries() {
        // Kim's approximation switches formula at 2222 K and 4000 K. A
        // discontinuity there would show as a jump while dragging the slider.
        for boundary in [2222.0, 4000.0] {
            let below = planckian_chromaticity(boundary - 0.5);
            let above = planckian_chromaticity(boundary + 0.5);
            assert!(
                (below.x - above.x).abs() < 1e-3 && (below.y - above.y).abs() < 1e-3,
                "discontinuity at {boundary} K: {below:?} vs {above:?}"
            );
        }
    }
}
