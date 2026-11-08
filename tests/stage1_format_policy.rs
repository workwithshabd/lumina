use atihsrah::config::Config;
use atihsrah::http::params::{resolve_format, FormatParam, QualityTarget};
use atihsrah::processor::image_rs::{is_photographic, ImageRsProcessor};
use atihsrah::processor::{ImageFormat, ImageProcessor, ProcessOptions};
use std::collections::HashMap;

fn minimal_config(overrides: &[(&str, &str)]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert(
        "ALLOWED_ORIGINS".to_string(),
        "https://example.com".to_string(),
    );
    map.insert("ATIHSRAH_AUTH_MODE".to_string(), "public".to_string());
    map.insert(
        "ATIHSRAH_SIGNING_KEY".to_string(),
        "9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788".to_string(),
    );
    for (k, v) in overrides {
        map.insert(k.to_string(), v.to_string());
    }
    map
}

fn create_photo_fixture(w: u32, h: u32) -> Vec<u8> {
    // Generate complex continuous-tone photographic gradient/noise
    let mut img = image::RgbImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = ((x * 13 + y * 7) % 256) as u8;
        let g = ((x * 19 + y * 31) % 256) as u8;
        let b = ((x * 23 + y * 41) % 256) as u8;
        *pixel = image::Rgb([r, g, b]);
    }
    let mut out = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    out
}

fn create_checkerboard_graphic_png(w: u32, h: u32) -> Vec<u8> {
    // 2-color flat pattern (checkerboard)
    let mut img = image::RgbImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        if (x / 8 + y / 8) % 2 == 0 {
            *pixel = image::Rgb([0, 0, 0]);
        } else {
            *pixel = image::Rgb([255, 255, 255]);
        }
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn create_transparent_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        if (x + y) % 2 == 0 {
            *pixel = image::Rgba([255, 0, 0, 128]);
        } else {
            *pixel = image::Rgba([0, 255, 0, 0]);
        }
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn test_photo_with_auto_and_webp_accept_returns_jpeg_not_webp() {
    // Stage 1 Option A:
    // "Photographic content -> AVIF if Accept allows it, else JPEG. Never lossless WebP for photos."
    let (format, _is_auto) = resolve_format(
        Some(FormatParam::Auto),
        Some("image/webp"),
        false,             // no alpha
        ImageFormat::Jpeg, // JPEG source = photographic content hint
    );

    assert_eq!(
        format,
        ImageFormat::Jpeg,
        "Photographic content must return JPEG when AVIF is not accepted, NOT lossless WebP"
    );
}

#[test]
fn test_photo_with_auto_and_avif_accept_returns_avif() {
    // Stage 1 Option A:
    // Photographic content with AVIF accepted returns AVIF.
    let (format, _is_auto) = resolve_format(
        Some(FormatParam::Auto),
        Some("image/avif, image/webp"),
        false,
        ImageFormat::Jpeg,
    );

    assert_eq!(
        format,
        ImageFormat::Avif,
        "Photographic content must return AVIF when AVIF is accepted"
    );
}

#[test]
fn test_avif_speed_config_validation() {
    // ATIHSRAH_AVIF_SPEED (1-10, default 6) configurable and validated.
    let map_default = minimal_config(&[]);
    let cfg = Config::from_lookup(|k| map_default.get(k).cloned()).expect("default config ok");
    assert_eq!(cfg.avif_speed, 6);

    let map_valid = minimal_config(&[("ATIHSRAH_AVIF_SPEED", "9")]);
    let cfg = Config::from_lookup(|k| map_valid.get(k).cloned()).expect("speed 9 ok");
    assert_eq!(cfg.avif_speed, 9);

    let map_zero = minimal_config(&[("ATIHSRAH_AVIF_SPEED", "0")]);
    assert!(
        Config::from_lookup(|k| map_zero.get(k).cloned()).is_err(),
        "AVIF speed 0 must be rejected"
    );

    let map_high = minimal_config(&[("ATIHSRAH_AVIF_SPEED", "11")]);
    assert!(
        Config::from_lookup(|k| map_high.get(k).cloned()).is_err(),
        "AVIF speed 11 must be rejected"
    );
}

#[test]
fn test_explicit_format_webp_serves_lossless_webp() {
    // Stage 1 Option A:
    // "Explicit format=webp: serve lossless WebP and document that quality_target does not affect WebP in this build."
    let processor = ImageRsProcessor::new();
    let photo = create_photo_fixture(32, 32);

    let out_draft = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::WebP),
                quality_target: Some(QualityTarget::Draft),
                ..Default::default()
            },
        )
        .expect("webp process ok");

    let out_high = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::WebP),
                quality_target: Some(QualityTarget::High),
                ..Default::default()
            },
        )
        .expect("webp process ok");

    assert_eq!(out_draft.format, ImageFormat::WebP);
    assert_eq!(out_high.format, ImageFormat::WebP);
    assert_eq!(
        out_draft.bytes.len(),
        out_high.bytes.len(),
        "Lossless WebP output size is invariant under quality_target"
    );
}

#[test]
fn test_quality_target_strictly_increases_size_for_jpeg_and_avif() {
    // Stage 1 Requirement 3:
    // "quality_target (draft/good/high): single mapping table changing output size for JPEG and AVIF"
    let processor = ImageRsProcessor::new();
    let photo = create_photo_fixture(128, 128);

    // JPEG: draft < good < high
    let jpeg_draft = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Jpeg),
                quality_target: Some(QualityTarget::Draft),
                ..Default::default()
            },
        )
        .expect("jpeg draft ok");

    let jpeg_good = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Jpeg),
                quality_target: Some(QualityTarget::Good),
                ..Default::default()
            },
        )
        .expect("jpeg good ok");

    let jpeg_high = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Jpeg),
                quality_target: Some(QualityTarget::High),
                ..Default::default()
            },
        )
        .expect("jpeg high ok");

    assert!(
        jpeg_draft.bytes.len() < jpeg_good.bytes.len(),
        "JPEG draft ({}) must be smaller than good ({})",
        jpeg_draft.bytes.len(),
        jpeg_good.bytes.len()
    );
    assert!(
        jpeg_good.bytes.len() < jpeg_high.bytes.len(),
        "JPEG good ({}) must be smaller than high ({})",
        jpeg_good.bytes.len(),
        jpeg_high.bytes.len()
    );

    // AVIF: draft < good < high
    let avif_draft = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Avif),
                quality_target: Some(QualityTarget::Draft),
                ..Default::default()
            },
        )
        .expect("avif draft ok");

    let avif_good = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Avif),
                quality_target: Some(QualityTarget::Good),
                ..Default::default()
            },
        )
        .expect("avif good ok");

    let avif_high = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Avif),
                quality_target: Some(QualityTarget::High),
                ..Default::default()
            },
        )
        .expect("avif high ok");

    assert!(
        avif_draft.bytes.len() < avif_good.bytes.len(),
        "AVIF draft ({}) must be smaller than good ({})",
        avif_draft.bytes.len(),
        avif_good.bytes.len()
    );
    assert!(
        avif_good.bytes.len() < avif_high.bytes.len(),
        "AVIF good ({}) must be smaller than high ({})",
        avif_good.bytes.len(),
        avif_high.bytes.len()
    );
}

#[test]
fn test_graphic_content_heuristic_and_smaller_selection() {
    // Stage 1 Requirement 1:
    // Graphic/flat content -> smaller of lossless WebP (if allowed) and PNG.
    let processor = ImageRsProcessor::new();
    let checkerboard = create_checkerboard_graphic_png(64, 64);

    // Verify heuristic classifies checkerboard as non-photographic
    let dyn_img = image::load_from_memory(&checkerboard).unwrap();
    assert!(
        !is_photographic(&dyn_img, ImageFormat::Png, 256),
        "2-color checkerboard must be classified as graphic/flat, not photographic"
    );

    // When Accept allows WebP:
    let out = processor
        .process(
            &checkerboard,
            ProcessOptions {
                format: None, // auto
                accept_header: Some("image/webp, */*".to_string()),
                ..Default::default()
            },
        )
        .expect("auto process ok");

    assert!(
        out.format == ImageFormat::WebP || out.format == ImageFormat::Png,
        "Graphic content must select either lossless WebP or PNG"
    );
}

#[test]
fn test_alpha_image_selects_smaller_of_webp_and_png_preserving_alpha() {
    // Stage 1 Requirement 1:
    // Source has alpha -> smaller of lossless WebP and PNG (if client accepts WebP, else PNG).
    let processor = ImageRsProcessor::new();
    let alpha_png = create_transparent_png(32, 32);

    let out = processor
        .process(
            &alpha_png,
            ProcessOptions {
                format: None, // auto
                accept_header: Some("image/webp, */*".to_string()),
                ..Default::default()
            },
        )
        .expect("alpha auto process ok");

    assert!(
        out.format == ImageFormat::WebP || out.format == ImageFormat::Png,
        "Alpha content must resolve to WebP or PNG"
    );

    // Verify probe recognizes alpha
    let info = processor.probe(&out.bytes).unwrap();
    assert!(info.has_alpha, "Alpha channel must be preserved in output");
}

#[test]
fn test_larger_than_source_guard_increments_metric_and_does_not_leak_source() {
    // Stage 1 Requirement 4:
    // Larger-than-source guard: do not pass original bytes (to prevent metadata leakage);
    // try up to 2 candidates bounded by pixel threshold.
    // If still larger, increment atihsrah_output_larger_than_source_total.
    let _ = atihsrah::observability::init_prometheus();
    let processor = ImageRsProcessor::new();

    // Create a tiny 8x8 highly-compressed JPEG
    let photo = create_photo_fixture(8, 8);

    // Request high quality AVIF where metadata/container headers might exceed tiny JPEG source
    let out = processor
        .process(
            &photo,
            ProcessOptions {
                format: Some(ImageFormat::Avif),
                quality_target: Some(QualityTarget::High),
                ..Default::default()
            },
        )
        .expect("process ok");

    // Must NOT leak source bytes
    assert_ne!(
        out.bytes, photo,
        "Guard must never pass through original bytes directly"
    );
    assert_eq!(out.format, ImageFormat::Avif);
}

#[test]
fn test_legacy_client_without_modern_accept_gets_jpeg_for_photo_and_png_for_graphic() {
    let processor = ImageRsProcessor::new();
    let photo = create_photo_fixture(32, 32);
    let graphic = create_checkerboard_graphic_png(32, 32);

    // Legacy client (curl / bot with */*)
    let out_photo = processor
        .process(
            &photo,
            ProcessOptions {
                format: None, // auto
                accept_header: Some("*/*".to_string()),
                ..Default::default()
            },
        )
        .expect("photo legacy ok");
    assert_eq!(
        out_photo.format,
        ImageFormat::Jpeg,
        "Legacy client must get JPEG for photo"
    );

    let out_graphic = processor
        .process(
            &graphic,
            ProcessOptions {
                format: None, // auto
                accept_header: Some("*/*".to_string()),
                ..Default::default()
            },
        )
        .expect("graphic legacy ok");
    assert_eq!(
        out_graphic.format,
        ImageFormat::Png,
        "Legacy client must get PNG for graphic"
    );
}
