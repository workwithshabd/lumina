use std::io::Cursor;

use image::codecs::avif::AvifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader};

use crate::processor::{
    FitMode, ImageFormat, ImageInfo, ImageProcessor, ProcessError, ProcessOptions, ProcessOutput,
};

#[derive(Debug, Clone)]
pub struct ImageRsProcessor {
    pub avif_speed: u8,
    pub distinct_color_threshold: u32,
    pub max_retry_pixels: u64,
}

impl Default for ImageRsProcessor {
    fn default() -> Self {
        Self {
            avif_speed: 6,
            distinct_color_threshold: 256,
            max_retry_pixels: 16_777_216,
        }
    }
}

impl ImageRsProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_config(config: &crate::config::Config) -> Self {
        Self {
            avif_speed: config.avif_speed,
            distinct_color_threshold: config.distinct_color_threshold,
            max_retry_pixels: config.max_retry_pixels,
        }
    }
}

/// Heuristic to classify content as photographic vs graphic/flat.
///
/// 1. JPEG source format is treated as photographic hint (`true`).
/// 2. For PNG/WebP sources, samples up to 64x64 downscaled pixels and counts distinct RGB colors.
///    If distinct colors > `threshold` (default 256), returns `true` (photo), otherwise `false` (graphic/flat).
///
/// An explicit `format=` query parameter always overrides this heuristic.
pub fn is_photographic(dyn_img: &DynamicImage, src_format: ImageFormat, threshold: u32) -> bool {
    if src_format == ImageFormat::Jpeg {
        return true;
    }

    let sample_w = dyn_img.width().min(64);
    let sample_h = dyn_img.height().min(64);
    if sample_w == 0 || sample_h == 0 {
        return false;
    }

    let sample = dyn_img.resize_exact(sample_w, sample_h, FilterType::Nearest);
    let rgb = sample.to_rgb8();

    let mut distinct_colors = std::collections::HashSet::new();
    for pixel in rgb.pixels() {
        distinct_colors.insert(pixel.0);
        if distinct_colors.len() > threshold as usize {
            return true;
        }
    }

    false
}

impl ImageProcessor for ImageRsProcessor {
    fn probe(&self, input: &[u8]) -> Result<ImageInfo, ProcessError> {
        if input.is_empty() {
            return Err(ProcessError::DecodeFailed(
                "Cannot probe empty byte buffer".to_string(),
            ));
        }

        let reader = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .map_err(|e| ProcessError::DecodeFailed(format!("Failed to guess format: {e}")))?;

        let raw_format = reader.format().ok_or_else(|| {
            ProcessError::UnsupportedFormat("Could not identify image format".to_string())
        })?;

        let format = map_from_image_format(raw_format)?;

        let decoder = reader.into_decoder().map_err(|e| {
            ProcessError::DecodeFailed(format!("Failed to initialize decoder: {e}"))
        })?;

        let (width, height) = decoder.dimensions();
        let has_alpha = decoder.color_type().has_alpha();

        Ok(ImageInfo {
            width,
            height,
            format,
            has_alpha,
        })
    }

    fn process(&self, input: &[u8], opts: ProcessOptions) -> Result<ProcessOutput, ProcessError> {
        if input.is_empty() {
            return Err(ProcessError::DecodeFailed(
                "Cannot process empty byte buffer".to_string(),
            ));
        }

        // 1. Probe input to determine source format
        let info = self.probe(input)?;

        // Ensure source format is supported for decoding (JPEG, PNG, WebP)
        match info.format {
            ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP => {}
            other => {
                return Err(ProcessError::UnsupportedFormat(format!(
                    "Decoding '{other}' is not supported"
                )));
            }
        }

        // 2. Decode image
        let decode_start = std::time::Instant::now();
        let reader = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .map_err(|e| ProcessError::DecodeFailed(format!("Failed to read image: {e}")))?;

        let mut decoder = reader.into_decoder().map_err(|e| {
            ProcessError::DecodeFailed(format!("Failed to read image decoder: {e}"))
        })?;
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut dyn_img = DynamicImage::from_decoder(decoder)
            .map_err(|e| ProcessError::DecodeFailed(format!("Failed to decode image: {e}")))?;
        dyn_img.apply_orientation(orientation);
        crate::observability::record_decode_duration(decode_start.elapsed().as_secs_f64());

        let orig_w = dyn_img.width();
        let orig_h = dyn_img.height();

        // 3. Compute target dimensions and apply resize
        let transform_start = std::time::Instant::now();
        let (target_w, target_h) =
            calculate_dimensions(orig_w, orig_h, opts.target_w, opts.target_h);

        if (target_w, target_h) != (orig_w, orig_h) {
            dyn_img = match opts.fit {
                FitMode::Cover => dyn_img.resize_to_fill(target_w, target_h, FilterType::Lanczos3),
                FitMode::Contain | FitMode::Inside => {
                    dyn_img.resize(target_w, target_h, FilterType::Lanczos3)
                }
                FitMode::Fill => dyn_img.resize_exact(target_w, target_h, FilterType::Lanczos3),
                FitMode::Outside => {
                    // Scale such that both dimensions are at least target dimensions
                    let scale_w = target_w as f64 / orig_w as f64;
                    let scale_h = target_h as f64 / orig_h as f64;
                    let scale = scale_w.max(scale_h);
                    let new_w = ((orig_w as f64 * scale).round() as u32).max(1);
                    let new_h = ((orig_h as f64 * scale).round() as u32).max(1);
                    dyn_img.resize_exact(new_w, new_h, FilterType::Lanczos3)
                }
            };
        }
        crate::observability::record_transform_duration(transform_start.elapsed().as_secs_f64());

        let out_w = dyn_img.width();
        let out_h = dyn_img.height();
        let can_retry = ((out_w as u64) * (out_h as u64)) <= self.max_retry_pixels;

        // 4. Encode to target format with Stage 1 (Option A) format policy & larger-than-source guard
        let encode_start = std::time::Instant::now();

        struct Candidate {
            bytes: Vec<u8>,
            format: ImageFormat,
        }

        let chosen: Candidate = match opts.format {
            Some(explicit_fmt) => {
                let quality = opts.quality.unwrap_or_else(|| {
                    crate::processor::resolve_quality(explicit_fmt, opts.quality_target)
                });
                let bytes = encode_image(&dyn_img, explicit_fmt, quality, self.avif_speed)?;
                Candidate {
                    bytes,
                    format: explicit_fmt,
                }
            }
            None => {
                let accept_class =
                    crate::http::params::resolve_accept_class(opts.accept_header.as_deref());
                let client_accept_avif =
                    matches!(accept_class, crate::http::params::AcceptClass::Avif);
                let client_accept_webp = matches!(
                    accept_class,
                    crate::http::params::AcceptClass::Avif | crate::http::params::AcceptClass::Webp
                );

                let is_photo =
                    is_photographic(&dyn_img, info.format, self.distinct_color_threshold);

                if info.has_alpha || !is_photo {
                    // Source has alpha or graphic/flat content:
                    // Smaller of lossless WebP (if allowed) and PNG.
                    let png_bytes = encode_image(&dyn_img, ImageFormat::Png, 100, self.avif_speed)?;
                    if client_accept_webp {
                        let webp_bytes =
                            encode_image(&dyn_img, ImageFormat::WebP, 100, self.avif_speed)?;
                        if webp_bytes.len() < png_bytes.len() {
                            Candidate {
                                bytes: webp_bytes,
                                format: ImageFormat::WebP,
                            }
                        } else {
                            Candidate {
                                bytes: png_bytes,
                                format: ImageFormat::Png,
                            }
                        }
                    } else {
                        Candidate {
                            bytes: png_bytes,
                            format: ImageFormat::Png,
                        }
                    }
                } else {
                    // Photographic content:
                    // AVIF if Accept allows it, else JPEG. Never lossless WebP for photos!
                    if client_accept_avif {
                        let avif_q = opts.quality.unwrap_or_else(|| {
                            crate::processor::resolve_quality(
                                ImageFormat::Avif,
                                opts.quality_target,
                            )
                        });
                        let primary_bytes =
                            encode_image(&dyn_img, ImageFormat::Avif, avif_q, self.avif_speed)?;
                        if primary_bytes.len() <= input.len() || !can_retry {
                            Candidate {
                                bytes: primary_bytes,
                                format: ImageFormat::Avif,
                            }
                        } else {
                            // Larger than source: try up to 2 candidates (lower quality step or alternate format)
                            let mut candidates = vec![Candidate {
                                bytes: primary_bytes,
                                format: ImageFormat::Avif,
                            }];
                            if let Ok(b) =
                                encode_image(&dyn_img, ImageFormat::Avif, 50, self.avif_speed)
                            {
                                candidates.push(Candidate {
                                    bytes: b,
                                    format: ImageFormat::Avif,
                                });
                            }
                            if let Ok(b) =
                                encode_image(&dyn_img, ImageFormat::Jpeg, 65, self.avif_speed)
                            {
                                candidates.push(Candidate {
                                    bytes: b,
                                    format: ImageFormat::Jpeg,
                                });
                            }
                            candidates.sort_by_key(|c| c.bytes.len());
                            candidates.into_iter().next().unwrap()
                        }
                    } else {
                        let jpeg_q = opts.quality.unwrap_or_else(|| {
                            crate::processor::resolve_quality(
                                ImageFormat::Jpeg,
                                opts.quality_target,
                            )
                        });
                        let primary_bytes =
                            encode_image(&dyn_img, ImageFormat::Jpeg, jpeg_q, self.avif_speed)?;
                        if primary_bytes.len() <= input.len() || !can_retry {
                            Candidate {
                                bytes: primary_bytes,
                                format: ImageFormat::Jpeg,
                            }
                        } else {
                            // Larger than source: try up to 2 candidates (lower quality steps)
                            let mut candidates = vec![Candidate {
                                bytes: primary_bytes,
                                format: ImageFormat::Jpeg,
                            }];
                            let lower_q = if jpeg_q > 65 { 65 } else { 50 };
                            if let Ok(b) =
                                encode_image(&dyn_img, ImageFormat::Jpeg, lower_q, self.avif_speed)
                            {
                                candidates.push(Candidate {
                                    bytes: b,
                                    format: ImageFormat::Jpeg,
                                });
                            }
                            let lowest_q = if lower_q > 50 { 50 } else { 40 };
                            if let Ok(b) =
                                encode_image(&dyn_img, ImageFormat::Jpeg, lowest_q, self.avif_speed)
                            {
                                candidates.push(Candidate {
                                    bytes: b,
                                    format: ImageFormat::Jpeg,
                                });
                            }
                            candidates.sort_by_key(|c| c.bytes.len());
                            candidates.into_iter().next().unwrap()
                        }
                    }
                }
            }
        };

        if chosen.bytes.len() > input.len() {
            crate::observability::record_output_larger_than_source();
        }

        crate::observability::record_encode_duration(encode_start.elapsed().as_secs_f64());

        Ok(ProcessOutput {
            bytes: chosen.bytes,
            format: chosen.format,
            width: out_w,
            height: out_h,
        })
    }
}

fn map_from_image_format(fmt: image::ImageFormat) -> Result<ImageFormat, ProcessError> {
    match fmt {
        image::ImageFormat::Jpeg => Ok(ImageFormat::Jpeg),
        image::ImageFormat::Png => Ok(ImageFormat::Png),
        image::ImageFormat::WebP => Ok(ImageFormat::WebP),
        image::ImageFormat::Avif => Ok(ImageFormat::Avif),
        image::ImageFormat::Gif => Ok(ImageFormat::Gif),
        other => Err(ProcessError::UnsupportedFormat(format!("{other:?}"))),
    }
}

fn calculate_dimensions(
    orig_w: u32,
    orig_h: u32,
    req_w: Option<u32>,
    req_h: Option<u32>,
) -> (u32, u32) {
    match (req_w, req_h) {
        (None, None) => (orig_w, orig_h),
        (Some(w), None) => {
            let scale = w as f64 / orig_w as f64;
            let h = ((orig_h as f64 * scale).round() as u32).max(1);
            (w, h)
        }
        (None, Some(h)) => {
            let scale = h as f64 / orig_h as f64;
            let w = ((orig_w as f64 * scale).round() as u32).max(1);
            (w, h)
        }
        (Some(w), Some(h)) => (w, h),
    }
}

fn encode_image(
    img: &DynamicImage,
    format: ImageFormat,
    quality: u8,
    avif_speed: u8,
) -> Result<Vec<u8>, ProcessError> {
    let mut out = Vec::new();

    match format {
        ImageFormat::Jpeg => {
            // JPEG cannot represent alpha; convert to RGB8
            let rgb_img = img.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
            encoder
                .encode(
                    rgb_img.as_raw(),
                    rgb_img.width(),
                    rgb_img.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| ProcessError::ProcessingFailed(format!("JPEG encode error: {e}")))?;
        }
        ImageFormat::Png => {
            let encoder = PngEncoder::new(&mut out);
            let (raw_bytes, color_type) = color_and_bytes(img);
            encoder
                .write_image(&raw_bytes, img.width(), img.height(), color_type)
                .map_err(|e| ProcessError::ProcessingFailed(format!("PNG encode error: {e}")))?;
        }
        ImageFormat::Avif => {
            let encoder = AvifEncoder::new_with_speed_quality(&mut out, avif_speed, quality);
            let (raw_bytes, color_type) = color_and_bytes(img);
            encoder
                .write_image(&raw_bytes, img.width(), img.height(), color_type)
                .map_err(|e| ProcessError::ProcessingFailed(format!("AVIF encode error: {e}")))?;
        }
        ImageFormat::WebP => {
            let encoder = WebPEncoder::new_lossless(&mut out);
            let (raw_bytes, color_type) = color_and_bytes(img);
            encoder
                .write_image(&raw_bytes, img.width(), img.height(), color_type)
                .map_err(|e| ProcessError::ProcessingFailed(format!("WebP encode error: {e}")))?;
        }
        ImageFormat::Gif | ImageFormat::Svg => {
            return Err(ProcessError::UnsupportedFormat(format!(
                "Encoding to '{format}' is not supported"
            )));
        }
    }

    Ok(out)
}

fn color_and_bytes(img: &DynamicImage) -> (Vec<u8>, image::ExtendedColorType) {
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        (rgba.into_raw(), image::ExtendedColorType::Rgba8)
    } else {
        let rgb = img.to_rgb8();
        (rgb.into_raw(), image::ExtendedColorType::Rgb8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn make_test_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(w, h);
        for x in 0..w {
            for y in 0..h {
                img.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
            }
        }
        let mut buf = Vec::new();
        let encoder = PngEncoder::new(&mut buf);
        encoder
            .write_image(&img.into_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    #[test]
    fn test_image_rs_probe() {
        let processor = ImageRsProcessor::new();
        let png_bytes = make_test_png(50, 30);

        let info = processor.probe(&png_bytes).unwrap();
        assert_eq!(info.width, 50);
        assert_eq!(info.height, 30);
        assert_eq!(info.format, ImageFormat::Png);
        assert!(info.has_alpha);
    }

    #[test]
    fn test_image_rs_resize_fit_cover() {
        let processor = ImageRsProcessor::new();
        let png_bytes = make_test_png(100, 50);

        let opts = ProcessOptions {
            target_w: Some(40),
            target_h: Some(40),
            fit: FitMode::Cover,
            format: Some(ImageFormat::Png),
            quality: Some(90),
            ..Default::default()
        };

        let out = processor.process(&png_bytes, opts).unwrap();
        assert_eq!(out.width, 40);
        assert_eq!(out.height, 40);
        assert_eq!(out.format, ImageFormat::Png);

        // Verify output is valid image
        let probed = processor.probe(&out.bytes).unwrap();
        assert_eq!(probed.width, 40);
        assert_eq!(probed.height, 40);
    }

    #[test]
    fn test_image_rs_resize_fit_contain() {
        let processor = ImageRsProcessor::new();
        let png_bytes = make_test_png(100, 50);

        let opts = ProcessOptions {
            target_w: Some(40),
            target_h: Some(40),
            fit: FitMode::Contain,
            format: Some(ImageFormat::Jpeg),
            quality: Some(85),
            ..Default::default()
        };

        let out = processor.process(&png_bytes, opts).unwrap();
        // 100x50 contained in 40x40 -> 40x20
        assert_eq!(out.width, 40);
        assert_eq!(out.height, 20);
        assert_eq!(out.format, ImageFormat::Jpeg);
    }

    #[test]
    fn test_image_rs_resize_fit_fill() {
        let processor = ImageRsProcessor::new();
        let png_bytes = make_test_png(100, 50);

        let opts = ProcessOptions {
            target_w: Some(30),
            target_h: Some(60),
            fit: FitMode::Fill,
            format: Some(ImageFormat::WebP),
            quality: Some(80),
            ..Default::default()
        };

        let out = processor.process(&png_bytes, opts).unwrap();
        assert_eq!(out.width, 30);
        assert_eq!(out.height, 60);
        assert_eq!(out.format, ImageFormat::WebP);
    }

    #[test]
    fn test_image_rs_encode_all_target_formats() {
        let processor = ImageRsProcessor::new();
        let png_bytes = make_test_png(20, 20);

        for target_fmt in [
            ImageFormat::Jpeg,
            ImageFormat::Png,
            ImageFormat::WebP,
            ImageFormat::Avif,
        ] {
            let opts = ProcessOptions {
                target_w: Some(10),
                target_h: Some(10),
                fit: FitMode::Fill,
                format: Some(target_fmt),
                quality: Some(75),
                ..Default::default()
            };

            let out = processor.process(&png_bytes, opts).unwrap();
            assert_eq!(out.format, target_fmt);
            assert_eq!(out.width, 10);
            assert_eq!(out.height, 10);
            assert!(!out.bytes.is_empty());
        }
    }
}
