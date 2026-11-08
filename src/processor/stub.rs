use super::{ImageFormat, ImageInfo, ImageProcessor, ProcessError, ProcessOptions, ProcessOutput};

/// 67-byte valid 1x1 transparent RGBA PNG
pub const STUB_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
    0x00, 0x00, 0x00, 0x0D, // IHDR chunk length (13)
    0x49, 0x48, 0x44, 0x52, // "IHDR"
    0x00, 0x00, 0x00, 0x01, // width: 1
    0x00, 0x00, 0x00, 0x01, // height: 1
    0x08, 0x06, 0x00, 0x00, 0x00, // 8-bit depth, RGBA, deflate, default filter, no interlace
    0x1F, 0x15, 0xC4, 0x89, // IHDR CRC
    0x00, 0x00, 0x00, 0x0A, // IDAT chunk length (10)
    0x49, 0x44, 0x41, 0x54, // "IDAT"
    0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // zlib payload
    0x0D, 0x0A, 0x2D, 0xB4, // IDAT CRC
    0x00, 0x00, 0x00, 0x00, // IEND chunk length (0)
    0x49, 0x45, 0x4E, 0x44, // "IEND"
    0xAE, 0x42, 0x60, 0x82, // IEND CRC
];

#[derive(Debug, Default, Clone)]
pub struct StubImageProcessor;

impl StubImageProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ImageProcessor for StubImageProcessor {
    fn probe(&self, input: &[u8]) -> Result<ImageInfo, ProcessError> {
        if input.is_empty() {
            return Err(ProcessError::DecodeFailed(
                "Cannot probe empty byte buffer".to_string(),
            ));
        }

        Ok(ImageInfo {
            width: 1,
            height: 1,
            format: ImageFormat::Png,
            has_alpha: true,
        })
    }

    fn process(&self, input: &[u8], opts: ProcessOptions) -> Result<ProcessOutput, ProcessError> {
        if input.is_empty() {
            return Err(ProcessError::DecodeFailed(
                "Cannot process empty byte buffer".to_string(),
            ));
        }

        let width = opts.target_w.unwrap_or(1);
        let height = opts.target_h.unwrap_or(1);

        Ok(ProcessOutput {
            bytes: STUB_1X1_PNG.to_vec(),
            format: opts.format.unwrap_or(ImageFormat::Png),
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_processor_probe() {
        let processor = StubImageProcessor::new();
        let err = processor.probe(&[]).unwrap_err();
        assert_eq!(
            err,
            ProcessError::DecodeFailed("Cannot probe empty byte buffer".to_string())
        );

        let info = processor.probe(b"fake-bytes").unwrap();
        assert_eq!(info.width, 1);
        assert_eq!(info.height, 1);
        assert_eq!(info.format, ImageFormat::Png);
        assert!(info.has_alpha);
    }

    #[test]
    fn test_stub_processor_process() {
        let processor = StubImageProcessor::new();
        let opts = ProcessOptions {
            target_w: Some(100),
            target_h: Some(200),
            ..Default::default()
        };
        let out = processor.process(b"fake-bytes", opts).unwrap();
        assert_eq!(out.width, 100);
        assert_eq!(out.height, 200);
        assert_eq!(out.format, ImageFormat::Png);
        assert_eq!(out.bytes, STUB_1X1_PNG);
    }
}
