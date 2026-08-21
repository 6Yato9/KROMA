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
}

/// Renders one texture into another, rotating the gamut on the way.
pub struct TransformPass {
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
            pipeline,
            bind_group_layout,
            sampler,
            uniform,
        }
    }

    /// Encode a pass converting `src` into `dst_view`, rotating from `from` to
    /// `to`.
    ///
    /// Only the gamut is handled here; the transfer functions belong to the
    /// texture formats. See `shaders/transform.wgsl`.
    pub fn encode(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        from: &ColorSpace,
        to: &ColorSpace,
    ) {
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
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
        let dst = ImageTexture::new_working(&gpu.device, source.width, source.height, "working");
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
        );
        gpu.queue.submit([encoder.finish()]);
        dst
    }
}

/// The format the transform pass writes when targeting an intermediate.
pub const INTERMEDIATE_FORMAT: wgpu::TextureFormat = WORKING_FORMAT;
