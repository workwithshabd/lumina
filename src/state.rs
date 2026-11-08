use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::auth::RateLimitManager;
use crate::cache::inflight::InflightManager;
use crate::cache::ImageCache;
use crate::config::Config;
use crate::error::AppError;
use crate::fetch::SourceFetcher;
use crate::observability::{update_cache_gauges, update_concurrency_gauges};
use crate::processor::ImageProcessor;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub processor: Arc<dyn ImageProcessor>,
    pub fetcher: Arc<SourceFetcher>,
    pub cache: Arc<ImageCache>,
    pub inflight: Arc<InflightManager>,
    pub transform_semaphore: Arc<Semaphore>,
    pub rate_limiter: Arc<RateLimitManager>,
    ready: Arc<AtomicBool>,
    is_draining: Arc<AtomicBool>,
    is_wedged: Arc<AtomicBool>,
    is_saturated_override: Arc<AtomicBool>,
    pub inflight_requests: Arc<AtomicUsize>,
    pub inflight_transforms: Arc<AtomicUsize>,
    pub queued_transforms: Arc<AtomicUsize>,
    total_served_bytes: Arc<AtomicU64>,
    cache_hit_bytes: Arc<AtomicU64>,
    cache_hits: Arc<AtomicU64>,
    cache_misses: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(config: Arc<Config>, processor: Arc<dyn ImageProcessor>) -> Result<Self, AppError> {
        let fetcher = Arc::new(SourceFetcher::new(config.clone())?);
        let cache = Arc::new(ImageCache::new(&config));
        let inflight = Arc::new(InflightManager::new());
        let transform_semaphore = Arc::new(Semaphore::new(config.max_concurrent_transforms));
        let rate_limiter = Arc::new(RateLimitManager::new(&config));

        Ok(Self {
            config,
            processor,
            fetcher,
            cache,
            inflight,
            transform_semaphore,
            rate_limiter,
            ready: Arc::new(AtomicBool::new(false)),
            is_draining: Arc::new(AtomicBool::new(false)),
            is_wedged: Arc::new(AtomicBool::new(false)),
            is_saturated_override: Arc::new(AtomicBool::new(false)),
            inflight_requests: Arc::new(AtomicUsize::new(0)),
            inflight_transforms: Arc::new(AtomicUsize::new(0)),
            queued_transforms: Arc::new(AtomicUsize::new(0)),
            total_served_bytes: Arc::new(AtomicU64::new(0)),
            cache_hit_bytes: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Constructs AppState with custom fetcher (used in testing with mock DNS/origins).
    pub fn new_with_fetcher(
        config: Arc<Config>,
        processor: Arc<dyn ImageProcessor>,
        fetcher: Arc<SourceFetcher>,
    ) -> Self {
        let cache = Arc::new(ImageCache::new(&config));
        let inflight = Arc::new(InflightManager::new());
        let transform_semaphore = Arc::new(Semaphore::new(config.max_concurrent_transforms));
        let rate_limiter = Arc::new(RateLimitManager::new(&config));

        Self {
            config,
            processor,
            fetcher,
            cache,
            inflight,
            transform_semaphore,
            rate_limiter,
            ready: Arc::new(AtomicBool::new(false)),
            is_draining: Arc::new(AtomicBool::new(false)),
            is_wedged: Arc::new(AtomicBool::new(false)),
            is_saturated_override: Arc::new(AtomicBool::new(false)),
            inflight_requests: Arc::new(AtomicUsize::new(0)),
            inflight_transforms: Arc::new(AtomicUsize::new(0)),
            queued_transforms: Arc::new(AtomicUsize::new(0)),
            total_served_bytes: Arc::new(AtomicU64::new(0)),
            cache_hit_bytes: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    pub fn start_draining(&self) {
        self.is_draining.store(true, Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.is_draining.load(Ordering::SeqCst)
    }

    pub fn set_wedged(&self, wedged: bool) {
        self.is_wedged.store(wedged, Ordering::SeqCst);
    }

    pub fn is_wedged(&self) -> bool {
        self.is_wedged.load(Ordering::SeqCst)
    }

    pub fn set_saturated_override(&self, saturated: bool) {
        self.is_saturated_override
            .store(saturated, Ordering::SeqCst);
    }

    pub fn is_saturated(&self) -> bool {
        if self.is_saturated_override.load(Ordering::SeqCst) {
            return true;
        }
        self.queued_transforms.load(Ordering::SeqCst) >= self.config.queue_saturation_threshold
    }

    pub fn readiness_check(&self) -> Result<(), &'static str> {
        if !self.ready.load(Ordering::SeqCst) {
            return Err("starting");
        }
        if self.is_draining.load(Ordering::SeqCst) {
            return Err("draining");
        }
        if self.is_saturated() {
            return Err("saturated");
        }
        Ok(())
    }

    pub fn on_request_start(&self) {
        let val = self.inflight_requests.fetch_add(1, Ordering::SeqCst) + 1;
        self.sync_concurrency_gauges(Some(val), None, None);
    }

    pub fn on_request_end(&self) {
        let val = self
            .inflight_requests
            .fetch_sub(1, Ordering::SeqCst)
            .saturating_sub(1);
        self.sync_concurrency_gauges(Some(val), None, None);
    }

    pub fn on_transform_queue_enter(&self) {
        let val = self.queued_transforms.fetch_add(1, Ordering::SeqCst) + 1;
        self.sync_concurrency_gauges(None, None, Some(val));
    }

    pub fn on_transform_start(&self) {
        let q = self
            .queued_transforms
            .fetch_sub(1, Ordering::SeqCst)
            .saturating_sub(1);
        let t = self.inflight_transforms.fetch_add(1, Ordering::SeqCst) + 1;
        self.sync_concurrency_gauges(None, Some(t), Some(q));
    }

    pub fn on_transform_end(&self) {
        let t = self
            .inflight_transforms
            .fetch_sub(1, Ordering::SeqCst)
            .saturating_sub(1);
        self.sync_concurrency_gauges(None, Some(t), None);
    }

    fn sync_concurrency_gauges(
        &self,
        reqs: Option<usize>,
        active_t: Option<usize>,
        queue_d: Option<usize>,
    ) {
        let r = reqs.unwrap_or_else(|| self.inflight_requests.load(Ordering::Relaxed));
        let t = active_t.unwrap_or_else(|| self.inflight_transforms.load(Ordering::Relaxed));
        let q = queue_d.unwrap_or_else(|| self.queued_transforms.load(Ordering::Relaxed));
        update_concurrency_gauges(r, t, q);
    }

    pub fn record_cache_hit(&self, bytes: u64) {
        self.cache_hits.fetch_add(1, Ordering::SeqCst);
        self.cache_hit_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.total_served_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.sync_cache_gauges();
    }

    pub fn record_cache_miss(&self, bytes: u64) {
        self.cache_misses.fetch_add(1, Ordering::SeqCst);
        self.total_served_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.sync_cache_gauges();
    }

    pub fn sync_cache_gauges(&self) {
        let hits = self.cache_hits.load(Ordering::Relaxed) as f64;
        let misses = self.cache_misses.load(Ordering::Relaxed) as f64;
        let total_reqs = hits + misses;
        let hit_ratio = if total_reqs > 0.0 {
            hits / total_reqs
        } else {
            0.0
        };

        let hit_b = self.cache_hit_bytes.load(Ordering::Relaxed) as f64;
        let total_b = self.total_served_bytes.load(Ordering::Relaxed) as f64;
        let byte_hit_ratio = if total_b > 0.0 { hit_b / total_b } else { 0.0 };

        let size_bytes = self.cache.weighted_size();
        let entries = self.cache.entry_count();

        update_cache_gauges(size_bytes, entries, hit_ratio, byte_hit_ratio);
    }
}

/// RAII guard for the transform waiting queue.
/// Automatically decrements `queued_transforms` on drop if cancelled before starting.
pub struct TransformQueueGuard<'a> {
    state: &'a AppState,
    active: bool,
}

impl<'a> TransformQueueGuard<'a> {
    pub fn enter(state: &'a AppState) -> Self {
        state.on_transform_queue_enter();
        Self {
            state,
            active: true,
        }
    }

    pub fn transition_to_active(mut self) -> ActiveTransformGuard<'a> {
        self.active = false;
        ActiveTransformGuard::start(self.state)
    }
}

impl<'a> Drop for TransformQueueGuard<'a> {
    fn drop(&mut self) {
        if self.active {
            let q = self
                .state
                .queued_transforms
                .fetch_sub(1, Ordering::SeqCst)
                .saturating_sub(1);
            self.state.sync_concurrency_gauges(None, None, Some(q));
        }
    }
}

/// RAII guard for active in-flight transformation.
/// Automatically decrements `inflight_transforms` on drop.
pub struct ActiveTransformGuard<'a> {
    state: &'a AppState,
}

impl<'a> ActiveTransformGuard<'a> {
    pub fn start(state: &'a AppState) -> Self {
        state.on_transform_start();
        Self { state }
    }
}

impl<'a> Drop for ActiveTransformGuard<'a> {
    fn drop(&mut self) {
        self.state.on_transform_end();
    }
}
