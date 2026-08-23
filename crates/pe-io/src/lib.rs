//! Image decode and encode.
//!
//! M0 handles 8-bit JPEG and PNG. RAW arrives at M5 via LibRaw, which is where
//! this crate stops being trivial — the interface here is deliberately narrow
//! so that adding a second decoder does not ripple outward.

use std::path::Path;

/// A decoded image in memory: tightly packed 8-bit RGBA.
///
/// Always RGBA even for opaque sources, because that is what
/// `Rgba8UnormSrgb` upload wants and a repack at the GPU boundary is wasted
/// work on every image load.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// The colour space the file said it was in, if it said and we knew the
    /// name. `None` covers a file with no profile, one with a profile we do not
    /// recognise, and one whose profile is damaged — all three mean the same
    /// thing to a caller: nothing was declared, assume what you were going to
    /// assume.
    pub space: Option<&'static str>,
}

impl std::fmt::Debug for DecodedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print a few megabytes of pixels into a test failure message.
        f.debug_struct("DecodedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

impl DecodedImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, IoError> {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(IoError::PixelCountMismatch {
                expected,
                found: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
            // Only `load` knows what a file declared; anything constructed from
            // loose pixels is declaring nothing.
            space: None,
        })
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Iterate RGB triples as 0..1 floats, dropping alpha.
    pub fn iter_rgb(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.pixels.as_chunks::<4>().0.iter().map(|p| {
            [
                p[0] as f64 / 255.0,
                p[1] as f64 / 255.0,
                p[2] as f64 / 255.0,
            ]
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Largest absolute per-channel difference against another image.
    ///
    /// The primitive the golden tests are built on. Returns `None` if the
    /// dimensions differ.
    pub fn max_channel_delta(&self, other: &DecodedImage) -> Option<u8> {
        if self.size() != other.size() {
            return None;
        }
        Some(
            self.pixels
                .iter()
                .zip(&other.pixels)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0),
        )
    }
}

pub fn load(path: impl AsRef<Path>) -> Result<DecodedImage, IoError> {
    let reader = image::ImageReader::open(path.as_ref())?.with_guessed_format()?;
    decode(reader.into_decoder()?)
}

pub fn load_from_memory(bytes: &[u8]) -> Result<DecodedImage, IoError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    decode(reader.into_decoder()?)
}

/// Decode, taking the embedded profile on the way past.
///
/// Through a decoder rather than `image::open`, which throws the profile away
/// before we ever see it. The profile has to be read *before* the pixels,
/// because decoding consumes the decoder — which is why this is one function
/// rather than two calls at the call site that could be put in the wrong order.
fn decode<'a>(mut decoder: impl image::ImageDecoder + 'a) -> Result<DecodedImage, IoError> {
    // A profile we cannot read is not a reason to refuse the photograph.
    let profile = decoder.icc_profile().ok().flatten();
    let img = image::DynamicImage::from_decoder(decoder)?.to_rgba8();
    let (width, height) = img.dimensions();
    let mut out = DecodedImage::new(width, height, img.into_raw())?;
    out.space = profile
        .as_deref()
        .and_then(pe_color::icc::identify)
        .map(|space| space.name);
    Ok(out)
}

/// The sRGB decode, tabulated. Two hundred and fifty-six entries, so a
/// reduction never calls `powf`.
fn srgb_to_linear() -> &'static [f32; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let s = i as f32 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// A box-filtered reduction to fit `long_edge`, averaged in linear light.
///
/// The linear part is not a nicety. Averaging half black and half white in the
/// sRGB encoding gives 128; averaging the light they represent gives 188. Get
/// it wrong and every thumbnail in the filmstrip comes out darker than the
/// photograph it stands for, which is the one job a thumbnail has.
///
/// A box filter rather than anything cleverer because the ratios here are
/// large — a 6000 pixel frame down to 128 — and at that reduction a box filter
/// is averaging fifty pixels a cell and there is nothing a windowed sinc would
/// add except time.
pub fn thumbnail(img: &DecodedImage, long_edge: u32) -> DecodedImage {
    let long_edge = long_edge.max(1);
    let scale = (long_edge as f32 / img.width.max(img.height).max(1) as f32).min(1.0);
    let w = ((img.width as f32 * scale).round() as u32).max(1);
    let h = ((img.height as f32 * scale).round() as u32).max(1);
    if (w, h) == (img.width, img.height) {
        return img.clone();
    }

    let table = srgb_to_linear();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        // Source rows this output row averages, as a half-open range. Derived
        // from the edges rather than a fixed block size so that a size that
        // does not divide evenly still covers every source pixel exactly once.
        let y0 = (y as u64 * img.height as u64 / h as u64) as u32;
        let y1 = (((y + 1) as u64 * img.height as u64).div_ceil(h as u64) as u32).min(img.height);
        for x in 0..w {
            let x0 = (x as u64 * img.width as u64 / w as u64) as u32;
            let x1 = (((x + 1) as u64 * img.width as u64).div_ceil(w as u64) as u32).min(img.width);

            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for sy in y0..y1.max(y0 + 1) {
                for sx in x0..x1.max(x0 + 1) {
                    let p = img.pixel(sx, sy);
                    for c in 0..3 {
                        sum[c] += table[p[c] as usize];
                    }
                    n += 1.0;
                }
            }
            let n = n.max(1.0);
            out.extend_from_slice(&[
                linear_to_srgb(sum[0] / n),
                linear_to_srgb(sum[1] / n),
                linear_to_srgb(sum[2] / n),
                255,
            ]);
        }
    }
    DecodedImage::new(w, h, out).expect("built to size")
}

/// Save as PNG. Lossless, which is what the golden references need — a JPEG
/// reference would drift with every encoder update.
/// Write a file so that a crash cannot leave half of one behind.
///
/// To a temporary name beside the target, then rename over it. Renaming within
/// a directory is atomic on every filesystem this runs on, so a reader sees
/// either the whole of the old file or the whole of the new one — never the
/// truncation that `fs::write` opens with.
///
/// Beside the target rather than in the system temp directory, deliberately: a
/// rename across volumes is a copy, and a copy is exactly the torn write this
/// exists to avoid.
///
/// It matters most for the file written most often. The autosave rewrites a
/// document every time you stop moving a slider, and it is the only copy of
/// work nobody asked to save — a torn write there is not a corrupt file, it is
/// an afternoon. It matters again for an export, where the truncation lands on
/// top of a good JPEG from the last run.
pub fn write_atomically(
    path: impl AsRef<Path>,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), IoError>,
) -> Result<(), IoError> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // The process id keeps two copies of the application from choosing the
    // same scratch name and finishing each other's write.
    let name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("out"));
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));

    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        write(&mut file)?;
        // Before the rename, not after. Without this the rename can reach the
        // disc first and a power cut leaves the new name pointing at nothing,
        // which is the one outcome worse than the old file surviving.
        file.sync_all()?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            std::fs::rename(&temp, path)?;
            Ok(())
        }
        Err(e) => {
            // Leaving scratch files through somebody's photo library is its
            // own small rudeness.
            let _ = std::fs::remove_file(&temp);
            Err(e)
        }
    }
}

/// [`write_atomically`] for something already in memory.
pub fn write_bytes_atomically(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), IoError> {
    use std::io::Write as _;
    write_atomically(path, |file| {
        file.write_all(bytes)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Saying what a file is in
// ---------------------------------------------------------------------------

/// Put an ICC profile into an already-encoded JPEG.
///
/// As an APP2 segment straight after the start-of-image marker, which is where
/// every reader looks and where the specification puts it. Splicing the encoded
/// bytes rather than asking the encoder, because the encoder has no opinion on
/// colour and offers no way to express one — and byte surgery on a container
/// this simple is easier to be sure of than a fork of the encoder would be.
fn with_icc_jpeg(bytes: Vec<u8>, profile: &[u8]) -> Vec<u8> {
    // A JPEG that does not start with SOI is not one; hand it back untouched
    // rather than corrupting it further.
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return bytes;
    }
    // A segment's length field is two bytes and counts itself, so the payload
    // has 65533 to work with: twelve for the identifier, two for the chunk
    // numbering, and the rest for profile.
    const PER_CHUNK: usize = 65533 - 12 - 2;
    let total = profile.len().div_ceil(PER_CHUNK).max(1);
    if total > 255 {
        // The chunk counter is one byte. No profile this program writes comes
        // close, and a silently truncated one would be worse than none.
        return bytes;
    }

    let mut out = Vec::with_capacity(bytes.len() + profile.len() + total * 20);
    out.extend_from_slice(&bytes[0..2]);
    for (i, part) in profile.chunks(PER_CHUNK).enumerate() {
        out.extend_from_slice(&[0xFF, 0xE2]);
        out.extend_from_slice(&((part.len() + 16) as u16).to_be_bytes());
        out.extend_from_slice(b"ICC_PROFILE ");
        out.push(i as u8 + 1);
        out.push(total as u8);
        out.extend_from_slice(part);
    }
    out.extend_from_slice(&bytes[2..]);
    out
}

/// Put an ICC profile into an already-encoded PNG.
///
/// An `iCCP` chunk immediately after `IHDR`, which is where the specification
/// requires it — before `PLTE` and `IDAT`, and readers are entitled to stop
/// looking once the pixels start.
fn with_icc_png(bytes: Vec<u8>, profile: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    const SIGNATURE: usize = 8;
    // Signature, then IHDR's own length, type, 13 bytes of data and CRC.
    const AFTER_IHDR: usize = SIGNATURE + 4 + 4 + 13 + 4;
    if bytes.len() < AFTER_IHDR || &bytes[SIGNATURE + 4..SIGNATURE + 8] != b"IHDR" {
        return bytes;
    }

    // The profile goes in deflated; that is not optional in the format.
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    if encoder.write_all(profile).is_err() {
        return bytes;
    }
    let Ok(compressed) = encoder.finish() else {
        return bytes;
    };

    let mut data = Vec::with_capacity(compressed.len() + 8);
    // The profile's name, Latin-1 and null-terminated, then the one
    // compression method the format defines.
    data.extend_from_slice(b"ICC profile");
    data.push(0);
    data.push(0);
    data.extend_from_slice(&compressed);

    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"iCCP");
    chunk.extend_from_slice(&data);
    // The CRC covers the type and the data, and not the length.
    let mut crc = flate2::Crc::new();
    crc.update(&chunk[4..]);
    chunk.extend_from_slice(&crc.sum().to_be_bytes());

    let mut out = Vec::with_capacity(bytes.len() + chunk.len());
    out.extend_from_slice(&bytes[..AFTER_IHDR]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&bytes[AFTER_IHDR..]);
    out
}

/// Write an 8-bit PNG, saying what colour space it is in.
///
/// The space is required rather than optional. An untagged file is read as
/// sRGB by everything there is, so "forgot to say" and "said sRGB" are the same
/// file on disc and only one of them is honest — leaving the argument out was
/// how every export before this one came to be silently mislabelled.
pub fn save_png(
    img: &DecodedImage,
    path: impl AsRef<Path>,
    space: &pe_color::ColorSpace,
) -> Result<(), IoError> {
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()).ok_or(
        IoError::PixelCountMismatch {
            expected: img.width as usize * img.height as usize * 4,
            found: img.pixels.len(),
        },
    )?;
    let mut bytes = Vec::new();
    buf.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    let bytes = with_icc_png(bytes, &pe_color::icc::profile_for(space));
    write_bytes_atomically(path, &bytes)
}

/// Write 16-bit RGBA out as a PNG.
///
/// Takes the samples loose rather than a `DecodedImage`, which is 8-bit by
/// definition and should stay that way: it is what a *decoded photograph* is,
/// and a 16-bit export is not that — it is the far end of the pipeline on its
/// way to disc, and it exists for exactly one call.
pub fn save_png16(
    width: u32,
    height: u32,
    rgba: &[u16],
    path: impl AsRef<Path>,
    space: &pe_color::ColorSpace,
) -> Result<(), IoError> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(IoError::PixelCountMismatch {
            expected,
            found: rgba.len(),
        });
    }
    let buf: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
        image::ImageBuffer::from_raw(width, height, rgba.to_vec()).ok_or(
            IoError::PixelCountMismatch {
                expected,
                found: rgba.len(),
            },
        )?;
    let mut bytes = Vec::new();
    buf.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    let bytes = with_icc_png(bytes, &pe_color::icc::profile_for(space));
    write_bytes_atomically(path, &bytes)
}

pub fn save_jpeg(
    img: &DecodedImage,
    path: impl AsRef<Path>,
    quality: u8,
    space: &pe_color::ColorSpace,
) -> Result<(), IoError> {
    let rgb = image::RgbImage::from_fn(img.width, img.height, |x, y| {
        let p = img.pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    // Encoded to memory so the profile can be spliced in, then written through
    // a scratch file: an export usually lands on top of the last one, and a
    // write that stops halfway would otherwise have already truncated a JPEG
    // that was fine.
    let mut bytes = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.max(1));
    enc.encode_image(&rgb)?;
    let bytes = with_icc_jpeg(bytes, &pe_color::icc::profile_for(space));
    write_bytes_atomically(path, &bytes)
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("image decode/encode failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("file error: {0}")]
    Fs(#[from] std::io::Error),
    #[error("expected {expected} bytes of pixel data, got {found}")]
    PixelCountMismatch { expected: usize, found: usize },
}

/// Build a deterministic test image covering the full tonal and hue range.
///
/// Shared by the golden tests and the app's no-file-open state. Deterministic
/// so that a golden reference stays valid across machines.
pub fn test_chart(width: u32, height: u32) -> DecodedImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let u = x as f32 / (width.max(2) - 1) as f32;
            let v = y as f32 / (height.max(2) - 1) as f32;
            // Top band: neutral ramp, black to white. Catches banding and any
            // white-point error, which shows as a tint across the ramp.
            // Bottom: hue sweep at varying value, to catch gamut and hue shifts.
            let (r, g, b) = if v < 0.25 {
                (u, u, u)
            } else {
                let h = u * 6.0;
                let s = 1.0 - (v - 0.25) / 0.75 * 0.4;
                hsv_to_rgb(h, s, 0.2 + 0.8 * (1.0 - (v - 0.25) / 0.75))
            };
            pixels.extend_from_slice(&[
                (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                255,
            ]);
        }
    }
    DecodedImage {
        width,
        height,
        pixels,
        // It is built, not loaded, and it is built in sRGB.
        space: Some("sRGB"),
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = h.floor() as i32 % 6;
    let f = h - h.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;

    fn flat(w: u32, h: u32, c: [u8; 3]) -> DecodedImage {
        let px: Vec<u8> = std::iter::repeat_n([c[0], c[1], c[2], 255], (w * h) as usize)
            .flatten()
            .collect();
        DecodedImage::new(w, h, px).unwrap()
    }

    #[test]
    fn a_thumbnail_fits_the_long_edge_and_keeps_the_shape() {
        let t = thumbnail(&flat(4000, 3000, [90, 90, 90]), 128);
        assert_eq!(t.size(), (128, 96));
    }

    #[test]
    fn a_portrait_frame_fits_its_own_long_edge() {
        let t = thumbnail(&flat(1000, 2000, [90, 90, 90]), 100);
        assert_eq!(t.size(), (50, 100));
    }

    /// A thumbnail is a stand-in for the photograph. If it came out darker
    /// than the photograph, the filmstrip would be lying about every frame in
    /// it — and averaging in the encoding rather than in the light is exactly
    /// how that happens.
    #[test]
    fn reducing_averages_in_linear_light() {
        let mut px = Vec::new();
        for y in 0..64u32 {
            for x in 0..64u32 {
                let v = if (x + y).is_multiple_of(2) { 255u8 } else { 0 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let src = DecodedImage::new(64, 64, px).unwrap();
        let t = thumbnail(&src, 8);
        assert_eq!(t.size(), (8, 8));
        for y in 0..8 {
            for x in 0..8 {
                let v = t.pixel(x, y)[0] as i32;
                assert!(
                    (v - 188).abs() <= 2,
                    "a checkerboard averaged to {v}, not 188 — the reduction                      is happening in the encoding rather than in the light"
                );
            }
        }
    }

    #[test]
    fn a_flat_colour_survives_intact() {
        let t = thumbnail(&flat(300, 200, [200, 40, 90]), 50);
        for y in 0..t.height {
            for x in 0..t.width {
                let p = t.pixel(x, y);
                assert!(
                    (p[0] as i32 - 200).abs() <= 1
                        && (p[1] as i32 - 40).abs() <= 1
                        && (p[2] as i32 - 90).abs() <= 1,
                    "{p:?} at {x},{y}"
                );
            }
        }
    }

    /// Enlarging invents detail, and a filmstrip full of soft upscales of
    /// small files would be worse than one showing them at their own size.
    #[test]
    fn something_already_smaller_is_left_alone() {
        let src = flat(60, 40, [10, 20, 30]);
        assert_eq!(thumbnail(&src, 128), src);
    }

    /// Every source pixel has to land in exactly one cell. A size that does
    /// not divide evenly is the normal case, not the exception.
    #[test]
    fn an_awkward_ratio_still_covers_the_whole_frame() {
        // Left half black, right half white, at a width that does not divide.
        let mut px = Vec::new();
        for _ in 0..30u32 {
            for x in 0..101u32 {
                let v = if x < 50 { 0u8 } else { 255 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let src = DecodedImage::new(101, 30, px).unwrap();
        let t = thumbnail(&src, 7);
        assert_eq!(t.pixel(0, 0)[0], 0, "the dark end went missing");
        assert_eq!(t.pixel(t.width - 1, 0)[0], 255, "so did the bright end");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The trap this helper exists to walk into on purpose.
    ///
    /// `fs::rename` over an existing file is an error on some platforms and a
    /// replacement on others. If it ever stopped replacing here, every save in
    /// the application would fail on the second attempt and only on the second
    /// attempt — the kind of thing that passes a demo and fails a user.
    #[test]
    fn writing_atomically_replaces_what_was_there() {
        let path = scratch("replace.txt");
        write_bytes_atomically(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");

        write_bytes_atomically(&path, b"second").unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"second",
            "the rename did not replace the existing file"
        );
    }

    /// A write that goes wrong must leave the previous file untouched, and no
    /// scratch file behind.
    ///
    /// The whole argument for the temporary is that the moment of danger is
    /// moved off the real file. If a failure still truncated it, this would be
    /// ceremony rather than safety.
    #[test]
    fn a_failed_write_leaves_the_old_file_alone() {
        let path = scratch("failure.txt");
        write_bytes_atomically(&path, b"the good copy").unwrap();

        let outcome = write_atomically(&path, |_| {
            Err(IoError::Fs(std::io::Error::other("encoder gave up")))
        });
        assert!(outcome.is_err(), "the failure was swallowed");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"the good copy",
            "a failed write destroyed the file it was replacing"
        );

        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(".failure.txt") && n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "scratch files left behind: {leftovers:?}"
        );
    }

    /// And nothing is left lying about after a write that went fine either.
    #[test]
    fn a_good_write_leaves_no_scratch_file() {
        let path = scratch("tidy.txt");
        write_bytes_atomically(&path, b"done").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(".tidy.txt") && n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "scratch files left behind: {leftovers:?}"
        );
    }

    /// The whole loop: write a file saying what it is, read it back, believe
    /// it.
    ///
    /// Every piece is separately tested — the profile writer against the
    /// reader, the reader against rubbish — and none of that would catch an
    /// APP2 segment one byte too long or an `iCCP` chunk with a bad CRC. This
    /// goes through the real containers and out through a decoder that has
    /// never heard of our splicing code, which is the only way to know the
    /// surgery held.
    #[test]
    fn an_exported_file_says_which_space_it_is_in() {
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let img = test_chart(48, 32);
        let space = &pe_color::space::DISPLAY_P3;

        let jpeg = dir.join("tagged.jpg");
        save_jpeg(&img, &jpeg, 92, space).unwrap();
        assert_eq!(
            load(&jpeg).unwrap().space,
            Some("Display P3"),
            "the APP2 segment did not survive the round trip"
        );

        let png = dir.join("tagged.png");
        save_png(&img, &png, space).unwrap();
        assert_eq!(
            load(&png).unwrap().space,
            Some("Display P3"),
            "the iCCP chunk did not survive the round trip"
        );

        let wide = dir.join("tagged16.png");
        let samples = vec![32768u16; 48 * 32 * 4];
        save_png16(48, 32, &samples, &wide, space).unwrap();
        assert_eq!(load(&wide).unwrap().space, Some("Display P3"));
    }

    /// And the pixels still decode, which splicing bytes into a container is
    /// exactly the way to break.
    #[test]
    fn tagging_a_file_does_not_disturb_it() {
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let img = test_chart(40, 24);

        let path = dir.join("intact.png");
        save_png(&img, &path, &pe_color::space::SRGB).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.size(), img.size());
        assert_eq!(
            back.pixels, img.pixels,
            "PNG is lossless; tagging it changed the picture"
        );
    }

    /// A file that says nothing about itself must say nothing, not guess.
    ///
    /// `None` and `Some("sRGB")` are different answers: the first lets the
    /// caller keep whatever it had, the second overwrites it. Most photographs
    /// in the world carry no profile at all, so this is the common case, not
    /// the corner one.
    ///
    /// Written by the encoder directly rather than through `save_jpeg`, which
    /// now always attaches a profile — that being the entire point of it.
    #[test]
    fn a_file_with_no_profile_declares_nothing() {
        let img = test_chart(32, 32);
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("unprofiled.jpg");

        let rgb = image::RgbImage::from_fn(img.width, img.height, |x, y| {
            let p = img.pixel(x, y);
            image::Rgb([p[0], p[1], p[2]])
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode_image(&rgb)
            .unwrap();
        std::fs::write(&path, &bytes).unwrap();

        let back = load(&path).unwrap();
        assert_eq!(back.size(), img.size(), "the decoder path lost the pixels");
        assert_eq!(
            back.space, None,
            "a file with no profile was reported as declaring one"
        );
    }

    #[test]
    fn a_mismatched_buffer_is_rejected() {
        assert!(DecodedImage::new(4, 4, vec![0; 10]).is_err());
        assert!(DecodedImage::new(4, 4, vec![0; 64]).is_ok());
    }

    #[test]
    fn png_round_trips_losslessly() {
        let img = test_chart(64, 64);
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chart.png");

        save_png(&img, &path, &pe_color::space::SRGB).unwrap();
        let back = load(&path).unwrap();

        assert_eq!(back.size(), img.size());
        assert_eq!(
            back.max_channel_delta(&img),
            Some(0),
            "PNG must be bit-exact"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn jpeg_round_trips_within_encoder_tolerance() {
        let img = test_chart(64, 64);
        let dir = std::env::temp_dir().join("pe-io-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chart.jpg");

        save_jpeg(&img, &path, 100, &pe_color::space::SRGB).unwrap();
        let back = load(&path).unwrap();

        assert_eq!(back.size(), img.size());
        // JPEG is lossy even at quality 100; this bounds how lossy.
        let delta = back.max_channel_delta(&img).unwrap();
        assert!(delta < 24, "quality-100 JPEG drifted by {delta}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_test_chart_is_deterministic() {
        assert_eq!(test_chart(32, 32), test_chart(32, 32));
    }

    #[test]
    fn the_test_chart_spans_black_to_white() {
        let c = test_chart(64, 64);
        let ramp: Vec<u8> = (0..64).map(|x| c.pixel(x, 0)[0]).collect();
        assert_eq!(ramp.first(), Some(&0));
        assert_eq!(ramp.last(), Some(&255));
        assert!(ramp.windows(2).all(|w| w[0] <= w[1]), "ramp not monotonic");
    }

    #[test]
    fn the_neutral_ramp_is_actually_neutral() {
        // If this ever fails, the chart itself has a tint and every golden
        // reference built on it is measuring the wrong thing.
        let c = test_chart(64, 64);
        for x in 0..64 {
            let [r, g, b, _] = c.pixel(x, 0);
            assert_eq!((r, g), (g, b), "column {x} is not neutral");
        }
    }

    #[test]
    fn delta_against_a_different_size_is_none() {
        assert!(
            test_chart(8, 8)
                .max_channel_delta(&test_chart(9, 9))
                .is_none()
        );
    }

    #[test]
    fn iter_rgb_yields_one_entry_per_pixel() {
        let c = test_chart(16, 8);
        assert_eq!(c.iter_rgb().count(), 128);
    }
}
