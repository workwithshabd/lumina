use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use crate::error::AppError;
pub use crate::processor::{resolve_quality, QualityTarget};
use crate::processor::{FitMode, ImageFormat};

pub const MAX_QUERY_STRING_LENGTH: usize = 4096;
pub const MAX_QUERY_PARAMS_COUNT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatParam {
    Auto,
    Specific(ImageFormat),
}

impl FromStr for FormatParam {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "webp" => Ok(Self::Specific(ImageFormat::WebP)),
            "avif" => Ok(Self::Specific(ImageFormat::Avif)),
            "jpeg" | "jpg" => Ok(Self::Specific(ImageFormat::Jpeg)),
            "png" => Ok(Self::Specific(ImageFormat::Png)),
            other => Err(AppError::InvalidParam(format!(
                "Invalid format '{other}': expected webp, avif, jpeg, png, or auto"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformParams {
    pub src: String,
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub format: Option<FormatParam>,
    pub quality_target: Option<QualityTarget>,
    pub q: Option<u8>,
    pub fit: FitMode,
    pub v: Option<String>,
    pub sig: Option<String>,
    pub exp: Option<u64>,
    pub kid: Option<String>,
    pub allow_upscale: bool,
}

impl TransformParams {
    /// Parses and strictly validates query parameters from raw query string.
    ///
    /// Rejects duplicate parameters or unknown parameters with 422 (UNPROCESSABLE_ENTITY).
    pub fn parse(raw_query: Option<&str>) -> Result<Self, AppError> {
        let query_str = raw_query.unwrap_or("").trim();

        if query_str.len() > MAX_QUERY_STRING_LENGTH {
            return Err(AppError::InvalidParam(format!(
                "Query string exceeds maximum allowed length of {MAX_QUERY_STRING_LENGTH} bytes"
            )));
        }

        if query_str.is_empty() {
            return Err(AppError::InvalidParam(
                "Query parameters required (src is mandatory)".to_string(),
            ));
        }

        let allowed_keys: HashSet<&'static str> = [
            "src",
            "url",
            "w",
            "h",
            "format",
            "f",
            "quality_target",
            "qt",
            "q",
            "quality",
            "fit",
            "v",
            "sig",
            "exp",
            "kid",
            "allow_upscale",
            "upscale",
        ]
        .into_iter()
        .collect();

        let mut seen_keys = HashSet::new();
        let mut params_map = HashMap::new();

        let pairs: Vec<&str> = query_str.split('&').filter(|s| !s.is_empty()).collect();
        if pairs.len() > MAX_QUERY_PARAMS_COUNT {
            return Err(AppError::InvalidParam(format!(
                "Number of query parameters exceeds maximum of {MAX_QUERY_PARAMS_COUNT}"
            )));
        }

        for pair in pairs {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let raw_val = parts.next().unwrap_or("").trim();

            if key.is_empty() {
                continue;
            }

            // 1. Strict duplicate param check (reject with 422)
            if !seen_keys.insert(key.to_string()) {
                return Err(AppError::DuplicateParam(key.to_string()));
            }

            // 2. Strict unknown param check (reject with 422)
            if !allowed_keys.contains(key) {
                return Err(AppError::UnknownParam(key.to_string()));
            }

            // Decode URL-encoded value
            let decoded_val = percent_encoding::percent_decode_str(raw_val)
                .decode_utf8()
                .map_err(|e| {
                    AppError::InvalidParam(format!("Invalid URL encoding in '{key}': {e}"))
                })?
                .to_string();

            params_map.insert(key.to_string(), decoded_val);
        }

        // src is mandatory (url accepted as alias)
        let src = match (params_map.remove("src"), params_map.remove("url")) {
            (Some(s), None) => s,
            (None, Some(u)) => u,
            (Some(_), Some(_)) => {
                return Err(AppError::DuplicateParam(
                    "Both 'src' and 'url' parameters cannot be specified together".to_string(),
                ));
            }
            (None, None) => {
                return Err(AppError::InvalidParam(
                    "Missing required parameter 'src' (or 'url')".to_string(),
                ));
            }
        };

        // w (optional u32)
        let w = match params_map.remove("w") {
            Some(s) if !s.is_empty() => {
                let val = s.parse::<u32>().map_err(|_| {
                    AppError::InvalidParam("Parameter 'w' must be a positive integer".to_string())
                })?;
                if val == 0 {
                    return Err(AppError::InvalidParam(
                        "Parameter 'w' must be greater than 0".to_string(),
                    ));
                }
                Some(val)
            }
            _ => None,
        };

        // h (optional u32)
        let h = match params_map.remove("h") {
            Some(s) if !s.is_empty() => {
                let val = s.parse::<u32>().map_err(|_| {
                    AppError::InvalidParam("Parameter 'h' must be a positive integer".to_string())
                })?;
                if val == 0 {
                    return Err(AppError::InvalidParam(
                        "Parameter 'h' must be greater than 0".to_string(),
                    ));
                }
                Some(val)
            }
            _ => None,
        };

        // format (optional webp|avif|jpeg|png|auto, f accepted as alias)
        let format = match params_map.remove("format").or_else(|| params_map.remove("f")) {
            Some(s) if !s.is_empty() => Some(s.parse::<FormatParam>()?),
            _ => None,
        };

        // q / quality (optional u8 between 1 and 100)
        let q = match params_map.remove("q").or_else(|| params_map.remove("quality")) {
            Some(s) if !s.is_empty() => {
                let val = s.parse::<u8>().map_err(|_| {
                    AppError::InvalidParam("Parameter 'q' must be an integer between 1 and 100".to_string())
                })?;
                if !(1..=100).contains(&val) {
                    return Err(AppError::InvalidParam(
                        "Parameter 'q' must be between 1 and 100".to_string(),
                    ));
                }
                Some(val)
            }
            _ => None,
        };

        // quality_target / qt (optional draft|good|high|lossless)
        let quality_target = match params_map.remove("quality_target").or_else(|| params_map.remove("qt")) {
            Some(s) if !s.is_empty() => Some(
                s.parse::<QualityTarget>()
                    .map_err(|e| AppError::InvalidParam(e.to_string()))?,
            ),
            _ => q.map(|val| {
                if val >= 95 {
                    QualityTarget::Lossless
                } else if val >= 80 {
                    QualityTarget::High
                } else if val >= 60 {
                    QualityTarget::Good
                } else {
                    QualityTarget::Draft
                }
            }),
        };

        // fit (cover|contain|fill, default: cover)
        let fit = match params_map.remove("fit") {
            Some(s) if !s.is_empty() => match s.to_lowercase().as_str() {
                "cover" => FitMode::Cover,
                "contain" => FitMode::Contain,
                "fill" => FitMode::Fill,
                other => {
                    return Err(AppError::InvalidParam(format!(
                        "Invalid fit '{other}': expected cover, contain, or fill"
                    )))
                }
            },
            _ => FitMode::Cover,
        };

        // v (opaque cache-version string, max 32 chars)
        let v = match params_map.remove("v") {
            Some(s) if !s.is_empty() => {
                if s.len() > 32 {
                    return Err(AppError::InvalidParam(
                        "Parameter 'v' must not exceed 32 characters".to_string(),
                    ));
                }
                if !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(AppError::InvalidParam(
                        "Parameter 'v' must only contain alphanumeric, dash, or underscore characters".to_string(),
                    ));
                }
                Some(s)
            }
            _ => None,
        };

        // sig (optional signature)
        let sig = params_map.remove("sig");

        // exp (optional expiration timestamp)
        let exp = match params_map.remove("exp") {
            Some(s) if !s.is_empty() => Some(s.parse::<u64>().map_err(|_| {
                AppError::InvalidParam("Parameter 'exp' must be a valid timestamp".to_string())
            })?),
            _ => None,
        };

        // kid (optional key ID)
        let kid = match params_map.remove("kid") {
            Some(s) if !s.is_empty() => {
                if s.len() > 64 {
                    return Err(AppError::InvalidParam(
                        "Parameter 'kid' exceeds maximum allowed length of 64 characters"
                            .to_string(),
                    ));
                }
                if !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(AppError::InvalidParam(
                        "Parameter 'kid' must only contain alphanumeric, dash, or underscore characters".to_string(),
                    ));
                }
                Some(s)
            }
            _ => None,
        };

        // allow_upscale / upscale
        let allow_upscale = params_map
            .remove("allow_upscale")
            .or_else(|| params_map.remove("upscale"))
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);

        Ok(Self {
            src,
            w,
            h,
            format,
            quality_target,
            q,
            fit,
            v,
            sig,
            exp,
            kid,
            allow_upscale,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AcceptClass {
    Avif,
    Webp,
    Legacy,
}

impl AcceptClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Avif => "avif",
            Self::Webp => "webp",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CacheFormat {
    Specific(ImageFormat),
    Auto(AcceptClass),
}

impl CacheFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Specific(ImageFormat::Jpeg) => "specific:jpeg",
            Self::Specific(ImageFormat::Png) => "specific:png",
            Self::Specific(ImageFormat::WebP) => "specific:webp",
            Self::Specific(ImageFormat::Avif) => "specific:avif",
            Self::Specific(ImageFormat::Gif) => "specific:gif",
            Self::Specific(ImageFormat::Svg) => "specific:svg",
            Self::Auto(AcceptClass::Avif) => "auto:avif",
            Self::Auto(AcceptClass::Webp) => "auto:webp",
            Self::Auto(AcceptClass::Legacy) => "auto:legacy",
        }
    }
}

impl From<ImageFormat> for CacheFormat {
    fn from(f: ImageFormat) -> Self {
        Self::Specific(f)
    }
}

/// A parsed entry from an HTTP Accept header.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptPreference {
    pub media_range: String,
    pub q: f32,
}

/// Parses an HTTP `Accept` header into media ranges with quality factors (RFC 9110).
pub fn parse_accept_header(accept_header: Option<&str>) -> Vec<AcceptPreference> {
    let raw = match accept_header {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Vec::new(),
    };

    let mut ranges = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let mut tokens = part.split(';');
        let media_range = match tokens.next() {
            Some(m) => m.trim().to_lowercase(),
            None => continue,
        };

        if media_range.is_empty() {
            continue;
        }

        let mut q_val = 1.0f32;
        for param in tokens {
            let param = param.trim();
            if let Some(val_str) = param.strip_prefix("q=") {
                if let Ok(v) = val_str.trim().parse::<f32>() {
                    q_val = v.clamp(0.0, 1.0);
                }
            }
        }

        ranges.push(AcceptPreference {
            media_range,
            q: q_val,
        });
    }
    ranges
}

/// Resolves the effective quality factor for a concrete MIME type against parsed Accept preferences.
/// Returns None if explicitly rejected (`q == 0.0`) or unmentioned when non-wildcard ranges are present.
pub fn effective_q_for_mime(ranges: &[AcceptPreference], mime: &str) -> Option<f32> {
    if ranges.is_empty() {
        return Some(1.0);
    }

    let mut exact_q: Option<f32> = None;
    let mut sub_wildcard_q: Option<f32> = None;
    let mut any_wildcard_q: Option<f32> = None;

    for r in ranges {
        if r.media_range == mime {
            exact_q = Some(exact_q.map_or(r.q, |cur| cur.max(r.q)));
        } else if r.media_range == "image/*" {
            sub_wildcard_q = Some(sub_wildcard_q.map_or(r.q, |cur| cur.max(r.q)));
        } else if r.media_range == "*/*" {
            any_wildcard_q = Some(any_wildcard_q.map_or(r.q, |cur| cur.max(r.q)));
        }
    }

    let q = exact_q.or(sub_wildcard_q).or(any_wildcard_q);
    q.filter(|&val| val > 0.0)
}

/// Checks if a concrete `ImageFormat` is acceptable according to the client's `Accept` header.
pub fn is_format_acceptable(format: ImageFormat, accept_header: Option<&str>) -> bool {
    let ranges = parse_accept_header(accept_header);
    if ranges.is_empty() {
        return true;
    }
    let mime = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::WebP => "image/webp",
        ImageFormat::Avif => "image/avif",
        _ => return true,
    };
    effective_q_for_mime(&ranges, mime).is_some()
}

/// Resolves the client's Accept class for modern formats vs legacy formats.
pub fn resolve_accept_class(accept_header: Option<&str>) -> AcceptClass {
    let ranges = parse_accept_header(accept_header);
    if ranges.is_empty() {
        return AcceptClass::Legacy;
    }

    let mut explicit_avif: Option<f32> = None;
    let mut explicit_webp: Option<f32> = None;

    for r in &ranges {
        if r.media_range == "image/avif" {
            explicit_avif = Some(explicit_avif.map_or(r.q, |cur| cur.max(r.q)));
        } else if r.media_range == "image/webp" {
            explicit_webp = Some(explicit_webp.map_or(r.q, |cur| cur.max(r.q)));
        }
    }

    let avif_ok = explicit_avif.filter(|&q| q > 0.0);
    let webp_ok = explicit_webp.filter(|&q| q > 0.0);

    match (avif_ok, webp_ok) {
        (Some(aq), Some(wq)) => {
            if aq >= wq {
                AcceptClass::Avif
            } else {
                AcceptClass::Webp
            }
        }
        (Some(_), None) => AcceptClass::Avif,
        (None, Some(_)) => AcceptClass::Webp,
        (None, None) => AcceptClass::Legacy,
    }
}

/// Resolves the deterministic cache format representation BEFORE origin fetch or probe.
pub fn resolve_cache_format(
    requested: Option<FormatParam>,
    accept_header: Option<&str>,
) -> CacheFormat {
    match requested {
        Some(FormatParam::Specific(fmt)) => CacheFormat::Specific(fmt),
        Some(FormatParam::Auto) | None => CacheFormat::Auto(resolve_accept_class(accept_header)),
    }
}

/// Legacy helper for modern formats parsing.
pub fn parse_accept_modern_formats(accept_header: Option<&str>) -> (Option<f32>, Option<f32>) {
    let ranges = parse_accept_header(accept_header);
    let mut avif_q: Option<f32> = None;
    let mut webp_q: Option<f32> = None;

    for r in &ranges {
        if r.media_range == "image/avif" {
            avif_q = Some(avif_q.map_or(r.q, |cur| cur.max(r.q)));
        } else if r.media_range == "image/webp" {
            webp_q = Some(webp_q.map_or(r.q, |cur| cur.max(r.q)));
        }
    }

    (avif_q.filter(|&q| q > 0.0), webp_q.filter(|&q| q > 0.0))
}

/// Resolves candidate format before fetching source image (if format is specified or modern Accept match).
pub fn resolve_candidate_format(
    requested: Option<FormatParam>,
    accept_header: Option<&str>,
) -> Option<(ImageFormat, bool)> {
    match requested {
        Some(FormatParam::Specific(fmt)) => Some((fmt, false)),
        Some(FormatParam::Auto) | None => match resolve_accept_class(accept_header) {
            AcceptClass::Avif => Some((ImageFormat::Avif, true)),
            AcceptClass::Webp => Some((ImageFormat::WebP, true)),
            AcceptClass::Legacy => None,
        },
    }
}

/// Resolves the concrete output format, evaluating `Accept` header when `format=auto`.
///
/// Stage 1 (Option A - pure Rust) format=auto policy:
/// - Source has alpha -> smaller of lossless WebP and PNG (WebP if accepted, else PNG).
/// - Photographic content -> AVIF if Accept allows it, else JPEG. Never lossless WebP for photos.
/// - Graphic/flat content -> smaller of lossless WebP (if allowed) and PNG.
/// - Photo-vs-graphic: JPEG source format is photographic hint. For PNG/WebP sources,
///   the processor applies a downscaled distinct-color heuristic.
///
/// Returns `(resolved_format, is_auto)`.
pub fn resolve_format(
    requested: Option<FormatParam>,
    accept_header: Option<&str>,
    source_has_alpha: bool,
    source_format: ImageFormat,
) -> (ImageFormat, bool) {
    match requested {
        Some(FormatParam::Specific(fmt)) => (fmt, false),
        Some(FormatParam::Auto) | None => {
            let accept_class = resolve_accept_class(accept_header);
            if source_has_alpha {
                // Source has alpha -> AVIF if accepted (natively supports alpha with superior compression),
                // else WebP (lossless alpha), else PNG.
                match accept_class {
                    AcceptClass::Avif => (ImageFormat::Avif, true),
                    AcceptClass::Webp => (ImageFormat::WebP, true),
                    AcceptClass::Legacy => (ImageFormat::Png, true),
                }
            } else if source_format == ImageFormat::Jpeg {
                // Photographic content -> AVIF if Accept allows it, else JPEG.
                // Never lossless WebP for photos!
                match accept_class {
                    AcceptClass::Avif => (ImageFormat::Avif, true),
                    AcceptClass::Webp | AcceptClass::Legacy => (ImageFormat::Jpeg, true),
                }
            } else {
                // Graphic / flat content hint (PNG/WebP sources without alpha) -> AVIF if accepted, else WebP/PNG
                match accept_class {
                    AcceptClass::Avif => (ImageFormat::Avif, true),
                    AcceptClass::Webp => (ImageFormat::WebP, true),
                    AcceptClass::Legacy => match source_format {
                        ImageFormat::Png | ImageFormat::Jpeg => (source_format, true),
                        _ => (ImageFormat::Png, true),
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_params_parsing() {
        let q = "src=https%3A%2F%2Fimages.unsplash.com%2Fphoto.jpg&w=400&h=300&fit=contain&format=webp&quality_target=high&v=release1";
        let params = TransformParams::parse(Some(q)).unwrap();

        assert_eq!(params.src, "https://images.unsplash.com/photo.jpg");
        assert_eq!(params.w, Some(400));
        assert_eq!(params.h, Some(300));
        assert_eq!(params.fit, FitMode::Contain);
        assert_eq!(
            params.format,
            Some(FormatParam::Specific(ImageFormat::WebP))
        );
        assert_eq!(params.quality_target, Some(QualityTarget::High));
        assert_eq!(params.v.as_deref(), Some("release1"));
    }

    #[test]
    fn test_reject_duplicate_params_with_422() {
        let q = "src=https://example.com/a.jpg&w=400&w=500";
        let err = TransformParams::parse(Some(q)).unwrap_err();
        assert_eq!(err.code(), "DUPLICATE_PARAM");
        assert_eq!(err.status_code().as_u16(), 422);
    }

    #[test]
    fn test_reject_unknown_params_with_422() {
        let q = "src=https://example.com/a.jpg&w=400&h=300&evil_param=true";
        let err = TransformParams::parse(Some(q)).unwrap_err();
        assert_eq!(err.code(), "UNKNOWN_PARAM");
        assert_eq!(err.status_code().as_u16(), 422);
    }

    #[test]
    fn test_reject_oversize_v_param() {
        let long_v = "a".repeat(33);
        let q = format!("src=https://example.com/a.jpg&v={long_v}");
        let err = TransformParams::parse(Some(&q)).unwrap_err();
        assert_eq!(err.code(), "INVALID_PARAM");
    }

    #[test]
    fn test_auto_format_resolution() {
        // 1. Client accepts AVIF
        let (f1, is_auto1) = resolve_format(
            Some(FormatParam::Auto),
            Some("text/html,image/avif,image/webp,*/*"),
            false,
            ImageFormat::Jpeg,
        );
        assert_eq!(f1, ImageFormat::Avif);
        assert!(is_auto1);

        // 2. Client accepts WebP (no AVIF):
        // Photographic content (JPEG source) returns JPEG under Option A (never lossless WebP for photos)
        let (f2_photo, is_auto2_photo) = resolve_format(
            Some(FormatParam::Auto),
            Some("text/html,image/webp,*/*"),
            false,
            ImageFormat::Jpeg,
        );
        assert_eq!(f2_photo, ImageFormat::Jpeg);
        assert!(is_auto2_photo);

        // Graphic content (PNG source) returns WebP
        let (f2_graphic, is_auto2_graphic) = resolve_format(
            Some(FormatParam::Auto),
            Some("text/html,image/webp,*/*"),
            false,
            ImageFormat::Png,
        );
        assert_eq!(f2_graphic, ImageFormat::WebP);
        assert!(is_auto2_graphic);

        // 3. Client does not accept modern formats, source has alpha -> PNG
        let (f3, is_auto3) = resolve_format(
            Some(FormatParam::Auto),
            Some("image/jpeg,*/*"),
            true,
            ImageFormat::Png,
        );
        assert_eq!(f3, ImageFormat::Png);
        assert!(is_auto3);

        // 4. Explicit format overrides Accept header
        let (f4, is_auto4) = resolve_format(
            Some(FormatParam::Specific(ImageFormat::Png)),
            Some("image/avif"),
            false,
            ImageFormat::Jpeg,
        );
        assert_eq!(f4, ImageFormat::Png);
        assert!(!is_auto4);
    }

    #[test]
    fn test_quality_and_format_aliases_parsing() {
        let p = TransformParams::parse(Some("src=https://example.com/img.jpg&f=webp&q=85&allow_upscale=1")).unwrap();
        assert_eq!(p.format, Some(FormatParam::Specific(ImageFormat::WebP)));
        assert_eq!(p.q, Some(85));
        assert_eq!(p.quality_target, Some(QualityTarget::High));
        assert!(p.allow_upscale);

        let p2 = TransformParams::parse(Some("src=https://example.com/img.jpg&format=jpeg&qt=lossless")).unwrap();
        assert_eq!(p2.format, Some(FormatParam::Specific(ImageFormat::Jpeg)));
        assert_eq!(p2.quality_target, Some(QualityTarget::Lossless));
        assert_eq!(p2.q, None);
    }
}
