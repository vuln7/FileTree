//! The GPU side. It draws lists of instances and holds very little of its own: no
//! meshes, and only two textures (text atlas and the current preview).

use crate::text::{ATLAS_SIZE, Upload};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BranchInst {
    pub p0: [f32; 2],
    pub c1: [f32; 2],
    pub c2: [f32; 2],
    pub p1: [f32; 2],
    /// Start width, end width, grow, seed.
    pub w: [f32; 4],
    /// Segments, lift, dim, unused.
    pub misc: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct FlowerInst {
    pub pos: [f32; 2],
    pub radius: f32,
    pub rotation: f32,
    pub bloom: f32,
    pub species: f32,
    pub seed: f32,
    pub lift: f32,
    pub dim: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RectInst {
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// Radius, blur, mode (0 box, 1 capsule, 2 ring), ring thickness.
    pub params: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GlyphInst {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub halo: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ImageInst {
    rect: [f32; 4],
    radius: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    time: f32,
    scale: f32,
    world_origin: [f32; 2],
    zoom: f32,
    wind: f32,
    ground: [f32; 4],
    ground_deep: [f32; 4],
    soil: [f32; 4],
    shadow: [f32; 4],
    sky: [f32; 4],
    scene: [f32; 4],
    species: [[f32; 4]; 39],
}

/// One interface layer, drawn in order: shapes, picture, flowers, text.
#[derive(Default)]
pub struct UiLayer {
    pub rects: Vec<RectInst>,
    pub image_rect: Option<([f32; 4], f32)>,
    pub flowers: Vec<FlowerInst>,
    pub glyphs: Vec<GlyphInst>,
    pub clip: Option<[f32; 4]>,
}

#[derive(Default)]
pub struct DrawList {
    pub time: f32,
    pub scale: f32,
    pub world_origin: [f32; 2],
    pub zoom: f32,
    pub wind: f32,
    /// Where the sun or moon is, how dark it is, how warm the horizon is.
    pub sky: [f32; 4],
    /// The world's clock, the width of the stage, the gust.
    pub scene: [f32; 4],
    /// Stems so fine they cast no shadow worth drawing.
    pub branches_fine: Vec<BranchInst>,
    /// Long limbs get more segments than short twigs.
    pub branches_long: Vec<BranchInst>,
    pub branches_short: Vec<BranchInst>,
    pub path_branches: Vec<BranchInst>,
    /// Things far off in the world, in front of the sky and behind the tree.
    pub backdrop: Vec<FlowerInst>,
    pub leaves: Vec<FlowerInst>,
    pub flowers: Vec<FlowerInst>,
    pub top_flowers: Vec<FlowerInst>,
    pub label_halos: Vec<GlyphInst>,
    pub labels: Vec<GlyphInst>,
    pub ui: Vec<UiLayer>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.branches_fine.clear();
        self.branches_long.clear();
        self.branches_short.clear();
        self.path_branches.clear();
        self.backdrop.clear();
        self.leaves.clear();
        self.flowers.clear();
        self.top_flowers.clear();
        self.label_halos.clear();
        self.labels.clear();
        // Keep allocated layers so their vectors are reused frame to frame.
        for layer in &mut self.ui {
            layer.rects.clear();
            layer.flowers.clear();
            layer.glyphs.clear();
            layer.image_rect = None;
            layer.clip = None;
        }
    }
}

pub const LONG_SEGMENTS: u32 = 16;
pub const SHORT_SEGMENTS: u32 = 4;
/// Extra rows that round off the end of a strip. Must match CAP_ROWS in the shader.
const CAP_ROWS: u32 = 4;

struct GrowBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    usage: wgpu::BufferUsages,
    cursor: u64,
}

impl GrowBuffer {
    fn new(device: &wgpu::Device, capacity: u64, usage: wgpu::BufferUsages) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: capacity,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        GrowBuffer { buffer, capacity, usage, cursor: 0 }
    }

    fn ensure(&mut self, device: &wgpu::Device, needed: u64) {
        if needed > self.capacity {
            self.capacity = (needed * 3 / 2).max(self.capacity * 2);
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.capacity,
                usage: self.usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    /// Writes data at the cursor, returning the range it occupies.
    fn push<T: Pod>(&mut self, queue: &wgpu::Queue, data: &[T]) -> std::ops::Range<u64> {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let start = self.cursor;
        queue.write_buffer(&self.buffer, start, bytes);
        self.cursor += bytes.len() as u64;
        start..self.cursor
    }
}

enum Target {
    Window { surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration },
    Offscreen { texture: wgpu::Texture, width: u32, height: u32 },
}

pub struct Gpu {
    /// Drawing on the processor because there's no working graphics card.
    pub in_software: bool,
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,

    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    sampler_linear: wgpu::Sampler,
    atlas: wgpu::Texture,
    atlas_bind: wgpu::BindGroup,
    image_bind: Option<wgpu::BindGroup>,

    ground: wgpu::RenderPipeline,
    soil_front: wgpu::RenderPipeline,
    branch: wgpu::RenderPipeline,
    branch_shadow: wgpu::RenderPipeline,
    flower: wgpu::RenderPipeline,
    rect: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,

    branches: GrowBuffer,
    flowers: GrowBuffer,
    rects: GrowBuffer,
    glyphs: GrowBuffer,
    images: GrowBuffer,
}

pub const fn rgb(hex: u32) -> [f32; 4] {
    [
        ((hex >> 16) & 255) as f32 / 255.0,
        ((hex >> 8) & 255) as f32 / 255.0,
        (hex & 255) as f32 / 255.0,
        1.0,
    ]
}

fn species_palette() -> [[f32; 4]; 39] {
    use crate::palette as p;
    let mut out = [[0.0; 4]; 39];
    for (i, row) in p::SPECIES.iter().enumerate() {
        out[i * 3] = rgb(row[0]);
        out[i * 3 + 1] = rgb(row[1]);
        out[i * 3 + 2] = rgb(row[2]);
    }
    out
}

impl Gpu {
    /// Tries each way of drawing this machine might offer until one works from start to
    /// finish, so an old driver that fails part-way just means moving on to the next.
    pub fn for_window(window: Arc<winit::window::Window>) -> Result<Gpu, String> {
        let size = window.inner_size();
        let mut last_error = String::from("No graphics driver was found.");
        for (backends, software) in adapter_options() {
            let instance = make_instance(backends);
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(err) => {
                    last_error = format!("{err}");
                    continue;
                }
            };
            let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: software,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })) {
                Ok(adapter) => adapter,
                Err(err) => {
                    last_error = format!("{err}");
                    continue;
                }
            };
            // Drawing on the processor is the last resort, chosen only when asked for below.
            if !software && adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                continue;
            }
            let (device, queue) = match make_device(&adapter) {
                Ok(pair) => pair,
                Err(err) => {
                    last_error = err;
                    continue;
                }
            };
            let caps = surface.get_capabilities(&adapter);
            let format = caps
                .formats
                .iter()
                .copied()
                .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
                .or_else(|| caps.formats.first().copied());
            let Some(format) = format else {
                last_error = "This display can't present the app's colour format.".to_string();
                continue;
            };
            let Some(mut config) = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)) else {
                last_error = "This window can't be drawn to.".to_string();
                continue;
            };
            config.format = format;
            config.present_mode = wgpu::PresentMode::AutoVsync;
            config.desired_maximum_frame_latency = 1;
            surface.configure(&device, &config);
            let mut gpu = Self::build(device, queue, Target::Window { surface, config }, format);
            gpu.in_software = software || adapter.get_info().device_type == wgpu::DeviceType::Cpu;
            return Ok(gpu);
        }
        Err(last_error)
    }

    pub fn offscreen(width: u32, height: u32) -> Result<Gpu, String> {
        let mut last_error = String::from("No graphics driver was found.");
        let mut found = None;
        for (backends, software) in adapter_options() {
            let instance = make_instance(backends);
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: software,
                compatible_surface: None,
                apply_limit_buckets: false,
            })) {
                Ok(adapter) => {
                    found = Some(adapter);
                    break;
                }
                Err(err) => last_error = format!("{err}"),
            }
        }
        let adapter = found.ok_or(last_error)?;
        let (device, queue) = make_device(&adapter)?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Ok(Self::build(device, queue, Target::Offscreen { texture, width, height }, format))
    }

    fn build(device: wgpu::Device, queue: wgpu::Queue, target: Target, format: wgpu::TextureFormat) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
            ],
        });
        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyphs"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&Default::default());
        let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler_nearest) },
            ],
        });

        let plain_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let tex_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });

        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make = |vs: &str,
                    fs: &str,
                    layout: &wgpu::PipelineLayout,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    topology: wgpu::PrimitiveTopology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let branch_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4
        ];
        let branch_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BranchInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &branch_attrs,
        };
        let flower_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Float32,
            4 => Float32, 5 => Float32, 6 => Float32, 7 => Float32
        ];
        let flower_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FlowerInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &flower_attrs,
        };
        let rect_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4];
        let rect_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &rect_attrs,
        };
        let glyph_attrs = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4
        ];
        let glyph_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &glyph_attrs,
        };
        let image_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32];
        let image_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &image_attrs,
        };

        use wgpu::PrimitiveTopology::{TriangleList, TriangleStrip};
        let ground = make("vs_ground", "fs_ground", &plain_layout, &[], TriangleList);
        let soil_front = make("vs_ground", "fs_soil_front", &plain_layout, &[], TriangleList);
        let branch = make("vs_branch", "fs_branch", &plain_layout, &[Some(branch_layout.clone())], TriangleStrip);
        let branch_shadow =
            make("vs_branch_shadow", "fs_branch_shadow", &plain_layout, &[Some(branch_layout.clone())], TriangleStrip);
        let flower = make("vs_flower", "fs_flower", &plain_layout, &[Some(flower_layout.clone())], TriangleStrip);
        let rect = make("vs_rect", "fs_rect", &plain_layout, &[Some(rect_layout.clone())], TriangleStrip);
        let glyph = make("vs_glyph", "fs_glyph", &tex_pipeline_layout, &[Some(glyph_layout.clone())], TriangleStrip);
        let image = make("vs_image", "fs_image", &tex_pipeline_layout, &[Some(image_layout.clone())], TriangleStrip);

        let vertex = wgpu::BufferUsages::VERTEX;
        Gpu {
            in_software: false,
            branches: GrowBuffer::new(&device, 64 * 1024, vertex),
            flowers: GrowBuffer::new(&device, 64 * 1024, vertex),
            rects: GrowBuffer::new(&device, 16 * 1024, vertex),
            glyphs: GrowBuffer::new(&device, 64 * 1024, vertex),
            images: GrowBuffer::new(&device, 256, vertex),
            device,
            queue,
            target,
            globals,
            globals_bind,
            tex_layout,
            sampler_linear,
            atlas,
            atlas_bind,
            image_bind: None,
            ground,
            soil_front,
            branch,
            branch_shadow,
            flower,
            rect,
            glyph,
            image,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Target::Window { surface, config } = &mut self.target {
            config.width = width.max(1);
            config.height = height.max(1);
            surface.configure(&self.device, config);
        }
    }

    pub fn size(&self) -> (u32, u32) {
        match &self.target {
            Target::Window { config, .. } => (config.width, config.height),
            Target::Offscreen { width, height, .. } => (*width, *height),
        }
    }

    pub fn upload_glyphs(&mut self, uploads: &mut Vec<Upload>) {
        for up in uploads.drain(..) {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: up.x, y: up.y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &up.pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(up.width), rows_per_image: None },
                wgpu::Extent3d { width: up.width, height: up.height, depth_or_array_layers: 1 },
            );
        }
    }

    /// Swaps the preview picture. `None` frees it.
    pub fn set_image(&mut self, image: Option<(u32, u32, &[u8])>) {
        let Some((width, height, rgba)) = image else {
            self.image_bind = None;
            return;
        };
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("preview"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        let view = texture.create_view(&Default::default());
        self.image_bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_linear) },
            ],
        }));
    }

    pub fn render(&mut self, list: &DrawList) -> RenderResult {
        use crate::palette as p;
        let (width, height) = self.size();
        let acquired = match &self.target {
            Target::Window { surface, .. } => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    Some(frame)
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    return RenderResult::Reconfigure;
                }
                _ => return RenderResult::Skipped,
            },
            Target::Offscreen { .. } => None,
        };
        let view = match (&acquired, &self.target) {
            (Some(frame), _) => frame.texture.create_view(&Default::default()),
            (None, Target::Offscreen { texture, .. }) => texture.create_view(&Default::default()),
            _ => return RenderResult::Skipped,
        };

        let globals = Globals {
            viewport: [width as f32, height as f32],
            time: list.time,
            scale: list.scale,
            world_origin: list.world_origin,
            zoom: list.zoom,
            wind: list.wind,
            ground: rgb(p::GROUND),
            ground_deep: rgb(p::GROUND_DEEP),
            soil: rgb(p::SOIL),
            shadow: rgb(p::SHADOW),
            sky: list.sky,
            scene: list.scene,
            species: species_palette(),
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        const BRANCH: u64 = std::mem::size_of::<BranchInst>() as u64;
        const FLOWER: u64 = std::mem::size_of::<FlowerInst>() as u64;
        const RECT: u64 = std::mem::size_of::<RectInst>() as u64;
        const GLYPH: u64 = std::mem::size_of::<GlyphInst>() as u64;

        // Size each buffer for everything this frame needs before writing any of it.
        let branch_total =
            (list.branches_fine.len() + list.branches_long.len() + list.branches_short.len() + list.path_branches.len())
                as u64;
        let mut flower_total = (list.backdrop.len() + list.leaves.len() + list.flowers.len() + list.top_flowers.len()) as u64;
        let mut rect_total = 0u64;
        let mut glyph_total = (list.labels.len() + list.label_halos.len()) as u64;
        for layer in &list.ui {
            flower_total += layer.flowers.len() as u64;
            rect_total += layer.rects.len() as u64;
            glyph_total += layer.glyphs.len() as u64;
        }
        self.branches.ensure(&self.device, branch_total.max(1) * BRANCH);
        self.flowers.ensure(&self.device, flower_total.max(1) * FLOWER);
        self.rects.ensure(&self.device, rect_total.max(1) * RECT);
        self.glyphs.ensure(&self.device, glyph_total.max(1) * GLYPH);
        self.branches.cursor = 0;
        self.flowers.cursor = 0;
        self.rects.cursor = 0;
        self.glyphs.cursor = 0;
        self.images.cursor = 0;

        let fine = self.branches.push(&self.queue, &list.branches_fine);
        let long = self.branches.push(&self.queue, &list.branches_long);
        let short = self.branches.push(&self.queue, &list.branches_short);
        let path = self.branches.push(&self.queue, &list.path_branches);
        let backdrop = self.flowers.push(&self.queue, &list.backdrop);
        let leaves = self.flowers.push(&self.queue, &list.leaves);
        let flowers = self.flowers.push(&self.queue, &list.flowers);
        let top = self.flowers.push(&self.queue, &list.top_flowers);
        let halos = self.glyphs.push(&self.queue, &list.label_halos);
        let labels = self.glyphs.push(&self.queue, &list.labels);

        struct LayerRanges {
            rects: std::ops::Range<u64>,
            flowers: std::ops::Range<u64>,
            glyphs: std::ops::Range<u64>,
            image: Option<std::ops::Range<u64>>,
        }
        let mut layers = Vec::with_capacity(list.ui.len());
        for layer in &list.ui {
            let rects = self.rects.push(&self.queue, &layer.rects);
            let flowers = self.flowers.push(&self.queue, &layer.flowers);
            let glyphs = self.glyphs.push(&self.queue, &layer.glyphs);
            let image = layer.image_rect.map(|(rect, radius)| {
                self.images.ensure(&self.device, self.images.cursor + std::mem::size_of::<ImageInst>() as u64);
                self.images.push(&self.queue, &[ImageInst { rect, radius }])
            });
            layers.push(LayerRanges { rects, flowers, glyphs, image });
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bind, &[]);

            pass.set_pipeline(&self.ground);
            pass.draw(0..3, 0..1);

            if !list.backdrop.is_empty() {
                pass.set_vertex_buffer(0, self.flowers.buffer.slice(..));
                pass.set_pipeline(&self.flower);
                pass.draw(0..4, (backdrop.start / FLOWER) as u32..(backdrop.end / FLOWER) as u32);
            }

            // Paper goes down in layers, thinnest first. Each layer's shadow falls on what's
            // already there, so a limb visibly lies over the twigs tucked beneath it.
            let long_verts = 0..(LONG_SEGMENTS + 1 + CAP_ROWS) * 2;
            let short_verts = 0..(SHORT_SEGMENTS + 1 + CAP_ROWS) * 2;
            let long_count = list.branches_long.len() as u32;
            let short_count = list.branches_short.len() as u32;
            let path_count = list.path_branches.len() as u32;
            pass.set_vertex_buffer(0, self.branches.buffer.slice(..));
            if !list.branches_fine.is_empty() {
                pass.set_pipeline(&self.branch);
                pass.draw(short_verts.clone(), (fine.start / BRANCH) as u32..(fine.end / BRANCH) as u32);
            }
            if short_count > 0 {
                pass.set_pipeline(&self.branch_shadow);
                pass.draw(short_verts.clone(), (short.start / BRANCH) as u32..(short.end / BRANCH) as u32);
                pass.set_pipeline(&self.branch);
                pass.draw(short_verts, (short.start / BRANCH) as u32..(short.end / BRANCH) as u32);
            }
            if long_count > 0 {
                pass.set_pipeline(&self.branch_shadow);
                pass.draw(long_verts.clone(), (long.start / BRANCH) as u32..(long.end / BRANCH) as u32);
                pass.set_pipeline(&self.branch);
                pass.draw(long_verts.clone(), (long.start / BRANCH) as u32..(long.end / BRANCH) as u32);
            }

            pass.set_vertex_buffer(0, self.flowers.buffer.slice(..));
            pass.set_pipeline(&self.flower);
            if !list.leaves.is_empty() {
                pass.draw(0..4, (leaves.start / FLOWER) as u32..(leaves.end / FLOWER) as u32);
            }

            // The selected path lies over the rest of the tree.
            if path_count > 0 {
                pass.set_vertex_buffer(0, self.branches.buffer.slice(..));
                pass.set_pipeline(&self.branch_shadow);
                pass.draw(long_verts.clone(), (path.start / BRANCH) as u32..(path.end / BRANCH) as u32);
                pass.set_pipeline(&self.branch);
                pass.draw(long_verts, (path.start / BRANCH) as u32..(path.end / BRANCH) as u32);
            }

            // The front layer of soil covers the foot of the trunk.
            pass.set_pipeline(&self.soil_front);
            pass.draw(0..3, 0..1);

            pass.set_vertex_buffer(0, self.flowers.buffer.slice(..));
            pass.set_pipeline(&self.flower);
            if !list.flowers.is_empty() {
                pass.draw(0..4, (flowers.start / FLOWER) as u32..(flowers.end / FLOWER) as u32);
            }
            if !list.top_flowers.is_empty() {
                pass.draw(0..4, (top.start / FLOWER) as u32..(top.end / FLOWER) as u32);
            }

            if !list.labels.is_empty() {
                pass.set_pipeline(&self.glyph);
                pass.set_bind_group(1, &self.atlas_bind, &[]);
                pass.set_vertex_buffer(0, self.glyphs.buffer.slice(..));
                pass.draw(0..4, (halos.start / GLYPH) as u32..(halos.end / GLYPH) as u32);
                pass.draw(0..4, (labels.start / GLYPH) as u32..(labels.end / GLYPH) as u32);
            }

            for (layer, ranges) in list.ui.iter().zip(&layers) {
                match layer.clip {
                    Some([x0, y0, x1, y1]) => {
                        let x = x0.max(0.0) as u32;
                        let y = y0.max(0.0) as u32;
                        let w = (x1.min(width as f32) as u32).saturating_sub(x);
                        let h = (y1.min(height as f32) as u32).saturating_sub(y);
                        if w == 0 || h == 0 {
                            continue;
                        }
                        pass.set_scissor_rect(x, y, w, h);
                    }
                    None => pass.set_scissor_rect(0, 0, width, height),
                }
                if !layer.rects.is_empty() {
                    pass.set_pipeline(&self.rect);
                    pass.set_vertex_buffer(0, self.rects.buffer.slice(..));
                    pass.draw(0..4, (ranges.rects.start / RECT) as u32..(ranges.rects.end / RECT) as u32);
                }
                if let (Some(range), Some(bind)) = (&ranges.image, &self.image_bind) {
                    pass.set_pipeline(&self.image);
                    pass.set_bind_group(1, bind, &[]);
                    pass.set_vertex_buffer(0, self.images.buffer.slice(..));
                    let size = std::mem::size_of::<ImageInst>() as u64;
                    pass.draw(0..4, (range.start / size) as u32..(range.end / size) as u32);
                }
                if !layer.flowers.is_empty() {
                    pass.set_pipeline(&self.flower);
                    pass.set_vertex_buffer(0, self.flowers.buffer.slice(..));
                    pass.draw(0..4, (ranges.flowers.start / FLOWER) as u32..(ranges.flowers.end / FLOWER) as u32);
                }
                if !layer.glyphs.is_empty() {
                    pass.set_pipeline(&self.glyph);
                    pass.set_bind_group(1, &self.atlas_bind, &[]);
                    pass.set_vertex_buffer(0, self.glyphs.buffer.slice(..));
                    pass.draw(0..4, (ranges.glyphs.start / GLYPH) as u32..(ranges.glyphs.end / GLYPH) as u32);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        if let Some(frame) = acquired {
            self.queue.present(frame);
        }
        RenderResult::Drawn
    }

    /// Reads the offscreen frame back as tightly packed RGBA.
    pub fn read_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let Target::Offscreen { texture, width, height } = &self.target else { return None };
        let (width, height) = (*width, *height);
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |res| {
            let _ = tx.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;
        let data = buffer.get_mapped_range(..).ok()?;
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            out.extend_from_slice(&data[start..start + (width * 4) as usize]);
        }
        Some((width, height, out))
    }

}

pub enum RenderResult {
    Drawn,
    Skipped,
    Reconfigure,
}

/// The ways to draw, best first. Vulkan measured faster to start and lighter than DX12, and
/// starting only one keeps the others' libraries from loading. With no usable graphics card
/// at all, Windows can still draw in software.
fn adapter_options() -> Vec<(wgpu::Backends, bool)> {
    if std::env::var_os("FILETREE_SOFTWARE").is_some() {
        return vec![(wgpu::Backends::PRIMARY, true)];
    }
    if let Some(chosen) = wgpu::Backends::from_env() {
        return vec![(chosen, false), (chosen, true)];
    }
    if cfg!(target_os = "macos") {
        vec![(wgpu::Backends::PRIMARY, false)]
    } else {
        vec![
            (wgpu::Backends::VULKAN, false),
            (wgpu::Backends::PRIMARY, false),
            (wgpu::Backends::PRIMARY, true),
        ]
    }
}

fn make_instance(backends: wgpu::Backends) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        flags: wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    })
}

fn make_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("filetree"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: Default::default(),
    }))
    .map_err(|e| format!("{e}"))
}
