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
        self.pixels.chunks_exact(4).map(|p| {
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
