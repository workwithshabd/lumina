# Atihsrah: Product Brief & Architectural Decisions (v1.0 Roadmap)

> **Document Status**: Active Product Definition & Decision Register (Aligned Revision)  
> **Target Scope**: Next Phase Evaluation, Architecture Boundaries & Benchmark Standards  
> **Baseline Commit**: Master branch, verified against pure-Rust `#![forbid(unsafe_code)]` codebase.

---

## 1. Product Brief

### 1.1 Target Users & Operating Model
Atihsrah is designed primarily for **self-hosting engineers and small-to-medium product teams** deploying modern web applications on standard containerized cloud infrastructure (Kubernetes, AWS ECS, GCP Cloud Run, Fly.io) or dedicated Linux/macOS nodes.
- **Primary Persona**: The developer or platform engineer who needs an on-the-fly image optimization proxy that is **deployable without external C image-codec libraries (`libvips`, `libwebp`, `libheif`)**, and strictly enforces **`#![forbid(unsafe_code)]` in all first-party application code**.
- **Secondary Persona**: Local Mac developers running frontend development workflows where an image proxy mock/engine is needed locally without running heavy multi-gigabyte Docker/C-toolchain dependencies.
- **Out of Scope**: Global hyperscale multi-tenant CDNs (e.g., replacing Cloudflare Images global edge network) or specialized pre-press print production pipelines requiring 16-bit CMYK or complex vector rasterization.

### 1.2 Input & Output Format Contract
- **Supported HTTP Ingestion Formats (Decodable by Proxy)**:
  - `JPEG` (baseline and progressive via pure-Rust `image-rs`)
  - `PNG` (8-bit, 16-bit, grayscale, RGB, RGBA via pure-Rust `png`/`image-rs`)
  - `WebP` (VP8 lossy decoding and VP8L lossless decoding via pure-Rust `image-webp`)
  - `GIF` (first frame static extraction via pure-Rust `gif`/`image-rs`)
  - *Verification*: Confirmed via `test_image_rs_decode_all_supported_input_formats`.
- **Explicitly Unsupported Ingestion (Fails Closed with HTTP 415 / `UnsupportedFormat`)**:
  - `AVIF`: In `image 0.25`, pure-Rust AVIF support is **encoder-only (`ravif`)**. Decoding AVIF requires C `libdav1d` (`avif-native`), which is excluded to preserve `#![forbid(unsafe_code)]`. Attempting to ingest AVIF fails safely and fast with `ProcessError::UnsupportedFormat` (verified via `test_image_rs_decode_avif_fails_safely_as_unsupported`).
  - `HEIC / HEIF / AV1 HDR10+ / SVG / TIFF / RAW` (NEF, CR2, DNG).
  - *HEIC Pipeline Contract*: Atihsrah does **not** decode HEIC. Any HEIC processing (e.g., iPhone uploads in the Studio UI) occurs upstream at the origin layer via Apple's native `sips` CLI tool before passing standard JPEG/PNG to Atihsrah.
- **Output Delivery Formats (Encodable by Proxy)**:
  - `AVIF`: Lossy intra-frame encoding via pure-Rust `ravif` (speed 6 default profile, speed 9 benchmark/turbo profile).
  - `WebP`: **Lossless VP8L encoding only** via pure-Rust `image-rs`. The `q` quality parameter is not used for WebP output.
  - `JPEG`: Lossy baseline DCT encoding via pure-Rust `image::codecs::jpeg::JpegEncoder` (`image-rs`) honoring `q=1..100`.
  - `PNG`: Lossless deflate encoding honoring RGBA alpha channels.

### 1.3 Core Value Proposition & Service Promises
1. **Memory Safety Posture**: First-party code strictly enforces `#![forbid(unsafe_code)]` with zero `unsafe` blocks. External C image-codec libraries (`libvips`, `libwebp-sys`, `libheif`) are eliminated from the dependency tree. Pinned dependencies containing internal `unsafe` (e.g. `image`, `tokio`, `aws-lc-rs`) are audited via `cargo auditable` and automated test suites.
2. **Minimal Operational Surface**: Ships as a lightweight binary running on standard system glibc (`distroless/cc-debian12` container) without external C codec dynamic libraries.
3. **Bandwidth Optimization**: Modern AVIF delivery yielding substantial byte reductions against photographic web assets meeting strict quality gates.
4. **Resilient Network Edge**: Single-flight leader-follower request coalescing (prevents thundering herd on cache misses), HMAC-SHA256 URL signing with expiration (`exp`), and strict SSRF defenses (blocking RFC 1918, link-local metadata 169.254.169.254, and DNS rebinding).
5. **Sub-millisecond Warm Cache**: Embedded concurrent in-memory LRU cache (`moka`) with byte-weighted memory enforcement.

### 1.4 Intentional Architectural Trade-Offs
| Design Choice | Benefit | Intentional Trade-Off |
| :--- | :--- | :--- |
| **Pure-Rust First-Party Code (`#![forbid(unsafe_code)]`)** | Eliminates C codec zero-days, simplifies cross-compilation, zero system C image libraries. | Cannot decode HEIC natively. AVIF is output-only. WebP output is limited to lossless VP8L (larger output bytes than C `libwebp` lossy). |
| **Lossless-Only WebP** | No unsafe C `libwebp` dependency; maximum fidelity for screenshots and logos. | Cannot produce lossy WebP (e.g. 30 KB photographic WebP); AVIF is the primary lossy format. |
| **Embedded In-Memory Cache (`moka`)** | Zero Redis/Memcached dependency; sub-millisecond local RAM response times (p50: 0.18 ms). | Cache is local to each instance node; scaling across multiple instances requires consistent hashing or a shared reverse proxy (e.g., NGINX/Envoy/Cloudflare). |
| **Fail-Closed Aspect Containment** | `fit=contain` preserves exact image aspect ratio without crop distortion. | Rendered canvas dimensions (e.g., 450×600) differ from requested bounding box (800×600) unless padded. |
| **No Original Byte Leakage** | Never returns raw origin bytes to prevent leaking un-sanitized camera EXIF, GPS, or malformed bytes. | When transformed output is larger than source, Atihsrah serves the transformed output anyway, recording `atihsrah_output_larger_than_source_total`. |

### 1.5 Measurable Acceptance Criteria
- **Observable Reliability**: 0 panics across all dynamically discovered cargo unit/integration tests (currently 135 passing tests) and security attack suites; bounded execution time (max 15s) and bounded memory (max 25 MB input, max 40 MP decoded raster).
- **Deterministic Error Handling**: Tested and predictable HTTP error status codes (400, 403, 404, 415, 422, 502) with structured JSON error bodies.
- **Quality Gates**:
  - Photos: `SSIM ≥ 0.95` against canonical Pillow Lanczos3 reference.
  - Graphics / Diagrams / UI / Text / Alpha: `SSIM ≥ 0.98` against canonical Pillow Lanczos3 reference.
- **Latency Gates**:
  - Warm Cache (p95): `< 0.8 ms` on local host (Apple Silicon / native Linux).
  - Cold Transform (AVIF 800×600, speed 6): `< 500 ms` p95.
  - *Production Scope & Hardware Qualification*: Local Apple Silicon macOS benchmarks validate developer ergonomics, memory bounds, single-flight coalescing, and relative codec efficiency under identical hardware. However, local macOS runs do not certify Linux production latency SLAs or multi-tenant throughput targets; official production SLA verification requires benchmarking in target Linux environments (e.g. Ubuntu LTS on AMD64/Graviton).

---

## 2. Decision Gate: Answers to the 37 Strategic Roadmap Questions

### Product and Users
1. **Operating Tier**: Atihsrah is a **production-facing, self-hosted proxy** designed to sit behind a public reverse proxy/CDN or directly serve as an internal image resizing microservice.
2. **First Real User**: Self-hosting developers building media-heavy web apps (blogs, marketplaces, galleries) who want image optimization without enterprise CDN lock-in or fragile C-based microservices.
3. **Core Prioritization**: **Memory safety & operational simplicity > Bandwidth savings > Latency > Feature sprawl**.
4. **Project Intent**: A production-grade, open-source infrastructure tool designed to ship and be maintained as an independent proxy.
5. **Minimum Viable Success (v1.0)**: Correct, stable, and secure on-the-fly resizing and AVIF/JPEG/WebP/PNG delivery across standard web workloads with comprehensive Prometheus observability and reproducible local workflows.

### Input and Output Support
6. **HEIC Stance**: **Atihsrah will NOT natively decode HEIC in v1.0**. Upstream conversion (or dedicated sidecar transcode) remains the documented architectural path.
7. **Pure-Rust Invariant vs. `libheif`**: **Pure Rust is a hard invariant**. We will not compromise `#![forbid(unsafe_code)]` or introduce C dependencies for HEIC. When a pure-safe-Rust HEVC intra crate matures on crates.io, it will be evaluated.
8. **Required Input Formats (v1.0)**: JPEG, PNG, WebP (lossy & lossless), static GIF. (AVIF is output-only).
9. **Required Output Formats (v1.0)**: AVIF (lossy), WebP (lossless VP8L), JPEG (lossy), PNG (lossless).
10. **Handling of `q` on Lossless WebP**: The service will **accept `q` without error, but document clearly that pure-Rust WebP encoding is lossless VP8L**. Future releases may emit an informative response header (`X-Atihsrah-Warning: webp-lossless-encoder-ignores-q`).
11. **Canvas Behavior for `fit=contain`**: Retain **aspect-preserving dimensional scaling** (e.g., returning a 450×600 image rather than adding artificial transparent letterbox padding). Add optional parameter `pad=1&bg=hex` in v1.1 for explicit padding when requested.
12. **Metadata & Orientation Handling**: **Normalize and strip**. EXIF orientation must always be applied to pixel orientation on decode (as implemented via `dyn_img.apply_orientation()`), and private camera metadata/GPS tags must be stripped by default to protect user privacy.

### Quality and Compression
13. **Quality Thresholds**:
    - Photographic content: Hard gate **`SSIM ≥ 0.95`**; informational PSNR target **> 38 dB**.
    - Graphics / Diagrams / Typography / UI / Alpha: Hard gate **`SSIM ≥ 0.98`**; informational PSNR target **> 45 dB**.
14. **Quality vs. Size**: **Quality is a hard constraint**. Delivering an unusable or artifact-laden image with 99.9% savings is considered a failure.
15. **Output Larger Than Source**: Enforce the **no-source-leakage** rule. When transformed candidates exceed source bytes (e.g. re-encoding high-compression small JPEGs into uncompressed WebP), serve the smallest transformed candidate and increment `atihsrah_output_larger_than_source_total`.
16. **Original Bytes Fallback**: **Disallowed**. Raw un-sanitized source bytes are never returned to ensure metadata stripping and format guarantees.
17. **Guiding Metric**: SSIM (Wang et al. 2004, 11x11 Gaussian window) as the binding gate, alongside informational PSNR.
18. **Authoritative Ground Truth**: The **canonical reference is Pillow Lanczos3 (`Image.Resampling.LANCZOS`) applied to the decoded 24-bit RGB/RGBA source canvas at target dimensions**.
19. **Corpus Split (Calibration vs Held-Out Evaluation)**:
    - **Calibration Corpus (Real-World Standard)**: The 20 real-world assets in `bench/corpus_manifest.csv` (15 photos, 2 UI screenshots, 2 alpha logos, 1 flat graphic) with provenance from Kodak and Wikimedia Commons. This suite evaluates real-world photographic rate-distortion performance.
    - **Held-Out Evaluation Corpus (Synthetic Stress Suite)**: The 20 procedurally generated stress assets in `benchmarks/corpus_synthetic/` (smooth gradients, typography, noise grain, 4K large, checkerboard, HDR high-contrast, low-contrast fog, transparent shadow).
    - *Synthetic Stress Suite Role Disclosure*: The synthetic stress suite functions strictly as an adversarial edge-case stress test (exercising encoder memory bounds, high-frequency boundary clipping, gradient banding, and alpha edge handling). **Procedural stress assets do not measure real-world photographic generalization**, and synthetic compression ratios must never be cited to claim general web optimization superiority. Tuning decisions use the calibration set only.

### Performance and Deployment
20. **Target Volume**: 100 to 500 concurrent requests per node on 4-vCPU/8GB cloud instances (or Apple Silicon M-series developer machines).
21. **Primary Latency Priority**: **Warm cache hit latency (< 0.8 ms p95)** and **single-flight coalescing** (ensuring 100 concurrent requests for an uncached asset execute only 1 cold transcode).
22. **Latency Targets**:
    - Warm Cache: p50 < 0.3 ms, p95 < 0.8 ms, p99 < 2.0 ms.
    - Cold Transform (AVIF 800×600, speed 6 default): p50 < 300 ms, p95 < 500 ms.
    - Cold Transform (AVIF 800×600, speed 9 turbo): p50 < 180 ms, p95 < 300 ms.
    - Cold Transform (JPEG 800×600): p50 < 80 ms, p95 < 150 ms.
23. **Cache Sizing & Profiles**:
    - **Production Default Profile**: `CACHE_MAX_SIZE_MB=500`, `CACHE_SINGLE_ENTRY_MAX_MB=10`, `ATIHSRAH_AVIF_SPEED=6`.
    - **Benchmark Throughput Profile**: `CACHE_MAX_SIZE_MB=500`, `CACHE_SINGLE_ENTRY_MAX_MB=25`, `ATIHSRAH_AVIF_SPEED=9`.
24. **Deployment Scope**: Dual-target support:
    - **Local Developer Workflow**: Native macOS (Apple Silicon & Intel).
    - **Production Cloud Workflow**: Minimal Linux container (`distroless/cc-debian12`, AMD64 & ARM64), dynamically linked only to system glibc.
25. **Container & Platform Evaluation**:
    - Acknowledge that Docker Desktop on macOS introduces virtualization overhead; Colima/Docker measurements are isolated in dedicated columns.
    - *Hardware Certification Boundary*: Local macOS Apple Silicon executions provide developer confidence and relative codec comparisons, but **do not certify Linux production latency SLAs**. Operators must validate SLA targets on their production Linux cloud infrastructure.

### Security and Operations
26. **Allowed Origins**: Whitelist enforcement via `ALLOWED_ORIGINS` (comma-separated origins). Arbitrary URLs are strictly rejected unless explicitly configured in open-development mode.
27. **URL Signing**: **Mandatory in production mode** (`ATIHSRAH_AUTH_MODE=signed`). Secret key passed via `ATIHSRAH_SIGNING_KEY` (32-byte hex). Unsigned requests fail with HTTP 403 Forbidden.
28. **Network Edge Security**: Fail closed on private IPs (RFC 1918, link-local, loopback) unless specified in `ATIHSRAH_PRIVATE_ORIGINS`. Disallow HTTP redirects to prevent SSRF hopping.
29. **Resource Caps**:
    - Max input payload: 25 MB (`MAX_IMAGE_SIZE_BYTES`).
    - Max decoded pixel count: 40,000,000 pixels (~40 MP) to prevent decompression bombs.
    - Processing timeout: 15 seconds per transform.
30. **Telemetry & Logs**: Structured JSON logging (`tracing`). Prometheus metrics strictly tracking operational health: request counts, cache hits/misses, duration histograms with explicit buckets, queue depth, and security rejection counters (`atihsrah_source_rejected_total{reason}`). Never log raw query signatures or private URLs.
31. **Operational Readiness**: Graceful shutdown on SIGTERM/SIGINT, health endpoint (`/healthz`), readiness probe, and pre-registered Prometheus zero-state counters.

### Benchmark and Success Criteria
32. **Relevant Competitors**:
    - **imgproxy**: Industry standard in Go/C (`libvips`).
    - **Thumbor**: Mature Python baseline.
33. **Evaluation Purpose**: **Empirical characterization, not marketing claims**. Discover exactly where pure-Rust wins (memory safety, AVIF speed, cache efficiency) and where it loses (lossless-only WebP file sizes, lack of native HEIC).
34. **Compression Win Rule**: A candidate output only "wins" if it achieves a smaller byte payload than the comparison baseline **AND** meets the predeclared quality threshold (`SSIM ≥ 0.95` for photos, `SSIM ≥ 0.98` for graphics/UI/alpha).
35. **Sample Size**:
    - Cold Latency: Minimum $N=5$ repeated runs per scenario (report median and spread).
    - Warm Cache Latency: Minimum $N=100$ repeated runs (report p50, p90, p95, p99).
36. **Project Success Threshold**: Complete functional correctness, verifiable memory safety posture, and sub-millisecond warm caching. Slower cold encoding (e.g. pure-Rust AVIF vs optimized assembly) is an acceptable trade-off for zero-C dependency safety.
37. **Public Claims Standard**: Every claim must link to a committed verification script, reproducible Docker compose configuration, and immutable raw output dataset. Single-image tests must never be extrapolated to market-wide claims.
