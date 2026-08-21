//! The two-space rule, in code.
//!
//! Every effect declares the space it needs. The renderer reads that
//! declaration and inserts the transform. No effect ever converts its own
//! input — that is the entire discipline, and it is what stops the pipeline
//! rotting into a pile of ad-hoc `pow(x, 2.2)` calls two years from now.
//!
//! ```text
//!   input space ──▶ ACEScg (linear) ──▶ ACEScct (log) ──▶ ACEScg ──▶ output
//!                        │                    │              │
//!                    bloom, halation      wheels, curves   vignette
//!                    exposure, blur       HSL, grain       sharpen
//! ```

use crate::space::{self, ColorSpace};

/// Which of the two working spaces an operation runs in.
///
/// Deliberately not `Option<something>` and deliberately not defaulted: every
/// effect must make a positive choice, and the compiler enforces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSpace {
    /// Scene-linear ACEScg. For anything that simulates the behaviour of light:
    /// exposure, white balance, bloom, halation, blur, optical defects.
    ///
    /// Blur a highlight anywhere else and it turns grey and muddy.
    Linear,
    /// ACEScct log. For anything that shapes perception: lift/gamma/gain, log
    /// wheels, curves, contrast, HSL, grain.
    ///
    /// Put a lift wheel on linear data and every useful adjustment crams into
    /// the bottom 3% of the control's travel.
    Log,
}

impl WorkingSpace {
    pub fn color_space(self) -> ColorSpace {
        match self {
            WorkingSpace::Linear => space::ACESCG,
            WorkingSpace::Log => space::ACESCCT,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WorkingSpace::Linear => "linear",
            WorkingSpace::Log => "log",
        }
    }
}

/// The colour-management configuration of a document. This is the user-facing
/// panel from Resolve's project settings, not a hidden implementation detail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pipeline {
    /// What the file on disk is. Usually sRGB for JPEG; camera-native for RAW.
    pub input: ColorSpace,
    /// What we render for display or export.
    pub output: ColorSpace,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            input: space::SRGB,
            output: space::SRGB,
        }
    }
}

impl Pipeline {
    pub fn new(input: ColorSpace, output: ColorSpace) -> Self {
        Self { input, output }
    }

    /// Decode a pixel from the input space into working linear (ACEScg).
    pub fn to_working(&self, rgb: [f64; 3]) -> [f64; 3] {
        space::convert(&self.input, &space::ACESCG, rgb)
    }

    /// Encode a working-linear pixel out to the output space.
    pub fn from_working(&self, rgb: [f64; 3]) -> [f64; 3] {
        space::convert(&space::ACESCG, &self.output, rgb)
    }

    /// Move a working pixel from linear into whichever space an effect asked
    /// for. A no-op when the effect wants linear, which is the common case and
    /// costs nothing.
    pub fn enter(&self, rgb: [f64; 3], target: WorkingSpace) -> [f64; 3] {
        match target {
            WorkingSpace::Linear => rgb,
            WorkingSpace::Log => space::convert(&space::ACESCG, &space::ACESCCT, rgb),
        }
    }

    /// The inverse of [`Pipeline::enter`] — back to working linear.
    pub fn leave(&self, rgb: [f64; 3], source: WorkingSpace) -> [f64; 3] {
        match source {
            WorkingSpace::Linear => rgb,
            WorkingSpace::Log => space::convert(&space::ACESCCT, &space::ACESCG, rgb),
        }
    }

    /// Run a whole ordered list of space requirements over one pixel, applying
    /// `op` in each. This is the CPU mirror of what the renderer does with
    /// textures, and exists so the transform bookkeeping can be tested without
    /// a GPU.
    pub fn apply_chain<F>(&self, rgb: [f64; 3], steps: &[WorkingSpace], mut op: F) -> [f64; 3]
    where
        F: FnMut(usize, WorkingSpace, [f64; 3]) -> [f64; 3],
    {
        let mut working = self.to_working(rgb);
        for (i, &space) in steps.iter().enumerate() {
            let entered = self.enter(working, space);
            let processed = op(i, space, entered);
            working = self.leave(processed, space);
        }
        self.from_working(working)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(got: [f64; 3], want: [f64; 3], eps: f64, ctx: &str) {
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() < eps,
                "{ctx}: channel {i} got {} want {}",
                got[i],
                want[i]
            );
        }
    }

    /// **The M0 exit criterion.**
    ///
    /// An sRGB pixel goes in, travels linear → log → linear, and comes back
    /// out matching what went in. If this fails, nothing built on top of it can
    /// be trusted.
    #[test]
    fn srgb_round_trips_through_both_working_spaces() {
        let p = Pipeline::default();
        let samples = [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.18, 0.18, 0.18],
            [0.5, 0.5, 0.5],
            [0.9, 0.2, 0.05],
            [0.05, 0.6, 0.95],
            [0.004, 0.002, 0.001], // deep shadow, inside the ACEScct toe
            [0.99, 0.98, 0.97],
        ];
        for rgb in samples {
            let out = p.apply_chain(
                rgb,
                &[
                    WorkingSpace::Linear,
                    WorkingSpace::Log,
                    WorkingSpace::Linear,
                    WorkingSpace::Log,
                ],
                |_, _, px| px, // identity effects: only the transforms run
            );
            assert_close(out, rgb, 1e-9, &format!("{rgb:?}"));
        }
    }

    #[test]
    fn every_8bit_srgb_level_round_trips() {
        // Exhaustive over the actual input domain of a JPEG.
        let p = Pipeline::default();
        for i in 0..=255u32 {
            let v = i as f64 / 255.0;
            let out = p.apply_chain(
                [v, v, v],
                &[WorkingSpace::Log, WorkingSpace::Linear],
                |_, _, px| px,
            );
            assert_close(
                [out[0], out[1], out[2]],
                [v, v, v],
                1e-9,
                &format!("level {i}"),
            );
        }
    }

    #[test]
    fn enter_and_leave_are_inverses() {
        let p = Pipeline::default();
        for space in [WorkingSpace::Linear, WorkingSpace::Log] {
            for rgb in [[0.18, 0.18, 0.18], [2.5, 0.01, 0.4], [0.0, 0.0, 0.0]] {
                let back = p.leave(p.enter(rgb, space), space);
                assert_close(back, rgb, 1e-9, space.as_str());
            }
        }
    }

    #[test]
    fn entering_linear_is_free() {
        let p = Pipeline::default();
        let rgb = [0.3, 0.4, 0.5];
        assert_eq!(p.enter(rgb, WorkingSpace::Linear), rgb);
        assert_eq!(p.leave(rgb, WorkingSpace::Linear), rgb);
    }

    #[test]
    fn chain_visits_each_step_in_order() {
        let p = Pipeline::default();
        let mut seen = Vec::new();
        p.apply_chain(
            [0.5, 0.5, 0.5],
            &[WorkingSpace::Log, WorkingSpace::Linear, WorkingSpace::Log],
            |i, s, px| {
                seen.push((i, s));
                px
            },
        );
        assert_eq!(
            seen,
            vec![
                (0, WorkingSpace::Log),
                (1, WorkingSpace::Linear),
                (2, WorkingSpace::Log),
            ]
        );
    }

    #[test]
    fn an_effect_in_log_space_sees_log_values() {
        // 18% scene grey should arrive at a log-space effect as ~0.41, not 0.18.
        // If this ever reads 0.18, an effect is being handed linear data and
        // its controls will feel broken.
        let p = Pipeline::default();
        let mut observed = [0.0; 3];
        p.apply_chain([0.18, 0.18, 0.18], &[WorkingSpace::Log], |_, _, px| {
            observed = px;
            px
        });
        // sRGB-encoded 0.18 is linear ~0.0273, which in ACEScct is ~0.29.
        assert!(observed[0] > 0.25 && observed[0] < 0.35, "got {observed:?}");
    }

    #[test]
    fn output_space_is_honoured() {
        let p = Pipeline::new(space::SRGB, space::DISPLAY_P3);
        let out = p.apply_chain([0.9, 0.2, 0.05], &[WorkingSpace::Linear], |_, _, px| px);
        let direct = space::convert(&space::SRGB, &space::DISPLAY_P3, [0.9, 0.2, 0.05]);
        assert_close(out, direct, 1e-9, "sRGB -> P3");
    }
}
