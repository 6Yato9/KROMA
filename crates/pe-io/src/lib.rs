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
    let img = image::open(path.as_ref())?.to_rgba8();
    let (width, height) = img.dimensions();
    DecodedImage::new(width, height, img.into_raw())
}

pub fn load_from_memory(bytes: &[u8]) -> Result<DecodedImage, IoError> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (width, height) = img.dimensions();
    DecodedImage::new(width, height, img.into_raw())
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
pub fn save_png(img: &DecodedImage, path: impl AsRef<Path>) -> Result<(), IoError> {
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()).ok_or(
        IoError::PixelCountMismatch {
            expected: img.width as usize * img.height as usize * 4,
            found: img.pixels.len(),
        },
    )?;
    buf.save_with_format(path.as_ref(), image::ImageFormat::Png)?;
    Ok(())
}

pub fn save_jpeg(img: &DecodedImage, path: impl AsRef<Path>, quality: u8) -> Result<(), IoError> {
    let rgb = image::RgbImage::from_fn(img.width, img.height, |x, y| {
        let p = img.pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    let mut file = std::io::BufWriter::new(std::fs::File::create(path.as_ref())?);
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality.max(1));
    enc.encode_image(&rgb)?;
    Ok(())
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

        save_png(&img, &path).unwrap();
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

        save_jpeg(&img, &path, 100).unwrap();
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
