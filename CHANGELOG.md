# Changelog

All notable changes to Atihsrah are documented in this file.
The project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Prometheus Metrics, Scrape-Time Cache Sync & Grafana Dashboard (2026-11-08)
- **Synchronous Cache Gauge Scrape (`await cache.run_pending_tasks()`)**:
  - `atihsrah_cache_entries` and `atihsrah_cache_size_bytes` now await `cache.run_pending_tasks().await` at scrape time prior to querying Moka LRU statistics, ensuring immediate post-transform scrapes accurately reflect cached entries and memory size without delay.
- **Histogram Quantiles with Explicit Duration Buckets**:
  - Configured `metrics-exporter-prometheus` with fine bucket boundaries from 1 ms to 30 s (`0.001` to `30.0` seconds) for all `*_duration_seconds` metrics (`atihsrah_request_duration_seconds`, `atihsrah_fetch_duration_seconds`, `atihsrah_decode_duration_seconds`, `atihsrah_transform_duration_seconds`, `atihsrah_encode_duration_seconds`). All latency metrics now export standard `_bucket{le="..."}`, `_sum`, and `_count` series for `histogram_quantile(..., rate(..._bucket[5m]))` evaluation.
- **Replaced Ratio Gauges with Prometheus Counters**:
  - Deprecated and removed gauges:
    - Old: `atihsrah_cache_hit_ratio` (gauge) &rarr; New: `atihsrah_cache_hits_total` (counter) and `atihsrah_cache_misses_total` (counter).
    - Old: `atihsrah_cache_byte_hit_ratio` (gauge) &rarr; New: `atihsrah_cache_hit_bytes_total` (counter) and `atihsrah_response_bytes_total` (counter).
  - Ratios are now computed dynamically in PromQL dashboard expressions:
    - Request Hit Ratio: `sum(rate(atihsrah_cache_hits_total[5m])) / (sum(rate(atihsrah_cache_hits_total[5m])) + sum(rate(atihsrah_cache_misses_total[5m]))) * 100`
    - Byte Hit Ratio: `sum(rate(atihsrah_cache_hit_bytes_total[5m])) / sum(rate(atihsrah_response_bytes_total[5m])) * 100`
- **Pre-Registered Startup Counters & Origin Security Tracking**:
  - Pre-registered all counters (`atihsrah_ssrf_blocked_total`, `atihsrah_rate_limited_total`, `atihsrah_signature_invalid_total`, `atihsrah_oversize_not_cached_total`, `atihsrah_output_larger_than_source_total`, `atihsrah_cache_hits_total`, `atihsrah_cache_misses_total`, `atihsrah_cache_hit_bytes_total`, `atihsrah_response_bytes_total`, `atihsrah_source_rejected_total`) at 0 so all series exist on clean scrapes before traffic.
  - Added `atihsrah_source_rejected_total{reason}` counter tracking disallowed origin, scheme, and port rejections.
- **Official Grafana Dashboard**:
  - Added `monitoring/grafana-dashboard.json` covering throughput, cache ratios, p50/p90/p99 latency quantiles, stage breakdowns, concurrency, and security defense events.

### Audit Remediation, Async DNS Offload & Local macOS Benchmark Suite (2026-11-08)
- **Non-Blocking Async DNS Resolution**:
  - Wrapped synchronous DNS lookups in `src/fetch/resolver.rs` in `tokio::task::spawn_blocking`, eliminating async worker stalling on Tokio worker threads while retaining 100% of IPv4/IPv6 private IP filtering, link-local metadata protection, and DNS-rebinding prevention.
  - Added regression test `test_dns_resolver_runs_on_blocking_pool_without_stalling_runtime` in `tests/fetch_security.rs`.
- **Connected Graceful Shutdown Timeout**:
  - Wired `config.shutdown_timeout_secs` in `src/main.rs` to server drain via `tokio::select!` timeout, ensuring clean process termination within configured bounds.
- **Cleaned Redundant Configuration & Code**:
  - Removed redundant `signing_key` and `log_level` fields from `Config`, keeping `signing_keys` as single source of truth.
  - Delegated URL hashing in `src/fetch/mod.rs` directly to `crate::observability::sha256_prefix(url)`.
- **Rebuilt Reproducible Benchmark Suite on Local macOS**:
  - Rebuilt `bench/run_all.sh` and `bench/benchmark_suite.py` from scratch for local execution on Apple Silicon macOS hardware without remote VPS runners.
  - Added support for Native Atihsrah (arm64 release binary), Containerized Atihsrah (Colima Linux VM), imgproxy OSS (v4.0.21), imgproxy Cached (Nginx reverse proxy), and Native Thumbor (v7.8.0).
  - Evaluated the 20-image corpus across delivered bytes, visual fidelity (SSIM via Pillow Lanczos reference), cold latency, warm cache distributions, single-flight coalescing, and memory RSS.
  - Quantified Docker virtualization overhead on macOS and emitted machine-readable results (`benchmarks/raw_results.json`, `benchmarks/results.csv`, `benchmarks/system_info.json`, `benchmarks/methodology.md`, `benchmarks/analysis.md`).

### Pure-Rust Format Policy & Stage 1 Remediation (2026-11-08)
- **Pure-Rust Codec Architecture (Option A)**:
  - Finalized on pure-Rust image codecs (`image-rs` v0.25 and `ravif` v0.11), completely removing the `webp` / `libwebp-sys` crate.
  - Superseded historical Finding G (which briefly evaluated wrapping C `libwebp-sys` on 2026-11-03). The shipping build links zero external C image libraries (`libwebp-sys`, `libvips`).
  - WebP encoding is strictly lossless-only via pure-Rust `image-webp v0.2.4`.
- **Intelligent `format=auto` Negotiation**:
  - Images with alpha transparency evaluate lossless WebP and PNG, serving whichever produces smaller bytes.
  - Photographic content evaluates AVIF (if client's `Accept` allows) or falls back to JPEG. Photos are **never** routed to lossless WebP, preventing photo file bloat.
  - Graphic and flat content evaluates lossless WebP (if allowed) and PNG, selecting the smaller output.
  - Photo vs. graphic heuristic: JPEG sources are treated as photos; non-alpha PNG/WebP sources are classified via a 64x64 pixel sample counting distinct colors against `ATIHSRAH_DISTINCT_COLOR_THRESHOLD` (default 256).
- **Larger-Than-Source Guard**:
  - Re-encoding never leaks raw original source bytes. If transformed output exceeds source byte length, evaluates up to 2 retry candidates (quality step or format fallback) bounded by `ATIHSRAH_MAX_RETRY_PIXELS` (default 16M pixels). If still larger, emits `atihsrah_output_larger_than_source_total`.
- **Enforced `#![forbid(unsafe_code)]`**:
  - Added `#![forbid(unsafe_code)]` to crate roots (`src/lib.rs` and `src/main.rs`). Zero unsafe code in `src/`.

### URL Signing, Clients & Test Vectors (Stage 2) (2026-11-08)
- **15+ Canonical Test Vectors**:
  - Generated deterministic cross-platform test vectors in `docs/signing-vectors.json` verifying canonical query construction, RFC 3986 percent-encoding, parameter order, clock skew, and error handling.
- **Multi-Language Reference Implementations**:
  - Added tested reference client implementations in Python (`examples/python/signer.py`), Node.js/TypeScript (`examples/node/signer.ts`), and Go (`examples/go/signer.go`).
- **Signing Key Safety Guardrails**:
  - Implemented startup refusal in signed mode if `ATIHSRAH_SIGNING_KEY` is empty, an all-zero key, a repeated character key, or matches a documented example/denylisted key.
  - Replaced example keys in `.env.example` and documentation with `<generate with: openssl rand -hex 32>` placeholders.

### Reproducible VPS Benchmark Harness (Stage 3) (2026-11-08)
- **Automated Fair Benchmark Suite**:
  - Added complete, self-contained VPS benchmark harness in `bench/` (`RUNME.md`, `run_all.sh`, `docker-compose.bench.yml`, `benchmark_suite.py`).
  - Added 20-image diverse corpus manifest (`bench/corpus_manifest.csv`) with public Creative Commons / Unsplash test assets.
  - Added `bench/analysis_TEMPLATE.md` with strict ground-truth reporting standards and zero fabricated numbers.

### Deprecations & Superseded Components (2026-11-08)
- **`webp` / `libwebp-sys` Crate**:
  - Removed and superseded by Pure Rust Option A on 2026-11-08.
- **`ATIHSRAH_DANGER_ACCEPT_INVALID_CERTS`**:
  - Removed on 2026-11-08 in favor of granular `ATIHSRAH_PRIVATE_ORIGINS` and custom root CA anchoring via `ATIHSRAH_PRIVATE_CA_FILE`. TLS validation cannot be globally bypassed.

### Security & Networking (Phase B)
- **Granular Private Origins (`ATIHSRAH_PRIVATE_ORIGINS`)**:
  - Replaced the global `ATIHSRAH_ALLOW_PRIVATE_NETWORKS` toggle with `ATIHSRAH_PRIVATE_ORIGINS`: an explicit comma-separated list of exact origins (`scheme://host[:port]`) allowed to resolve to private networks.
  - Private IP address resolution is strictly permitted ONLY for exact matching origins. The same private IP reached via any unlisted hostname is blocked.
  - `http://` scheme is permitted ONLY if that exact origin is listed with `http://` in `ATIHSRAH_PRIVATE_ORIGINS`. Public origins in `ALLOWED_ORIGINS` remain strictly `https://` on port 443.
  - Cloud metadata (`169.254.0.0/16`), IPv6 link-local (`fe80::/10`), multicast (`224.0.0.0/4`, `ff00::/8`), broadcast (`255.255.255.255/32`), and unspecified networks remain strictly BLOCKED even for listed private origins.
- **Mandatory TLS Verification & Internal CA Trust (`ATIHSRAH_PRIVATE_CA_FILE`)**:
  - Removed `ATIHSRAH_DANGER_ACCEPT_INVALID_CERTS`. TLS certificate validation is now ALWAYS enforced on all HTTPS upstream requests.
  - Added `ATIHSRAH_PRIVATE_CA_FILE` to allow specifying a custom PEM-encoded root CA certificate for internal services.
- **Startup Audit Logging**:
  - Emits a mandatory `WARN` log at startup listing all enabled private origins.

### Operational, CI & Observability Enhancements (Phase C)
- **CI & Feature Cleanup**:
  - Removed unused optional `libvips` crate dependency and feature flag from `Cargo.toml`.
- **Unknown Configuration Detection**:
  - At startup, scans environment variables. Any variable prefixed with `ATIHSRAH_` or within edit distance <= 3 of a documented configuration name that is unrecognized generates a `WARN` log proposing the closest valid name.
- **Prometheus Metric Prefixing**:
  - Prefixed all Prometheus metrics with `atihsrah_` to avoid namespace collisions in multi-tenant Prometheus clusters.

| Old Metric Name | New Metric Name | Type | Description |
|---|---|---|---|
| `requests_total` | `atihsrah_requests_total` | Counter | Total requests by cache result and status class |
| `request_duration_seconds` | `atihsrah_request_duration_seconds` | Histogram | End-to-end request latency |
| `fetch_duration_seconds` | `atihsrah_fetch_duration_seconds` | Histogram | Origin source fetch duration |
| `decode_duration_seconds` | `atihsrah_decode_duration_seconds` | Histogram | Image decoding duration |
| `transform_duration_seconds` | `atihsrah_transform_duration_seconds` | Histogram | Image resize/process duration |
| `encode_duration_seconds` | `atihsrah_encode_duration_seconds` | Histogram | Target image encoding duration |
| `errors_total` | `atihsrah_errors_total` | Counter | Total application errors by error type |
| `ssrf_blocked_total` | `atihsrah_ssrf_blocked_total` | Counter | Total SSRF attempts blocked |
| `rate_limited_total` | `atihsrah_rate_limited_total` | Counter | Total 429 rate limit events |
| `signature_invalid_total` | `atihsrah_signature_invalid_total` | Counter | Total invalid signature rejections |
| `oversize_not_cached_total` | `atihsrah_oversize_not_cached_total` | Counter | Total output images exceeding single-entry cache limit |
| `cache_size_bytes` | `atihsrah_cache_size_bytes` | Gauge | Current byte size of the in-memory cache |
| `cache_entries` | `atihsrah_cache_entries` | Gauge | Number of image entries in cache |
| `cache_hit_ratio` | `atihsrah_cache_hit_ratio` | Gauge | Overall cache hit ratio |
| `cache_byte_hit_ratio` | `atihsrah_cache_byte_hit_ratio` | Gauge | Byte-weighted cache hit ratio |
| `inflight_requests` | `atihsrah_inflight_requests` | Gauge | Currently active HTTP requests |
| `inflight_transforms` | `atihsrah_inflight_transforms` | Gauge | Transforms currently running in worker pool |
| `transform_queue_depth` | `atihsrah_transform_queue_depth` | Gauge | Requests waiting for transform execution permit |

---

## [0.1.0-remediated] - 2026-11-03

### Security Fixes
- **Finding A (Rate-Limit Identity & Auth Ordering)**:
  - Implemented pre-authentication IP rate limiting in `handle_transform_inner` to prevent unauthenticated `kid` spraying and memory exhaustion.
  - Ensured signature verification occurs before applying authenticated `kid` rate limiter quotas, preventing denial-of-service attacks against legitimate API keys.
  - Hardened `extract_client_ip` with RFC 7239 reverse chain traversal: traversing `X-Forwarded-For` from right to left, stopping at the first untrusted IP.
- **Finding E (Private Networking & TLS Decoupling)**:
  - Decoupled `allow_private_networks` from `danger_accept_invalid_certs(true)`. Enabling private networking no longer disables TLS certificate verification globally.
  - Added independent configuration `ATIHSRAH_DANGER_ACCEPT_INVALID_CERTS` (default `false`) with explicit security warning logs.
- **Finding F (Signed URL Expiration & HTTP Cache Lifetime)**:
  - Bounded `Cache-Control: max-age` by the signature's remaining time-to-live (`exp.saturating_sub(now)`).
  - Emits `Cache-Control: no-cache, no-store, must-revalidate` for expired signatures, preventing downstream proxies from caching expired responses.
- **Finding I (SSRF IPv6 Special Addresses)**:
  - Added extraction and recursive private network validation for 6to4 (`2002::/16`, RFC 3056), Teredo (`2001::/32`, RFC 4380), and SIIT (`::ffff:0:0/96`, RFC 7915).
  - Added Discard-only (`100::/64`), BMWG Benchmarking (`2001:2::/48`), ORCHID (`2001:10::/28`, `2001:20::/28`), and Local NAT64 (`64:ff9b:1::/48`) to `BLOCKED_IPV6`.

### Correctness & Bug Fixes
- **Finding B (Incomplete Cache Identity)**:
  - Introduced `CacheKeyParams` struct and integrated `allow_upscale` into SHA-256 cache key calculation and `inflight_key`.
- **Finding G (WebP Lossy Encoding & Quality Mapping)**:
  - Integrated `webp = "0.3"` crate (Google `libwebp` bindings).
  - Implemented lossy WebP encoding for `quality < 100` and lossless for `quality >= 100`, preserving transparency channels.
- **Finding J (HTTP Accept Negotiation & RFC 9110 Compliance)**:
  - Implemented RFC 9110 Accept header parser respecting `q` values and explicit media rejection (`q=0.0`).
  - Unified candidate format resolution before fetch and full format resolution during transform.
- **Finding K (In-Flight Key Collision on Differing Accept Headers)**:
  - Included candidate format in `inflight_key` to prevent concurrent clients requesting `format=auto` with differing `Accept` headers (e.g. AVIF vs WebP) from receiving the wrong image format.
- **Finding L (EXIF Orientation Normalization)**:
  - Extracted EXIF orientation tags from decoders and applied rotation/flip via `dyn_img.apply_orientation` before processing, ensuring mobile camera photos render right-side up.

### Performance & Concurrency Enhancements
- **Finding C (Cancellation-Safe Single-Flight Coordination)**:
  - Implemented RAII `InflightLeaderGuard` in `InflightManager`. Aborting leader tasks automatically evicts the in-flight key and closes broadcast channels.
  - Added follower takeover retry logic, preventing permanent map poisoning and eliminating 500 Internal Errors.
- **Finding D (Concurrency Counter Leakage Prevention)**:
  - Implemented `TransformQueueGuard` and `ActiveTransformGuard` with RAII `Drop` handlers.
  - Aborted requests during queue semaphore acquisition now properly decrement `queued_transforms`, preventing spurious readiness saturation failures.

### Architecture & Documentation
- Authoring of `SECURITY_REVIEW.md`, `ENGINEERING_REVIEW.md`, and `BENCHMARKS.md`.
- Added 12 regression test cases in `tests/audit_remediation.rs`.
- Remediated `bench/benchmark_suite.py` to filter still-image 0.0 VMAF scores.
- Zero Clippy warnings across `--all-targets -- -D warnings`.
- Verified minimal distroless container image build (14.1 MB content size, zero CVEs).

