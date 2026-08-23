//! Reading a texture back to the CPU.
//!
//! Needed for export, and — more importantly right now — for testing the GPU
//! against `pe-color`'s CPU reference. Without a readback path the shaders are
//! unverified, and "the window showed something" is not the same as "the maths
//! is right".

use crate::texture::padded_bytes_per_row;
use crate::{GpuContext, ImageTexture, RenderError};

/// Copy a texture back and hand each row to a closure, tightly packed.
///
/// The row callback exists because the alternative is a `Vec<u8>` of the whole
/// frame that every caller then walks again to make what it actually wanted. A
/// 24-megapixel `Rgba16Float` frame is 192 MB; going through an intermediate to
/// produce a 192 MB `Vec<u16>` means holding both, and that is a third of a
/// gigabyte spent on the word `map`.
fn each_row(
    gpu: &GpuContext,
    tex: &ImageTexture,
    bytes_per_pixel: u32,
    mut row: impl FnMut(&[u8]),
) -> Result<(), RenderError> {
    // Rows in a texture-to-buffer copy must start on a 256-byte boundary, so
    // the buffer is wider than the image and has to be unpadded afterwards.
    let padded = padded_bytes_per_row(tex.width, bytes_per_pixel);
    let unpadded = tex.width * bytes_per_pixel;

    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * tex.height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(tex.height),
            },
        },
        wgpu::Extent3d {
            width: tex.width,
            height: tex.height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    rx.recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;

    let view = slice.get_mapped_range();
    for r in 0..tex.height {
        let start = (r * padded) as usize;
        row(&view[start..start + unpadded as usize]);
    }
    drop(view);
    buffer.unmap();

    Ok(())
}

/// Copy an `Rgba8Unorm`-family texture back to a tightly packed CPU buffer.
///
/// The texture must have been created with [`wgpu::TextureUsages::COPY_SRC`].
pub fn read_rgba8(gpu: &GpuContext, tex: &ImageTexture) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::with_capacity((tex.width * tex.height * 4) as usize);
    each_row(gpu, tex, 4, |row| out.extend_from_slice(row))?;
    Ok(out)
}

/// Copy an `Rgba16Float` texture back, converting each sample on the way.
///
/// The samples arrive as IEEE half floats and are handed to `convert` as
/// `f32`. What comes back out is whatever the caller wants — the 16-bit export
/// asks for `u16` and never materialises the `f32` frame at all.
pub fn read_rgba16f<T>(
    gpu: &GpuContext,
    tex: &ImageTexture,
    mut convert: impl FnMut(f32) -> T,
) -> Result<Vec<T>, RenderError> {
    let mut out = Vec::with_capacity((tex.width * tex.height * 4) as usize);
    each_row(gpu, tex, 8, |row| {
        for sample in row.as_chunks::<2>().0 {
            let bits = u16::from_le_bytes(*sample);
            out.push(convert(half::f16::from_bits(bits).to_f32()));
        }
    })?;
    Ok(out)
}
