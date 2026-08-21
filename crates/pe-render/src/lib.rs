//! The GPU pipeline.
//!
//! At M0 this is the two ends of the diagram — source in, working space, screen
//! out — plus the [`cache`] that M1's interactivity depends on. The effect rows
//! slot in between the ends without either end changing.
//!
//! Three invariants this crate exists to hold:
//!
//! 1. **Every intermediate is `Rgba16Float`.** See [`texture`].
//! 2. **Transfer functions belong to texture formats, gamut rotation belongs to
//!    shaders.** Doing either in the wrong place double-applies it.
//! 3. **Nothing re-renders that has not changed.** See [`cache`].

pub mod cache;
pub mod device;
pub mod effect;
pub mod export;
pub mod readback;
pub mod texture;
pub mod transform;

pub use cache::{RenderContext, RenderPlan, RowFingerprint, StageCache, fingerprint};
pub use device::GpuContext;
pub use effect::{EffectRenderer, Scratch};
pub use export::render_full;
pub use readback::read_rgba8;
pub use texture::padded_bytes_per_row;
pub use texture::{ImageTexture, SOURCE_FORMAT, WORKING_FORMAT};
pub use transform::TransformPass;

/// Which rectangle of the whole frame a pass renders, in frame uv.
///
/// `Region::FULL` is the whole image, and is what export always uses. The
/// interactive preview narrows it as the view zooms in, so that 100% renders
/// the visible pixels at their own resolution instead of magnifying a
/// downscaled preview.
///
/// Anything in a shader that reasons about the *frame* — a vignette's centre,
/// a grain lattice, a halation radius — goes through `frame_uv()` /
/// `frame_to_uv()` in `common.wgsl`, which are driven by this.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub offset: [f32; 2],
    pub size: [f32; 2],
}

impl Region {
    pub const FULL: Region = Region {
        offset: [0.0, 0.0],
        size: [1.0, 1.0],
    };

    pub fn to_array(self) -> [f32; 4] {
        [self.offset[0], self.offset[1], self.size[0], self.size[1]]
    }

    /// The same rectangle as an affine map, so it composes with the crop.
    pub fn to_affine(self) -> pe_core::Affine {
        pe_core::Affine {
            x_axis: [self.size[0], 0.0],
            y_axis: [0.0, self.size[1]],
            origin: self.offset,
        }
    }

    /// A stable key for the stage cache. Panning or zooming must invalidate
    /// every cached stage, because they were rendered for a different
    /// rectangle — quantised so that sub-pixel jitter does not thrash the
    /// cache on every frame.
    pub fn cache_key(self) -> u64 {
        let q = |v: f32| (v * 4096.0).round() as i64 as u64;
        q(self.offset[0])
            ^ q(self.offset[1]).rotate_left(16)
            ^ q(self.size[0]).rotate_left(32)
            ^ q(self.size[1]).rotate_left(48)
    }
}

impl Default for Region {
    fn default() -> Self {
        Region::FULL
    }
}

/// How a transform pass reads its source.
///
/// The map and the out-of-bounds rule travel together because they are only
/// ever meaningful together: blanking matters exactly when the map can point
/// somewhere the photograph is not, which is what straightening does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampling {
    /// Output uv to source uv.
    pub map: pe_core::Affine,
    /// Whether a sample landing outside the source reads as blank rather than
    /// smearing the nearest edge pixel across it.
    pub blank_outside: bool,
}

impl Sampling {
    /// Read the whole source, one to one.
    pub const WHOLE: Sampling = Sampling {
        map: pe_core::Affine::IDENTITY,
        blank_outside: false,
    };

    /// Read what a crop describes, blanking the overhang a straightening angle
    /// leaves behind.
    pub fn of(geometry: &pe_core::Geometry, source_w: u32, source_h: u32) -> Sampling {
        Sampling {
            map: geometry.sampling(source_w, source_h),
            blank_outside: !geometry.fits(source_w, source_h),
        }
    }

    /// Narrow this to a sub-rectangle of its own output — what the preview
    /// does as the view zooms in.
    pub fn within(self, region: Region) -> Sampling {
        Sampling {
            map: region.to_affine().then(self.map),
            ..self
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("could not acquire a GPU device: {0}")]
    NoDevice(String),
    #[error("expected {expected} bytes of pixel data, got {found}")]
    PixelCountMismatch { expected: usize, found: usize },
    #[error("GPU surface error: {0}")]
    Surface(String),
    #[error("could not read a texture back to the CPU: {0}")]
    Readback(String),
}

/// Hash the crop and straighten settings for [`RenderContext::geometry`].
///
/// Serialised rather than hand-hashed for the same reason row fingerprints
/// are: it is all floats, and a hand-rolled hash is the kind of thing that
/// silently stops covering a field the day someone adds one.
pub fn geometry_fingerprint(g: &pe_core::Geometry) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match serde_json::to_string(g) {
        Ok(s) => s.hash(&mut h),
        // Unhashable geometry is a bug, but rendering something stale is worse
        // than rendering something twice.
        Err(_) => u64::MAX.hash(&mut h),
    }
    h.finish()
}

/// Hash the colour-management settings for [`RenderContext::color`].
pub fn color_fingerprint(settings: &pe_core::ColorSettings) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    settings.input.hash(&mut h);
    settings.output.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pe_core::ColorSettings;

    #[test]
    fn colour_settings_hash_distinguishes_output_spaces() {
        let a = ColorSettings {
            input: "sRGB".into(),
            output: "sRGB".into(),
        };
        let b = ColorSettings {
            input: "sRGB".into(),
            output: "Display P3".into(),
        };
        assert_ne!(color_fingerprint(&a), color_fingerprint(&b));
    }

    #[test]
    fn colour_settings_hash_is_stable() {
        let a = ColorSettings::default();
        assert_eq!(color_fingerprint(&a), color_fingerprint(&a.clone()));
    }
}
