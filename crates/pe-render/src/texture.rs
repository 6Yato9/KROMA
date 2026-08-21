//! Textures.
//!
//! One rule: **every intermediate is `Rgba16Float`.** 8-bit banding appears the
//! instant a user pushes a curve and is not recoverable downstream, and the
//! working gamut carries values outside 0..1 (highlights above diffuse white,
//! negative channels where a colour does not fit a narrower gamut) which a
//! unorm format cannot represent at all.

use crate::RenderError;

/// The format of every intermediate texture in the pipeline.
pub const WORKING_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The format an 8-bit source image is uploaded as.
///
/// `...Srgb` so the hardware applies the sRGB EOTF on sample, for free and
/// exactly. Doing it in a shader would be slower and no more correct.
pub const SOURCE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct ImageTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl ImageTexture {
    /// An empty working-space render target.
    pub fn new_working(device: &wgpu::Device, width: u32, height: u32, label: &str) -> Self {
        Self::new(
            device,
            width,
            height,
            WORKING_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            label,
        )
    }

    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// Upload 8-bit RGBA pixels as an sRGB-encoded source texture.
    pub fn upload_rgba8(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
        label: &str,
    ) -> Result<Self, RenderError> {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(RenderError::PixelCountMismatch {
                expected,
                found: pixels.len(),
            });
        }

        let tex = Self::new(
            device,
            width,
            height,
            SOURCE_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label,
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        Ok(tex)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Compute the padded bytes-per-row a texture readback needs.
///
/// `copy_texture_to_buffer` requires each row to start on a 256-byte boundary.
/// Forgetting this is the standard "my readback is sheared diagonally" bug, so
/// it lives here with a test rather than being open-coded at each call site.
pub fn padded_bytes_per_row(width: u32, bytes_per_pixel: u32) -> u32 {
    let unpadded = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unpadded.div_ceil(align) * align
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_format_is_16_bit_float() {
        // Guards the one rule this module exists to enforce.
        assert_eq!(WORKING_FORMAT, wgpu::TextureFormat::Rgba16Float);
    }

    #[test]
    fn source_format_is_srgb_so_the_hardware_linearises() {
        assert_eq!(SOURCE_FORMAT, wgpu::TextureFormat::Rgba8UnormSrgb);
    }

    #[test]
    fn row_padding_rounds_up_to_the_copy_alignment() {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        // 100px * 8 bytes = 800, which is not a multiple of 256.
        assert_eq!(padded_bytes_per_row(100, 8), 1024);
        // Already aligned widths are left alone.
        assert_eq!(padded_bytes_per_row(64, 4), 256);
        for width in [1u32, 7, 63, 64, 1920, 6000] {
            let p = padded_bytes_per_row(width, 8);
            assert!(p >= width * 8, "width {width} lost bytes");
            assert_eq!(p % align, 0, "width {width} is not aligned");
        }
    }
}
