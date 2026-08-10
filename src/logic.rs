use std::path::{Path, PathBuf};

pub const SUPPORTED_SUFFIXES: &[&str] = &["jpg", "jpeg", "png"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    High,
    Balanced,
    Compact,
    Custom,
}

pub fn is_supported_image(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| SUPPORTED_SUFFIXES.contains(&value.to_ascii_lowercase().as_str()))
}

pub fn calculate_dimensions(
    original_width: u32,
    original_height: u32,
    requested_width: u32,
) -> Result<(u32, u32), String> {
    if original_width == 0 || original_height == 0 {
        return Err("Image dimensions must be positive".into());
    }
    if requested_width == 0 {
        return Err("Requested width must be positive".into());
    }

    let target_width = original_width.min(requested_width);
    if target_width == original_width {
        return Ok((original_width, original_height));
    }

    let numerator = u64::from(original_height) * u64::from(target_width);
    let denominator = u64::from(original_width);
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let doubled = remainder * 2;
    let rounded = if doubled < denominator {
        quotient
    } else if doubled > denominator || quotient % 2 == 1 {
        quotient + 1
    } else {
        quotient
    };

    Ok((target_width, u32::try_from(rounded.max(1)).map_err(|_| "Image height overflow")?))
}

pub fn png_compression_for_quality(quality: u8) -> Result<u8, String> {
    if !(1..=100).contains(&quality) {
        return Err("Quality must be between 1 and 100".into());
    }

    const ANCHORS: &[(u8, u8)] = &[(100, 0), (92, 40), (85, 65), (75, 90), (1, 100)];
    for window in ANCHORS.windows(2) {
        let (quality_high, compression_low) = window[0];
        let (quality_low, compression_high) = window[1];
        if quality_low <= quality && quality <= quality_high {
            let span = u16::from(quality_high - quality_low);
            if span == 0 {
                return Ok(compression_low);
            }
            let numerator = u32::from(quality_high - quality)
                * u32::from(compression_high - compression_low);
            let value = u32::from(compression_low)
                + round_ratio_half_even(numerator, u32::from(span));
            return Ok(value.min(100) as u8);
        }
    }

    Ok(100)
}

fn round_ratio_half_even(numerator: u32, denominator: u32) -> u32 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let doubled = remainder * 2;
    if doubled < denominator {
        quotient
    } else if doubled > denominator || quotient % 2 == 1 {
        quotient + 1
    } else {
        quotient
    }
}

pub fn encoding_values(
    preset: QualityPreset,
    suffix: &str,
    custom_quality: Option<u8>,
) -> Result<(Option<u8>, Option<u8>), String> {
    let normalized = suffix.trim_start_matches('.').to_ascii_lowercase();
    let quality = match preset {
        QualityPreset::Custom => custom_quality
            .filter(|value| (1..=100).contains(value))
            .ok_or_else(|| "Custom quality is required and must be between 1 and 100".to_string())?,
        QualityPreset::High => 92,
        QualityPreset::Balanced => 85,
        QualityPreset::Compact => 75,
    };

    match normalized.as_str() {
        "jpg" | "jpeg" => Ok((Some(quality), None)),
        "png" => {
            let compression = match preset {
                QualityPreset::High => 40,
                QualityPreset::Balanced => 65,
                QualityPreset::Compact => 90,
                QualityPreset::Custom => png_compression_for_quality(quality)?,
            };
            Ok((None, Some(compression)))
        }
        _ => Err(format!("Unsupported output format: {suffix}")),
    }
}

pub fn output_mime_type(suffix: &str) -> Result<&'static str, String> {
    match suffix.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "png" => Ok("image/png"),
        _ => Err(format!("Unsupported output format: {suffix}")),
    }
}

pub fn quality_percentage(
    preset: QualityPreset,
    custom_quality: Option<u8>,
) -> Result<u8, String> {
    match preset {
        QualityPreset::High => Ok(92),
        QualityPreset::Balanced => Ok(85),
        QualityPreset::Compact => Ok(75),
        QualityPreset::Custom => custom_quality
            .filter(|value| (1..=100).contains(value))
            .ok_or_else(|| "Custom quality is required and must be between 1 and 100".to_string()),
    }
}

pub fn normalize_filename_suffix(value: &str) -> Result<String, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Ok(String::new());
    }
    if normalized.contains('\0') || normalized.contains('/') || normalized.contains('\\') {
        return Err("Filename suffix cannot contain path separators".into());
    }
    Ok(normalized.to_string())
}

pub fn collision_safe_destination(
    output_dir: &Path,
    source: &Path,
    filename_suffix: &str,
    quality_suffix: Option<u8>,
) -> Result<PathBuf, String> {
    let source_stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Source filename is not valid UTF-8".to_string())?;
    let source_extension = source
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Source file has no extension".to_string())?;

    let mut stem = format!("{}{}", source_stem, normalize_filename_suffix(filename_suffix)?);
    if let Some(value) = quality_suffix {
        if !(1..=100).contains(&value) {
            return Err("Quality suffix must be between 1 and 100".into());
        }
        stem.push_str(&format!("-{value}"));
    }

    let candidate = output_dir.join(format!("{stem}.{source_extension}"));
    if !candidate.exists() {
        return Ok(candidate);
    }

    let mut index = 2u32;
    loop {
        let candidate = output_dir.join(format!("{stem}-{index}.{source_extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        index = index.checked_add(1).ok_or_else(|| "Too many filename collisions".to_string())?;
    }
}


pub fn should_skip_non_smaller_jpeg_export(
    source_suffix: &str,
    original_bytes: u64,
    encoded_bytes: u64,
    original_width: u32,
    original_height: u32,
    output_width: u32,
    output_height: u32,
) -> bool {
    let suffix = source_suffix.trim_start_matches('.').to_ascii_lowercase();
    let is_jpeg = matches!(suffix.as_str(), "jpg" | "jpeg");
    let same_dimensions =
        (original_width, original_height) == (output_width, output_height);

    is_jpeg
        && (encoded_bytes > original_bytes
            || (same_dimensions && encoded_bytes == original_bytes))
}

pub fn human_bytes(value: u64) -> String {
    let mut size = value as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if size < 1024.0 || unit == "GB" {
            if unit == "B" {
                return format!("{} B", size as u64);
            }
            return format!("{size:.1} {unit}");
        }
        size /= 1024.0;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("image-bench-rust-test-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn landscape_resize_preserves_ratio() {
        assert_eq!(calculate_dimensions(6000, 4000, 1920).unwrap(), (1920, 1280));
    }

    #[test]
    fn portrait_resize_preserves_ratio() {
        assert_eq!(calculate_dimensions(3000, 4500, 800).unwrap(), (800, 1200));
    }

    #[test]
    fn never_upscales() {
        assert_eq!(calculate_dimensions(640, 480, 1920).unwrap(), (640, 480));
    }

    #[test]
    fn jpeg_balanced_quality() {
        assert_eq!(encoding_values(QualityPreset::Balanced, ".jpg", None).unwrap(), (Some(85), None));
    }

    #[test]
    fn png_compact_compression() {
        assert_eq!(encoding_values(QualityPreset::Compact, ".png", None).unwrap(), (None, Some(90)));
    }

    #[test]
    fn custom_jpeg_quality() {
        assert_eq!(encoding_values(QualityPreset::Custom, ".jpg", Some(81)).unwrap(), (Some(81), None));
    }

    #[test]
    fn custom_png_preserves_preset_anchor_mapping() {
        assert_eq!(encoding_values(QualityPreset::Custom, ".png", Some(85)).unwrap(), (None, Some(65)));
    }

    #[test]
    fn png_custom_quality_is_lossless_compression_mapping() {
        assert_eq!(png_compression_for_quality(92).unwrap(), 40);
        assert_eq!(png_compression_for_quality(75).unwrap(), 90);
    }

    #[test]
    fn collision_safe_destination_increments() {
        let output = temp_dir();
        fs::write(output.join("photo.jpg"), []).unwrap();
        let result = collision_safe_destination(&output, Path::new("/source/photo.jpg"), "", None).unwrap();
        assert_eq!(result.file_name().unwrap(), "photo-2.jpg");
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn quality_percentage_presets_and_custom() {
        assert_eq!(quality_percentage(QualityPreset::High, None).unwrap(), 92);
        assert_eq!(quality_percentage(QualityPreset::Balanced, None).unwrap(), 85);
        assert_eq!(quality_percentage(QualityPreset::Compact, None).unwrap(), 75);
        assert_eq!(quality_percentage(QualityPreset::Custom, Some(68)).unwrap(), 68);
    }

    #[test]
    fn filename_suffix_and_quality_suffix() {
        let output = temp_dir();
        let result = collision_safe_destination(&output, Path::new("/source/photo.png"), "-optimized", Some(85)).unwrap();
        assert_eq!(result.file_name().unwrap(), "photo-optimized-85.png");
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn suffix_collision_keeps_suffixes_before_counter() {
        let output = temp_dir();
        fs::write(output.join("photo-optimized-85.png"), []).unwrap();
        let result = collision_safe_destination(&output, Path::new("/source/photo.png"), "-optimized", Some(85)).unwrap();
        assert_eq!(result.file_name().unwrap(), "photo-optimized-85-2.png");
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn filename_suffix_rejects_path_separators() {
        assert!(normalize_filename_suffix("../escape").is_err());
    }


    #[test]
    fn jpeg_export_never_allows_output_larger_than_source() {
        assert!(should_skip_non_smaller_jpeg_export(
            ".jpg", 62_243, 89_720, 612, 384, 612, 384
        ));
        assert!(should_skip_non_smaller_jpeg_export(
            ".jpg", 62_243, 89_720, 612, 384, 320, 201
        ));
        assert!(should_skip_non_smaller_jpeg_export(
            "jpeg", 62_243, 62_243, 612, 384, 612, 384
        ));
        assert!(!should_skip_non_smaller_jpeg_export(
            ".jpg", 62_243, 62_243, 612, 384, 320, 201
        ));
        assert!(!should_skip_non_smaller_jpeg_export(
            ".jpg", 62_243, 60_000, 612, 384, 320, 201
        ));
        assert!(!should_skip_non_smaller_jpeg_export(
            ".png", 62_243, 89_720, 612, 384, 612, 384
        ));
    }

    #[test]
    fn human_bytes_formats_kibibyte() {
        assert_eq!(human_bytes(1024), "1.0 KB");
    }
}
