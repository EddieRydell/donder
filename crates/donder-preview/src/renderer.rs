use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{
    PreviewCamera, PreviewColor, PreviewInstance, PreviewScene, PreviewSize, PreviewStyle,
};

const SHADER: &str = include_str!("preview.wgsl");

pub struct PreviewRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    max_surface_dimension: u32,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    color_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: usize,
    uploaded_revision: Option<u64>,
    uniform_revision: Option<u64>,
    uniform_size: Option<PreviewSize>,
    uniform_style: Option<(u32, u32)>,
}

impl PreviewRenderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        size: PreviewSize,
    ) -> Result<Self, PreviewRendererError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| PreviewRendererError::Adapter(error.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Donder Preview Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| PreviewRendererError::Device(error.to_string()))?;
        let max_surface_dimension = device.limits().max_texture_dimension_2d;
        let size = size.clamp_to(max_surface_dimension);
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(PreviewRendererError::NoSurfaceFormat)?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .or_else(|| capabilities.present_modes.first().copied())
            .ok_or(PreviewRendererError::NoPresentMode)?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Donder Preview Uniforms"),
            contents: bytemuck::bytes_of(&PreviewUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Donder Preview Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Donder Preview Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Donder Preview Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Donder Preview Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Donder Preview Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<PreviewInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<PreviewColor>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 0,
                            shader_location: 1,
                        }],
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let instance_buffer = empty_buffer(&device, "Donder Preview Instances");
        let color_buffer = empty_buffer(&device, "Donder Preview Colors");

        Ok(Self {
            device,
            queue,
            max_surface_dimension,
            config,
            pipeline,
            bind_group,
            uniform_buffer,
            instance_buffer,
            color_buffer,
            instance_capacity: 1,
            instance_count: 0,
            uploaded_revision: None,
            uniform_revision: None,
            uniform_size: None,
            uniform_style: None,
        })
    }

    pub fn render(
        &mut self,
        surface: &wgpu::Surface<'_>,
        size: PreviewSize,
        scene: &PreviewScene,
        colors: &[PreviewColor],
        style: PreviewStyle,
    ) -> Result<PreviewRenderOutcome, PreviewRendererError> {
        if colors.len() != scene.instances.len() {
            return Err(PreviewRendererError::ColorCount {
                colors: colors.len(),
                instances: scene.instances.len(),
            });
        }
        let size = size.clamp_to(self.max_surface_dimension);
        if self.config.width != size.width || self.config.height != size.height {
            self.config.width = size.width;
            self.config.height = size.height;
            surface.configure(&self.device, &self.config);
            self.uniform_size = None;
        }
        if self.uploaded_revision != Some(scene.revision) {
            self.upload_scene(scene);
        }
        let style_key = (
            style.canvas_fill_ratio.to_bits(),
            style.minimum_radius_pixels.to_bits(),
        );
        if self.uniform_revision != Some(scene.revision)
            || self.uniform_size != Some(size)
            || self.uniform_style != Some(style_key)
        {
            self.update_uniforms(scene, size, style);
            self.uniform_revision = Some(scene.revision);
            self.uniform_size = Some(size);
            self.uniform_style = Some(style_key);
        }
        if !colors.is_empty() {
            self.queue
                .write_buffer(&self.color_buffer, 0, bytemuck::cast_slice(colors));
        }

        let output = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&self.device, &self.config);
                return Ok(PreviewRenderOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(PreviewRenderOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(PreviewRendererError::SurfaceValidation);
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Donder Preview Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Donder Preview Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color(style.background_rgb)),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if self.instance_count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                pass.set_vertex_buffer(1, self.color_buffer.slice(..));
                pass.draw(0..6, 0..self.instance_count as u32);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
        Ok(PreviewRenderOutcome::Presented)
    }

    fn upload_scene(&mut self, scene: &PreviewScene) {
        self.instance_count = scene.instances.len();
        self.ensure_instance_capacity(scene.instances.len());
        if !scene.instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&scene.instances),
            );
        }
        self.uploaded_revision = Some(scene.revision);
    }

    fn update_uniforms(&self, scene: &PreviewScene, size: PreviewSize, style: PreviewStyle) {
        let camera = PreviewCamera::fit(scene.bounds(), size, style.canvas_fill_ratio);
        let uniforms = PreviewUniforms {
            screen_zoom_min_radius: [
                size.width as f32,
                size.height as f32,
                camera.zoom,
                style.minimum_radius_pixels,
            ],
            pan: [camera.pan.x, camera.pan.y, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    fn ensure_instance_capacity(&mut self, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let capacity = needed.next_power_of_two();
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Donder Preview Instances"),
            size: (capacity * std::mem::size_of::<PreviewInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.color_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Donder Preview Colors"),
            size: (capacity * std::mem::size_of::<PreviewColor>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = capacity;
    }
}

fn color(rgb: [u8; 3]) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(rgb[0]) / f64::from(u8::MAX),
        g: f64::from(rgb[1]) / f64::from(u8::MAX),
        b: f64::from(rgb[2]) / f64::from(u8::MAX),
        a: 1.0,
    }
}

fn empty_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 16,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PreviewUniforms {
    screen_zoom_min_radius: [f32; 4],
    pan: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewRenderOutcome {
    Presented,
    Skipped,
}

#[derive(Debug)]
pub enum PreviewRendererError {
    Adapter(String),
    Device(String),
    NoSurfaceFormat,
    NoPresentMode,
    ColorCount { colors: usize, instances: usize },
    SurfaceValidation,
}

impl std::fmt::Display for PreviewRendererError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(error) => {
                write!(formatter, "cannot select a preview GPU adapter: {error}")
            }
            Self::Device(error) => {
                write!(formatter, "cannot create the preview GPU device: {error}")
            }
            Self::NoSurfaceFormat => formatter.write_str("preview surface has no supported format"),
            Self::NoPresentMode => formatter.write_str("preview surface has no present mode"),
            Self::ColorCount { colors, instances } => write!(
                formatter,
                "preview received {colors} colors for {instances} instances"
            ),
            Self::SurfaceValidation => formatter.write_str("preview surface validation failed"),
        }
    }
}

impl std::error::Error for PreviewRendererError {}
