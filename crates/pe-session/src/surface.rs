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

#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    #[error("the layer pointer was null")]
    NullLayer,
    #[error("could not build a surface on the layer: {0}")]
    Create(String),
    /// The adapter and the layer cannot work together.
    ///
    /// `get_capabilities` returns nothing at all in this case, which is the
    /// only warning given. It happens when an adapter obtained for one surface
    /// is reused for another it was never checked against — plausible the
    /// moment a session opens a second window.
    #[error("this adapter cannot present to this layer")]
    Incompatible,
    #[error("could not acquire a drawable: {0}")]
    Acquire(wgpu::SurfaceError),
    /// There is no `CAMetalLayer` to attach to here.
    ///
    /// The type and the entry point exist on every platform so the C ABI has
    /// one shape everywhere — a function that is present on one build and
    /// missing on another is a worse thing to hand a caller than one that
    /// answers honestly.
    #[error("attaching a layer is an Apple-platform thing")]
    NotThisPlatform,
}

/// Build a wgpu surface on a host-owned layer.
///
/// `SurfaceTargetUnsafe::CoreAnimationLayer` exists only on Apple platforms,
/// and it is the single genuinely platform-specific line in this crate. It sat
/// here ungated for about a hundred commits, during which the workspace did
/// not compile on Windows at all — the CI matrix that would have said so on
/// the first push was aimed at a branch this repository has never had.
///
/// # Safety
/// `layer` must be a live `CAMetalLayer` that outlives the surface.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) unsafe fn surface_on_layer(
    instance: &wgpu::Instance,
    layer: *mut c_void,
) -> Result<wgpu::Surface<'static>, SurfaceError> {
    unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer)) }
        .map_err(|e| SurfaceError::Create(e.to_string()))
}

/// # Safety
/// Takes the same arguments as its Apple counterpart and reads neither.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) unsafe fn surface_on_layer(
    _instance: &wgpu::Instance,
    _layer: *mut c_void,
) -> Result<wgpu::Surface<'static>, SurfaceError> {
    Err(SurfaceError::NotThisPlatform)
}

/// A layer we have been given, and the surface configured onto it.
///
/// `config` is private because it and the surface have to agree: changing the
/// stored size without reconfiguring leaves this reporting a shape the layer
/// does not have. [`Attached::resize`] is the only way to move it.
pub struct Attached {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Attached {
    /// Build a surface on a `CAMetalLayer`.
    ///
    /// `adapter` must be one obtained for a surface on *this* layer — pass the
    /// layer to `GpuContext::from_instance` as its compatible surface first.
    /// An adapter that cannot present here is refused with
    /// [`SurfaceError::Incompatible`] rather than panicking later.
    ///
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives this `Attached`.
    /// The Swift side guarantees this by keeping the layer on a view it owns
    /// and detaching before the view goes away. A null pointer is refused
    /// rather than dereferenced.
    pub unsafe fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        layer: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfaceError> {
        if layer.is_null() {
            return Err(SurfaceError::NullLayer);
        }
        let surface = unsafe { surface_on_layer(instance, layer) }?;

        let caps = surface.get_capabilities(adapter);
        // Ask for an sRGB-encoding target so the transfer function is applied
        // on write by the hardware, which is what the Windows display texture
        // does through `SOURCE_FORMAT`. Falling back to whatever the surface
        // does offer keeps this from being a hard failure on a device that
        // surprises us — but an empty list is not a surprising device, it is
        // an adapter that cannot present here at all, and indexing it would
        // turn that into a panic with nothing to read.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == TextureFormat::Bgra8UnormSrgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or(SurfaceError::Incompatible)?;

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

    /// The drawable size the surface is configured for, in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn format(&self) -> TextureFormat {
        self.config.format
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(device, &self.config);
    }

    /// Take hold of the next drawable.
    ///
    /// `Ok(None)` means the swapchain was stale and has been rebuilt, so there
    /// is no frame this tick. That is the ordinary consequence of a window
    /// being resized or dragged to another display, not a failure — but the
    /// caller must treat it as *not drawn* and keep whatever "needs render"
    /// flag it has, or the picture waits for the next thing that happens to
    /// ask for a frame.
    ///
    /// Every caller acquires through here so the recovery lives in one place.
    pub fn acquire(
        &self,
        device: &wgpu::Device,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfaceError> {
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(device, &self.config);
                Ok(None)
            }
            Err(e) => Err(SurfaceError::Acquire(e)),
        }
    }

    /// Fill the surface with one colour and present it.
    ///
    /// The proof that the layer path works, and afterwards the thing that
    /// draws the background behind a photograph that has not loaded.
    ///
    /// Returns whether a frame was actually presented; see [`Attached::acquire`].
    pub fn present_clear(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        colour: [f64; 4],
    ) -> Result<bool, SurfaceError> {
        let Some(frame) = self.acquire(device)? else {
            return Ok(false);
        };
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
        Ok(true)
    }
}
