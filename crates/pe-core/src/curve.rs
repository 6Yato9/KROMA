//! Curve evaluation and LUT baking.
//!
//! Monotone cubic Hermite (Fritsch–Carlson), not Catmull-Rom. The difference
//! matters here in a way it does not for most spline work: Catmull-Rom
//! overshoots between control points, so a user dragging a highlight point
//! down can make the curve bulge *above* where it started somewhere in the
//! middle. On a tone curve that shows as a bright halo appearing in a tonal
//! band the user never touched, and it looks like a bug because it is one.
//!
//! Fritsch–Carlson guarantees the interpolant is monotone wherever the data
//! is, which is exactly the property a tone curve needs.

use crate::params::Curve;

/// Samples in a baked LUT. Matches the 256-wide LUT texture the shader reads.
pub const LUT_SIZE: usize = 256;

impl Curve {
    /// Evaluate the curve at `x`, clamped to 0..1.
    pub fn sample(&self, x: f32) -> f32 {
        let pts = self.sorted();
        if pts.len() < 2 {
            return x.clamp(0.0, 1.0);
        }
        let x = x.clamp(0.0, 1.0);

        // Outside the control range the curve holds its endpoint, matching what
        // the user sees drawn.
        if x <= pts[0][0] {
            return pts[0][1].clamp(0.0, 1.0);
        }
        if x >= pts[pts.len() - 1][0] {
            return pts[pts.len() - 1][1].clamp(0.0, 1.0);
        }

        let tangents = monotone_tangents(&pts);
        let i = pts
            .windows(2)
            .position(|w| x >= w[0][0] && x <= w[1][0])
            .unwrap_or(0);

        let (x0, y0) = (pts[i][0], pts[i][1]);
        let (x1, y1) = (pts[i + 1][0], pts[i + 1][1]);
        let h = x1 - x0;
        if h.abs() < f32::EPSILON {
            return y1.clamp(0.0, 1.0);
        }
        let t = (x - x0) / h;
        let t2 = t * t;
        let t3 = t2 * t;

        // Hermite basis.
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;

        let y = h00 * y0 + h10 * h * tangents[i] + h01 * y1 + h11 * h * tangents[i + 1];
        y.clamp(0.0, 1.0)
    }

    /// Bake to a LUT for upload to the GPU.
    pub fn bake(&self) -> [f32; LUT_SIZE] {
        let mut out = [0.0f32; LUT_SIZE];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.sample(i as f32 / (LUT_SIZE - 1) as f32);
        }
        out
    }
}

/// Fritsch–Carlson tangents: secant slopes, then limited so no segment can
/// overshoot.
fn monotone_tangents(pts: &[[f32; 2]]) -> Vec<f32> {
    let n = pts.len();
    let mut secants = vec![0.0f32; n.saturating_sub(1)];
    for i in 0..n - 1 {
        let dx = pts[i + 1][0] - pts[i][0];
        secants[i] = if dx.abs() < f32::EPSILON {
            0.0
        } else {
            (pts[i + 1][1] - pts[i][1]) / dx
        };
    }

    let mut m = vec![0.0f32; n];
    m[0] = secants[0];
    m[n - 1] = secants[n - 2];
    for i in 1..n - 1 {
        // A local extremum gets a flat tangent, which is what stops the curve
        // wandering past the point the user placed.
        m[i] = if secants[i - 1] * secants[i] <= 0.0 {
            0.0
        } else {
            (secants[i - 1] + secants[i]) * 0.5
        };
    }

    // Limit each tangent to three times the adjacent secant. This is the
    // Fritsch-Carlson condition; without it the cubic can still overshoot even
    // with correctly signed tangents.
    for i in 0..n - 1 {
        if secants[i].abs() < f32::EPSILON {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let a = m[i] / secants[i];
        let b = m[i + 1] / secants[i];
        let s = a * a + b * b;
        if s > 9.0 {
            let t = 3.0 / s.sqrt();
            m[i] = t * a * secants[i];
            m[i + 1] = t * b * secants[i];
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(points: &[[f32; 2]]) -> Curve {
        Curve {
            points: points.to_vec(),
        }
    }

    #[test]
    fn the_identity_curve_is_the_identity() {
        let c = Curve::default();
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert!((c.sample(x) - x).abs() < 1e-5, "at {x} got {}", c.sample(x));
        }
    }

    #[test]
    fn a_baked_identity_lut_is_a_ramp() {
        let lut = Curve::default().bake();
        assert!((lut[0] - 0.0).abs() < 1e-6);
        assert!((lut[LUT_SIZE - 1] - 1.0).abs() < 1e-6);
        assert!(lut.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn control_points_are_interpolated_exactly() {
        let c = curve(&[[0.0, 0.0], [0.25, 0.1], [0.75, 0.9], [1.0, 1.0]]);
        for p in &c.points {
            assert!(
                (c.sample(p[0]) - p[1]).abs() < 1e-5,
                "point {p:?} evaluated to {}",
                c.sample(p[0])
            );
        }
    }

    /// The reason this is Fritsch-Carlson and not Catmull-Rom.
    #[test]
    fn a_monotone_curve_never_overshoots() {
        // A hard S-curve: Catmull-Rom bulges above 1.0 and below 0.0 near the
        // steep section, which shows as haloing in a tonal band the user never
        // touched.
        let c = curve(&[[0.0, 0.0], [0.45, 0.05], [0.55, 0.95], [1.0, 1.0]]);
        let mut prev = -1.0f32;
        for i in 0..=1000 {
            let x = i as f32 / 1000.0;
            let y = c.sample(x);
            assert!((0.0..=1.0).contains(&y), "at {x} the curve reached {y}");
            assert!(y >= prev - 1e-6, "at {x} the curve went backwards");
            prev = y;
        }
    }

    #[test]
    fn a_flat_segment_stays_flat() {
        // Two points at the same height must not bow between them.
        let c = curve(&[[0.0, 0.0], [0.3, 0.5], [0.7, 0.5], [1.0, 1.0]]);
        for i in 30..=70 {
            let x = i as f32 / 100.0;
            assert!(
                (c.sample(x) - 0.5).abs() < 1e-4,
                "at {x} the flat section reached {}",
                c.sample(x)
            );
        }
    }

    #[test]
    fn an_inverted_curve_works() {
        let c = curve(&[[0.0, 1.0], [1.0, 0.0]]);
        assert!((c.sample(0.0) - 1.0).abs() < 1e-5);
        assert!((c.sample(1.0) - 0.0).abs() < 1e-5);
        assert!((c.sample(0.5) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn points_dragged_out_of_order_still_evaluate() {
        // The UI can hand us unsorted points mid-drag.
        let a = curve(&[[0.0, 0.0], [0.8, 0.4], [0.3, 0.9], [1.0, 1.0]]);
        let b = curve(&[[0.0, 0.0], [0.3, 0.9], [0.8, 0.4], [1.0, 1.0]]);
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert!((a.sample(x) - b.sample(x)).abs() < 1e-6, "at {x}");
        }
    }

    #[test]
    fn a_degenerate_curve_falls_back_to_identity() {
        assert_eq!(curve(&[]).sample(0.4), 0.4);
        assert_eq!(curve(&[[0.5, 0.2]]).sample(0.4), 0.4);
    }

    #[test]
    fn outside_the_control_range_the_curve_holds_its_endpoints() {
        let c = curve(&[[0.2, 0.3], [0.8, 0.7]]);
        assert!((c.sample(0.0) - 0.3).abs() < 1e-5);
        assert!((c.sample(1.0) - 0.7).abs() < 1e-5);
    }

    #[test]
    fn duplicate_x_values_do_not_divide_by_zero() {
        let c = curve(&[[0.0, 0.0], [0.5, 0.2], [0.5, 0.8], [1.0, 1.0]]);
        for i in 0..=100 {
            let y = c.sample(i as f32 / 100.0);
            assert!(y.is_finite(), "non-finite at {i}");
        }
    }

    #[test]
    fn baking_is_deterministic() {
        let c = curve(&[[0.0, 0.0], [0.4, 0.6], [1.0, 1.0]]);
        assert_eq!(c.bake(), c.bake());
    }
}
