//! wgpu device and surface, image texture upload, mip generation, the quad pipeline and egui
//! painting. All of it runs on the main thread.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use winit::window::Window;

use crate::view::domain::{MagnificationFilter, Transform};

/// Setting this to `1` skips the hardware adapter and runs on WARP, so the fallback path can be
/// tested on a machine that has a GPU.
const FORCE_WARP_VAR: &str = "SUPERSLIDE_FORCE_WARP";

/// Replaces the checkerboard stand-in: `stripes` is an 8000 × 6000 image of 1 px stripes, the
/// worst case for moiré and a 48 MP upload to time; `white` and `black` are solid, for checking
/// that overlays stay legible.
const STAND_IN_VAR: &str = "SUPERSLIDE_STAND_IN";

/// Bytes of the `Uniforms` struct in `quad.wgsl`: a `mat2x2<f32>`, then two `vec2<f32>`.
const UNIFORM_SIZE: usize = 32;

/// Pixels go to the GPU in slices of about this many bytes. `write_texture` stages each call in
/// one buffer, and a 50 MP image in one call (200 MB) would pass wgpu's default 256 MB buffer
/// limit at about 64 MP.
const UPLOAD_CHUNK_BYTES: u32 = 16 << 20;

const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

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

/// An image on the GPU with its full mip chain. Dropping it frees the texture.
pub struct ImageTexture {
    size: [u32; 2],
    bind_group: wgpu::BindGroup,
}

impl ImageTexture {
    /// Stored pixel size, before orientation or rotation.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// GPU memory held, counted against the preload budget.
    pub fn bytes(&self) -> u64 {
        texture_bytes(self.size)
    }
}

/// One frame of egui output, tessellated, ready to paint over the image.
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    /// Uniforms plus the sampler; one group per magnification filter, so a toggle rebuilds nothing.
    linear_group: wgpu::BindGroup,
    nearest_group: wgpu::BindGroup,
    image_layout: wgpu::BindGroupLayout,
    mip_pipeline: wgpu::RenderPipeline,
    mip_layout: wgpu::BindGroupLayout,
    mip_sampler: wgpu::Sampler,
    egui: egui_wgpu::Renderer,
    pub magnification_filter: MagnificationFilter,
    /// The largest texture side the device accepts. `decode` downscales anything bigger.
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

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad frame"),
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
                sampler_entry(1),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad image"),
            entries: &[texture_entry(0)],
        });
        let pipeline = create_pipeline(
            &device,
            "quad",
            wgpu::include_wgsl!("quad.wgsl"),
            &[&frame_layout, &image_layout],
            wgpu::PrimitiveTopology::TriangleStrip,
            format,
        );

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad uniforms"),
            size: UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = |mag_filter| {
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("image"),
                mag_filter,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("quad frame"),
                layout: &frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };
        let linear_group = frame_group(wgpu::FilterMode::Linear);
        let nearest_group = frame_group(wgpu::FilterMode::Nearest);

        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let mip_pipeline = create_pipeline(
            &device,
            "mip",
            wgpu::include_wgsl!("mip.wgsl"),
            &[&mip_layout],
            wgpu::PrimitiveTopology::TriangleList,
            IMAGE_FORMAT,
        );
        let mip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mip"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let egui = egui_wgpu::Renderer::new(&device, format, Default::default());

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            uniforms,
            linear_group,
            nearest_group,
            image_layout,
            mip_pipeline,
            mip_layout,
            mip_sampler,
            egui,
            magnification_filter: MagnificationFilter::default(),
            max_texture_dimension,
        })
    }

    /// Uploads RGBA pixels (sRGB, row-major, no padding) and builds the mip chain.
    ///
    /// Blocks until the GPU has finished: the logged time is the real upload time, and the staging
    /// copies are freed before the next upload queues more.
    pub fn upload(&self, size: [u32; 2], pixels: &[u8]) -> ImageTexture {
        let [width, height] = size;
        assert_eq!(
            pixels.len() as u64,
            u64::from(width) * u64::from(height) * 4,
            "pixel buffer does not match {width} × {height}"
        );
        let start = Instant::now();
        let levels = mip_level_count(size);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        let row_bytes = width * 4;
        let chunk_rows = (UPLOAD_CHUNK_BYTES / row_bytes).max(1);
        for top in (0..height).step_by(chunk_rows as usize) {
            let rows = chunk_rows.min(height - top);
            let start = top as usize * row_bytes as usize;
            let end = start + rows as usize * row_bytes as usize;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: top, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[start..end],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width,
                    height: rows,
                    depth_or_array_layers: 1,
                },
            );
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        let level_view = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for level in 1..levels {
            let source = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip"),
                layout: &self.mip_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&level_view(level - 1)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.mip_sampler),
                    },
                ],
            });
            let target = level_view(level);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, &source, &[]);
            pass.draw(0..3, 0..1);
        }
        let submission = self.queue.submit([encoder.finish()]);
        // An error here means the device is lost; the next frame's surface reconfigure handles it.
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        });

        let image = ImageTexture {
            size,
            bind_group: self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("quad image"),
                layout: &self.image_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                }],
            }),
        };
        eprintln!(
            "superslide: uploaded {width} × {height} ({levels} mip levels, {} MB) in {} ms",
            image.bytes() >> 20,
            start.elapsed().as_millis()
        );
        image
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

    /// The image, then egui over it, in one pass. egui's texture changes apply even when the frame
    /// is skipped, or a font atlas sent then would never arrive.
    pub fn draw(&mut self, image: &ImageTexture, transform: Transform, mut ui: UiFrame) {
        // Draining matters: epaint asserts that no delta is dropped unapplied. Deltas for one
        // texture apply in order: a full image, then patches to it.
        for (id, deltas) in ui.textures.set.drain() {
            for delta in &deltas {
                self.egui
                    .update_texture(&self.device, &self.queue, id, delta);
            }
        }
        self.paint(image, transform, &ui);
        for id in ui.textures.free.drain() {
            self.egui.free_texture(&id);
        }
    }

    fn paint(&mut self, image: &ImageTexture, transform: Transform, ui: &UiFrame) {
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
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ui.pixels_per_point,
        };
        let egui_commands = self.egui.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &ui.primitives,
            &screen,
        );
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
            let frame_group = match self.magnification_filter {
                MagnificationFilter::Linear => &self.linear_group,
                MagnificationFilter::Nearest => &self.nearest_group,
            };
            pass.set_bind_group(0, frame_group, &[]);
            pass.set_bind_group(1, &image.bind_group, &[]);
            pass.draw(0..4, 0..1);
            self.egui
                .render(&mut pass.forget_lifetime(), &ui.primitives, &screen);
        }
        self.queue
            .submit(egui_commands.into_iter().chain([encoder.finish()]));
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

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// A pipeline with no vertex buffers, entry points `vs_main` and `fs_main`, and no blending.
fn create_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: wgpu::ShaderModuleDescriptor<'_>,
    layouts: &[&wgpu::BindGroupLayout],
    topology: wgpu::PrimitiveTopology,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);
    let layouts: Vec<_> = layouts.iter().copied().map(Some).collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &layouts,
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
            topology,
            // Mirroring and rotation flip the quad's winding; no side may be culled.
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
    })
}

/// Halving down to 1 × 1 on the longer side: `floor(log2(max(w, h))) + 1`.
fn mip_level_count([width, height]: [u32; 2]) -> u32 {
    u32::BITS - width.max(height).leading_zeros()
}

/// The full chain adds a third to the base level; rounding on odd sizes is ignored.
fn texture_bytes([width, height]: [u32; 2]) -> u64 {
    u64::from(width) * u64::from(height) * 4 * 4 / 3
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

/// A generated image until decoding exists: the checkerboard, or what `STAND_IN_VAR` selects.
pub fn stand_in() -> ([u32; 2], Vec<u8>) {
    let solid = |value| ([384, 256], [value, value, value, 255].repeat(384 * 256));
    match std::env::var(STAND_IN_VAR).as_deref() {
        Ok("stripes") => stripes(),
        Ok("white") => solid(255),
        Ok("black") => solid(0),
        _ => checkerboard(),
    }
}

/// 3:2, not square, so a stretched fit shows as non-square cells.
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

/// 1 px black-and-white stripes, vertical on the left half and horizontal on the right. Any
/// shrunk view should be flat sRGB 188 grey; moiré shows as bands.
fn stripes() -> ([u32; 2], Vec<u8>) {
    let size: [u32; 2] = [8000, 6000];
    let mut pixels = Vec::with_capacity(size[0] as usize * size[1] as usize * 4);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let stripe = if x < size[0] / 2 { x } else { y };
            let value = if stripe.is_multiple_of(2) { 255 } else { 0 };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    (size, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chains_halve_the_longer_side_down_to_one_pixel() {
        assert_eq!(mip_level_count([1, 1]), 1);
        assert_eq!(mip_level_count([1, 2]), 2);
        assert_eq!(mip_level_count([1, 1000]), 10);
        assert_eq!(mip_level_count([1024, 1]), 11);
        // Odd sizes round down at each halving: 1023 → 511 → … → 1 is 10 levels, 1025 is 11.
        assert_eq!(mip_level_count([1023, 7]), 10);
        assert_eq!(mip_level_count([5, 1025]), 11);
        assert_eq!(mip_level_count([8192, 8192]), 14);
        assert_eq!(mip_level_count([16384, 3]), 15);
    }

    #[test]
    fn texture_bytes_count_the_mip_chain() {
        assert_eq!(texture_bytes([1000, 750]), 4_000_000);
        // The largest texture the device takes: 16 × its pixels overflows 32 bits.
        assert_eq!(texture_bytes([16384, 16384]), 1_431_655_765);
    }
}
