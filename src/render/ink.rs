//! Ink and cel shading: the comic-book look (Borderlands-style outlines and
//! softly banded light). One full-screen pass over the opaque scene, after
//! blocks, meshes and decals and before translucent draws, particles and
//! post, so ink lands on rock, characters and props but never on effects.
//!
//! Quasar has no toon shading or edge-detection post effect, so this pass
//! reads `SceneTarget`'s colour (copied to a scratch texture) and depth
//! directly (see QUASAR-ISSUES).

use quasar_frame::{Gpu, GraphicsResult, texture_2d};
use quasar_render_3d::SceneTarget;
use vulkanic_gal::util::{BufferInitDescriptor, DeviceExt};
use vulkanic_gal::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BufferBindingType, BufferUsages,
    ColorTargetState, ColorWrites, CommandBuffer, Extent3d, FragmentState, LoadOp, Operations,
    PipelineLayoutDescriptor, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TexelCopyTextureInfo, Texture, TextureSampleType, TextureUsages, TextureView,
    TextureViewDimension, VertexState,
};

/// How the ink and bands look.
#[derive(Clone, Copy, Debug)]
pub struct InkStyle {
    /// Relative depth jump that draws a silhouette line.
    pub silhouette: f32,
    /// Relative slope change that draws a crease line (block edges).
    pub crease: f32,
    /// Brightness bands and how strongly shading snaps to them (0..1).
    pub bands: f32,
    pub band_strength: f32,
    /// Ink colour (multiplied in) and strength.
    pub ink: [f32; 3],
    pub ink_strength: f32,
}

impl Default for InkStyle {
    fn default() -> Self {
        Self {
            silhouette: 0.025,
            crease: 0.006,
            bands: 4.0,
            band_strength: 0.45,
            ink: [0.07, 0.055, 0.05],
            ink_strength: 0.92,
        }
    }
}

pub struct InkPass {
    layout: BindGroupLayout,
    pipeline: RenderPipeline,
    /// A copy of the scene colour to read while the pass writes the scene.
    scratch: Option<(Texture, TextureView)>,
}

impl InkPass {
    pub fn new(gpu: &Gpu) -> GraphicsResult<Self> {
        let layout = gpu
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("ink"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: false },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Depth,
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let module = gpu.device.create_shader_module(ShaderModuleDescriptor {
            label: Some("ink"),
            source: ShaderSource::Wgsl(include_str!("ink.wgsl").into()),
        });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("ink"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("ink"),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(FragmentState {
                    module: &module,
                    entry_point: Some("fs_ink"),
                    compilation_options: Default::default(),
                    targets: &[Some(ColorTargetState {
                        format: quasar_render_3d::SCENE_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        Ok(Self {
            layout,
            pipeline,
            scratch: None,
        })
    }

    /// Ink `scene` (drawn with a perspective camera from `near` to `far`).
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        scene: &SceneTarget,
        near: f32,
        far: f32,
        style: &InkStyle,
    ) -> GraphicsResult<CommandBuffer> {
        let (width, height) = scene.size();
        if self
            .scratch
            .as_ref()
            .is_none_or(|(t, _)| (t.width(), t.height()) != (width, height))
        {
            let texture = texture_2d(
                &gpu.device,
                "ink scratch",
                width,
                height,
                quasar_render_3d::SCENE_FORMAT,
                TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            )?;
            let view = texture.create_view(&Default::default());
            self.scratch = Some((texture, view));
        }
        let (scratch, scratch_view) = self.scratch.as_ref().expect("created above");
        // Thicker lines at higher resolutions, so they read the same.
        let thickness = (height as f32 / 480.0).round().max(1.0);
        let values: [f32; 16] = [
            width as f32,
            height as f32,
            near,
            far,
            thickness,
            style.silhouette,
            style.crease,
            style.bands,
            style.band_strength,
            style.ink_strength,
            0.0,
            0.0,
            style.ink[0],
            style.ink[1],
            style.ink[2],
            1.0,
        ];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let params = gpu.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("ink params"),
            contents: &bytes,
            usage: BufferUsages::UNIFORM,
        });
        let group = gpu.device.create_bind_group(&BindGroupDescriptor {
            label: Some("ink"),
            layout: &self.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(scratch_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(scene.depth_view()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let mut encoder = gpu.encoder("ink");
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: scene.color_texture(),
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            TexelCopyTextureInfo {
                texture: scratch,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        {
            let colors = [Some(RenderPassColorAttachment {
                view: scene.color_view(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("ink"),
                color_attachments: &colors,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(encoder.finish())
    }

    pub fn release(&mut self, _gpu: &Gpu) -> GraphicsResult<()> {
        self.scratch = None;
        Ok(())
    }
}
