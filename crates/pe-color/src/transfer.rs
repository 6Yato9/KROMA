//! Transfer functions — the encoding between linear light and whatever the
//! signal is stored or graded in.
//!
//! Every function here is **sign-preserving**: negative inputs are mirrored
//! through the origin rather than clamped. This matters more than it looks.
//! Converting *out* of the wide working gamut into a narrower output space
//! routinely produces small negative channel values, and clamping them inside
//! the transfer function bakes a permanent hue shift into the image before any
//! gamut mapping has had a chance to handle it properly.

/// How a signal is encoded relative to linear light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransferFn {
    /// Scene-linear. `encode` and `decode` are the identity.
    Linear,
    /// IEC 61966-2-1 sRGB, the piecewise curve with the 12.92 linear segment.
    Srgb,
    /// ACEScct — the ACES grading log encoding, with a linear toe below
    /// 0.0078125. The toe is the whole point: pure log has infinite dynamic
    /// range near black, which makes lift controls behave erratically.
    AcesCct,
    /// A pure power law. `Gamma(2.2)` etc.
    Gamma(f64),
}

// --- ACEScct constants, from S-2016-001 ---------------------------------------

const CCT_A: f64 = 10.5402377416545;
const CCT_B: f64 = 0.0729055341958355;
/// Linear value at which ACEScct switches from its linear toe to log.
const CCT_BREAK_LINEAR: f64 = 0.0078125;
/// The encoded value corresponding to `CCT_BREAK_LINEAR`.
const CCT_BREAK_LOG: f64 = 0.155251141552511;
/// Encoded value of half-float max (65504.0), where the encoding saturates.
const CCT_MAX_ENCODED: f64 = 1.468;

impl TransferFn {
    /// Encoded → linear light.
    pub fn decode(self, v: f64) -> f64 {
        let s = v.signum();
        let a = v.abs();
        s * match self {
            TransferFn::Linear => a,
            TransferFn::Srgb => {
                if a <= 0.04045 {
                    a / 12.92
                } else {
                    ((a + 0.055) / 1.055).powf(2.4)
                }
            }
            TransferFn::AcesCct => {
                if a <= CCT_BREAK_LOG {
                    (a - CCT_B) / CCT_A
                } else if a < CCT_MAX_ENCODED {
                    (2.0f64).powf(a * 17.52 - 9.72)
                } else {
                    65504.0
                }
            }
            TransferFn::Gamma(g) => a.powf(g),
        }
    }

    /// Linear light → encoded.
    pub fn encode(self, v: f64) -> f64 {
        let s = v.signum();
        let a = v.abs();
        s * match self {
            TransferFn::Linear => a,
            TransferFn::Srgb => {
                if a <= 0.0031308 {
                    a * 12.92
                } else {
                    1.055 * a.powf(1.0 / 2.4) - 0.055
                }
            }
            TransferFn::AcesCct => {
                if a <= CCT_BREAK_LINEAR {
                    CCT_A * a + CCT_B
                } else {
                    (a.log2() + 9.72) / 17.52
                }
            }
            TransferFn::Gamma(g) => a.powf(1.0 / g),
        }
    }

    /// Apply `decode` to an RGB triple.
    pub fn decode_rgb(self, rgb: [f64; 3]) -> [f64; 3] {
        [
            self.decode(rgb[0]),
            self.decode(rgb[1]),
            self.decode(rgb[2]),
        ]
    }

    /// Apply `encode` to an RGB triple.
    pub fn encode_rgb(self, rgb: [f64; 3]) -> [f64; 3] {
        [
            self.encode(rgb[0]),
            self.encode(rgb[1]),
            self.encode(rgb[2]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACES: [TransferFn; 4] = [
        TransferFn::Linear,
        TransferFn::Srgb,
        TransferFn::AcesCct,
        TransferFn::Gamma(2.2),
    ];

    #[test]
    fn encode_decode_round_trips() {
        for tf in SPACES {
            for i in 0..=1000 {
                let lin = i as f64 / 1000.0;
                let back = tf.decode(tf.encode(lin));
                assert!(
                    (back - lin).abs() < 1e-9,
                    "{tf:?}: {lin} -> {} -> {back}",
                    tf.encode(lin)
                );
            }
        }
    }

    #[test]
    fn round_trips_above_diffuse_white() {
        // Highlights above 1.0 are the normal case in a linear pipeline.
        for tf in SPACES {
            for lin in [1.5, 4.0, 16.0, 100.0] {
                let back = tf.decode(tf.encode(lin));
                assert!((back - lin).abs() / lin < 1e-9, "{tf:?} at {lin} -> {back}");
            }
        }
    }

    #[test]
    fn negatives_are_mirrored_not_clamped() {
        for tf in SPACES {
            for lin in [-0.001, -0.05, -0.4] {
                let back = tf.decode(tf.encode(lin));
                assert!(back < 0.0, "{tf:?} clamped {lin} to {back}");
                assert!((back - lin).abs() < 1e-9, "{tf:?}: {lin} -> {back}");
            }
        }
    }

    #[test]
    fn display_referred_anchors_are_exact() {
        // Black is 0 and white is 1 — but only for display-referred encodings.
        for tf in [TransferFn::Linear, TransferFn::Srgb, TransferFn::Gamma(2.2)] {
            assert!(tf.encode(0.0).abs() < 1e-12, "{tf:?} black");
            assert!((tf.encode(1.0) - 1.0).abs() < 1e-9, "{tf:?} white");
        }
    }

    #[test]
    fn acescct_black_is_not_zero() {
        // ACEScct puts linear 0.0 at 0.0729, not at 0.0, because of its linear
        // toe. Any shader that assumes "0 means black" is wrong in log space,
        // and that assumption is the most common cause of crushed shadows.
        assert!((TransferFn::AcesCct.encode(0.0) - CCT_B).abs() < 1e-12);
        assert!(TransferFn::AcesCct.decode(CCT_B).abs() < 1e-12);
    }

    #[test]
    fn acescct_white_has_headroom_above_it() {
        // Linear 1.0 encodes to 9.72/17.52 ~ 0.555, leaving room above for the
        // many stops of highlight a linear pipeline carries.
        let white = TransferFn::AcesCct.encode(1.0);
        assert!((white - 9.72 / 17.52).abs() < 1e-12, "got {white}");
        assert!(white < 1.0, "ACEScct should leave highlight headroom");
    }

    #[test]
    fn srgb_matches_published_values() {
        // Mid-grey 0.5 encoded is ~0.7353569; 8-bit 128/255 decodes to ~0.2158605.
        assert!((TransferFn::Srgb.encode(0.5) - 0.735356983052).abs() < 1e-9);
        assert!((TransferFn::Srgb.decode(128.0 / 255.0) - 0.215860500274).abs() < 1e-9);
    }

    #[test]
    fn acescct_toe_is_continuous_at_the_break() {
        // The linear toe and the log segment must meet exactly, or grading
        // controls show a visible kink in the deep shadows.
        let from_toe = CCT_A * CCT_BREAK_LINEAR + CCT_B;
        let from_log = (CCT_BREAK_LINEAR.log2() + 9.72) / 17.52;
        assert!(
            (from_toe - from_log).abs() < 1e-12,
            "toe {from_toe} vs log {from_log}"
        );
        assert!((from_toe - CCT_BREAK_LOG).abs() < 1e-12);
    }

    #[test]
    fn acescct_encodes_mid_grey_near_spec() {
        // 18% grey should land close to 0.4135 in ACEScct.
        let v = TransferFn::AcesCct.encode(0.18);
        assert!((v - 0.413588667).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn acescct_is_monotonic() {
        let mut prev = f64::NEG_INFINITY;
        for i in 0..=10_000 {
            let lin = i as f64 / 1000.0;
            let e = TransferFn::AcesCct.encode(lin);
            assert!(e > prev, "not monotonic at {lin}");
            prev = e;
        }
    }
}
