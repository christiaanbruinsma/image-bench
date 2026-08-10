use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Instant;

use glycin::MemoryFormat;

use crate::image_io::RawImage;

const GPU_MIN_SOURCE_PIXELS: u64 = 12_000_000;
const WORKGROUP_SIZE: u32 = 16;

const RESIZE_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

@group(0) @binding(2)
var destination_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let destination_size = textureDimensions(destination_texture);
    if (id.x >= destination_size.x || id.y >= destination_size.y) {
        return;
    }

    let uv = (vec2<f32>(id.xy) + vec2<f32>(0.5, 0.5)) /
        vec2<f32>(destination_size);
    let color = textureSampleLevel(source_texture, source_sampler, uv, 0.0);
    textureStore(destination_texture, vec2<i32>(id.xy), color);
}
"#;

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    bind_group_layout: wgpu::BindGroupLayout,
    compute_pipeline: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
    source_cache: Mutex<Option<GpuSourceTextureCache>>,
}

struct GpuSourceTextureCache {
    key: GpuSourceKey,
    texture: Arc<wgpu::Texture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GpuSourceKey {
    width: u32,
    height: u32,
    pixels_len: usize,
    channels: u8,
    sample_hash: u64,
}

#[derive(Debug)]
pub struct GpuResizeTimings {
    pub source_cache_hit: bool,
    pub prepare_input_ms: f64,
    pub texture_setup_ms: f64,
    pub upload_ms: f64,
    pub gpu_resize_ms: f64,
    pub readback_ms: f64,
    pub pack_output_ms: f64,
    pub total_ms: f64,
}

pub struct GpuResizeResult {
    pub image: RawImage,
    pub timings: GpuResizeTimings,
}

static GPU_CONTEXT: OnceLock<Option<GpuContext>> = OnceLock::new();

pub fn context() -> Option<&'static GpuContext> {
    GPU_CONTEXT.get().and_then(Option::as_ref)
}

pub fn available() -> bool {
    context().is_some()
}

pub fn should_use_gpu(source: &RawImage, width: u32, height: u32) -> bool {
    let Some(context) = context() else {
        return false;
    };

    let max_dimension = context.device.limits().max_texture_dimension_2d;
    let source_pixels = u64::from(source.width) * u64::from(source.height);

    source_pixels >= GPU_MIN_SOURCE_PIXELS
        && source.width <= max_dimension
        && source.height <= max_dimension
        && width <= max_dimension
        && height <= max_dimension
}

pub fn resize_bilinear(source: &RawImage, width: u32, height: u32) -> Result<GpuResizeResult, String> {
    let context = context().ok_or_else(|| "GPU context is unavailable".to_string())?;
    let total_started = Instant::now();

    let source_key = gpu_source_key(source)?;
    let mut source_cache_hit = false;
    let mut prepare_input_ms = 0.0;
    let mut source_texture_setup_ms = 0.0;
    let mut upload_ms = 0.0;

    let cached_texture = {
        let cache = context
            .source_cache
            .lock()
            .map_err(|_| "GPU source cache lock was poisoned".to_string())?;
        cache
            .as_ref()
            .filter(|cached| cached.key == source_key)
            .map(|cached| Arc::clone(&cached.texture))
    };

    let source_texture = if let Some(texture) = cached_texture {
        source_cache_hit = true;
        texture
    } else {
        let prepare_started = Instant::now();
        let rgba_source = to_rgba8(source)?;
        prepare_input_ms = prepare_started.elapsed().as_secs_f64() * 1000.0;

        let source_extent = wgpu::Extent3d {
            width: source.width,
            height: source.height,
            depth_or_array_layers: 1,
        };
        let source_texture_started = Instant::now();
        let texture = Arc::new(context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Image Bench GPU resize source"),
            size: source_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }));
        source_texture_setup_ms = source_texture_started.elapsed().as_secs_f64() * 1000.0;

        let upload_started = Instant::now();
        context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture.as_ref(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_source,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(source.width * 4),
                rows_per_image: Some(source.height),
            },
            source_extent,
        );
        upload_ms = upload_started.elapsed().as_secs_f64() * 1000.0;

        let mut cache = context
            .source_cache
            .lock()
            .map_err(|_| "GPU source cache lock was poisoned".to_string())?;
        *cache = Some(GpuSourceTextureCache {
            key: source_key,
            texture: Arc::clone(&texture),
        });
        texture
    };

    let texture_setup_started = Instant::now();
    let destination_extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let destination_texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Image Bench GPU resize destination"),
        size: destination_extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let source_view = source_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let destination_view = destination_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Image Bench GPU resize bind group"),
        layout: &context.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&context.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&destination_view),
            },
        ],
    });

    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = align_to(
        unpadded_bytes_per_row,
        wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
    );
    let readback_size = u64::from(padded_bytes_per_row) * u64::from(height);
    let readback_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Image Bench GPU resize readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let texture_setup_ms = source_texture_setup_ms
        + texture_setup_started.elapsed().as_secs_f64() * 1000.0;

    let gpu_started = Instant::now();
    let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Image Bench GPU resize encoder"),
    });
    {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Image Bench GPU resize pass"),
            timestamp_writes: None,
        });
        compute_pass.set_pipeline(&context.compute_pipeline);
        compute_pass.set_bind_group(0, &bind_group, &[]);
        compute_pass.dispatch_workgroups(
            width.div_ceil(WORKGROUP_SIZE),
            height.div_ceil(WORKGROUP_SIZE),
            1,
        );
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &destination_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        destination_extent,
    );
    context.queue.submit([encoder.finish()]);

    let slice = readback_buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    context
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|error| format!("GPU wait failed: {error}"))?;
    receiver
        .recv()
        .map_err(|error| format!("GPU readback callback failed: {error}"))?
        .map_err(|error| format!("GPU readback mapping failed: {error}"))?;
    let gpu_resize_ms = gpu_started.elapsed().as_secs_f64() * 1000.0;

    let readback_started = Instant::now();
    let mapped = slice
        .get_mapped_range()
        .map_err(|error| format!("GPU readback access failed: {error}"))?;
    let mut rgba_output = Vec::with_capacity(width as usize * height as usize * 4);
    let row_bytes = unpadded_bytes_per_row as usize;
    let padded_row_bytes = padded_bytes_per_row as usize;
    for row in 0..height as usize {
        let start = row * padded_row_bytes;
        rgba_output.extend_from_slice(&mapped[start..start + row_bytes]);
    }
    drop(mapped);
    readback_buffer.unmap();
    let readback_ms = readback_started.elapsed().as_secs_f64() * 1000.0;

    let pack_started = Instant::now();
    let pixels = from_rgba8(&rgba_output, source.format)?;
    let pack_output_ms = pack_started.elapsed().as_secs_f64() * 1000.0;

    Ok(GpuResizeResult {
        image: RawImage {
            width,
            height,
            format: source.format,
            pixels,
        },
        timings: GpuResizeTimings {
            source_cache_hit,
            prepare_input_ms,
            texture_setup_ms,
            upload_ms,
            gpu_resize_ms,
            readback_ms,
            pack_output_ms,
            total_ms: total_started.elapsed().as_secs_f64() * 1000.0,
        },
    })
}

pub async fn initialize() {
    if GPU_CONTEXT.get().is_some() {
        return;
    }

    match create_context().await {
        Ok(context) => {
            let info = &context.adapter_info;
            eprintln!(
                "[Image Bench GPU] status=available backend={:?} device_type={:?} adapter={} vendor=0x{:04x} device=0x{:04x} driver={} driver_info={}",
                info.backend,
                info.device_type,
                info.name,
                info.vendor,
                info.device,
                info.driver,
                info.driver_info,
            );
            let _ = GPU_CONTEXT.set(Some(context));
        }
        Err(error) => {
            eprintln!("[Image Bench GPU] status=unavailable fallback=Cpu reason={error}");
            let _ = GPU_CONTEXT.set(None);
        }
    }
}

async fn create_context() -> Result<GpuContext, String> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })
        .await
        .map_err(|error| format!("adapter request failed: {error}"))?;

    let adapter_info = adapter.get_info();
    if adapter_info.device_type == wgpu::DeviceType::Cpu {
        return Err(format!(
            "software adapter rejected: {} ({:?})",
            adapter_info.name, adapter_info.backend
        ));
    }

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Image Bench GPU device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("device request failed: {error}"))?;

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Image Bench GPU resize bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Image Bench GPU resize pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Image Bench GPU resize shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(RESIZE_SHADER)),
    });
    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Image Bench GPU resize pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Image Bench GPU resize sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });

    Ok(GpuContext {
        device,
        queue,
        adapter_info,
        bind_group_layout,
        compute_pipeline,
        sampler,
        source_cache: Mutex::new(None),
    })
}

fn gpu_source_key(source: &RawImage) -> Result<GpuSourceKey, String> {
    let channels = match source.format {
        MemoryFormat::R8g8b8a8 => 4,
        MemoryFormat::R8g8b8 => 3,
        other => return Err(format!("Unsupported GPU resize format: {other:?}")),
    };

    let mut hasher = DefaultHasher::new();
    source.width.hash(&mut hasher);
    source.height.hash(&mut hasher);
    source.pixels.len().hash(&mut hasher);
    channels.hash(&mut hasher);

    let len = source.pixels.len();
    for offset in [0, len / 2, len.saturating_sub(64)] {
        let end = (offset + 64).min(len);
        source.pixels[offset..end].hash(&mut hasher);
    }

    Ok(GpuSourceKey {
        width: source.width,
        height: source.height,
        pixels_len: source.pixels.len(),
        channels,
        sample_hash: hasher.finish(),
    })
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn to_rgba8(source: &RawImage) -> Result<Vec<u8>, String> {
    match source.format {
        MemoryFormat::R8g8b8a8 => Ok(source.pixels.clone()),
        MemoryFormat::R8g8b8 => {
            let mut rgba = Vec::with_capacity(source.width as usize * source.height as usize * 4);
            for pixel in source.pixels.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
            Ok(rgba)
        }
        other => Err(format!("Unsupported GPU resize format: {other:?}")),
    }
}

fn from_rgba8(rgba: &[u8], format: MemoryFormat) -> Result<Vec<u8>, String> {
    match format {
        MemoryFormat::R8g8b8a8 => Ok(rgba.to_vec()),
        MemoryFormat::R8g8b8 => {
            let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
            for pixel in rgba.chunks_exact(4) {
                rgb.extend_from_slice(&pixel[..3]);
            }
            Ok(rgb)
        }
        other => Err(format!("Unsupported GPU resize format: {other:?}")),
    }
}
