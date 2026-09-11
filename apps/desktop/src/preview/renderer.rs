use super::render_loop::{empty_instance_buffer, window_size};
use super::scene::{PreviewBounds, PreviewCamera, PreviewSize};
use super::*;

pub(crate) struct PreviewRenderer {
    surface: wgpu::Surface<'static>,
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
    camera: PreviewCamera,
    color_workspace: Vec<PreviewColorGpu>,
}

impl PreviewRenderer {
    pub(crate) fn new(window: &Window) -> Result<Self, String> {
        block_on(Self::new_async(window))
    }

    async fn new_async(window: &Window) -> Result<Self, String> {
        let size = window_size(window)?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| error.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| error.to_string())?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Preview Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| error.to_string())?;
        let max_surface_dimension = device.limits().max_texture_dimension_2d;
        let size = size.clamp_to_max_dimension(max_surface_dimension);
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| "Preview surface has no supported formats.".to_string())?;
        let present_mode = if capabilities
            .present_modes
            .iter()
            .any(|mode| matches!(mode, wgpu::PresentMode::Fifo))
        {
            wgpu::PresentMode::Fifo
        } else {
            capabilities
                .present_modes
                .first()
                .copied()
                .ok_or_else(|| "Preview surface has no present modes.".to_string())?
        };
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
            label: Some("Preview Uniforms"),
            contents: bytemuck::bytes_of(&PreviewUniforms::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Preview Bind Group Layout"),
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
            label: Some("Preview Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Preview Shader"),
            source: wgpu::ShaderSource::Wgsl(PREVIEW_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Preview Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Preview Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(PreviewInstanceGpu::layout()),
                    Some(PreviewColorGpu::layout()),
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

        let instance_buffer = empty_instance_buffer(&device, "Preview Instances");
        let color_buffer = empty_instance_buffer(&device, "Preview Colors");

        Ok(Self {
            surface,
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
            camera: PreviewCamera::fit(PreviewBounds::default(), size),
            color_workspace: Vec::new(),
        })
    }

    pub(crate) fn render(
        &mut self,
        size: PreviewSize,
        scene: Option<&PreviewScene>,
        frame: Option<&dawn_elaboration::RenderedSequenceFrame>,
    ) {
        let size = size.clamp_to_max_dimension(self.max_surface_dimension);
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.config.width != size.width || self.config.height != size.height {
            self.resize(size);
        }

        if let Some(scene) = scene {
            self.update_scene(scene, frame, size);
        } else {
            self.instance_count = 0;
            self.uploaded_revision = None;
            self.uniform_revision = None;
        }

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Preview Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
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
    }

    fn resize(&mut self, size: PreviewSize) {
        let size = size.clamp_to_max_dimension(self.max_surface_dimension);
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.uniform_size = None;
    }

    fn update_scene(
        &mut self,
        scene: &PreviewScene,
        frame: Option<&dawn_elaboration::RenderedSequenceFrame>,
        size: PreviewSize,
    ) {
        if self.uploaded_revision != Some(scene.revision) {
            self.upload_scene(scene);
        }
        if self.uniform_revision != Some(scene.revision) || self.uniform_size != Some(size) {
            self.update_uniforms(scene, size);
        }
        self.update_colors(scene, frame);
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

    fn update_uniforms(&mut self, scene: &PreviewScene, size: PreviewSize) {
        self.camera = PreviewCamera::fit(scene.bounds, size);
        let uniforms = PreviewUniforms {
            screen_zoom_min_radius: [size.width as f32, size.height as f32, self.camera.zoom, 2.0],
            pan: [self.camera.pan.x, self.camera.pan.y, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.uniform_revision = Some(scene.revision);
        self.uniform_size = Some(size);
    }

    fn update_colors(
        &mut self,
        scene: &PreviewScene,
        frame: Option<&dawn_elaboration::RenderedSequenceFrame>,
    ) {
        self.color_workspace.clear();
        self.color_workspace
            .resize(scene.instances.len(), PreviewColorGpu::black());
        if let Some(frame) = frame {
            for span in &scene.fixtures {
                if let Some(fixture) = frame
                    .fixtures
                    .iter()
                    .find(|fixture| fixture.fixture_id == span.fixture.0)
                {
                    for (target, color) in self.color_workspace[span.pixels.clone()]
                        .iter_mut()
                        .zip(&fixture.pixels)
                    {
                        *target = PreviewColorGpu::from_color(*color);
                    }
                }
            }
        }
        if !self.color_workspace.is_empty() {
            self.queue.write_buffer(
                &self.color_buffer,
                0,
                bytemuck::cast_slice(&self.color_workspace),
            );
        }
    }

    fn ensure_instance_capacity(&mut self, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let capacity = needed.next_power_of_two();
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Preview Instances"),
            size: (capacity * std::mem::size_of::<PreviewInstanceGpu>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.color_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Preview Colors"),
            size: (capacity * std::mem::size_of::<PreviewColorGpu>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = capacity;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PreviewUniforms {
    screen_zoom_min_radius: [f32; 4],
    pan: [f32; 4],
}

impl Default for PreviewUniforms {
    fn default() -> Self {
        Self {
            screen_zoom_min_radius: [1.0, 1.0, 1.0, 2.0],
            pan: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PreviewInstanceGpu {
    pub(crate) center_radius: [f32; 4],
}

impl PreviewInstanceGpu {
    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PreviewColorGpu {
    color: [f32; 4],
}

impl PreviewColorGpu {
    pub(crate) fn black() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0],
        }
    }

    pub(crate) fn from_color(color: dawn_language::values::Color) -> Self {
        Self {
            color: [
                f32::from(color.red) / 255.0,
                f32::from(color.green) / 255.0,
                f32::from(color.blue) / 255.0,
                1.0,
            ],
        }
    }

    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 1,
            }],
        }
    }
}
