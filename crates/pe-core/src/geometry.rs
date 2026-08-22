//! Crop, straighten and output size.
//!
//! Geometry is not an effect and deliberately does not live in the stack. Every
//! row in the stack takes an image and returns an image of the same size;
//! cropping changes the size, and letting one row do that would mean every
//! later row — and the cache, and the export path — had to cope with the frame
//! changing shape underneath it.
//!
//! It sits *before* the stack instead, which also settles a question that
//! otherwise has no good answer: a vignette darkens the corners of the
//! photograph the user is making, not the corners of the sensor. Because the
//! crop happens first, "the frame" already means the cropped frame everywhere
//! downstream, and nothing else had to learn about cropping at all.
//!
//! The model is Lightroom's. The whole image is rotated about its centre into
//! what this module calls **straightened space**, and the crop is an
//! axis-aligned rectangle in there. That is what makes the crop overlay
//! tractable: in crop mode the viewer shows straightened space directly, so the
//! rectangle the user drags is axis-aligned on screen no matter what the angle
//! is.

use serde::{Deserialize, Serialize};

/// An affine map from one uv space to another.
///
/// Everything geometry does — crop, rotate, quarter-turn, flip, and the
/// preview's own zoom and pan — is affine, so all of it composes into a single
/// map and the source is sampled exactly once. A separate pass per operation
/// would resample the picture two or three times over and lose a little detail
/// each time for no reason.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Where a unit step along the output's x takes you in the input.
    pub x_axis: [f32; 2],
    pub y_axis: [f32; 2],
    /// The input point that output (0, 0) reads from.
    pub origin: [f32; 2],
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        x_axis: [1.0, 0.0],
        y_axis: [0.0, 1.0],
        origin: [0.0, 0.0],
    };

    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.origin[0] + self.x_axis[0] * p[0] + self.y_axis[0] * p[1],
            self.origin[1] + self.x_axis[1] * p[0] + self.y_axis[1] * p[1],
        ]
    }

    /// Apply `self`, then `outer`.
    pub fn then(self, outer: Affine) -> Affine {
        let o = outer.apply(self.origin);
        // The linear parts compose without the translation, which is why these
        // are differences from the mapped origin rather than mapped points.
        let x = outer.apply([
            self.origin[0] + self.x_axis[0],
            self.origin[1] + self.x_axis[1],
        ]);
        let y = outer.apply([
            self.origin[0] + self.y_axis[0],
            self.origin[1] + self.y_axis[1],
        ]);
        Affine {
            x_axis: [x[0] - o[0], x[1] - o[1]],
            y_axis: [y[0] - o[0], y[1] - o[1]],
            origin: o,
        }
    }

    /// The map that undoes this one, or `None` if it collapses the plane.
    ///
    /// Needed to answer "where is the crop inside the frame the crop tool is
    /// showing" — both are maps *out* of their own uv, so relating them means
    /// running one of them backwards.
    pub fn invert(self) -> Option<Affine> {
        let det = self.x_axis[0] * self.y_axis[1] - self.y_axis[0] * self.x_axis[1];
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let x_axis = [self.y_axis[1] * inv, -self.x_axis[1] * inv];
        let y_axis = [-self.y_axis[0] * inv, self.x_axis[0] * inv];
        Some(Affine {
            x_axis,
            y_axis,
            origin: [
                -(x_axis[0] * self.origin[0] + y_axis[0] * self.origin[1]),
                -(x_axis[1] * self.origin[0] + y_axis[1] * self.origin[1]),
            ],
        })
    }

    /// Reading order for the GPU: two columns then the translation.
    pub fn to_array(self) -> [f32; 6] {
        [
            self.x_axis[0],
            self.x_axis[1],
            self.y_axis[0],
            self.y_axis[1],
            self.origin[0],
            self.origin[1],
        ]
    }
}

/// What the crop's proportions are pinned to while the user drags it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AspectLock {
    #[default]
    Free,
    /// The source image's own proportions.
    Original,
    /// A fixed ratio, width to height.
    Ratio { w: f32, h: f32 },
}

impl AspectLock {
    /// The ratio to hold, given what the source is. `None` means free.
    pub fn ratio(&self, source_w: u32, source_h: u32) -> Option<f32> {
        match *self {
            AspectLock::Free => None,
            AspectLock::Original => Some(source_w.max(1) as f32 / source_h.max(1) as f32),
            AspectLock::Ratio { w, h } => {
                let r = w / h.max(1e-6);
                r.is_finite().then_some(r).filter(|v| *v > 0.0)
            }
        }
    }
}

/// How big the exported file should be.
///
/// Separate from the crop because they answer different questions: the crop
/// decides *what* is in the picture, this decides how many pixels it is
/// delivered in. Resampling on the way out is the last thing that happens, so
/// grain and sharpening are rendered at full resolution first — which is the
/// only order that looks right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Resize {
    /// Whatever the crop works out to.
    #[default]
    Native,
    /// Fit the long edge to this many pixels, never enlarging.
    LongEdge { pixels: u32 },
}

impl Resize {
    /// Apply this to a native output size.
    pub fn apply(&self, w: u32, h: u32) -> (u32, u32) {
        match *self {
            Resize::Native => (w.max(1), h.max(1)),
            Resize::LongEdge { pixels } => {
                let long = w.max(h).max(1);
                // Never upscale. Enlarging invents detail, and a user asking
                // for "2048 on the long edge" from a 1600px file wants their
                // file, not a soft version of it.
                if pixels == 0 || pixels >= long {
                    return (w.max(1), h.max(1));
                }
                let k = pixels as f32 / long as f32;
                (
                    ((w as f32 * k).round() as u32).max(1),
                    ((h as f32 * k).round() as u32).max(1),
                )
            }
        }
    }
}

/// Crop, straighten, flip and quarter-turn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geometry {
    /// Centre of the crop in straightened space, as an offset from the middle
    /// of the source in units of the source's own width and height. (0, 0) is
    /// dead centre.
    pub centre: [f32; 2],
    /// Size of the crop as a fraction of the source's width and height.
    pub size: [f32; 2],
    /// Straightening angle in degrees. Positive turns the picture
    /// anticlockwise, which is the direction Lightroom's Angle slider moves
    /// and therefore the one users' hands already know.
    pub angle: f32,
    /// Quarter-turns clockwise, applied after straightening. 0 to 3.
    pub turns: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    pub aspect: AspectLock,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            centre: [0.0, 0.0],
            size: [1.0, 1.0],
            angle: 0.0,
            turns: 0,
            flip_h: false,
            flip_v: false,
            aspect: AspectLock::Free,
        }
    }
}

impl Geometry {
    /// True when this does nothing at all, so the renderer can take the plain
    /// path and the crop panel can show "Original".
    pub fn is_identity(&self) -> bool {
        self.centre == [0.0, 0.0]
            && self.size == [1.0, 1.0]
            && self.angle == 0.0
            && self.turns.is_multiple_of(4)
            && !self.flip_h
            && !self.flip_v
    }

    /// Pixel size of the result.
    ///
    /// A quarter-turn swaps the two, which is the whole reason this is not
    /// just the crop size: a portrait crop turned on its side is a landscape
    /// file.
    pub fn output_size(&self, source_w: u32, source_h: u32) -> (u32, u32) {
        let w = (self.size[0].abs() * source_w as f32).round().max(1.0) as u32;
        let h = (self.size[1].abs() * source_h as f32).round().max(1.0) as u32;
        if self.turns % 2 == 1 { (h, w) } else { (w, h) }
    }

    /// The map from output uv to source uv.
    ///
    /// Read it bottom-up: an output pixel is placed in the output's own frame,
    /// un-turned and un-flipped into the crop's frame, offset to where the crop
    /// sits in straightened space, rotated back into the source, and finally
    /// normalised.
    pub fn sampling(&self, source_w: u32, source_h: u32) -> Affine {
        let (sw, sh) = (source_w.max(1) as f32, source_h.max(1) as f32);
        let (ow, oh) = self.output_size(source_w, source_h);
        let (ow, oh) = (ow as f32, oh as f32);

        // Output uv to output pixels, centred.
        let mut x = [ow, 0.0];
        let mut y = [0.0, oh];
        let mut o = [-ow * 0.5, -oh * 0.5];

        // Undo the flips first, because they act on the *output* — Flip
        // Horizontal mirrors what is on screen left to right whatever the turn
        // is. Flipping the crop instead and then turning it would make the
        // button mirror the picture vertically after a quarter turn, which is
        // correct by some reading and surprising by every other.
        if self.flip_h {
            x[0] = -x[0];
            y[0] = -y[0];
            o[0] = -o[0];
        }
        if self.flip_v {
            x[1] = -x[1];
            y[1] = -y[1];
            o[1] = -o[1];
        }

        // Undo the quarter turn. Odd turns swap the axes, which is what puts
        // the output's long edge back along the crop's long edge.
        let quarter = |v: [f32; 2], t: u8| -> [f32; 2] {
            match t % 4 {
                0 => v,
                1 => [v[1], -v[0]],
                2 => [-v[0], -v[1]],
                _ => [-v[1], v[0]],
            }
        };
        x = quarter(x, self.turns);
        y = quarter(y, self.turns);
        o = quarter(o, self.turns);

        // Into straightened space, relative to the middle of the source.
        o[0] += self.centre[0] * sw;
        o[1] += self.centre[1] * sh;

        // Straightened space back into the source. Sampling runs backwards:
        // to turn the picture anticlockwise, the sample point turns clockwise,
        // and screen y points down, so this is the textbook matrix with the
        // angle already negated twice over — which is to say, as written.
        let r = self.angle.to_radians();
        let (s, c) = (r.sin(), r.cos());
        let rot = |v: [f32; 2]| [v[0] * c - v[1] * s, v[0] * s + v[1] * c];
        x = rot(x);
        y = rot(y);
        o = rot(o);

        // Source pixels to source uv.
        Affine {
            x_axis: [x[0] / sw, x[1] / sh],
            y_axis: [y[0] / sw, y[1] / sh],
            origin: [0.5 + o[0] / sw, 0.5 + o[1] / sh],
        }
    }

    /// The frame the crop tool shows: the whole source, straightened.
    ///
    /// Big enough to hold the rotated picture, so the blank corners are
    /// visible and the user can see exactly what straightening is costing
    /// them. Keeps the turns and flips, so the crop rectangle stays
    /// axis-aligned on screen whatever else is set.
    pub fn enclosing(&self, source_w: u32, source_h: u32) -> Geometry {
        let (sw, sh) = (source_w.max(1) as f32, source_h.max(1) as f32);
        let r = self.angle.to_radians();
        let (s, c) = (r.sin().abs(), r.cos().abs());
        Geometry {
            centre: [0.0, 0.0],
            size: [(sw * c + sh * s) / sw, (sw * s + sh * c) / sh],
            aspect: AspectLock::Free,
            ..*self
        }
    }

    /// Where this crop sits inside `frame`, as min x, min y, max x, max y in
    /// that frame's uv.
    ///
    /// Both are maps out of their own output uv into the source, so the two
    /// are related by running `frame`'s backwards. The result is axis-aligned
    /// whenever the two share an angle and a turn, which is the only way this
    /// is ever called.
    pub fn crop_uv_in(&self, frame: &Geometry, source_w: u32, source_h: u32) -> [f32; 4] {
        let Some(back) = frame.sampling(source_w, source_h).invert() else {
            return [0.0, 0.0, 1.0, 1.0];
        };
        let mine = self.sampling(source_w, source_h);
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            let p = back.apply(mine.apply(corner));
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        [lo[0], lo[1], hi[0], hi[1]]
    }

    /// Move the crop to a rectangle given in `frame`'s uv. The inverse of
    /// [`Geometry::crop_uv_in`], and what the crop overlay writes back.
    pub fn set_crop_uv_in(
        &mut self,
        frame: &Geometry,
        source_w: u32,
        source_h: u32,
        rect: [f32; 4],
    ) {
        let (sw, sh) = (source_w.max(1) as f32, source_h.max(1) as f32);
        let map = frame.sampling(source_w, source_h);
        // Undo the straightening to get back to the space the crop is stored
        // in. The rectangle is axis-aligned there too, so its extent is just
        // the extent of its corners.
        let r = self.angle.to_radians();
        let (s, c) = (r.sin(), r.cos());
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for corner in [
            [rect[0], rect[1]],
            [rect[2], rect[1]],
            [rect[2], rect[3]],
            [rect[0], rect[3]],
        ] {
            let uv = map.apply(corner);
            let (px, py) = ((uv[0] - 0.5) * sw, (uv[1] - 0.5) * sh);
            let t = [px * c + py * s, -px * s + py * c];
            lo = [lo[0].min(t[0]), lo[1].min(t[1])];
            hi = [hi[0].max(t[0]), hi[1].max(t[1])];
        }
        self.centre = [(lo[0] + hi[0]) * 0.5 / sw, (lo[1] + hi[1]) * 0.5 / sh];
        self.size = [
            ((hi[0] - lo[0]) / sw).max(1e-3),
            ((hi[1] - lo[1]) / sh).max(1e-3),
        ];
    }

    /// The four corners of the crop, in source uv.
    ///
    /// Anything outside 0..1 is a corner of the frame that has no photograph
    /// behind it.
    pub fn corners(&self, source_w: u32, source_h: u32) -> [[f32; 2]; 4] {
        let m = self.sampling(source_w, source_h);
        [
            m.apply([0.0, 0.0]),
            m.apply([1.0, 0.0]),
            m.apply([1.0, 1.0]),
            m.apply([0.0, 1.0]),
        ]
    }

    /// Whether every pixel of the output has real image behind it.
    pub fn fits(&self, source_w: u32, source_h: u32) -> bool {
        self.corners(source_w, source_h)
            .iter()
            .all(|c| (-1e-4..=1.0 + 1e-4).contains(&c[0]) && (-1e-4..=1.0 + 1e-4).contains(&c[1]))
    }

    /// Shrink the crop about its own centre until it fits inside the source.
    ///
    /// Straightening a photograph always costs some of its edges; the question
    /// is only whether the tool takes them or leaves the user with blank
    /// corners. Bisection rather than a closed form because the closed form
    /// has a different case for each aspect and each off-centre position, and
    /// this is exact to within a ten-thousandth of the frame in 24 steps.
    pub fn shrink_to_fit(&mut self, source_w: u32, source_h: u32) {
        if self.fits(source_w, source_h) {
            return;
        }
        let full = self.size;
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            self.size = [full[0] * mid, full[1] * mid];
            if self.fits(source_w, source_h) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // A hair inside rather than exactly on the boundary. The renderer
        // blanks samples that fall outside the source, and a corner sitting
        // within a rounding error of the edge would blank a pixel of it.
        let safe = lo * 0.999;
        self.size = [full[0] * safe, full[1] * safe];
    }

    /// Slide the crop back inside the source, keeping its size.
    ///
    /// The counterpart to [`Self::shrink_to_fit`], and the distinction is the
    /// whole point: *straightening* a crop has to cost some of its edges,
    /// because the rotated rectangle genuinely does not fit any more. *Moving*
    /// one does not — the rectangle is the same rectangle, it is simply
    /// somewhere it cannot be. Shrinking it there means dragging Position
    /// towards an edge quietly zooms in, which is a control changing another
    /// control's value behind the user's back.
    ///
    /// Bisected from a position known to be good, for the same reason the
    /// shrink is: the closed form has a case for every aspect, angle and
    /// quadrant, and this is exact to within a ten-thousandth of the frame.
    pub fn slide_to_fit(&mut self, from: [f32; 2], source_w: u32, source_h: u32) {
        if self.fits(source_w, source_h) {
            return;
        }
        let want = self.centre;
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            self.centre = [
                from[0] + (want[0] - from[0]) * mid,
                from[1] + (want[1] - from[1]) * mid,
            ];
            if self.fits(source_w, source_h) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let safe = lo * 0.999;
        self.centre = [
            from[0] + (want[0] - from[0]) * safe,
            from[1] + (want[1] - from[1]) * safe,
        ];
    }

    /// Re-shape the crop to the locked aspect, keeping its centre and never
    /// growing it.
    pub fn apply_aspect(&mut self, source_w: u32, source_h: u32) {
        let Some(ratio) = self.aspect.ratio(source_w, source_h) else {
            return;
        };
        // The aspect is a property of the picture, so it is measured in
        // pixels; the sizes are fractions of two different dimensions.
        let (sw, sh) = (source_w.max(1) as f32, source_h.max(1) as f32);
        let (w_px, h_px) = (self.size[0] * sw, self.size[1] * sh);
        let (w_px, h_px) = if w_px / h_px.max(1e-6) > ratio {
            (h_px * ratio, h_px)
        } else {
            (w_px, w_px / ratio)
        };
        self.size = [w_px / sw, h_px / sh];
    }
}

#[cfg(test)]
mod tests {

    /// Moving a crop must not resize it.
    ///
    /// Shrinking it when it runs off the edge made the Position control
    /// quietly change the Zoom readout, because Zoom *is* the reciprocal of
    /// the size — one control writing another's value behind the user's back.
    #[test]
    fn sliding_a_crop_off_the_edge_keeps_its_size() {
        let mut g = Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        };
        let before = g.size;
        let from = g.centre;
        g.centre = [0.9, 0.0];
        g.slide_to_fit(from, 4000, 3000);

        assert_eq!(g.size, before, "the crop was resized by a move");
        assert!(g.fits(4000, 3000), "it was left hanging off the edge");
        // It went as far as it could, which is up against the right edge.
        assert!(g.centre[0] > 0.2, "it barely moved: {:?}", g.centre);
    }

    /// And a move that was always legal is left exactly alone.
    #[test]
    fn a_move_that_fits_is_not_touched() {
        let mut g = Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        };
        g.centre = [0.1, -0.05];
        g.slide_to_fit([0.0, 0.0], 4000, 3000);
        assert_eq!(g.centre, [0.1, -0.05]);
    }

    /// Straightening still costs edges. The two operations answer different
    /// questions and this pins the difference: a rotated rectangle genuinely
    /// does not fit any more, so that one shrinks.
    #[test]
    fn straightening_still_shrinks_rather_than_sliding() {
        let mut g = Geometry {
            angle: 8.0,
            ..Default::default()
        };
        g.shrink_to_fit(4000, 3000);
        assert!(
            g.size[0] < 1.0,
            "a straightened crop should have been cut in"
        );
        assert!(g.fits(4000, 3000));
    }
    use super::*;

    const W: u32 = 400;
    const H: u32 = 300;

    fn close(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
        (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
    }

    /// The one that matters most. A document with no crop must read the source
    /// pixel for pixel, or opening a file and saving it would resample it.
    #[test]
    fn the_default_geometry_is_the_identity_map() {
        let g = Geometry::default();
        assert!(g.is_identity());
        assert_eq!(g.output_size(W, H), (W, H));
        let m = g.sampling(W, H);
        for p in [[0.0, 0.0], [1.0, 0.0], [0.5, 0.5], [1.0, 1.0]] {
            assert!(
                close(m.apply(p), p, 1e-5),
                "{p:?} mapped to {:?}",
                m.apply(p)
            );
        }
    }

    #[test]
    fn a_crop_selects_the_rectangle_it_names() {
        // The right half of the frame.
        let g = Geometry {
            centre: [0.25, 0.0],
            size: [0.5, 1.0],
            ..Default::default()
        };
        assert_eq!(g.output_size(W, H), (200, 300));
        let m = g.sampling(W, H);
        assert!(close(m.apply([0.0, 0.0]), [0.5, 0.0], 1e-5));
        assert!(close(m.apply([1.0, 1.0]), [1.0, 1.0], 1e-5));
        assert!(g.fits(W, H));
    }

    /// A quarter-turn has to swap the output's dimensions, not just rotate the
    /// samples — otherwise a portrait crop turned sideways would be squashed
    /// into a portrait file.
    #[test]
    fn a_quarter_turn_swaps_the_output_dimensions() {
        let g = Geometry {
            turns: 1,
            ..Default::default()
        };
        assert_eq!(g.output_size(W, H), (H, W));
        assert!(!g.is_identity());
    }

    /// Pins the direction of rotation. Turning clockwise takes the top-left of
    /// the picture to the top-right of the result.
    #[test]
    fn one_turn_clockwise_moves_the_top_left_corner_to_the_top_right() {
        let g = Geometry {
            turns: 1,
            ..Default::default()
        };
        let m = g.sampling(W, H);
        assert!(
            close(m.apply([1.0, 0.0]), [0.0, 0.0], 1e-5),
            "output top-right should read the source top-left, got {:?}",
            m.apply([1.0, 0.0])
        );
    }

    #[test]
    fn four_quarter_turns_are_no_turn_at_all() {
        let g = Geometry {
            turns: 4,
            ..Default::default()
        };
        let m = g.sampling(W, H);
        assert!(close(m.apply([0.25, 0.75]), [0.25, 0.75], 1e-5));
    }

    #[test]
    fn a_horizontal_flip_mirrors_left_and_right_only() {
        let g = Geometry {
            flip_h: true,
            ..Default::default()
        };
        let m = g.sampling(W, H);
        assert!(close(m.apply([0.0, 0.25]), [1.0, 0.25], 1e-5));
        assert!(close(m.apply([1.0, 0.25]), [0.0, 0.25], 1e-5));
    }

    /// Flip Horizontal mirrors what is on screen, not what is in the crop.
    /// After a quarter turn those are two different axes, and the button has
    /// to mean the one the user can see.
    #[test]
    fn a_flip_mirrors_the_output_even_after_a_turn() {
        let turned = Geometry {
            turns: 1,
            ..Default::default()
        }
        .sampling(W, H);
        let flipped = Geometry {
            turns: 1,
            flip_h: true,
            ..Default::default()
        }
        .sampling(W, H);
        for p in [[0.0, 0.25], [0.3, 0.8], [1.0, 0.5]] {
            let mirrored = turned.apply([1.0 - p[0], p[1]]);
            assert!(
                close(flipped.apply(p), mirrored, 1e-5),
                "{p:?} gave {:?}, not the mirror {mirrored:?}",
                flipped.apply(p)
            );
        }
    }

    #[test]
    fn inverting_an_affine_undoes_it() {
        let g = Geometry {
            centre: [0.1, -0.05],
            size: [0.6, 0.4],
            angle: 12.0,
            turns: 1,
            flip_h: true,
            ..Default::default()
        }
        .sampling(W, H);
        let back = g.invert().expect("invertible");
        for p in [[0.0, 0.0], [1.0, 0.0], [0.37, 0.81], [1.0, 1.0]] {
            assert!(close(back.apply(g.apply(p)), p, 1e-4));
        }
    }

    #[test]
    fn a_degenerate_affine_has_no_inverse() {
        let flat = Affine {
            x_axis: [1.0, 0.0],
            y_axis: [2.0, 0.0],
            origin: [0.0, 0.0],
        };
        assert!(flat.invert().is_none());
    }

    /// The frame the crop tool shows has to hold the whole rotated picture, or
    /// straightening would silently throw away the corners the user is trying
    /// to decide about.
    #[test]
    fn the_enclosing_frame_holds_the_rotated_source() {
        let g = Geometry {
            angle: 30.0,
            ..Default::default()
        };
        let e = g.enclosing(W, H);
        assert_eq!(e.centre, [0.0, 0.0]);
        assert!(e.size[0] > 1.0 && e.size[1] > 1.0, "{:?}", e.size);

        // Every corner of the source is inside it.
        let back = e.sampling(W, H).invert().expect("invertible");
        for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            let p = back.apply(corner);
            assert!(
                (-1e-3..=1.0 + 1e-3).contains(&p[0]) && (-1e-3..=1.0 + 1e-3).contains(&p[1]),
                "source corner {corner:?} landed at {p:?}, outside the frame"
            );
        }
    }

    #[test]
    fn an_unrotated_enclosing_frame_is_just_the_source() {
        let e = Geometry::default().enclosing(W, H);
        assert!(e.is_identity());
    }

    /// What the crop overlay does every frame: read the rectangle out, and
    /// write a dragged one back. A drift either way would make the box crawl
    /// while the pointer stood still.
    #[test]
    fn the_crop_rectangle_round_trips_through_the_frame_it_is_drawn_in() {
        for g in [
            Geometry {
                centre: [0.1, -0.2],
                size: [0.5, 0.3],
                ..Default::default()
            },
            Geometry {
                centre: [-0.05, 0.12],
                size: [0.4, 0.6],
                angle: 9.0,
                ..Default::default()
            },
            Geometry {
                centre: [0.2, 0.0],
                size: [0.3, 0.7],
                angle: -14.0,
                turns: 1,
                flip_h: true,
                ..Default::default()
            },
        ] {
            let frame = g.enclosing(W, H);
            let rect = g.crop_uv_in(&frame, W, H);
            let mut back = g;
            back.set_crop_uv_in(&frame, W, H, rect);
            assert!(
                close(back.centre, g.centre, 1e-3) && close(back.size, g.size, 1e-3),
                "{:?} / {:?} came back as {:?} / {:?}",
                g.centre,
                g.size,
                back.centre,
                back.size
            );
        }
    }

    #[test]
    fn a_full_crop_fills_its_own_frame() {
        let g = Geometry::default();
        let rect = g.crop_uv_in(&g.enclosing(W, H), W, H);
        for (got, want) in rect.iter().zip([0.0, 0.0, 1.0, 1.0]) {
            assert!((got - want).abs() < 1e-4, "{rect:?}");
        }
    }

    /// Straightening pivots on the middle of the picture. If it did not, the
    /// subject would slide across the frame as the angle slider moved, which
    /// makes the control impossible to use.
    #[test]
    fn straightening_leaves_the_centre_where_it_is() {
        for angle in [-30.0, -5.0, 5.0, 30.0] {
            let g = Geometry {
                angle,
                ..Default::default()
            };
            let m = g.sampling(W, H);
            assert!(
                close(m.apply([0.5, 0.5]), [0.5, 0.5], 1e-5),
                "the centre moved at {angle} degrees"
            );
        }
    }

    /// Pins the direction of straightening. Lightroom's Angle slider turns
    /// the picture anticlockwise as it goes positive, and a straightening
    /// control that runs the other way to the one in every user's muscle
    /// memory is worse than no control.
    ///
    /// Turning the picture anticlockwise means the middle of its left edge is
    /// filled from *above* centre in the source.
    #[test]
    fn a_positive_angle_turns_the_picture_anticlockwise() {
        let g = Geometry {
            angle: 15.0,
            ..Default::default()
        };
        let m = g.sampling(W, H);
        let left = m.apply([0.0, 0.5]);
        assert!(
            left[1] < 0.5,
            "the left edge should read from above centre, got {left:?}"
        );
        let right = m.apply([1.0, 0.5]);
        assert!(
            right[1] > 0.5,
            "and the right edge from below it, got {right:?}"
        );
    }

    #[test]
    fn straightening_a_full_frame_leaves_blank_corners_until_it_is_shrunk() {
        let mut g = Geometry {
            angle: 10.0,
            ..Default::default()
        };
        assert!(!g.fits(W, H), "a full-frame crop cannot survive a rotation");
        g.shrink_to_fit(W, H);
        assert!(g.fits(W, H), "shrink_to_fit did not");
        assert!(
            g.size[0] < 1.0 && g.size[0] > 0.5,
            "{:?} is not plausible",
            g.size
        );
    }

    #[test]
    fn shrinking_something_that_already_fits_changes_nothing() {
        let mut g = Geometry {
            size: [0.5, 0.5],
            ..Default::default()
        };
        let before = g;
        g.shrink_to_fit(W, H);
        assert_eq!(g, before);
    }

    #[test]
    fn an_aspect_lock_reshapes_the_crop_without_growing_it() {
        let mut g = Geometry {
            aspect: AspectLock::Ratio { w: 1.0, h: 1.0 },
            ..Default::default()
        };
        g.apply_aspect(W, H);
        let (w, h) = g.output_size(W, H);
        assert_eq!(w, h, "a 1:1 lock should give a square, got {w}x{h}");
        assert!(w <= W && h <= H, "the crop grew to {w}x{h}");
    }

    #[test]
    fn the_original_aspect_lock_is_the_source_shape() {
        assert_eq!(AspectLock::Original.ratio(W, H), Some(4.0 / 3.0));
        assert_eq!(AspectLock::Free.ratio(W, H), None);
    }

    #[test]
    fn composing_affines_applies_them_in_order() {
        let half = Affine {
            x_axis: [0.5, 0.0],
            y_axis: [0.0, 0.5],
            origin: [0.25, 0.25],
        };
        let g = Geometry {
            centre: [0.25, 0.0],
            size: [0.5, 1.0],
            ..Default::default()
        }
        .sampling(W, H);
        // The middle of the half-sized region is the middle of the crop, which
        // is three-quarters of the way across the source.
        let both = half.then(g);
        assert!(close(both.apply([0.5, 0.5]), [0.75, 0.5], 1e-5));
    }

    #[test]
    fn the_identity_affine_composes_to_nothing() {
        let g = Geometry {
            angle: 7.0,
            size: [0.8, 0.6],
            ..Default::default()
        }
        .sampling(W, H);
        // Composing rebuilds the axes by differencing mapped points, so the
        // last bit or two is expected to move; the map is not.
        let composed = g.then(Affine::IDENTITY);
        for p in [[0.0, 0.0], [1.0, 0.0], [0.5, 0.5], [1.0, 1.0]] {
            assert!(close(composed.apply(p), g.apply(p), 1e-6));
        }
    }

    #[test]
    fn resizing_the_long_edge_keeps_the_proportions() {
        assert_eq!(
            Resize::LongEdge { pixels: 1000 }.apply(4000, 3000),
            (1000, 750)
        );
        assert_eq!(Resize::Native.apply(4000, 3000), (4000, 3000));
    }

    /// Enlarging invents detail. A request for a size bigger than the file has
    /// should give the file, not a soft copy of it.
    #[test]
    fn resizing_never_enlarges() {
        assert_eq!(
            Resize::LongEdge { pixels: 4000 }.apply(1600, 1200),
            (1600, 1200)
        );
    }

    #[test]
    fn geometry_round_trips_through_json() {
        let g = Geometry {
            centre: [0.1, -0.2],
            size: [0.7, 0.5],
            angle: 3.5,
            turns: 3,
            flip_h: true,
            flip_v: false,
            aspect: AspectLock::Ratio { w: 16.0, h: 9.0 },
        };
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Geometry>(&json).unwrap(), g);
    }

    /// A document written before geometry existed has no such field, and must
    /// open uncropped rather than failing or collapsing to a zero-size crop.
    #[test]
    fn a_missing_geometry_field_defaults_to_no_crop() {
        let g: Geometry = serde_json::from_str("{}").unwrap();
        assert!(g.is_identity());
    }
}
