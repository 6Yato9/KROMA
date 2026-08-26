//! The Windows shell's end of the library.
//!
//! The library itself is [`pe_session::library`]: the set of photographs open
//! at once, the edit parked for each, the worker that decodes thumbnails. All
//! of that is the same question on a Mac and moved to the engine so it is
//! answered once.
//!
//! What could not move is the last inch of a thumbnail. It arrives as RGBA
//! bytes because a texture belongs to a graphics context and there are two
//! shells with two of those; this turns those bytes into egui's.
//!
//! Here rather than in `filmstrip.rs` because the strip is drawing code and
//! this is not: the upload has to happen when `collect` says something
//! arrived, which is at the top of the frame, and the strip may not even be
//! open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use pe_session::library::*;

/// The egui textures for the library's thumbnails.
///
/// Keyed by path rather than by index, for the same reason
/// [`Library::index_of`] is a search rather than a stored number: the set
/// shifts under every removal, and a texture that ends up on the wrong
/// photograph is a filmstrip quietly showing the wrong picture.
#[derive(Default)]
pub struct Thumbnails {
    uploaded: HashMap<PathBuf, egui::TextureHandle>,
}

impl Thumbnails {
    /// The texture for a photograph, once one has been uploaded.
    pub fn get(&self, path: &Path) -> Option<&egui::TextureHandle> {
        self.uploaded.get(path)
    }

    /// Upload what the library has and this does not.
    ///
    /// Called when `collect` reports a delivery. Walking the whole set to find
    /// the new arrivals is a lookup per photograph, which is exactly the kind
    /// of thing the strip refuses to do per frame — but this is not per frame,
    /// it is per thumbnail, and a thumbnail is a decode.
    pub fn upload(&mut self, ctx: &egui::Context, library: &Library) {
        for entry in library.entries() {
            let Some(thumb) = entry.thumb.as_ref() else {
                continue;
            };
            if self.uploaded.contains_key(&entry.path) {
                continue;
            }
            let image = egui::ColorImage {
                size: [thumb.width as usize, thumb.height as usize],
                pixels: thumb
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
                    .collect(),
                source_size: egui::vec2(thumb.width as f32, thumb.height as f32),
            };
            self.uploaded.insert(
                entry.path.clone(),
                ctx.load_texture(
                    entry.path.to_string_lossy(),
                    image,
                    egui::TextureOptions::LINEAR,
                ),
            );
        }
    }

    /// Drop the texture for a photograph that has left the set.
    ///
    /// Not tidiness. A texture is 64 KB of graphics memory, and a session that
    /// works through folder after folder would otherwise end up holding a
    /// picture of every photograph it had ever been shown — which is the
    /// accounting the library exists to avoid.
    pub fn forget(&mut self, path: &Path) {
        self.uploaded.remove(path);
    }
}
