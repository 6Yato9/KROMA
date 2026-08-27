//! The colour-space transform pass — the two ends of the pipeline.
//!
//! At M0 this is the entire GPU pipeline: source in, working space, screen out.
//! M1 inserts the effect rows between the two ends without changing either.

use bytemuck::{Pod, Zeroable};
use pe_color::{ColorSpace, Mat3, space};

use crate::device::GpuContext;
use crate::texture::{ImageTexture, WORKING_FORMAT};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TransformUniform {
    /// `mat3x3<f32>` in WGSL: three columns, each padded to 16 bytes.
    gamut: [[f32; 4]; 3],
    /// The two columns of the affine map's linear part.
    axes: [f32; 4],
    /// Its translation, plus the blank-outside flag.
    origin: [f32; 4],
}

/// A rectangle of a render target, in pixels, with the origin top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether this rectangle has no area.
    ///
    /// Worth asking, because a wipe dragged to either end asks for exactly
    /// that and wgpu does not stop it: `set_viewport` is refused only for a
    /// negative or oversized rectangle, so a zero-sized one goes on to the
    /// driver, and Vulkan requires a viewport wider than nothing. See
    /// [`TransformPass::encode`].
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Which part of its target a pass draws in, and how the picture gets there.
///
/// The two are different operations and the difference is the difference
/// between the comparison modes. A wipe's halves are one picture with a seam,
/// so neither may be moved or resized by so much as a pixel; a side by side's
/// are two pictures, so both are shrunk to fit where one did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Squeeze the whole picture into this rectangle: `set_viewport`.
    Into(Rect),
    /// Draw the picture where it would have gone anyway and let only this
    /// rectangle of it through: `set_scissor_rect`.
    Through(Rect),
}

impl Part {
    pub const fn rect(self) -> Rect {
        match self {
            Part::Into(r) | Part::Through(r) => r,
        }
    }
}

/// Where in its target a transform pass draws, and what happens to the rest.
///
/// [`Placement::WHOLE`] is what every pass in the pipeline wants but the second
/// one of a comparison: all of the target, cleared first, one picture. A
/// comparison is two passes into one target, and the second must not erase the
/// first — which is the only reason the clear is a value here rather than the
/// fixed black it used to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The part of the target to draw in. `None` is all of it.
    pub part: Option<Part>,
    /// What the whole target becomes before the draw. `None` keeps what is
    /// already there.
    pub clear: Option<wgpu::Color>,
}

impl Placement {
    /// All of the target, over black.
    pub const WHOLE: Placement = Placement {
        part: None,
        clear: Some(wgpu::Color::BLACK),
    };
}

/// Renders one texture into another, rotating the gamut on the way.
pub struct TransformPass {
    /// How the next `encode` reads its source. Reset to the whole frame after
    /// each mapped call, so the plain helpers stay plain.
    sampling: crate::Sampling,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
}

impl TransformPass {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("transform"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/transform.wgsl").into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transform-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transform-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transform-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_transform"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("transform-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("transform-uniform"),
            size: std::mem::size_of::<TransformUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            sampling: crate::Sampling::WHOLE,
            pipeline,
            bind_group_layout,
            sampler,
            uniform,
        }
    }

    /// Decode a sub-rectangle of the source straight into a working texture.
    ///
    /// This is what makes zooming honest: at 100% the preview renders the
    /// visible rectangle at its own resolution rather than magnifying a
    /// downscaled render of the whole frame.
    pub fn to_working_in(
        &mut self,
        gpu: &GpuContext,
        source: &ImageTexture,
        source_space: &ColorSpace,
        width: u32,
        height: u32,
        region: crate::Region,
    ) -> ImageTexture {
        self.to_working_mapped(
            gpu,
            source,
            source_space,
            width,
            height,
            crate::Sampling::WHOLE.within(region),
        )
    }

    /// The same, but reading through an arbitrary affine map.
    ///
    /// This is the one call that crop, straighten, flip and quarter-turn all
    /// go through, composed with whatever rectangle the preview is zoomed
    /// into. `blank_outside` decides what happens where the map falls off the
    /// source, which only comes up while the crop tool is open.
    pub fn to_working_mapped(
        &mut self,
        gpu: &GpuContext,
        source: &ImageTexture,
        source_space: &ColorSpace,
        width: u32,
        height: u32,
        sampling: crate::Sampling,
    ) -> ImageTexture {
        self.sampling = sampling;
        let out = self.to_working_sized(gpu, source, source_space, width, height);
        self.sampling = crate::Sampling::WHOLE;
        out
    }

    /// Resample a working texture to a different size, changing nothing else.
    ///
    /// Used by export to step down towards a requested output size. It happens
    /// in working space on purpose: averaging pixels is only meaningful in
    /// linear light, and downscaling a gamma-encoded image darkens it.
    pub fn resample(
        &self,
        gpu: &GpuContext,
        source: &ImageTexture,
        width: u32,
        height: u32,
    ) -> ImageTexture {
        self.to_working_sized(gpu, source, &space::ACESCG, width, height)
    }

    /// Encode a pass converting `src` into `dst_view`, rotating from `from` to
    /// `to`.
    ///
    /// Only the gamut is handled here; the transfer functions belong to the
    /// texture formats. See `shaders/transform.wgsl`.
    ///
    /// `at` is [`Placement::WHOLE`] for every pass that is the only thing
    /// drawing into its target. It is one parameter rather than a second entry
    /// point because a comparison's two halves must not diverge from the plain
    /// path or from each other, and a duplicated body is where they would.
    ///
    /// **An empty [`Rect`] draws nothing rather than reaching the driver.** A
    /// wipe at 0.0 is a place a user will drag to, and wgpu's own validation
    /// lets a zero-sized viewport through to a backend that may not — see
    /// [`Rect::is_empty`]. The clear, if there is one, still happens: the
    /// surround has to be painted whatever ends up on it.
    #[allow(
        clippy::too_many_arguments,
        reason = "one pass, described: the two textures, the two spaces and where it lands"
    )]
    pub fn encode(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        from: &ColorSpace,
        to: &ColorSpace,
        at: Placement,
    ) {
        let empty = at.part.is_some_and(|p| p.rect().is_empty());
        if empty && at.clear.is_none() {
            return;
        }
        let (device, queue) = (&gpu.device, &gpu.queue);
        let gamut = if from.primaries == to.primaries {
            Mat3::IDENTITY
        } else {
            space::gamut_matrix(&from.primaries, &to.primaries)
        };
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&TransformUniform {
                gamut: gamut.to_wgsl_mat3(),
                axes: [
                    self.sampling.map.x_axis[0],
                    self.sampling.map.x_axis[1],
                    self.sampling.map.y_axis[0],
                    self.sampling.map.y_axis[1],
                ],
                origin: [
                    self.sampling.map.origin[0],
                    self.sampling.map.origin[1],
                    if self.sampling.blank_outside {
                        1.0
                    } else {
                        0.0
                    },
                    0.0,
                ],
            }),
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transform-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("transform-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match at.clear {
                        Some(colour) => wgpu::LoadOp::Clear(colour),
                        None => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if empty {
            return;
        }
        match at.part {
            Some(Part::Into(r)) => pass.set_viewport(
                r.x as f32,
                r.y as f32,
                r.width as f32,
                r.height as f32,
                0.0,
                1.0,
            ),
            Some(Part::Through(r)) => pass.set_scissor_rect(r.x, r.y, r.width, r.height),
            None => {}
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Convenience: decode a source texture into a freshly allocated working
    /// texture. This is the "linearize + gamut" step from the pipeline diagram.
    pub fn to_working(
        &self,
        gpu: &GpuContext,
        source: &ImageTexture,
        source_space: &ColorSpace,
    ) -> ImageTexture {
        self.to_working_sized(gpu, source, source_space, source.width, source.height)
    }

    /// The same, at an explicit size.
    ///
    /// The interactive preview uses this to downsample to the viewport before
    /// the stack runs. Without it, a 24MP image would allocate a 192 MB working
    /// texture per row, and the stage cache — which is what keeps sliders
    /// responsive — would be unaffordable. The fullscreen triangle plus a
    /// linear sampler gives a bilinear downsample for free.
    pub fn to_working_sized(
        &self,
        gpu: &GpuContext,
        source: &ImageTexture,
        source_space: &ColorSpace,
        width: u32,
        height: u32,
    ) -> ImageTexture {
        let dst = ImageTexture::new_working(&gpu.device, width, height, "working");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("to-working"),
            });
        self.encode(
            gpu,
            &mut encoder,
            &source.view,
            &dst.view,
            source_space,
            &space::ACESCG,
            Placement::WHOLE,
        );
        gpu.queue.submit([encoder.finish()]);
        dst
    }
}

/// The format the transform pass writes when targeting an intermediate.
pub const INTERMEDIATE_FORMAT: wgpu::TextureFormat = WORKING_FORMAT;
