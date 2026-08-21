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
pub mod readback;
pub mod texture;
pub mod transform;

pub use cache::{RenderContext, RenderPlan, RowFingerprint, StageCache, fingerprint};
pub use device::GpuContext;
pub use readback::read_rgba8;
pub use texture::{ImageTexture, SOURCE_FORMAT, WORKING_FORMAT};
pub use transform::TransformPass;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("could not acquire a GPU device: {0}")]
    NoDevice(String),
    #[error("expected {expected} bytes of pixel data, got {found}")]
    PixelCountMismatch { expected: usize, found: usize },
    #[error("GPU surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),
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
