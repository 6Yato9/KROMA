//! Running an effect stack on the GPU.
//!
//! One pipeline per registry entry, built once from
//! `common.wgsl + effects/<name>.wgsl + epilogue.wgsl`. Because the uniform
//! layout is shared, adding an effect is a registry entry plus a shader file —
//! no Rust plumbing at all.
//!
//! # Why the preview renders at screen resolution
//!
//! The stage cache keeps one texture per row so that editing row 9 of 12
//! re-runs four rows instead of twelve. At full resolution that is impossible:
//! a 24MP `Rgba16Float` texture is 192 MB, so a twelve-row stack would want
//! 2.3 GB of VRAM. At 1920x1080 the same stack costs about 200 MB, which is
//! fine.
//!
//! So interactive rendering is capped to the viewport and export runs the
//! stack once at full resolution with no caching. Both paths run identical
//! shaders — only the size and the caching differ.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use pe_color::WorkingSpace;
use pe_core::{Document, ParamValue, Stack, StackRow};
use pe_effects::{EffectDef, PARAM_SLOTS, pack_all};

use crate::cache::{RenderContext, StageCache};
use crate::device::GpuContext;
use crate::texture::{ImageTexture, WORKING_FORMAT};

/// LUT texture is 256 wide by 4 rows: luma, red, green, blue.
const LUT_WIDTH: u32 = 256;
const LUT_ROWS: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectUniform {
    image_size: [f32; 2],
    inv_size: [f32; 2],
    opacity: f32,
    blend_mode: u32,
    space_is_log: u32,
    scale: f32,
    seed: f32,
    _pad: [f32; 3],
    region: [f32; 4],
    p: [[f32; 4]; 12],
}

/// The per-pass resources an effect needs beyond its input and output
/// textures.
///
/// The preview gives every row its own set, because all its passes go into one
/// command encoder and a shared buffer would only ever show the last write.
/// Export submits per pass, so it reuses a single set — see [`crate::export`].
pub struct Scratch {
    uniform: wgpu::Buffer,
    lut: wgpu::Texture,
    lut_view: wgpu::TextureView,
}

/// Per-row GPU resources. One of these exists for each row in the stack.
struct Stage {
    texture: ImageTexture,
    scratch: Scratch,
}

pub struct EffectRenderer {
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<&'static str, wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
    stages: Vec<Stage>,
    cache: StageCache,
    /// Which texture holds each row's output. `None` means "the row was a
    /// no-op, so its output is whatever its input was" — which avoids a
    /// full-texture copy for every disabled row.
    resolved: Vec<Option<usize>>,
    size: (u32, u32),
    last_passes: usize,
    /// Which part of the frame the next render covers. Set before `render`;
    /// export leaves it at `Region::FULL`.
    region: crate::Region,
}

impl EffectRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect-bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // Non-filterable: the curve LUT is read with textureLoad,
                    // never sampled. Declaring it filterable would demand a
                    // format guarantee we do not need.
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let mut pipelines = HashMap::new();
        for effect in pe_effects::all() {
            let source = assemble_shader(effect.shader);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(effect.key),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(effect.key),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: WORKING_FORMAT,
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
            pipelines.insert(effect.key, pipeline);
        }

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            layout,
            pipelines,
            sampler,
            stages: Vec::new(),
            cache: StageCache::new(),
            resolved: Vec::new(),
            size: (0, 0),
            last_passes: 0,
            region: crate::Region::FULL,
        }
    }

    /// Set the rectangle of the frame subsequent renders cover.
    ///
    /// Stored on the renderer rather than threaded through `render` because
    /// it applies to every pass in the stack uniformly, and the encode path
    /// already carries as many arguments as it can hold.
    pub fn set_region(&mut self, region: crate::Region) {
        self.region = region;
    }

    pub fn region(&self) -> crate::Region {
        self.region
    }

    /// Number of effect passes the last render actually executed.
    ///
    /// The interactivity metric: dragging one slider in a deep stack should
    /// report 1, not the stack depth.
    pub fn last_pass_count(&self) -> usize {
        self.last_passes
    }

    /// Render `doc`'s stack over `source`, returning the final texture.
    ///
    /// `source` must already be in the working space (ACEScg linear).
    pub fn render<'a>(
        &'a mut self,
        gpu: &GpuContext,
        source: &'a ImageTexture,
        doc: &Document,
        source_id: u64,
    ) -> &'a ImageTexture {
        let (width, height) = source.size();
        let context = RenderContext {
            source: source_id,
            width,
            height,
            color: crate::color_fingerprint(&doc.color),
            view: self.region.cache_key(),
        };

        let plan = self.cache.plan(&doc.stack, context, row_is_inert);
        self.ensure_stages(gpu, doc.stack.len(), width, height);
        self.resolved.resize(doc.stack.len(), None);
        self.last_passes = plan.execute.len();

        if !plan.execute.is_empty() {
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("effect-stack"),
                });

            for i in plan.first_dirty..doc.stack.len() {
                let row = &doc.stack.rows[i];
                let input_index = if i == 0 { None } else { self.resolved[i - 1] };

                if row_is_inert(row) {
                    // A skipped row outputs exactly its input. Recording the
                    // alias avoids a full-texture copy per disabled row, which
                    // matters when someone A/Bs a deep stack with the enable
                    // toggles.
                    self.resolved[i] = input_index;
                    continue;
                }

                let Some(effect) = pe_effects::by_key(&row.effect) else {
                    // An effect this build does not know about. The document
                    // keeps the row so it round-trips, but there is nothing to
                    // render, so pass the image through untouched.
                    self.resolved[i] = input_index;
                    continue;
                };

                self.encode_row(gpu, &mut encoder, source, i, input_index, row, effect);
                self.resolved[i] = Some(i);
            }

            gpu.queue.submit([encoder.finish()]);
        }

        self.cache.store_plan(&doc.stack, &plan);

        // The last row that actually produced a texture.
        match self.resolved.last().copied().flatten() {
            Some(i) => &self.stages[i].texture,
            None => source,
        }
    }

    /// Allocate a reusable [`Scratch`]. Export uses one of these for the whole
    /// stack.
    pub fn scratch(&self, device: &wgpu::Device) -> Scratch {
        new_scratch(device)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "an explicit description of one pass; bundling it would only move the arguments"
    )]
    fn encode_row(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        source: &ImageTexture,
        index: usize,
        input_index: Option<usize>,
        row: &StackRow,
        effect: &EffectDef,
    ) {
        let stage = &self.stages[index];
        let (width, height) = stage.texture.size();
        let input_view = match input_index {
            Some(i) => &self.stages[i].texture.view,
            None => &source.view,
        };
        self.encode_into(
            gpu,
            encoder,
            &stage.scratch,
            input_view,
            &stage.texture.view,
            (width, height),
            // Spatial effects need to know how much smaller the preview is than
            // the source, or grain and halation would change size on export.
            width as f32 / source.width.max(1) as f32,
            row,
            effect,
        );
    }

    /// Encode one effect pass. Shared by the preview and the export paths, so
    /// there is exactly one place where a row becomes GPU work.
    #[allow(clippy::too_many_arguments, reason = "an explicit pass description")]
    pub fn encode_into(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        scratch: &Scratch,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        size: (u32, u32),
        scale: f32,
        row: &StackRow,
        effect: &EffectDef,
    ) {
        let (width, height) = size;
        let stage = scratch;

        let uniform = EffectUniform {
            image_size: [width as f32, height as f32],
            inv_size: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            opacity: row.opacity,
            blend_mode: row.blend.as_index(),
            space_is_log: u32::from(effect.space == WorkingSpace::Log),
            scale,
            // Derived from the row id, so grain does not crawl when an
            // unrelated slider moves, and two grain rows do not correlate into
            // a visible pattern.
            seed: (row.id.0 % 991) as f32 * 37.0,
            _pad: [0.0; 3],
            region: self.region.to_array(),
            p: to_vec4s(pack_all(effect, &row.params)),
        };
        gpu.queue
            .write_buffer(&stage.uniform, 0, bytemuck::bytes_of(&uniform));

        if effect.key == "curves" {
            self.upload_lut(gpu, stage, row);
        }

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: stage.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&stage.lut_view),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(effect.key),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines[effect.key]);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn upload_lut(&self, gpu: &GpuContext, stage: &Scratch, row: &StackRow) {
        let mut data = Vec::with_capacity((LUT_WIDTH * LUT_ROWS) as usize);
        for key in ["luma", "red", "green", "blue"] {
            match row.params.get(key).and_then(ParamValue::as_curve) {
                Some(curve) => data.extend_from_slice(&curve.bake()),
                // A missing curve is the identity, not a black row. Getting
                // this wrong would make a fresh Curves layer crush the image.
                None => data.extend((0..LUT_WIDTH).map(|i| i as f32 / (LUT_WIDTH - 1) as f32)),
            }
        }

        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &stage.lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LUT_WIDTH * 4),
                rows_per_image: Some(LUT_ROWS),
            },
            wgpu::Extent3d {
                width: LUT_WIDTH,
                height: LUT_ROWS,
                depth_or_array_layers: 1,
            },
        );
    }

    fn ensure_stages(&mut self, gpu: &GpuContext, count: usize, width: u32, height: u32) {
        if self.size != (width, height) {
            self.stages.clear();
            self.resolved.clear();
            self.size = (width, height);
        }
        while self.stages.len() < count {
            let i = self.stages.len();
            self.stages.push(Stage {
                texture: ImageTexture::new_working(
                    &gpu.device,
                    width,
                    height,
                    &format!("stage-{i}"),
                ),
                scratch: new_scratch(&gpu.device),
            });
        }
        // Release stages beyond the current stack. Each one is a full working
        // texture, so holding onto them after the user deletes rows would keep
        // tens of megabytes of VRAM alive for nothing.
        self.stages.truncate(count);
    }

    /// Drop cached stages, forcing a full re-render. Used when the source image
    /// changes.
    pub fn invalidate(&mut self) {
        self.cache.clear();
        self.resolved.clear();
    }
}

fn new_scratch(device: &wgpu::Device) -> Scratch {
    let lut = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("curve-lut"),
        size: wgpu::Extent3d {
            width: LUT_WIDTH,
            height: LUT_ROWS,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
    Scratch {
        uniform: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect-uniform"),
            size: std::mem::size_of::<EffectUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        lut,
        lut_view,
    }
}

fn to_vec4s(slots: [f32; PARAM_SLOTS]) -> [[f32; 4]; 12] {
    let mut out = [[0.0f32; 4]; 12];
    for (i, chunk) in slots.as_chunks::<4>().0.iter().enumerate() {
        out[i].copy_from_slice(chunk);
    }
    out
}

/// Concatenate the shader prelude, the effect body, and the epilogue.
///
/// WGSL has no `#include`, and function declarations must precede use, so the
/// order is fixed: helpers, then `effect()`, then the `fs_main` that calls it.
fn assemble_shader(name: &str) -> String {
    let body = effect_source(name);
    format!(
        "{}\n{}\n{}",
        include_str!("../../../shaders/common.wgsl"),
        body,
        include_str!("../../../shaders/epilogue.wgsl"),
    )
}

/// Effect bodies are embedded at compile time so the binary is self-contained.
fn effect_source(name: &str) -> &'static str {
    match name {
        "exposure" => include_str!("../../../shaders/effects/exposure.wgsl"),
        "white_balance" => include_str!("../../../shaders/effects/white_balance.wgsl"),
        "contrast" => include_str!("../../../shaders/effects/contrast.wgsl"),
        "curves" => include_str!("../../../shaders/effects/curves.wgsl"),
        "hsl" => include_str!("../../../shaders/effects/hsl.wgsl"),
        "split_tone" => include_str!("../../../shaders/effects/split_tone.wgsl"),
        "primaries" => include_str!("../../../shaders/effects/primaries.wgsl"),
        "grain" => include_str!("../../../shaders/effects/grain.wgsl"),
        "halation" => include_str!("../../../shaders/effects/halation.wgsl"),
        "vignette" => include_str!("../../../shaders/effects/vignette.wgsl"),
        "bloom" => include_str!("../../../shaders/effects/bloom.wgsl"),
        "dehaze" => include_str!("../../../shaders/effects/dehaze.wgsl"),
        "film_damage" => include_str!("../../../shaders/effects/film_damage.wgsl"),
        "tone" => include_str!("../../../shaders/effects/tone.wgsl"),
        "presence" => include_str!("../../../shaders/effects/presence.wgsl"),
        "colour" => include_str!("../../../shaders/effects/colour.wgsl"),
        "log_wheels" => include_str!("../../../shaders/effects/log_wheels.wgsl"),
        "colour_mixer" => include_str!("../../../shaders/effects/colour_mixer.wgsl"),
        other => panic!("no shader source embedded for {other:?}"),
    }
}

/// Whether a row would leave the image untouched, so the renderer can skip it.
///
/// Wider than `StackRow::is_noop`, which only knows about the enable toggle and
/// the blend. This also asks the registry whether the parameters sit at their
/// neutral values, which matters now that every document carries nine pinned
/// panels that start out doing nothing — without it a freshly opened photo
/// would burn nine full-screen passes a frame to produce itself.
///
/// An effect this build does not recognise is inert too: the row round-trips
/// through the document, but there is nothing to render.
pub fn row_is_inert(row: &StackRow) -> bool {
    row.is_noop()
        || match pe_effects::by_key(&row.effect) {
            Some(def) => def.is_neutral(&row.params),
            None => true,
        }
}

/// Stack helper: the rows an export would actually execute.
pub fn active_rows(stack: &Stack) -> usize {
    stack.iter().filter(|r| !r.is_noop()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_effect_has_an_embedded_shader() {
        // A registry entry without a shader would panic at pipeline creation,
        // i.e. at startup on a user machine. Catch it here instead.
        for e in pe_effects::all() {
            let src = effect_source(e.shader);
            assert!(
                src.contains("fn effect("),
                "{} does not define fn effect()",
                e.key
            );
        }
    }

    #[test]
    fn assembled_shaders_contain_all_three_parts() {
        let s = assemble_shader("exposure");
        assert!(s.contains("struct EffectUniform"), "prelude missing");
        assert!(s.contains("fn effect("), "effect body missing");
        assert!(s.contains("fn fs_main("), "epilogue missing");
        assert!(
            s.find("fn effect(").unwrap() < s.find("fn fs_main(").unwrap(),
            "effect() must be declared before fs_main uses it"
        );
    }

    #[test]
    fn the_uniform_matches_the_shader_layout() {
        // Three 16-byte blocks of scalars, the region, then twelve vec4s.
        assert_eq!(std::mem::size_of::<EffectUniform>(), 48 + 16 + 16 * 12);
        assert_eq!(std::mem::align_of::<EffectUniform>(), 4);
    }

    #[test]
    fn params_pack_into_vec4_rows_in_order() {
        let mut slots = [0.0f32; PARAM_SLOTS];
        for (i, s) in slots.iter_mut().enumerate() {
            *s = i as f32;
        }
        let v = to_vec4s(slots);
        assert_eq!(v[0], [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(v[2], [8.0, 9.0, 10.0, 11.0]);
    }
}
