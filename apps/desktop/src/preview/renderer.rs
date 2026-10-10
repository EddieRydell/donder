use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::scene::{
    PreviewCamera, PreviewColor, PreviewInstance, PreviewScene, PreviewSize, PreviewStyle,
};

const SHADER: &str = include_str!("preview.wgsl");

/// The Preview window's renderer.
pub(crate) struct PreviewRenderer {
    drawing: PreviewDrawing,
    config: wgpu::SurfaceConfiguration,
}

/// Renders Preview frames offscreen and reads their pixels back, so an export
/// shows exactly what the Preview window shows.
pub(crate) struct PreviewFrameRenderer {
    drawing: PreviewDrawing,
    size: PreviewSize,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    padded_row_bytes: u32,
}

/// The device, pipeline and buffers that draw a Preview scene into a target.
struct PreviewDrawing {
    device: wgpu::Device,
    queue: wgpu::Queue,
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
        let (adapter, device, queue) = request_device(instance, Some(surface)).await?;
        let size = size.clamp_to(device.limits().max_texture_dimension_2d);
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
        Ok(Self {
            drawing: PreviewDrawing::new(device, queue, format),
            config,
        })
    }

    pub(crate) fn render(
        &mut self,
        surface: &wgpu::Surface<'_>,
        size: PreviewSize,
        scene: &PreviewScene,
        colors: &[PreviewColor],
        style: PreviewStyle,
    ) -> Result<PreviewRenderOutcome, PreviewRendererError> {
        let size = size.clamp_to(self.drawing.device.limits().max_texture_dimension_2d);
        if self.config.width != size.width || self.config.height != size.height {
            self.config.width = size.width;
            self.config.height = size.height;
            surface.configure(&self.drawing.device, &self.config);
        }
        self.drawing.prepare(size, scene, colors, style)?;
        let output = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&self.drawing.device, &self.config);
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
        let encoder = self.drawing.encode(&view, style);
        self.drawing.queue.submit([encoder.finish()]);
        self.drawing.queue.present(output);
        Ok(PreviewRenderOutcome::Presented)
    }
}

impl PreviewFrameRenderer {
    /// Frames are sRGB encoded, as the Preview window's surface encodes them.
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    const BYTES_PER_PIXEL: u32 = 4;

    pub(crate) fn new(size: PreviewSize) -> Result<Self, PreviewRendererError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let (_, device, queue) = tauri::async_runtime::block_on(request_device(&instance, None))?;
        let max_dimension = device.limits().max_texture_dimension_2d;
        if size.width > max_dimension || size.height > max_dimension {
            return Err(PreviewRendererError::FrameSize { max_dimension });
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Donder Preview Frame"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let padded_row_bytes = (size.width * Self::BYTES_PER_PIXEL)
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Donder Preview Frame Readback"),
            size: u64::from(padded_row_bytes) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            drawing: PreviewDrawing::new(device, queue, Self::FORMAT),
            size,
            texture,
            readback,
            padded_row_bytes,
        })
    }

    /// Render one frame and write its RGBA pixels to `rgba`, top row first.
    pub(crate) fn render(
        &mut self,
        scene: &PreviewScene,
        colors: &[PreviewColor],
        style: PreviewStyle,
        rgba: &mut [u8],
    ) -> Result<(), PreviewRendererError> {
        let row_bytes = (self.size.width * Self::BYTES_PER_PIXEL) as usize;
        if rgba.len() != row_bytes * self.size.height as usize {
            return Err(PreviewRendererError::FrameBuffer);
        }
        self.drawing.prepare(self.size, scene, colors, style)?;
        let view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.drawing.encode(&view, style);
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row_bytes),
                    rows_per_image: None,
                },
            },
            self.texture.size(),
        );
        self.drawing.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            // The receiver waits below, so it is still listening.
            let _ = sender.send(result);
        });
        self.drawing
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| PreviewRendererError::Readback(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| PreviewRendererError::Readback(error.to_string()))?
            .map_err(|error| PreviewRendererError::Readback(error.to_string()))?;
        {
            let mapped = slice
                .get_mapped_range()
                .map_err(|error| PreviewRendererError::Readback(error.to_string()))?;
            for (row, padded) in rgba
                .chunks_exact_mut(row_bytes)
                .zip(mapped.chunks_exact(self.padded_row_bytes as usize))
            {
                row.copy_from_slice(&padded[..row_bytes]);
            }
        }
        self.readback.unmap();
        Ok(())
    }
}

async fn request_device(
    instance: &wgpu::Instance,
    compatible_surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), PreviewRendererError> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface,
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
    Ok((adapter, device, queue))
}

impl PreviewDrawing {
    fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
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

        Self {
            device,
            queue,
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
        }
    }

    /// Upload what changed since the last frame: the scene, uniforms and colours.
    fn prepare(
        &mut self,
        size: PreviewSize,
        scene: &PreviewScene,
        colors: &[PreviewColor],
        style: PreviewStyle,
    ) -> Result<(), PreviewRendererError> {
        if colors.len() != scene.instances.len() {
            return Err(PreviewRendererError::ColorCount {
                colors: colors.len(),
                instances: scene.instances.len(),
            });
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
        Ok(())
    }

    /// Record the pass that clears `view` to the background and draws every pixel.
    fn encode(&self, view: &wgpu::TextureView, style: PreviewStyle) -> wgpu::CommandEncoder {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Donder Preview Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Donder Preview Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
        encoder
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
pub(crate) enum PreviewRenderOutcome {
    Presented,
    Skipped,
}

#[derive(Debug)]
pub(crate) enum PreviewRendererError {
    Adapter(String),
    Device(String),
    NoSurfaceFormat,
    NoPresentMode,
    ColorCount { colors: usize, instances: usize },
    SurfaceValidation,
    FrameSize { max_dimension: u32 },
    FrameBuffer,
    Readback(String),
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
            Self::FrameSize { max_dimension } => write!(
                formatter,
                "preview frames on this GPU are at most {max_dimension} pixels wide and high"
            ),
            Self::FrameBuffer => formatter.write_str("preview frame buffer has the wrong size"),
            Self::Readback(error) => write!(formatter, "cannot read the preview frame: {error}"),
        }
    }
}

impl std::error::Error for PreviewRendererError {}
