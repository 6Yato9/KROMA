//! GPU device acquisition.

use crate::RenderError;

/// The GPU handles everything else borrows.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    pub async fn new() -> Result<Self, RenderError> {
        Self::from_instance(Self::create_instance(), None).await
    }

    /// Create the instance separately, so a surface can be made from it *before*
    /// the adapter is chosen.
    ///
    /// This split is not optional. A `Surface` belongs to the `Instance` that
    /// created it, and an adapter obtained from a different instance cannot
    /// present to it — the failure is a panic deep inside wgpu's resource
    /// storage ("Surface does not exist"), which is a long way from the cause.
    pub fn create_instance() -> wgpu::Instance {
        wgpu::Instance::new(&wgpu::InstanceDescriptor::default())
    }

    /// Blocking convenience for tests and CLI entry points.
    pub fn new_blocking() -> Result<Self, RenderError> {
        pollster::block_on(Self::new())
    }

    /// Acquire a device from an existing instance, optionally one guaranteed to
    /// be compatible with a surface already created from that same instance.
    ///
    /// Passing the surface also matters on machines with more than one GPU:
    /// without it, the adapter picked may not be able to present to the window.
    pub async fn from_instance(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface,
            })
            .await
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("pe-device"),
                required_features: wgpu::Features::empty(),
                // Default limits, deliberately. They are the WebGPU baseline,
                // which keeps the door open for the browser and for mobile.
                // Raise them only when something concrete needs it.
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| RenderError::NoDevice(e.to_string()))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// One-line description of the GPU actually in use, for the About box and
    /// for bug reports.
    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend)
    }

    /// Whether the working texture format can be filtered and rendered to.
    ///
    /// `Rgba16Float` is filterable in core WebGPU, so this should always be
    /// true — it is checked rather than assumed because a false here would
    /// otherwise surface as silently wrong output much later.
    pub fn supports_working_format(&self) -> bool {
        let f = self
            .adapter
            .get_texture_format_features(crate::texture::WORKING_FORMAT)
            .allowed_usages;
        f.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            && f.contains(wgpu::TextureUsages::TEXTURE_BINDING)
    }
}
