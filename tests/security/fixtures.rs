use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

/// Generates a valid small PNG image of given dimensions.
pub fn make_valid_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for x in 0..w {
        for y in 0..h {
            img.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
        }
    }
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder
        .write_image(&img.into_raw(), w, h, ExtendedColorType::Rgba8)
        .expect("PNG encoding failed");
    buf
}

/// Generates a valid small JPEG image of given dimensions.
pub fn make_valid_jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for x in 0..w {
        for y in 0..h {
            img.put_pixel(x, y, image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
        }
    }
    let mut buf = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 80);
    encoder
        .encode(&img.into_raw(), w, h, ExtendedColorType::Rgb8)
        .expect("JPEG encoding failed");
    buf
}

/// Generates a valid small WebP image of given dimensions.
pub fn make_valid_webp(w: u32, h: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for x in 0..w {
        for y in 0..h {
            img.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
        }
    }
    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut buf);
    encoder
        .write_image(&img.into_raw(), w, h, ExtendedColorType::Rgba8)
        .expect("WebP encoding failed");
    buf
}

/// Creates a PNG decompression bomb claiming huge dimensions in the IHDR header.
pub fn make_png_decompression_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR chunk length = 13
        0x49, 0x48, 0x44, 0x52, // "IHDR"
    ];
    data.extend_from_slice(&w.to_be_bytes()); // Width
    data.extend_from_slice(&h.to_be_bytes()); // Height
    data.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, deflate, no filter, no interlace
    let crc = crc32fast::hash(&data[12..29]);
    data.extend_from_slice(&crc.to_be_bytes());
    // Minimal empty IDAT and IEND
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
        0xAE, 0x42, 0x60, 0x82,
    ]);
    data
}

/// Creates a JPEG decompression bomb claiming huge dimensions in the SOF0 marker.
pub fn make_jpeg_decompression_bomb(w: u16, h: u16) -> Vec<u8> {
    let mut data = vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xE0, // APP0
        0x00, 0x10, // length = 16
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
        0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xC0, // SOF0
        0x00, 0x11, // length = 17
        0x08, // precision = 8
    ];
    data.extend_from_slice(&h.to_be_bytes()); // Height
    data.extend_from_slice(&w.to_be_bytes()); // Width
    data.extend_from_slice(&[
        0x03, // 3 components (YCbCr)
        0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xD9, // EOI
    ]);
    data
}

/// Creates a WebP decompression bomb with VP8X canvas dimensions.
pub fn make_webp_decompression_bomb(w: u32, h: u32) -> Vec<u8> {
    let canvas_w = w.saturating_sub(1);
    let canvas_h = h.saturating_sub(1);
    let w_bytes = canvas_w.to_le_bytes();
    let h_bytes = canvas_h.to_le_bytes();

    let data = vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0x20, 0x00, 0x00, 0x00, // File size - 8
        0x57, 0x45, 0x42, 0x50, // "WEBP"
        0x56, 0x50, 0x38, 0x58, // "VP8X"
        0x0A, 0x00, 0x00, 0x00, // Chunk size = 10
        0x00, 0x00, 0x00, 0x00, // Flags
        w_bytes[0], w_bytes[1], w_bytes[2], // 24-bit width - 1
        h_bytes[0], h_bytes[1], h_bytes[2], // 24-bit height - 1
    ];
    data
}

/// Malformed PNG with invalid chunk CRC and truncated stream.
pub fn make_malformed_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // Valid signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x00, 0x10, // width 16
        0x00, 0x00, 0x00, 0x10, // height 16
        0x08, 0x02, 0x00, 0x00, 0x00, // RGB
        0xDE, 0xAD, 0xBE, 0xEF, // Corrupt CRC
        0x00, 0x00, 0x00, 0x05, // Truncated chunk
    ]
}

/// Malformed JPEG with corrupted frame markers.
pub fn make_malformed_jpeg() -> Vec<u8> {
    vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xC0, // SOF0
        0x00, 0x05, // Invalid length
        0x08, 0x00, 0x01, // Truncated header
    ]
}

/// Malformed WebP with truncated RIFF container.
pub fn make_malformed_webp() -> Vec<u8> {
    vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0xFF, 0xFF, 0x00, 0x00, // Stated size
        0x57, 0x45, 0x42, 0x50, // "WEBP"
        0x56, 0x50, 0x38, 0x20, // "VP8 "
        0x10, 0x00, 0x00, 0x00, // Chunk size 16
              // Missing payload
    ]
}

/// Malformed AVIF with truncated box structure.
pub fn make_malformed_avif() -> Vec<u8> {
    vec![
        0x00, 0x00, 0x00, 0x20, // Box length 32
        0x66, 0x74, 0x79, 0x70, // "ftyp"
        0x61, 0x76, 0x69, 0x66, // "avif"
        0x00, 0x00, 0x00, 0x00, // Minor version
        0x61, 0x76, 0x69, 0x66, // Compatible brands
        0x6D, 0x69, 0x66, 0x31, // Corrupt box follows
        0x00, 0x00, 0x00, 0x10, 0x6D, 0x65, 0x74, 0x61, // "meta"
        0xFF, 0xFF, // Truncated
    ]
}
