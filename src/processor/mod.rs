pub mod image_rs;
pub mod stub;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Avif,
    Gif,
    Svg,
}

impl ImageFormat {
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Avif => "image/avif",
            Self::Gif => "image/gif",
            Self::Svg => "image/svg+xml",
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Avif => "avif",
            Self::Gif => "gif",
            Self::Svg => "svg",
        }
    }
}

impl fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file_extension())
    }
}

impl FromStr for ImageFormat {
    type Err = ProcessError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "png" => Ok(Self::Png),
            "webp" => Ok(Self::WebP),
            "avif" => Ok(Self::Avif),
            "gif" => Ok(Self::Gif),
            "svg" | "svg+xml" => Ok(Self::Svg),
            other => Err(ProcessError::UnsupportedFormat(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    #[default]
    Cover,
    Contain,
    Fill,
    Inside,
    Outside,
}

impl FromStr for FitMode {
    type Err = ProcessError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "cover" => Ok(Self::Cover),
            "contain" => Ok(Self::Contain),
            "fill" => Ok(Self::Fill),
            "inside" => Ok(Self::Inside),
            "outside" => Ok(Self::Outside),
            other => Err(ProcessError::ProcessingFailed(format!(
                "Invalid fit mode '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityTarget {
    Draft,
    Good,
    High,
    Lossless,
}

impl FromStr for QualityTarget {
    type Err = ProcessError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "good" => Ok(Self::Good),
            "high" => Ok(Self::High),
            "lossless" => Ok(Self::Lossless),
            other => Err(ProcessError::ProcessingFailed(format!(
                "Invalid quality target '{other}': expected draft, good, high, or lossless"
            ))),
        }
    }
}

/// Quality mapping table: maps abstract targets to concrete 1-100 codec values.
///
/// Single mapping table changing output size for lossy formats (JPEG and AVIF).
/// Note: Under Option A (pure Rust), WebP encoding is lossless-only via image-rs,
/// so quality_target does not affect WebP in this build.
/// Values are provisional until empirically benchmarked with SSIM/VMAF.
pub fn resolve_quality(format: ImageFormat, target: Option<QualityTarget>) -> u8 {
    let q_target = target.unwrap_or(QualityTarget::Good);
    match (format, q_target) {
        (ImageFormat::Jpeg, QualityTarget::Draft) => 65,
        (ImageFormat::Jpeg, QualityTarget::Good) => 80,
        (ImageFormat::Jpeg, QualityTarget::High) => 90,
        (ImageFormat::Jpeg, QualityTarget::Lossless) => 100,

        (ImageFormat::Avif, QualityTarget::Draft) => 50,
        (ImageFormat::Avif, QualityTarget::Good) => 65,
        (ImageFormat::Avif, QualityTarget::High) => 80,
        (ImageFormat::Avif, QualityTarget::Lossless) => 100,

        (ImageFormat::WebP, QualityTarget::Draft) => 60,
        (ImageFormat::WebP, QualityTarget::Good) => 75,
        (ImageFormat::WebP, QualityTarget::High) => 88,
        (ImageFormat::WebP, QualityTarget::Lossless) => 100,

        (ImageFormat::Png, QualityTarget::Draft) => 70,
        (ImageFormat::Png, QualityTarget::Good) => 80,
        (ImageFormat::Png, QualityTarget::High) => 90,
        (ImageFormat::Png, QualityTarget::Lossless) => 100,

        _ => 80,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub has_alpha: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProcessOptions {
    pub target_w: Option<u32>,
    pub target_h: Option<u32>,
    pub fit: FitMode,
    pub format: Option<ImageFormat>,
    pub quality: Option<u8>,
    pub quality_target: Option<QualityTarget>,
    pub accept_header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProcessError {
    #[error("Unsupported image format: {0}")]
    UnsupportedFormat(String),

    #[error("Failed to decode image: {0}")]
    DecodeFailed(String),

    #[error("Failed to process image: {0}")]
    ProcessingFailed(String),

    #[error("Image dimensions or size limits exceeded: {0}")]
    LimitsExceeded(String),

    #[error("Internal processor error: {0}")]
    Internal(String),
}

pub trait ImageProcessor: Send + Sync + 'static {
    fn probe(&self, input: &[u8]) -> Result<ImageInfo, ProcessError>;
    fn process(&self, input: &[u8], opts: ProcessOptions) -> Result<ProcessOutput, ProcessError>;
}
