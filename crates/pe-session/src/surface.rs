//! The window's layer, as a wgpu surface.
//!
//! On Apple platforms the host owns the view hierarchy and we are given a
//! `CAMetalLayer` to draw into. That is the whole of the platform-specific
//! surface story: no window creation, no event loop, no swapchain management
//! beyond configuring one.
//!
//! The instance is held by the caller rather than created here, because a
//! `Surface` belongs to the `Instance` that made it and an adapter obtained
//! from a different instance cannot present to it — the failure is a panic
//! deep inside wgpu, a long way from the cause. `GpuContext::create_instance`
//! exists for exactly this split.

use std::ffi::c_void;

use wgpu::TextureFormat;

/// A layer we have been given, and the surface configured onto it.
pub struct Attached {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

impl Attached {
    /// Build a surface on a `CAMetalLayer`.
    ///
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives this `Attached`.
    /// The Swift side guarantees this by keeping the layer on a view it owns
    /// and detaching before the view goes away.
    pub unsafe fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        layer: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| e.to_string())?;

        let caps = surface.get_capabilities(adapter);
        // Ask for an sRGB-encoding target so the transfer function is applied
        // on write by the hardware, which is what the Windows display texture
        // does through `SOURCE_FORMAT`. Falling back to whatever the surface
        // does offer keeps this from being a hard failure on a device that
        // surprises us.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == TextureFormat::Bgra8UnormSrgb)
            .unwrap_or_else(|| caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(device, &config);
        Ok(Self { surface, config })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(device, &self.config);
    }

    /// Fill the surface with one colour and present it.
    ///
    /// The proof that the layer path works, and afterwards the thing that
    /// draws the background behind a photograph that has not loaded.
    pub fn present_clear(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        colour: [f64; 4],
    ) -> Result<(), String> {
        let frame = self
            .surface
            .get_current_texture()
            .map_err(|e| e.to_string())?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear"),
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: colour[0],
                        g: colour[1],
                        b: colour[2],
                        a: colour[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}
