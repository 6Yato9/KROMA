//! Minimal 3x3 matrix maths.
//!
//! Deliberately `f64`. These matrices are derived once from chromaticity
//! coordinates and then either used by the CPU reference path (where the extra
//! precision is free and makes the golden tests meaningful) or downconverted to
//! `f32` exactly once on the way to the GPU. Deriving them in `f32` loses
//! roughly three decimal digits through the inversion, which is enough to show
//! up as a visible shift after a round trip through a wide gamut.

/// A row-major 3x3 matrix: `m[row][col]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3(pub [[f64; 3]; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// Build from three *columns*. Handy when assembling from primaries, which
    /// are naturally column vectors in XYZ.
    pub fn from_cols(c0: [f64; 3], c1: [f64; 3], c2: [f64; 3]) -> Mat3 {
        Mat3([
            [c0[0], c1[0], c2[0]],
            [c0[1], c1[1], c2[1]],
            [c0[2], c1[2], c2[2]],
        ])
    }

    pub fn diag(d: [f64; 3]) -> Mat3 {
        Mat3([[d[0], 0.0, 0.0], [0.0, d[1], 0.0], [0.0, 0.0, d[2]]])
    }

    #[allow(
        clippy::needless_range_loop,
        reason = "row/column indices are the clearest form for a matrix product"
    )]
    pub fn mul(&self, rhs: &Mat3) -> Mat3 {
        let mut out = [[0.0f64; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                out[r][c] = (0..3).map(|k| self.0[r][k] * rhs.0[k][c]).sum();
            }
        }
        Mat3(out)
    }

    pub fn mul_vec(&self, v: [f64; 3]) -> [f64; 3] {
        [
            self.0[0][0] * v[0] + self.0[0][1] * v[1] + self.0[0][2] * v[2],
            self.0[1][0] * v[0] + self.0[1][1] * v[1] + self.0[1][2] * v[2],
            self.0[2][0] * v[0] + self.0[2][1] * v[1] + self.0[2][2] * v[2],
        ]
    }

    pub fn det(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    pub fn inverse(&self) -> Option<Mat3> {
        let d = self.det();
        if d.abs() < 1e-12 {
            return None;
        }
        let m = &self.0;
        let inv_d = 1.0 / d;
        Some(Mat3([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_d,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_d,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_d,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_d,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_d,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_d,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_d,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_d,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_d,
            ],
        ]))
    }

    pub fn transpose(&self) -> Mat3 {
        let m = &self.0;
        Mat3([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }

    /// Column-major with 16-byte row padding, which is what WGSL's `mat3x3<f32>`
    /// expects in a uniform buffer. Getting this layout wrong is the classic
    /// "my colours are subtly rotated" bug, so it lives in one place.
    pub fn to_wgsl_mat3(&self) -> [[f32; 4]; 3] {
        let m = &self.0;
        [
            [m[0][0] as f32, m[1][0] as f32, m[2][0] as f32, 0.0],
            [m[0][1] as f32, m[1][1] as f32, m[2][1] as f32, 0.0],
            [m[0][2] as f32, m[1][2] as f32, m[2][2] as f32, 0.0],
        ]
    }

    pub fn approx_eq(&self, other: &Mat3, eps: f64) -> bool {
        (0..3).all(|r| (0..3).all(|c| (self.0[r][c] - other.0[r][c]).abs() <= eps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_multiplicative_unit() {
        let m = Mat3([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 10.0]]);
        assert!(m.mul(&Mat3::IDENTITY).approx_eq(&m, 1e-15));
        assert!(Mat3::IDENTITY.mul(&m).approx_eq(&m, 1e-15));
    }

    #[test]
    fn inverse_round_trips() {
        let m = Mat3([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 10.0]]);
        let inv = m.inverse().expect("matrix is invertible");
        assert!(m.mul(&inv).approx_eq(&Mat3::IDENTITY, 1e-12));
        assert!(inv.mul(&m).approx_eq(&Mat3::IDENTITY, 1e-12));
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let m = Mat3([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [7.0, 8.0, 10.0]]);
        assert!(m.inverse().is_none());
    }

    #[test]
    fn mul_vec_matches_manual_expansion() {
        let m = Mat3([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        assert_eq!(m.mul_vec([1.0, 0.0, 0.0]), [1.0, 4.0, 7.0]);
        assert_eq!(m.mul_vec([0.0, 1.0, 0.0]), [2.0, 5.0, 8.0]);
        assert_eq!(m.mul_vec([1.0, 1.0, 1.0]), [6.0, 15.0, 24.0]);
    }
}
