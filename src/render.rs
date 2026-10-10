//! wgpu device and surface, image texture upload, mip generation, the quad pipeline and egui
//! painting. All of it runs on the main thread.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use winit::window::Window;

use crate::view::Transform;

/// Sampling when zoomed past native size. Minification always stays mipmapped linear, so a shrunk
/// image never aliases whichever filter is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MagnificationFilter {
    #[default]
    Linear,
    /// Sharp pixels at high zoom.
    Nearest,
}

/// Setting this to `1` skips the hardware adapter and runs on WARP, so the fallback path can be
/// tested on a machine that has a GPU.
const FORCE_WARP_VAR: &str = "SUPERSLIDE_FORCE_WARP";

/// Bytes of the `Uniforms` struct in `quad.wgsl`: a `mat2x2<f32>`, then two `vec2<f32>`.
const UNIFORM_SIZE: usize = 32;

#[derive(Debug)]
pub enum InitError {
    Surface(wgpu::CreateSurfaceError),
    Adapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
    /// Every adapter's surface formats lack an sRGB one; colours would come out wrong.
    NoSrgbFormat,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Surface(e) => write!(f, "cannot create a drawing surface: {e}"),
            Self::Adapter(e) => write!(f, "no usable GPU adapter, not even WARP: {e}"),
            Self::Device(e) => write!(f, "cannot open the GPU device: {e}"),
            Self::NoSrgbFormat => f.write_str("the window surface offers no sRGB format"),
        }
    }
}

impl std::error::Error for InitError {}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    image_size: [u32; 2],
    /// The largest texture side the device accepts. `decode` downscales anything bigger.
    #[expect(
        dead_code,
        reason = "scaffold: read once the preload cache starts decode workers"
    )]
    pub max_texture_dimension: u32,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, InitError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let size = window.inner_size();
        let surface = instance
            .create_surface(window)
            .map_err(InitError::Surface)?;

        let force_warp = std::env::var(FORCE_WARP_VAR).is_ok_and(|v| v == "1");
        let adapter = pollster::block_on(request_adapter(&instance, &surface, force_warp))
            .map_err(InitError::Adapter)?;
        let info = adapter.get_info();
        eprintln!(
            "superslide: GPU adapter {:?} ({:?}){}",
            info.name,
            info.device_type,
            if info.device_type == wgpu::DeviceType::Cpu {
                ", the WARP software fallback"
            } else {
                ""
            }
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("superslide"),
            // The default limits cap textures at 8192 px; Dx12 hardware takes 16384, which
            // spares most large photos the downscale.
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(InitError::Device)?;
        let max_texture_dimension = device.limits().max_texture_dimension_2d;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or(InitError::NoSrgbFormat)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // Vsync: the only mode that cannot tear.
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // Resolves to plain sRGB for an 8-bit format, which is what the images are in.
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("quad.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("quad"),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                // Mirroring and rotation flip the winding; no side may be culled.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad uniforms"),
            size: UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (image_size, pixels) = checkerboard();
        let texture = upload(&device, &queue, image_size, &pixels);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quad"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            uniforms,
            bind_group,
            image_size,
            max_texture_dimension,
        })
    }

    /// Pixel size of the image being drawn, before any transform.
    pub fn image_size(&self) -> [u32; 2] {
        self.image_size
    }

    /// A minimised window reports 0 × 0, which the surface cannot take; drawing is skipped until
    /// it has a size again.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn draw(&mut self, transform: Transform) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            // Present this one, then reconfigure so the next frame matches the surface again.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.surface.configure(&self.device, &self.config);
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Minimised, covered or busy: the next redraw tries again.
            _ => return,
        };

        self.queue
            .write_buffer(&self.uniforms, 0, &uniform_bytes(transform, &self.config));
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("quad"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..4, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }
}

/// Hardware first; WARP when there is none, or when the owner forces it for testing.
async fn request_adapter(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
    force_warp: bool,
) -> Result<wgpu::Adapter, wgpu::RequestAdapterError> {
    let options = |force_fallback_adapter| wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter,
        compatible_surface: Some(surface),
        ..Default::default()
    };
    if !force_warp && let Ok(adapter) = instance.request_adapter(&options(false)).await {
        return Ok(adapter);
    }
    instance.request_adapter(&options(true)).await
}

/// Packs `Uniforms` from `quad.wgsl`. A `mat2x2<f32>` is two 8-byte columns, so the layout has no
/// padding.
fn uniform_bytes(transform: Transform, config: &wgpu::SurfaceConfiguration) -> [u8; UNIFORM_SIZE] {
    let [[a, b], [c, d]] = transform.matrix;
    let [x, y] = transform.offset;
    let floats = [a, b, c, d, x, y, config.width as f32, config.height as f32];
    let mut bytes = [0; UNIFORM_SIZE];
    for (chunk, value) in bytes.chunks_exact_mut(4).zip(floats) {
        chunk.copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: [u32; 2],
    pixels: &[u8],
) -> wgpu::Texture {
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        },
        extent,
    );
    texture
}

/// A stand-in image until decoding exists. It is 3:2, not square, so a stretched fit shows as
/// non-square cells.
fn checkerboard() -> ([u32; 2], Vec<u8>) {
    const CELL: u32 = 32;
    let size = [CELL * 12, CELL * 8];
    let pixels = (0..size[1])
        .flat_map(|y| (0..size[0]).map(move |x| (x / CELL + y / CELL).is_multiple_of(2)))
        .flat_map(|light| {
            if light {
                [200, 200, 200, 255]
            } else {
                [60, 60, 60, 255]
            }
        })
        .collect();
    (size, pixels)
}
