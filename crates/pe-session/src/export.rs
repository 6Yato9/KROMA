//! What may be written, and where.
//!
//! The application does not modify the photograph you opened. It cannot: every
//! write is checked against every file in the open set first, and a collision
//! is refused rather than resolved.
//!
//! The naming and the check are two separate defences on purpose. A scheme
//! that happens to differ is not a guarantee — and it would not hold anyway
//! once you can export a PNG of a PNG, which the File page allows in one click.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What an export is written as.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Format {
    /// Small, universal, and eight bits with a lossy step on top. The right
    /// answer for a photograph that is finished and going somewhere.
    #[default]
    Jpeg,
    /// Eight bits, no lossy step. For anything that will be looked at closely
    /// or composited onto.
    Png,
    /// Sixteen bits. Where the wide working space stops being theoretical: a
    /// gradient pushed about by a dozen rows holds more distinct values than
    /// eight bits can name, and this is the only way out that keeps them.
    Png16,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png | Format::Png16 => "png",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG 8",
            Format::Png16 => "PNG 16",
        }
    }

    /// Whether the export path has to read the frame back at full depth.
    pub fn is_sixteen_bit(self) -> bool {
        self == Format::Png16
    }

    /// Parse the name the FFI uses. Unknown names are JPEG, because an export
    /// that happens is better than one refused over a spelling.
    pub fn from_name(name: &str) -> Format {
        match name.to_ascii_lowercase().as_str() {
            "png" => Format::Png,
            "png16" => Format::Png16,
            _ => Format::Jpeg,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Png16 => "png16",
        }
    }
}

/// The export settings, kept together so they can be handed about as one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Export {
    pub format: Format,
    /// JPEG only, 1-100.
    ///
    /// 95 rather than 100: the last few points of a JPEG quality scale buy
    /// almost nothing you can see and cost a great deal of file, and 100 is
    /// still lossy — a person who wants no loss wants PNG, not a bigger JPEG.
    pub quality: u8,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            format: Format::default(),
            quality: 95,
        }
    }
}

pub fn export_name(source: &Path, format: Format) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());
    format!("{stem}_KROMA.{}", format.extension())
}

/// An export name for this photograph that nothing in this run has used yet.
///
/// A batch writes every photograph into one directory, and the set it is
/// writing can have come from several. Two files called `sunset.jpg` in
/// different folders both want to be `sunset_KROMA.jpg`, and without this the
/// second lands on the first: one file on disc, two successes reported, and
/// nothing anywhere saying which one you kept.
///
/// Numbered rather than refused. Losing an original is unrecoverable and worth
/// being rude about; two of your own exports wanting one name is an ordinary
/// thing that has an obvious right answer.
///
/// Compared in lower case, because the directory this is being written into is
/// on Windows more often than not, and `A_KROMA.jpg` and `a_KROMA.jpg` are one
/// file there.
pub fn unclaimed_export_path(
    dir: &Path,
    source: &Path,
    format: Format,
    taken: &mut HashSet<String>,
) -> PathBuf {
    let first = export_name(source, format);
    if taken.insert(first.to_lowercase()) {
        return dir.join(first);
    }
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());
    // Two is where a human starts counting a second one of something.
    for n in 2u32.. {
        let name = format!("{stem}_KROMA_{n}.{}", format.extension());
        if taken.insert(name.to_lowercase()) {
            return dir.join(name);
        }
    }
    unreachable!("u32 ran out of numbers")
}

/// Whether two paths name the same file, as far as can be told without
/// creating one.
///
/// `canonicalize` is the right answer and it only works on files that already
/// exist — which an output path usually does not. So the *directories* are
/// canonicalised, which do exist, and the file names compared without regard
/// to case. Windows treats `photo.JPG` and `photo.jpg` as one file, and a
/// comparison that did not is exactly the comparison that would let a batch
/// export eat a folder of originals.
pub fn same_file(a: &Path, b: &Path) -> bool {
    let dir = |p: &Path| {
        let d = p.parent().unwrap_or(Path::new("."));
        std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf())
    };
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    match (name(a), name(b)) {
        (Some(x), Some(y)) => x == y && dir(a) == dir(b),
        _ => false,
    }
}

/// Whether writing here would land on a photograph we were given.
///
/// A hard refusal rather than a warning. The application is allowed to be
/// annoying about this exactly once — losing somebody's original is not a thing
/// to recover from, and there is no undo that reaches outside the process.
pub fn would_overwrite_a_source(open: &[PathBuf], out: &Path) -> bool {
    open.iter().any(|p| same_file(p, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn batch_path(dir: &str, source: &str, format: Format) -> PathBuf {
        let mut taken = HashSet::new();
        unclaimed_export_path(Path::new(dir), Path::new(source), format, &mut taken)
    }

    #[test]
    fn the_export_carries_the_kroma_suffix() {
        assert_eq!(
            export_name(Path::new("/photos/DJI_0001.JPG"), Format::Jpeg),
            "DJI_0001_KROMA.jpg"
        );
    }

    #[test]
    fn an_export_is_never_named_after_its_source() {
        let source = Path::new("/photos/sunset.jpg");
        let out = source.with_file_name(export_name(source, Format::Jpeg));
        assert_ne!(out, source.to_path_buf());
        assert!(!same_file(source, &out));
    }

    #[test]
    fn a_png_source_exported_as_png_is_still_safe() {
        // The case the naming scheme alone would not survive: same extension,
        // same folder. The suffix is what keeps them apart.
        let source = Path::new("/photos/chart.png");
        let out = source.with_file_name(export_name(source, Format::Png));
        assert!(!same_file(source, &out));
    }

    #[test]
    fn two_names_differing_only_in_case_are_the_same_file() {
        // Windows ignores case, so a comparison that did not is exactly the
        // comparison that would let a batch export eat a folder of originals.
        assert!(same_file(
            Path::new("/p/A_KROMA.jpg"),
            Path::new("/p/a_kroma.JPG")
        ));
    }

    #[test]
    fn two_photographs_with_one_name_do_not_share_an_export() {
        let mut taken = HashSet::new();
        let out = Path::new("/out");
        let first =
            unclaimed_export_path(out, Path::new("/a/sunset.jpg"), Format::Jpeg, &mut taken);
        let second =
            unclaimed_export_path(out, Path::new("/b/sunset.jpg"), Format::Jpeg, &mut taken);
        assert_ne!(first, second);
        assert_eq!(second.file_name().unwrap(), "sunset_KROMA_2.jpg");
    }

    #[test]
    fn export_names_collide_regardless_of_case() {
        let mut taken = HashSet::new();
        let out = Path::new("/out");
        unclaimed_export_path(out, Path::new("/a/Sunset.jpg"), Format::Jpeg, &mut taken);
        let second =
            unclaimed_export_path(out, Path::new("/b/sunset.jpg"), Format::Jpeg, &mut taken);
        assert_eq!(second.file_name().unwrap(), "sunset_KROMA_2.jpg");
    }

    #[test]
    fn exporting_an_export_does_not_land_on_it() {
        assert_eq!(
            batch_path("/out", "/out/sunset_KROMA.png", Format::Png)
                .file_name()
                .unwrap(),
            "sunset_KROMA_KROMA.png"
        );
    }

    #[test]
    fn a_write_onto_any_open_photograph_is_refused() {
        // Checked against every photograph in the set, not only the one on
        // screen: a batch writes into one folder and the name it builds for
        // photo A can collide with photo B sitting right beside it.
        let open = [
            PathBuf::from("/photos/a.jpg"),
            PathBuf::from("/photos/b.jpg"),
        ];
        assert!(would_overwrite_a_source(&open, Path::new("/photos/B.JPG")));
        assert!(!would_overwrite_a_source(&open, Path::new("/photos/c.jpg")));
    }
}
