//! Reading a texture back to the CPU.
//!
//! Needed for export, and — more importantly right now — for testing the GPU
//! against `pe-color`'s CPU reference. Without a readback path the shaders are
//! unverified, and "the window showed something" is not the same as "the maths
//! is right".

use crate::texture::padded_bytes_per_row;
use crate::{GpuContext, ImageTexture, RenderError};

/// Copy an `Rgba8Unorm`-family texture back to a tightly packed CPU buffer.
///
/// The texture must have been created with [`wgpu::TextureUsages::COPY_SRC`].
pub fn read_rgba8(gpu: &GpuContext, tex: &ImageTexture) -> Result<Vec<u8>, RenderError> {
    let bytes_per_pixel = 4u32;
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
        .poll(wgpu::PollType::Wait)
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    rx.recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;

    let view = slice.get_mapped_range();
    let mut out = Vec::with_capacity((unpadded * tex.height) as usize);
    for row in 0..tex.height {
        let start = (row * padded) as usize;
        out.extend_from_slice(&view[start..start + unpadded as usize]);
    }
    drop(view);
    buffer.unmap();

    Ok(out)
}
