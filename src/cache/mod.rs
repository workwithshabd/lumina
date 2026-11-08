pub mod inflight;

use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::http::params::CacheFormat;
use crate::processor::{FitMode, ImageFormat};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CacheStatus {
    Hit,
    Miss,
}

impl CacheStatus {
    pub fn as_header_str(&self) -> &'static str {
        match self {
            Self::Hit => "HIT",
            Self::Miss => "MISS",
        }
    }
}

pub struct ImageCache {
    cache: moka::future::Cache<String, Arc<CachedImage>>,
    single_entry_max_bytes: usize,
}

impl ImageCache {
    pub fn new(config: &Config) -> Self {
        let max_capacity_bytes = config.cache_max_size_mb * 1024 * 1024;
        let single_entry_max_bytes = (config.cache_single_entry_max_mb * 1024 * 1024) as usize;

        let cache = moka::future::Cache::builder()
            .weigher(|_key, val: &Arc<CachedImage>| val.bytes.len() as u32)
            .max_capacity(max_capacity_bytes)
            .time_to_live(Duration::from_secs(config.cache_ttl_secs))
            .build();

        Self {
            cache,
            single_entry_max_bytes,
        }
    }

    pub async fn get(&self, key: &str) -> Option<Arc<CachedImage>> {
        self.cache.get(key).await
    }

    /// Inserts into cache unless the entry exceeds `single_entry_max_bytes`.
    ///
    /// If oversize, returns false (and caller increments the oversize metric).
    pub async fn insert_if_size_allowed(&self, key: String, image: Arc<CachedImage>) -> bool {
        if image.bytes.len() > self.single_entry_max_bytes {
            return false;
        }
        self.cache.insert(key, image).await;
        true
    }

    pub fn single_entry_max_bytes(&self) -> usize {
        self.single_entry_max_bytes
    }

    pub fn weighted_size(&self) -> u64 {
        self.cache.weighted_size()
    }

    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }
}

/// Parameter bundle for generating a deterministic, canonical cache key.
#[derive(Clone, Copy, Debug)]
pub struct CacheKeyParams<'a> {
    pub src: &'a str,
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub format: CacheFormat,
    pub quality: u8,
    pub fit: FitMode,
    pub allow_upscale: bool,
    pub v: Option<&'a str>,
}

/// Generates a deterministic, canonical cache key for a transform operation.
///
/// Ensures every output-affecting parameter is represented:
/// - canonical `src` URL
/// - `w` (target width)
/// - `h` (target height)
/// - `format` (resolved output format or auto accept class)
/// - `quality` (resolved quality level 1-100)
/// - `fit` (cover, contain, fill)
/// - `allow_upscale` (whether upscaling beyond source is permitted)
/// - `v` (cache invalidation version)
pub fn compute_cache_key(params: &CacheKeyParams<'_>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"v2|");
    hasher.update(params.src.as_bytes());
    hasher.update(b"|w=");
    if let Some(width) = params.w {
        hasher.update(width.to_string().as_bytes());
    } else {
        hasher.update(b"auto");
    }
    hasher.update(b"|h=");
    if let Some(height) = params.h {
        hasher.update(height.to_string().as_bytes());
    } else {
        hasher.update(b"auto");
    }
    hasher.update(b"|fmt=");
    hasher.update(params.format.as_str().as_bytes());
    hasher.update(b"|q=");
    hasher.update(params.quality.to_string().as_bytes());
    hasher.update(b"|fit=");
    let fit_str = match params.fit {
        FitMode::Cover => "cover",
        FitMode::Contain => "contain",
        FitMode::Fill => "fill",
        FitMode::Inside => "inside",
        FitMode::Outside => "outside",
    };
    hasher.update(fit_str.as_bytes());
    hasher.update(b"|upscale=");
    hasher.update(if params.allow_upscale { b"1" } else { b"0" });
    hasher.update(b"|v=");
    if let Some(ver) = params.v {
        hasher.update(ver.as_bytes());
    }

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_uniqueness() {
        let base_params = CacheKeyParams {
            src: "https://images.unsplash.com/photo.jpg",
            w: Some(300),
            h: Some(200),
            format: ImageFormat::WebP.into(),
            quality: 80,
            fit: FitMode::Cover,
            allow_upscale: false,
            v: Some("v1"),
        };
        let base_key = compute_cache_key(&base_params);

        // Different allow_upscale -> different key
        let diff_upscale = compute_cache_key(&CacheKeyParams {
            allow_upscale: true,
            ..base_params
        });
        assert_ne!(base_key, diff_upscale);

        // Different width -> different key
        let diff_w = compute_cache_key(&CacheKeyParams {
            w: Some(301),
            ..base_params
        });
        assert_ne!(base_key, diff_w);

        // Different height -> different key
        let diff_h = compute_cache_key(&CacheKeyParams {
            h: Some(201),
            ..base_params
        });
        assert_ne!(base_key, diff_h);

        // Different format -> different key
        let diff_fmt = compute_cache_key(&CacheKeyParams {
            format: ImageFormat::Avif.into(),
            ..base_params
        });
        assert_ne!(base_key, diff_fmt);

        // Different quality -> different key
        let diff_q = compute_cache_key(&CacheKeyParams {
            quality: 85,
            ..base_params
        });
        assert_ne!(base_key, diff_q);

        // Different fit -> different key
        let diff_fit = compute_cache_key(&CacheKeyParams {
            fit: FitMode::Contain,
            ..base_params
        });
        assert_ne!(base_key, diff_fit);

        // Different version (invalidation) -> different key
        let diff_v = compute_cache_key(&CacheKeyParams {
            v: Some("v2"),
            ..base_params
        });
        assert_ne!(base_key, diff_v);

        // Identical inputs -> identical key
        let identical = compute_cache_key(&base_params);
        assert_eq!(base_key, identical);
    }
}
