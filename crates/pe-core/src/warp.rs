//! A lattice of displacements — the thing Resolve's Colour Warper edits.
//!
//! Every one of the warper's views is the same object seen through different
//! axes: a grid laid over a two-dimensional slice of colour, with a control
//! point at each intersection that can be dragged. Hue against saturation,
//! chroma against luma — the axes change, the model does not. That is why
//! there is one type here and not three, and it is what makes the views
//! switchable by an icon rather than being three separate tools.
//!
//! What is stored is the *displacement* at each vertex, not the position.
//! Those differ in the one way that matters: an untouched warp is all zeros,
//! so it is obviously identity, it costs nothing to compare, and resizing the
//! grid cannot accidentally move the picture.

use serde::{Deserialize, Serialize};

/// The largest grid either axis may have.
///
/// Bounded because the lattice travels to the GPU inside the curve LUT, whose
/// rows are 256 values wide — a 16 by 16 grid is 256 vertices, which is
/// exactly one row per component. It is also far past the point where another
/// control point helps: Resolve offers up to 16 and defaults to 6.
pub const MAX_DIVISIONS: u32 = 16;

/// The grid Resolve opens with, on both axes.
pub const DEFAULT_DIVISIONS: u32 = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    cols: u32,
    rows: u32,
    /// Row-major, `cols * rows` of them. Each is a displacement in the two
    /// axes of whichever view owns this warp, measured in the same units the
    /// axes are drawn in: a full turn of hue is 1.0, and so is the whole of
    /// saturation, chroma or luma.
    offsets: Vec<[f32; 2]>,
}

impl Default for Warp {
    fn default() -> Self {
        Self::identity(DEFAULT_DIVISIONS, DEFAULT_DIVISIONS)
    }
}

impl Warp {
    pub fn identity(cols: u32, rows: u32) -> Self {
        let cols = cols.clamp(2, MAX_DIVISIONS);
        let rows = rows.clamp(2, MAX_DIVISIONS);
        Self {
            cols,
            rows,
            offsets: vec![[0.0, 0.0]; (cols * rows) as usize],
        }
    }

    pub fn cols(&self) -> u32 {
        self.cols
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn offsets(&self) -> &[[f32; 2]] {
        &self.offsets
    }

    /// Whether this warp leaves the picture alone.
    ///
    /// Exactly zero rather than nearly: a vertex the user dragged and put back
    /// should read as untouched, and drags land on exact values because the
    /// widget writes the position it computed, not an accumulated delta.
    pub fn is_identity(&self) -> bool {
        self.offsets.iter().all(|o| o == &[0.0, 0.0])
    }

    fn index(&self, col: u32, row: u32) -> Option<usize> {
        (col < self.cols && row < self.rows).then(|| (row * self.cols + col) as usize)
    }

    pub fn at(&self, col: u32, row: u32) -> [f32; 2] {
        self.index(col, row)
            .map(|i| self.offsets[i])
            .unwrap_or([0.0, 0.0])
    }

    pub fn set(&mut self, col: u32, row: u32, offset: [f32; 2]) {
        if let Some(i) = self.index(col, row) {
            self.offsets[i] = offset;
        }
    }

    pub fn clear(&mut self) {
        self.offsets.fill([0.0, 0.0]);
    }

    /// Sample the lattice at a point in `0..1` on each axis.
    ///
    /// `wrap` says whether the first axis is a circle. Hue is: the vertex at
    /// the far right of the grid is the same vertex as the one at the far
    /// left, and treating it as an edge would leave a seam at red that no
    /// amount of dragging could smooth out. Chroma is not — its two ends are
    /// grey and full colour, which are as far apart as two colours get, and
    /// wrapping them would drag one into the other.
    ///
    /// The second axis always clamps. Saturation, chroma and luma all have
    /// ends rather than a far side.
    ///
    /// Mirrors the interpolation in `shaders/effects/colour_warper.wgsl`. If
    /// one changes, so must the other; there is a test that compares them.
    pub fn sample(&self, u: f32, v: f32, wrap: bool) -> [f32; 2] {
        let (c0, c1, tx) = if wrap {
            let fx = u.rem_euclid(1.0) * self.cols as f32;
            let x0 = fx.floor() as i64;
            (
                x0.rem_euclid(self.cols as i64) as u32,
                (x0 + 1).rem_euclid(self.cols as i64) as u32,
                fx - x0 as f32,
            )
        } else {
            let fx = u.clamp(0.0, 1.0) * (self.cols - 1) as f32;
            let x0 = (fx.floor() as u32).min(self.cols - 1);
            (x0, (x0 + 1).min(self.cols - 1), fx - x0 as f32)
        };

        let fy = v.clamp(0.0, 1.0) * (self.rows - 1) as f32;
        let y0 = (fy.floor() as u32).min(self.rows - 1);
        let y1 = (y0 + 1).min(self.rows - 1);
        let ty = fy - y0 as f32;

        let mix =
            |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let top = mix(self.at(c0, y0), self.at(c1, y0), tx);
        let bottom = mix(self.at(c0, y1), self.at(c1, y1), tx);
        mix(top, bottom, ty)
    }

    /// Change the grid, keeping the shape that has been drawn on it.
    ///
    /// Resampled rather than cleared. A colourist who has spent a minute
    /// pulling a grid around and then wants it finer is asking for more
    /// control points, not for their work back — and "changing the resolution
    /// discards the edit" is the kind of behaviour that teaches people never
    /// to touch a control.
    pub fn resize(&mut self, cols: u32, rows: u32) {
        let cols = cols.clamp(2, MAX_DIVISIONS);
        let rows = rows.clamp(2, MAX_DIVISIONS);
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let mut next = Vec::with_capacity((cols * rows) as usize);
        for r in 0..rows {
            for c in 0..cols {
                let u = c as f32 / cols as f32;
                let v = if rows > 1 {
                    r as f32 / (rows - 1) as f32
                } else {
                    0.0
                };
                // Resampled as a circle. A hue lattice is the case that would
                // notice the difference, and reading a chroma lattice this way
                // only ever mixes its two end columns, which are already the
                // ones being replaced.
                next.push(self.sample(u, v, true));
            }
        }
        self.cols = cols;
        self.rows = rows;
        self.offsets = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_warp_is_identity_and_samples_to_nothing() {
        let w = Warp::default();
        assert!(w.is_identity());
        assert_eq!(w.sample(0.3, 0.7, true), [0.0, 0.0]);
    }

    #[test]
    fn a_moved_vertex_is_read_back_where_it_was_put() {
        let mut w = Warp::identity(6, 6);
        w.set(2, 3, [0.25, -0.1]);
        assert_eq!(w.at(2, 3), [0.25, -0.1]);
        assert!(!w.is_identity());
        w.set(2, 3, [0.0, 0.0]);
        assert!(w.is_identity(), "putting it back should read as untouched");
    }

    /// Hue is a circle. The vertex at the far right of the grid is the same
    /// vertex as the one at the far left, and a warp that treated it as an
    /// edge would leave a seam at red that no amount of dragging could smooth.
    #[test]
    fn the_first_axis_wraps() {
        let mut w = Warp::identity(4, 2);
        w.set(0, 0, [0.5, 0.0]);
        w.set(0, 1, [0.5, 0.0]);
        // Just before the wrap point, most of the way back to vertex zero.
        let near_end = w.sample(0.99, 0.5, true);
        assert!(
            near_end[0] > 0.4,
            "the far edge did not reach the first vertex: {near_end:?}"
        );
    }

    /// The second axis does not. Sampling past the top must hold the last row
    /// rather than folding back to the first.
    #[test]
    fn the_second_axis_clamps() {
        let mut w = Warp::identity(4, 3);
        for c in 0..4 {
            w.set(c, 2, [0.0, 0.3]);
        }
        assert_eq!(w.sample(0.5, 1.0, true)[1], 0.3);
        assert_eq!(w.sample(0.5, 2.0, true)[1], 0.3, "past the end should hold");
    }

    /// A finer grid keeps the shape that was drawn on the coarse one.
    #[test]
    fn resizing_resamples_rather_than_discarding() {
        let mut w = Warp::identity(4, 4);
        for c in 0..4 {
            w.set(c, 0, [0.0, 0.4]);
        }
        let before = w.sample(0.5, 0.0, true);
        w.resize(8, 8);
        assert_eq!(w.cols(), 8);
        assert!(
            (w.sample(0.5, 0.0, true)[1] - before[1]).abs() < 0.05,
            "the shape was lost: {:?} became {:?}",
            before,
            w.sample(0.5, 0.0, true)
        );
    }

    /// Chroma is not a circle. Its two ends are grey and full colour, which
    /// are as far apart as two colours get — wrapping them would drag one
    /// into the other, so a pull on the grey end must not reach the vivid one.
    #[test]
    fn the_first_axis_clamps_when_it_is_told_not_to_wrap() {
        let mut w = Warp::identity(4, 2);
        w.set(0, 0, [0.0, 0.5]);
        w.set(0, 1, [0.0, 0.5]);
        assert!(
            w.sample(0.99, 0.5, false)[1].abs() < 1e-6,
            "the far end picked up the near end's displacement"
        );
        assert!(
            w.sample(0.99, 0.5, true)[1] > 0.4,
            "and wrapping should still reach it"
        );
    }

    #[test]
    fn a_grid_cannot_be_made_degenerate_or_enormous() {
        let w = Warp::identity(1, 400);
        assert!(w.cols() >= 2 && w.rows() <= MAX_DIVISIONS);
        assert_eq!(w.offsets().len(), (w.cols() * w.rows()) as usize);
    }
}
