//! Noir's Web-content GPU command backend. egui owns the window/render pass;
//! page quads, texture bindings, glyph coverage and draws use this own pipeline.
use eframe::{
    egui::{self, PaintCallbackInfo, Painter, Rect},
    egui_wgpu::{Callback, CallbackResources, CallbackTrait, ScreenDescriptor},
    wgpu::{self, util::DeviceExt as _},
};
use std::{
    borrow::Cow,
    collections::VecDeque,
    ops::Range,
    sync::{Arc, Mutex},
};
use web_graphics::{Scene, TextureFormat};

const VERTEX_STRIDE: usize = 36;
const MAX_VERTEX_BYTES: usize = VERTEX_STRIDE * 900_000;
const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DRAWS: usize = 100_000;
const MAX_TEXTURES: usize = 34;
const MAX_CACHED_SCENES: usize = 2;
const MAX_FRAME_TRANSFORMS: usize = 512;

/// The application keeps MSAA disabled in NativeOptions, so its render pass
/// and this pipeline both have one sample. This is a user-space graphics
/// backend; the operating system still supplies the hardware device driver.
#[derive(Clone)]
pub struct Gpu {
    maximum_texture_dimension: u32,
    maximum_buffer_size: u64,
    supported: Arc<Mutex<Option<(u64, bool)>>>,
}

impl Gpu {
    pub fn install(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let state = cc.wgpu_render_state.as_ref()?;
        let limits = state.device.limits();
        if limits.max_texture_dimension_2d < 2048 || limits.max_bind_groups < 2 {
            return None;
        }
        // Keep the native presentation path available if backend initialization
        // is rejected by the device. Page data never controls shader source.
        let resources = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Resources::new(&state.device, state.target_format)
        }))
        .ok()?;
        state.renderer.write().callback_resources.insert(resources);
        Some(Self {
            maximum_texture_dimension: limits.max_texture_dimension_2d,
            maximum_buffer_size: limits.max_buffer_size,
            supported: Arc::new(Mutex::new(None)),
        })
    }

    /// Scene IDs identify immutable prepared scenes. The pure graphics crate
    /// validates page coordinates/vertex components before generating bytes;
    /// this boundary additionally checks device limits and all buffer ranges.
    pub fn supports(&self, scene: &Scene) -> bool {
        let Ok(mut cached) = self.supported.lock() else {
            return false;
        };
        if let Some((id, supported)) = *cached
            && id == scene.id
        {
            return supported;
        }
        let supported = self.scene_fits(scene);
        *cached = Some((scene.id, supported));
        supported
    }

    fn scene_fits(&self, scene: &Scene) -> bool {
        if scene.vertex_bytes.len() > MAX_VERTEX_BYTES
            || scene.vertex_bytes.len() as u64 > self.maximum_buffer_size
            || !scene.vertex_bytes.len().is_multiple_of(VERTEX_STRIDE)
            || scene.draws.len() > MAX_DRAWS
            || scene.textures.is_empty()
            || scene.textures.len() > MAX_TEXTURES
        {
            return false;
        }
        let mut bytes = 0usize;
        for texture in &scene.textures {
            let channels = match &texture.format {
                TextureFormat::Rgba8 => 4usize,
                TextureFormat::Coverage8 => 1,
            };
            let expected = (texture.width as usize)
                .checked_mul(texture.height as usize)
                .and_then(|pixels| pixels.checked_mul(channels));
            if texture.width == 0
                || texture.height == 0
                || texture.width > self.maximum_texture_dimension
                || texture.height > self.maximum_texture_dimension
                || expected != Some(texture.bytes.len())
            {
                return false;
            }
            let Some(total) = bytes.checked_add(texture.bytes.len()) else {
                return false;
            };
            bytes = total;
            if bytes > MAX_TEXTURE_BYTES {
                return false;
            }
        }
        let vertices = scene.vertex_bytes.len() / VERTEX_STRIDE;
        scene.draws.iter().all(|draw| {
            draw.texture < scene.textures.len()
                && draw.vertices.start <= draw.vertices.end
                && draw.vertices.end as usize <= vertices
                && draw.vertices.start % 3 == 0
                && (draw.vertices.end - draw.vertices.start) % 3 == 0
                && finite_bounds(draw.bounds)
        })
    }

    /// Returns false before queuing a callback if the caller must select its
    /// existing fallback. Paint ranges are contiguous runs between controls,
    /// so native editing controls retain their original display-list order.
    pub fn paint(
        &self,
        painter: &Painter,
        scene: Arc<Scene>,
        draws: Range<usize>,
        canvas: Rect,
        zoom: f32,
    ) -> bool {
        if !self.supports(&scene)
            || draws.start > draws.end
            || draws.end > scene.draws.len()
            || !zoom.is_finite()
            || !(0.25..=4.).contains(&zoom)
            || !canvas.min.x.is_finite()
            || !canvas.min.y.is_finite()
            || !canvas.max.x.is_finite()
            || !canvas.max.y.is_finite()
        {
            return false;
        }
        let clip = canvas.intersect(painter.clip_rect());
        if draws.is_empty() || !clip.is_positive() {
            return true;
        }
        painter.add(Callback::new_paint_callback(
            clip,
            PageCallback {
                scene,
                draws,
                canvas,
                zoom,
                clip,
                frame: painter.ctx().cumulative_frame_nr(),
                prepared: Mutex::new(None),
            },
        ));
        true
    }
}

fn finite_bounds(rect: browser_ipc::Rect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
        && rect.width >= 0.
        && rect.height >= 0.
}

struct PageCallback {
    scene: Arc<Scene>,
    draws: Range<usize>,
    canvas: Rect,
    clip: Rect,
    zoom: f32,
    frame: u64,
    prepared: Mutex<Option<Prepared>>,
}

struct Prepared {
    scene: Arc<DeviceScene>,
    transform: Arc<Transform>,
}

struct DeviceScene {
    vertices: wgpu::Buffer,
    textures: Vec<DeviceTexture>,
}
struct DeviceTexture {
    // Keep explicit ownership, in addition to the bind group's internal refs.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}
struct Transform {
    _buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    transform_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    scenes: VecDeque<(u64, Arc<DeviceScene>)>,
    frame: Option<u64>,
    transforms: Vec<([u32; 8], Arc<Transform>)>,
}

impl Resources {
    fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let transform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Noir page transform layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(32),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Noir page texture layout"),
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Noir own page shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Noir page pipeline layout"),
            bind_group_layouts: &[Some(&transform_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Noir page triangles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(if target.is_srgb() {
                    "fragment_linear"
                } else {
                    "fragment_gamma"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Noir page linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            transform_layout,
            texture_layout,
            sampler,
            scenes: VecDeque::new(),
            frame: None,
            transforms: Vec::new(),
        }
    }

    fn scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
    ) -> Arc<DeviceScene> {
        if let Some(index) = self.scenes.iter().position(|(id, _)| *id == scene.id) {
            let cached = self.scenes.remove(index).unwrap();
            let result = cached.1.clone();
            self.scenes.push_back(cached);
            return result;
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Noir page vertex buffer"),
            contents: if scene.vertex_bytes.is_empty() {
                &[0; VERTEX_STRIDE]
            } else {
                &scene.vertex_bytes
            },
            usage: wgpu::BufferUsages::VERTEX,
        });
        let textures = scene
            .textures
            .iter()
            .map(|source| {
                let (format, channels) = match &source.format {
                    TextureFormat::Rgba8 => (wgpu::TextureFormat::Rgba8Unorm, 4),
                    TextureFormat::Coverage8 => (wgpu::TextureFormat::R8Unorm, 1),
                };
                let size = wgpu::Extent3d {
                    width: source.width,
                    height: source.height,
                    depth_or_array_layers: 1,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Noir page authorized texture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                // Filter associated RGB/alpha together so transparent PNG/WebP
                // pixels cannot introduce their hidden RGB into visible edges.
                // Scene data remains straight RGBA; this temporary conversion is
                // bounded by the already checked per-scene texture byte budget.
                let upload: Cow<'_, [u8]> = match &source.format {
                    TextureFormat::Coverage8 => Cow::Borrowed(&source.bytes),
                    TextureFormat::Rgba8 => {
                        let mut bytes = source.bytes.clone();
                        for pixel in bytes.as_chunks_mut::<4>().0 {
                            let alpha = u32::from(pixel[3]);
                            for channel in &mut pixel[..3] {
                                *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
                            }
                        }
                        Cow::Owned(bytes)
                    }
                };
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    upload.as_ref(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(source.width * channels),
                        rows_per_image: Some(source.height),
                    },
                    size,
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Noir page texture binding"),
                    layout: &self.texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                DeviceTexture {
                    _texture: texture,
                    bind_group,
                }
            })
            .collect();
        let result = Arc::new(DeviceScene { vertices, textures });
        self.scenes.push_back((scene.id, result.clone()));
        while self.scenes.len() > MAX_CACHED_SCENES {
            self.scenes.pop_front();
        }
        result
    }

    fn transform(&mut self, device: &wgpu::Device, frame: u64, values: [f32; 8]) -> Arc<Transform> {
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.transforms.clear();
        }
        let key = values.map(f32::to_bits);
        if let Some((_, transform)) = self.transforms.iter().find(|(cached, _)| *cached == key) {
            return transform.clone();
        }
        let bytes: Vec<u8> = values.into_iter().flat_map(f32::to_le_bytes).collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Noir page immutable transform"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Noir page transform binding"),
            layout: &self.transform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let transform = Arc::new(Transform {
            _buffer: buffer,
            bind_group,
        });
        if self.transforms.len() < MAX_FRAME_TRANSFORMS {
            self.transforms.push((key, transform.clone()));
        }
        transform
    }
}

impl CallbackTrait for PageCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Ok(mut prepared) = self.prepared.lock() else {
            return Vec::new();
        };
        let Some(resources) = resources.get_mut::<Resources>() else {
            return Vec::new();
        };
        if !screen.pixels_per_point.is_finite()
            || screen.pixels_per_point <= 0.
            || screen.size_in_pixels.contains(&0)
        {
            return Vec::new();
        }
        let scene = resources.scene(device, queue, &self.scene);
        let transform = resources.transform(
            device,
            self.frame,
            [
                self.canvas.min.x,
                self.canvas.min.y,
                screen.size_in_pixels[0] as f32 / screen.pixels_per_point,
                screen.size_in_pixels[1] as f32 / screen.pixels_per_point,
                self.zoom,
                0.,
                0.,
                0.,
            ],
        );
        // Immutable per-frame transforms cannot be overwritten by a later
        // callback before this callback reaches the render pass.
        *prepared = Some(Prepared { scene, transform });
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(resources) = resources.get::<Resources>() else {
            return;
        };
        let Ok(prepared) = self.prepared.lock() else {
            return;
        };
        let Some(prepared) = prepared.as_ref() else {
            return;
        };
        if info.screen_size_px.contains(&0)
            || !info.pixels_per_point.is_finite()
            || info.pixels_per_point <= 0.
        {
            return;
        }
        let clip = self.clip.intersect(info.clip_rect);
        if !clip.is_positive() {
            return;
        }
        let ppp = info.pixels_per_point;
        let left = (clip.left() * ppp)
            .floor()
            .clamp(0., info.screen_size_px[0] as f32) as u32;
        let top = (clip.top() * ppp)
            .floor()
            .clamp(0., info.screen_size_px[1] as f32) as u32;
        let right = (clip.right() * ppp)
            .ceil()
            .clamp(0., info.screen_size_px[0] as f32) as u32;
        let bottom = (clip.bottom() * ppp)
            .ceil()
            .clamp(0., info.screen_size_px[1] as f32) as u32;
        if right <= left || bottom <= top {
            return;
        }
        pass.set_viewport(
            0.,
            0.,
            info.screen_size_px[0] as f32,
            info.screen_size_px[1] as f32,
            0.,
            1.,
        );
        pass.set_scissor_rect(left, top, right - left, bottom - top);
        pass.set_pipeline(&resources.pipeline);
        pass.set_vertex_buffer(0, prepared.scene.vertices.slice(..));
        pass.set_bind_group(0, &prepared.transform.bind_group, &[]);
        let mut pending: Option<(usize, Range<u32>)> = None;
        for draw in &self.scene.draws[self.draws.clone()] {
            let rect = draw.bounds;
            let bounds = Rect::from_min_size(
                self.canvas.min + egui::vec2(rect.x, rect.y) * self.zoom,
                egui::vec2(rect.width, rect.height) * self.zoom,
            );
            if !bounds.intersects(clip) || draw.vertices.is_empty() {
                continue;
            }
            if let Some((texture, range)) = &mut pending
                && *texture == draw.texture
                && range.end == draw.vertices.start
            {
                range.end = draw.vertices.end;
                continue;
            }
            if let Some((texture, range)) = pending.take() {
                pass.set_bind_group(1, &prepared.scene.textures[texture].bind_group, &[]);
                pass.draw(range, 0..1);
            }
            pending = Some((draw.texture, draw.vertices.clone()));
        }
        if let Some((texture, range)) = pending {
            pass.set_bind_group(1, &prepared.scene.textures[texture].bind_group, &[]);
            pass.draw(range, 0..1);
        }
    }
}

const SHADER: &str = r#"
struct Transform {
    origin: vec2<f32>,
    screen: vec2<f32>,
    zoom: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}
@group(0) @binding(0) var<uniform> transform: Transform;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mode: f32,
}
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) mode: f32,
}
@vertex fn vertex_main(input: VertexInput) -> VertexOutput {
    let point = input.position * transform.zoom + transform.origin;
    var output: VertexOutput;
    output.position = vec4<f32>(point.x / transform.screen.x * 2.0 - 1.0,
        1.0 - point.y / transform.screen.y * 2.0, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    output.mode = input.mode;
    return output;
}
fn linear_color(color: vec3<f32>) -> vec3<f32> {
    return select(pow((color + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4)),
        color / vec3<f32>(12.92), color <= vec3<f32>(0.04045));
}
fn shaded(input: VertexOutput, linear_target: bool) -> vec4<f32> {
    let texel = textureSample(image, image_sampler, input.uv);
    if input.mode > 0.5 {
        let alpha = input.color.a * texel.r;
        var color = input.color.rgb;
        if linear_target { color = linear_color(color); }
        return vec4<f32>(color * alpha, alpha);
    }
    let alpha = input.color.a * texel.a;
    if linear_target {
        let straight = texel.rgb / max(texel.a, 0.000001);
        return vec4<f32>(linear_color(straight) * linear_color(input.color.rgb) * alpha, alpha);
    }
    return vec4<f32>(texel.rgb * input.color.rgb * input.color.a, alpha);
}
@fragment fn fragment_gamma(input: VertexOutput) -> @location(0) vec4<f32> {
    return shaded(input, false);
}
@fragment fn fragment_linear(input: VertexOutput) -> @location(0) vec4<f32> {
    return shaded(input, true);
}
"#;
