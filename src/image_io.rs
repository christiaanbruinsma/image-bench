use std::path::Path;

use glycin::{Loader, MemoryFormat, MemoryFormatSelection};
use gtk::{gio, glib};
use gtk::prelude::*;

#[derive(Debug, Clone)]
pub struct RawImage {
    pub width: u32,
    pub height: u32,
    pub format: MemoryFormat,
    pub pixels: Vec<u8>,
}

impl RawImage {
    pub fn channels(&self) -> usize {
        match self.format {
            MemoryFormat::R8g8b8 => 3,
            MemoryFormat::R8g8b8a8 => 4,
            _ => unreachable!("RawImage is normalized to RGB8/RGBA8"),
        }
    }

    pub fn stride(&self) -> u32 {
        self.width * self.channels() as u32
    }
}

pub fn texture_from_raw(image: &RawImage) -> gtk::gdk::Texture {
    let format = match image.channels() {
        3 => gtk::gdk::MemoryFormat::R8g8b8,
        4 => gtk::gdk::MemoryFormat::R8g8b8a8,
        _ => unreachable!("RawImage is normalized to RGB8/RGBA8"),
    };
    let bytes = glib::Bytes::from_owned(image.pixels.clone());
    gtk::gdk::MemoryTexture::new(
        image.width as i32,
        image.height as i32,
        format,
        &bytes,
        image.stride() as usize,
    )
    .upcast()
}

pub async fn decode(path: &Path, cancellable: Option<&gio::Cancellable>) -> Result<RawImage, String> {
    let file = gio::File::for_path(path);
    let mut loader = Loader::new(file);
    loader.accepted_memory_formats(
        MemoryFormatSelection::R8g8b8 | MemoryFormatSelection::R8g8b8a8,
    );
    if let Some(cancellable) = cancellable {
        loader.cancellable(cancellable.clone());
    }

    let image = loader
        .load()
        .await
        .map_err(|error| format!("Could not load {}: {error}", path.display()))?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;

    let format = frame.memory_format();
    let channels = match format {
        MemoryFormat::R8g8b8 => 3usize,
        MemoryFormat::R8g8b8a8 => 4usize,
        other => return Err(format!("Unsupported decoded memory format: {other:?}")),
    };
    let width = frame.width();
    let height = frame.height();
    let source_stride = frame.stride() as usize;
    let row_bytes = width as usize * channels;
    let source = frame.buf_slice();
    let mut pixels = Vec::with_capacity(row_bytes * height as usize);

    for row in 0..height as usize {
        let start = row * source_stride;
        let end = start + row_bytes;
        let slice = source
            .get(start..end)
            .ok_or_else(|| format!("Invalid decoded stride for {}", path.display()))?;
        pixels.extend_from_slice(slice);
    }

    Ok(RawImage {
        width,
        height,
        format,
        pixels,
    })
}

pub async fn decode_texture(
    path: &Path,
    cancellable: Option<&gio::Cancellable>,
) -> Result<gtk::gdk::Texture, String> {
    let file = gio::File::for_path(path);
    let mut loader = Loader::new(file);
    if let Some(cancellable) = cancellable {
        loader.cancellable(cancellable.clone());
    }
    let image = loader
        .load()
        .await
        .map_err(|error| format!("Could not load preview {}: {error}", path.display()))?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| format!("Could not decode preview {}: {error}", path.display()))?;
    Ok(frame.texture())
}

pub async fn decode_texture_vec(
    data: Vec<u8>,
    cancellable: Option<&gio::Cancellable>,
) -> Result<gtk::gdk::Texture, String> {
    let mut loader = Loader::new_vec(data);
    if let Some(cancellable) = cancellable {
        loader.cancellable(cancellable.clone());
    }
    let image = loader
        .load()
        .await
        .map_err(|error| format!("Could not load encoded preview: {error}"))?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| format!("Could not decode encoded preview: {error}"))?;
    Ok(frame.texture())
}
