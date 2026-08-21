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
