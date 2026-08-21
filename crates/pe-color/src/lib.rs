//! Colour science for the editor. Pure maths, no GPU, no I/O, no dependencies.
//!
//! This crate is deliberately the most boring and most heavily tested thing in
//! the repo, because everything else is downstream of it being right. It is
//! also the crate least likely to change, which is why it sits at the bottom of
//! the dependency graph on its own.
//!
//! # The two-space rule
//!
//! Effects are not interchangeable operations on pixels. Each one either
//! simulates *light* or manipulates *perception*, and it has to run in the
//! matching space:
//!
//! - **Linear** ([`space::ACESCG`]) — exposure, white balance, bloom, halation,
//!   blur, chromatic aberration. Blur a highlight in a gamma-encoded space and
//!   it turns grey and muddy instead of glowing.
//! - **Log** ([`space::ACESCCT`]) — lift/gamma/gain, log wheels, curves,
//!   contrast, HSL, grain. Put a lift wheel on linear data and every useful
//!   adjustment crams into the bottom 3% of the control.
//!
//! [`pipeline::Pipeline`] owns the transitions between them, and
//! [`pipeline::WorkingSpace`] is the declaration each effect makes. No effect
//! ever converts its own input.
//!
//! # Why ACES
//!
//! ACEScg and ACEScct are fully documented and freely implementable, unlike
//! Resolve's proprietary Wide Gamut / Intermediate. ACEScct in particular is
//! designed as a *grading* log space — its linear toe below 0.0078125 is what
//! makes lift controls behave in the deep shadows. As a bonus, third-party film
//! emulation LUTs already target ACES.

pub mod matrix;
pub mod pipeline;
pub mod primaries;
pub mod space;
pub mod transfer;
pub mod white_balance;

pub use matrix::Mat3;
pub use pipeline::{Pipeline, WorkingSpace};
pub use primaries::{Chromaticity, Primaries};
pub use space::ColorSpace;
pub use transfer::TransferFn;
pub use white_balance::working_gains;

/// Rec.709 luminance weights, for the many places that need a luma value.
///
/// Note these are the *sRGB/Rec.709* weights. Code working in AP1 should derive
/// its own from `primaries::AP1.rgb_to_xyz()` row 1 rather than reaching for
/// these out of habit — the difference is small but it is systematic, and it
/// shows up in saturation controls as a hue-dependent brightness error.
pub const REC709_LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Luminance weights for the working gamut (AP1), derived rather than copied.
pub fn ap1_luma() -> [f64; 3] {
    primaries::AP1.rgb_to_xyz().0[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ap1_luma_weights_sum_to_one() {
        let w = ap1_luma();
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-10);
    }

    /// The shader hardcodes AP1's luminance weights because deriving a matrix
    /// per pixel would be absurd. This is the only thing keeping that constant
    /// honest — without it, changing the working gamut would leave every
    /// saturation and grain calculation quietly weighted for the old one.
    #[test]
    fn ap1_luma_matches_the_shader_constant() {
        let shader = include_str!("../../../shaders/common.wgsl");
        let line = shader
            .lines()
            .find(|l| l.trim_start().starts_with("const AP1_LUMA"))
            .expect("AP1_LUMA is missing from common.wgsl");

        let inner = line
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(')'))
            .map(|(args, _)| args)
            .expect("could not find the vec3 arguments");

        let shader_weights: Vec<f64> = inner
            .split(',')
            .map(|s| s.trim().parse().expect("non-numeric weight"))
            .collect();

        let derived = ap1_luma();
        assert_eq!(shader_weights.len(), 3);
        for i in 0..3 {
            assert!(
                (shader_weights[i] - derived[i]).abs() < 1e-6,
                "channel {i}: shader has {}, AP1 derives {}",
                shader_weights[i],
                derived[i]
            );
        }
    }

    #[test]
    fn ap1_luma_differs_from_rec709() {
        // Guards the doc comment above: if these ever coincide, the warning is
        // stale and someone has changed the working gamut.
        let w = ap1_luma();
        assert!(
            (w[0] - REC709_LUMA[0]).abs() > 0.01,
            "AP1 and Rec.709 luma unexpectedly equal"
        );
    }
}
