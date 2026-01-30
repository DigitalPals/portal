//! VNC framebuffer widget for Iced
//!
//! Uses a custom wgpu Shader widget to render the VNC framebuffer directly
//! to a GPU texture via `queue.write_texture()`. This avoids the flickering
//! caused by `Handle::from_rgba()` creating a new unique ID each frame,
//! which triggers GPU texture cache eviction in Iced's image pipeline.

use std::sync::Arc;

use iced::widget::shader;
use iced::{Element, Length, Rectangle};
use parking_lot::Mutex;

use crate::message::Message;
use crate::vnc::session::FrameBuffer;

/// Create a VNC framebuffer element using a custom shader widget.
///
/// The shader updates the GPU texture in-place each frame, avoiding
/// the flicker that the Image widget causes with new Handle IDs.
pub fn vnc_framebuffer<'a>(framebuffer: &Arc<Mutex<FrameBuffer>>) -> Element<'a, Message> {
    let program = VncProgram {
        framebuffer: framebuffer.clone(),
    };

    shader::Shader::new(program)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The shader program that reads the VNC framebuffer and renders it.
struct VncProgram {
    framebuffer: Arc<Mutex<FrameBuffer>>,
}

/// Primitive carrying framebuffer snapshot to the GPU pipeline.
#[derive(Debug)]
struct VncPrimitive {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// The wgpu pipeline that owns the texture and renders it.
struct VncPipeline {
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    /// Current texture dimensions (to detect when resize is needed)
    tex_width: u32,
    tex_height: u32,
}

use iced::wgpu;
use wgpu::util::DeviceExt;

impl std::fmt::Debug for VncPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VncPipeline")
            .field("tex_width", &self.tex_width)
            .field("tex_height", &self.tex_height)
            .finish()
    }
}

// Fullscreen quad vertices: position (x, y) + texcoord (u, v)
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

const QUAD_VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, -1.0],
        tex_coords: [0.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0],
        tex_coords: [1.0, 1.0],
    },
    Vertex {
        position: [-1.0, 1.0],
        tex_coords: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, -1.0],
        tex_coords: [1.0, 1.0],
    },
    Vertex {
        position: [1.0, 1.0],
        tex_coords: [1.0, 0.0],
    },
    Vertex {
        position: [-1.0, 1.0],
        tex_coords: [0.0, 0.0],
    },
];

const SHADER_SRC: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    return out;
}

@group(0) @binding(0)
var t_framebuffer: texture_2d<f32>;
@group(0) @binding(1)
var s_framebuffer: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_framebuffer, s_framebuffer, in.tex_coords);
    return vec4<f32>(color.rgb, 1.0);
}
"#;

impl VncPipeline {
    fn create_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vnc_framebuffer"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        texture: &wgpu::Texture,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vnc_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

impl shader::Primitive for VncPrimitive {
    type Pipeline = VncPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        if self.width == 0 || self.height == 0 {
            return;
        }

        let expected = (self.width * self.height * 4) as usize;
        if self.pixels.len() != expected {
            return;
        }

        // Recreate texture if dimensions changed
        if self.width != pipeline.tex_width || self.height != pipeline.tex_height {
            pipeline.texture = VncPipeline::create_texture(device, self.width, self.height);
            pipeline.bind_group = VncPipeline::create_bind_group(
                device,
                &pipeline.bind_group_layout,
                &pipeline.texture,
                &pipeline.sampler,
            );
            pipeline.tex_width = self.width;
            pipeline.tex_height = self.height;
        }

        // Upload pixel data in-place — no new Handle IDs, no cache eviction
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &pipeline.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.width),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        if self.width == 0 || self.height == 0 {
            return true;
        }

        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.set_vertex_buffer(0, pipeline.vertex_buffer.slice(..));
        render_pass.draw(0..6, 0..1);
        true
    }
}

impl shader::Pipeline for VncPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vnc_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vnc_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vnc_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vnc_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vnc_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Initial 1x1 texture (will be resized on first frame)
        let texture = Self::create_texture(device, 1, 1);
        let bind_group = Self::create_bind_group(device, &bind_group_layout, &texture, &sampler);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vnc_vertex_buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            pipeline,
            texture,
            bind_group,
            bind_group_layout,
            sampler,
            vertex_buffer,
            tex_width: 1,
            tex_height: 1,
        }
    }
}

impl shader::Program<Message> for VncProgram {
    type State = ();
    type Primitive = VncPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        let fb = self.framebuffer.lock();
        VncPrimitive {
            pixels: fb.pixels.clone(),
            width: fb.width,
            height: fb.height,
        }
    }
}
