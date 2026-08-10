use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fast_image_resize as fir;
use glycin::{Creator, MemoryFormat, MimeType};
use gtk::gio;

use crate::image_io::{self, RawImage};
use crate::gpu;
use crate::logic::{
    QualityPreset, calculate_dimensions, collision_safe_destination,
    quality_percentage, encoding_values, output_mime_type,
    should_skip_non_smaller_jpeg_export,
};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeBackend {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy)]
pub struct ImageProcessor {
    resize_backend: ResizeBackend,
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self {
            resize_backend: if gpu::available() {
                ResizeBackend::Gpu
            } else {
                ResizeBackend::Cpu
            },
        }
    }
}

impl ImageProcessor {
    pub fn resize_backend(&self) -> ResizeBackend {
        self.resize_backend
    }
}

#[derive(Debug, Clone)]
pub struct ProcessOptions {
    pub target_width: u32,
    pub quality_preset: QualityPreset,
    pub custom_quality: Option<u8>,
    pub add_filename_suffix: bool,
    pub filename_suffix: String,
    pub add_quality_suffix: bool,
}

#[derive(Debug, Clone)]
pub struct ProcessResult {
    pub source: PathBuf,
    pub destination: Option<PathBuf>,
    pub original_bytes: u64,
    pub output_bytes: u64,
    pub original_width: u32,
    pub original_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub skipped_not_smaller: bool,
}

#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub encoded_data: Vec<u8>,
    pub original_bytes: u64,
    pub original_width: u32,
    pub original_height: u32,
    pub output_width: u32,
    pub output_height: u32,
}

struct EncodedRender {
    data: Vec<u8>,
    original_bytes: u64,
    original_width: u32,
    original_height: u32,
    output_width: u32,
    output_height: u32,
    source_suffix: String,
    effective_quality: Option<u8>,
}

pub async fn process(
    source: &Path,
    output_dir: &Path,
    options: &ProcessOptions,
    cancellable: Option<&gio::Cancellable>,
) -> Result<ProcessResult, String> {
    fs::create_dir_all(output_dir).map_err(|error| error.to_string())?;
    let render = render_encoded(source, None, options, cancellable).await?;

    let encoded_size = render.data.len() as u64;

    if should_skip_non_smaller_jpeg_export(
        &render.source_suffix,
        render.original_bytes,
        encoded_size,
        render.original_width,
        render.original_height,
        render.output_width,
        render.output_height,
    ) {
        return Ok(ProcessResult {
            source: source.to_path_buf(),
            destination: None,
            original_bytes: render.original_bytes,
            output_bytes: render.original_bytes,
            original_width: render.original_width,
            original_height: render.original_height,
            output_width: render.output_width,
            output_height: render.output_height,
            skipped_not_smaller: true,
        });
    }

    let filename_suffix = if options.add_filename_suffix {
        options.filename_suffix.as_str()
    } else {
        ""
    };
    let quality_suffix = if options.add_quality_suffix {
        Some(render.effective_quality.unwrap_or(quality_percentage(
            options.quality_preset,
            options.custom_quality,
        )?))
    } else {
        None
    };
    let destination = collision_safe_destination(
        output_dir,
        source,
        filename_suffix,
        quality_suffix,
    )?;

    write_new_file(&destination, &render.data)?;
    let output_bytes = fs::metadata(&destination)
        .map_err(|error| error.to_string())?
        .len();

    Ok(ProcessResult {
        source: source.to_path_buf(),
        destination: Some(destination),
        original_bytes: render.original_bytes,
        output_bytes,
        original_width: render.original_width,
        original_height: render.original_height,
        output_width: render.output_width,
        output_height: render.output_height,
        skipped_not_smaller: false,
    })
}

pub async fn render_preview(
    source: &Path,
    options: &ProcessOptions,
    cancellable: Option<&gio::Cancellable>,
) -> Result<PreviewResult, String> {
    render_preview_with_decoded(source, None, options, cancellable).await
}

pub async fn render_preview_with_decoded(
    source: &Path,
    decoded: Option<Arc<RawImage>>,
    options: &ProcessOptions,
    cancellable: Option<&gio::Cancellable>,
) -> Result<PreviewResult, String> {
    let render = render_encoded(source, decoded, options, cancellable).await?;
    Ok(PreviewResult {
        encoded_data: render.data,
        original_bytes: render.original_bytes,
        original_width: render.original_width,
        original_height: render.original_height,
        output_width: render.output_width,
        output_height: render.output_height,
    })
}

async fn render_encoded(
    source: &Path,
    decoded: Option<Arc<RawImage>>,
    options: &ProcessOptions,
    cancellable: Option<&gio::Cancellable>,
) -> Result<EncodedRender, String> {
    let total_started = Instant::now();
    let original_bytes = fs::metadata(source).map_err(|error| error.to_string())?.len();

    let processor = ImageProcessor::default();
    let resize_backend = processor.resize_backend();

    let decode_started = Instant::now();
    let (decoded, decoded_supplied) = match decoded {
        Some(decoded) => (decoded, true),
        None => (Arc::new(image_io::decode(source, cancellable).await?), false),
    };
    let decode_elapsed = decode_started.elapsed();
    let original_width = decoded.width;
    let original_height = decoded.height;
    let (mut output_width, mut output_height) = calculate_dimensions(
        original_width,
        original_height,
        options.target_width,
    )?;

    let resize_started = Instant::now();
    let resized = if (output_width, output_height) == (original_width, original_height) {
        None
    } else {
        let decoded = decoded.clone();
        Some(
            gio::spawn_blocking(move || {
                resize_with_backend(resize_backend, decoded.as_ref(), output_width, output_height)
            })
            .await
            .map_err(|_| "Resize worker failed".to_string())??,
        )
    };
    let actual_resize_backend = resized
        .as_ref()
        .map(|result| result.backend)
        .unwrap_or(ResizeBackend::Cpu);
    let image = resized
        .as_ref()
        .map(|result| &result.image)
        .unwrap_or(decoded.as_ref());
    let resize_elapsed = resize_started.elapsed();

    let source_suffix = source
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Source file has no extension".to_string())?
        .to_string();
    let mime = output_mime_type(&source_suffix)?;
    let (quality, compression) = encoding_values(
        options.quality_preset,
        &source_suffix,
        options.custom_quality,
    )?;

    let encode_started = Instant::now();
    let (mut data, mut effective_quality) = if let Some(max_quality) = quality {
        match encode_jpeg_with_source_limit(
            source,
            image,
            mime,
            max_quality,
            original_bytes,
            cancellable,
        )
        .await?
        {
            Some((data, quality)) => (data, Some(quality)),
            None => {
                // No JPEG quality in the supported 1..=max range could satisfy
                // the hard "never larger than source" invariant. Returning the
                // original bytes makes Preview match the export decision; process()
                // will skip writing because dimensions and byte size are unchanged.
                output_width = original_width;
                output_height = original_height;
                (
                    fs::read(source).map_err(|error| {
                        format!("Could not read {}: {error}", source.display())
                    })?,
                    None,
                )
            }
        }
    } else {
        (
            encode_image(source, image, mime, None, compression, cancellable).await?,
            None,
        )
    };

    let encode_elapsed = encode_started.elapsed();

    if should_skip_non_smaller_jpeg_export(
        &source_suffix,
        original_bytes,
        data.len() as u64,
        original_width,
        original_height,
        output_width,
        output_height,
    ) {
        data = fs::read(source)
            .map_err(|error| format!("Could not read {}: {error}", source.display()))?;
        output_width = original_width;
        output_height = original_height;
        effective_quality = None;
    }

    let total_elapsed = total_started.elapsed();
    eprintln!(
        "[Image Bench performance] stage=render path={} decode_ms={:.2} resize_ms={:.2} encode_ms={:.2} total_ms={:.2} source={}x{} output={}x{} output_bytes={} decoded_source={} resize_backend={:?}",
        source.display(),
        decode_elapsed.as_secs_f64() * 1000.0,
        resize_elapsed.as_secs_f64() * 1000.0,
        encode_elapsed.as_secs_f64() * 1000.0,
        total_elapsed.as_secs_f64() * 1000.0,
        original_width,
        original_height,
        output_width,
        output_height,
        data.len(),
        if decoded_supplied { "supplied" } else { "fresh" },
        actual_resize_backend,
    );

    Ok(EncodedRender {
        data,
        original_bytes,
        original_width,
        original_height,
        output_width,
        output_height,
        source_suffix,
        effective_quality,
    })
}

async fn encode_jpeg_with_source_limit(
    source: &Path,
    image: &RawImage,
    mime: &str,
    max_quality: u8,
    original_bytes: u64,
    cancellable: Option<&gio::Cancellable>,
) -> Result<Option<(Vec<u8>, u8)>, String> {
    let initial = encode_image(
        source,
        image,
        mime,
        Some(max_quality),
        None,
        cancellable,
    )
    .await?;
    if initial.len() as u64 <= original_bytes {
        return Ok(Some((initial, max_quality)));
    }

    // Search the valid JPEG quality range for the highest quality whose actual
    // encoded byte size does not exceed the source. Every accepted candidate is
    // verified against the encoded bytes themselves, so the size invariant does
    // not depend on an estimate.
    let mut low = 1u8;
    let mut high = max_quality.saturating_sub(1);
    let mut best: Option<(Vec<u8>, u8)> = None;

    while low <= high {
        let mid = low + (high - low) / 2;
        let encoded = encode_image(source, image, mime, Some(mid), None, cancellable).await?;
        if encoded.len() as u64 <= original_bytes {
            best = Some((encoded, mid));
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    Ok(best)
}

async fn encode_image(
    source: &Path,
    image: &RawImage,
    mime: &str,
    quality: Option<u8>,
    compression: Option<u8>,
    cancellable: Option<&gio::Cancellable>,
) -> Result<Vec<u8>, String> {
    let mut creator = Creator::new(MimeType::new(mime.to_string()))
        .await
        .map_err(|error| format!("Could not create encoder: {error}"))?;
    if let Some(cancellable) = cancellable {
        creator.cancellable(cancellable.clone());
    }
    if let Some(quality) = quality {
        creator
            .set_encoding_quality(quality)
            .map_err(|error| format!("Encoder rejected quality: {error}"))?;
    }
    if let Some(compression) = compression {
        creator
            .set_encoding_compression(compression)
            .map_err(|error| format!("Encoder rejected compression: {error}"))?;
    }

    creator
        .add_frame_with_stride(
            image.width,
            image.height,
            image.stride(),
            image.format,
            image.pixels.clone(),
        )
        .map_err(|error| format!("Could not add image frame: {error}"))?;
    let encoded = creator
        .create()
        .await
        .map_err(|error| format!("Could not encode {}: {error}", source.display()))?;
    encoded
        .data_full()
        .map_err(|error| format!("Could not read encoded image data: {error}"))
}

struct ResizeResult {
    image: RawImage,
    backend: ResizeBackend,
}

fn resize_with_backend(
    backend: ResizeBackend,
    source: &RawImage,
    width: u32,
    height: u32,
) -> Result<ResizeResult, String> {
    if backend == ResizeBackend::Gpu && gpu::should_use_gpu(source, width, height) {
        match gpu::resize_bilinear(source, width, height) {
            Ok(result) => {
                eprintln!(
                    "[Image Bench GPU] stage=resize status=success source={}x{} output={}x{} source_cache={} prepare_input_ms={:.2} texture_setup_ms={:.2} upload_ms={:.2} gpu_resize_ms={:.2} readback_ms={:.2} pack_output_ms={:.2} total_ms={:.2}",
                    source.width,
                    source.height,
                    width,
                    height,
                    if result.timings.source_cache_hit { "hit" } else { "miss" },
                    result.timings.prepare_input_ms,
                    result.timings.texture_setup_ms,
                    result.timings.upload_ms,
                    result.timings.gpu_resize_ms,
                    result.timings.readback_ms,
                    result.timings.pack_output_ms,
                    result.timings.total_ms,
                );
                return Ok(ResizeResult {
                    image: result.image,
                    backend: ResizeBackend::Gpu,
                });
            }
            Err(error) => {
                eprintln!(
                    "[Image Bench GPU] stage=resize status=fallback fallback=Cpu reason={error}"
                );
            }
        }
    }

    Ok(ResizeResult {
        image: resize_bilinear(source, width, height)?,
        backend: ResizeBackend::Cpu,
    })
}

fn resize_bilinear(source: &RawImage, width: u32, height: u32) -> Result<RawImage, String> {
    let pixel_type = match source.format {
        MemoryFormat::R8g8b8 => fir::PixelType::U8x3,
        MemoryFormat::R8g8b8a8 => fir::PixelType::U8x4,
        other => return Err(format!("Unsupported resize format: {other:?}")),
    };

    let source_image = fir::images::ImageRef::new(
        source.width,
        source.height,
        &source.pixels,
        pixel_type,
    )
    .map_err(|error| format!("Invalid source image buffer: {error}"))?;
    let mut destination = fir::images::Image::new(width, height, pixel_type);
    let options = fir::ResizeOptions::new().resize_alg(
        fir::ResizeAlg::Convolution(fir::FilterType::Bilinear),
    );
    fir::Resizer::new()
        .resize(&source_image, &mut destination, Some(&options))
        .map_err(|error| format!("Could not resize image: {error}"))?;

    Ok(RawImage {
        width,
        height,
        format: source.format,
        pixels: destination.into_vec(),
    })
}

fn write_new_file(destination: &Path, data: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    file.write_all(data)
        .map_err(|error| format!("Could not write {}: {error}", destination.display()))?;
    file.flush().map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(())
}
