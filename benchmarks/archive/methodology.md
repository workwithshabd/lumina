> [!WARNING]
> **SUPERSEDED AND UNRELIABLE**: These results were generated using a synthetic procedural corpus on a local developer Mac under Colima (2 vCPUs) with misaligned quality metrics (SSIM 0.05-0.42 and VMAF 0.0) and an unfair cache comparison (in-process cache vs. no cache).
> See `bench/RUNME.md` and `bench/analysis_TEMPLATE.md` for the reproducible, fair benchmark suite.

# Atihsrah Benchmark Methodology & Reproduction Guide

This document describes the environment specifications, container configurations, measurement protocols, and test corpus composition utilized in the empirical evaluation of **Atihsrah** (v0.1.0) versus **imgproxy** (v4.0.21).

---

## 1. Test Environment & Deployment Hardware

All benchmarks were executed on real deployment hardware using containerized workloads under identical host resource allocations.

| Specification | Hardware & Environment Details |
| :--- | :--- |
| **Host System** | Apple Silicon M4 (10 cores: 4 performance + 6 efficiency) |
| **Host Operating System** | Darwin Kernel Version 27.2.0 (macOS 15.x ARM64) |
| **Host Physical Memory** | 16.0 GB Unified Memory |
| **Container Engine** | Docker Engine v29.9.2 (Colima virtualization layer: 2 vCPUs, 2 GB RAM allocated) |
| **Compiler & Runtimes** | Rustc 1.97.1 (stable), Python 3.12 (Alpine container) & Python 3.14 (host runner) |
| **Quality Analysis Tools**| FFmpeg 9.0.2 (libvmaf v3.0.0 model, ssim video filter) |

---

## 2. Tested Services & Container Configurations

Both services were deployed on an isolated bridge network (`bench_default`) via Docker Compose.

### Atihsrah (System Under Test)
- **Container Image**: `atihsrah:latest` (Scratch-based Distroless Debian 12 rootfs)
- **Image Content Size**: **13.9 MB** (0 findings on Trivy (scope: 10 Debian packages) security scan)
- **Runtime Security**: Read-only root filesystem (`read_only: true`), `cap_drop: [ALL]`, `no-new-privileges: true`
- **Concurrency & Resource Limits**: `MAX_CONCURRENT_TRANSFORMS=4`, `CACHE_MAX_SIZE_MB=500`, `CACHE_SINGLE_ENTRY_MAX_MB=25`, `MAX_SOURCE_FETCH_MB=25`
- **Authentication**: `ATIHSRAH_AUTH_MODE=signed` (HMAC-SHA256 signature verification)
- **Internal Architecture**: Tokio async runtime, Axum HTTP engine, Moka byte-bounded LRU cache, `tokio::sync::broadcast` single-flight coalescer, `image-rs` + `ravif` transformation engine.

### imgproxy (Baseline Reference)
- **Container Image**: `darthsim/imgproxy:latest` (Release v4.0.21)
- **Image Content Size**: **85.7 MB** (6.1x larger container image)
- **Concurrency & Limits**: `IMGPROXY_CONCURRENCY=4`, `IMGPROXY_DOWNLOAD_TIMEOUT=15`, `IMGPROXY_MAX_SRC_RESOLUTION=33.5`
- **Network Configuration**: `IMGPROXY_ALLOW_LOCAL_ADDRESSES=true`, custom SSL certificate authority mounted for upstream TLS trust.
- **Internal Architecture**: Go runtime, libvips C pipeline, multi-threaded worker pools.

### Mock Origin Server
- **Container Image**: `python:3.12-alpine`
- **Network Protocol**: HTTPS / TLS 1.3 with 2048-bit RSA certificate and Subject Alternative Names (`DNS:origin, DNS:localhost, IP:127.0.0.1`).
- **Endpoints**:
  - `/corpus/<file>`: Direct streaming of static test corpus assets.
  - `/slow/<file>?delay=2.0`: Simulated upstream latency delay.
  - `/fail/<file>`: Deterministic HTTP 500 Internal Server Error simulation.
  - `/stats` & `/stats/reset`: Atomic counter tracking origin fetch count to verify request coalescing.

---

## 3. Benchmark Corpus (20 Representative Images)

The benchmark corpus consists of 20 diverse, programmatically generated test images representing realistic web delivery workloads: photographic textures, gradients, documents, UI layouts, high-frequency noise, transparency, and high dynamic range scenes.

| ID | Filename | Format | Dimensions | Original Size | Workload / Visual Characteristics |
| :--- | :--- | :---: | :---: | :---: | :--- |
| `01` | `01_landscape.png` | PNG | 1920x1080 | 1,206,358 B | Landscape scene with sky gradient, rolling hills, and sun disk. |
| `02` | `02_portrait.jpg` | JPEG | 800x1000 | 66,761 B | Studio portrait simulation with smooth skin gradients and circular vignette. |
| `03` | `03_macro_texture.png`| PNG | 2048x2048 | 3,072,094 B | High-frequency macro texture, sine/cosine grid lines, sharp edge transitions. |
| `04` | `04_night_skyline.jpg` | JPEG | 1920x1080 | 237,866 B | Low-light night skyline with illuminated geometric windows and deep shadows. |
| `05` | `05_architecture.png` | PNG | 1600x1200 | 2,035,919 B | Architectural facade with repeated perspective grid lines and window frames. |
| `06` | `06_vector_flat.png` | PNG | 1200x800 | 32,167 B | Flat vector graphic with primary geometric shapes and solid color fields. |
| `07` | `07_smooth_gradient.png`| PNG | 1920x1080 | 364,394 B | Diagonal 3-stop color blend stress-testing banding and chrominance fidelity. |
| `08` | `08_typography_poster.png`| PNG| 1200x1600 | 222,246 B | Typography poster with high-contrast text lines and vertical dividers. |
| `09` | `09_ui_desktop.png` | PNG | 1440x900 | 33,439 B | SaaS application mockup with dark sidebar, navigation cards, and buttons. |
| `10` | `10_ui_mobile.png` | PNG | 750x1334 | 18,535 B | Mobile app screen with card list, avatar circles, and action bar. |
| `11` | `11_document_text.png`| PNG | 2480x3508 | 4,725,797 B | High-resolution 300 DPI scanned document with dense paragraphs. |
| `12` | `12_transparent_icon.png`| PNG | 512x512 | 13,040 B | Vector app icon with transparent alpha background. |
| `13` | `13_transparent_shadow.png`| PNG| 800x800 | 24,390 B | Card element with Gaussian soft alpha drop-shadow over transparency. |
| `14` | `14_hdr_highcontrast.jpg`| JPEG | 1920x1080 | 60,780 B | High dynamic range scene with saturated light core and dark border. |
| `15` | `15_lowcontrast_fog.jpg` | JPEG | 1920x1080 | 54,118 B | Low-contrast foggy morning scene with narrow luminance histogram. |
| `16` | `16_pattern_checkerboard.png`| PNG | 1024x1024 | 42,529 B | Nyquist frequency limit 8x8 checkerboard pattern testing aliasing. |
| `17` | `17_noise_grain.png` | PNG | 1920x1080 | 2,203,506 B | Synthesized film grain pseudo-random noise testing encoder entropy limits. |
| `18` | `18_photo_4k_large.jpg` | JPEG | 3840x2160 | 364,042 B | 4K UHD landscape photo testing memory allocation during large decode. |
| `19` | `19_photo_medium.jpg` | JPEG | 1200x800 | 36,512 B | Standard medium-sized web hero photo. |
| `20` | `20_thumbnail_small.png`| PNG | 300x200 | 129,403 B | Small thumbnail representation with circular center element. |

---

## 4. Benchmark Scenarios & Execution Protocol

1. **Scenario 1: Cold Single Request Per Image Type**:
   Measures time-to-first-byte (TTFB) and total request duration for uncached requests across four primary web formats: JPEG, PNG, WebP, and AVIF.
2. **Scenario 2: Warm Cache Latency Distribution**:
   Executes 50 sequential requests to a previously primed URL. Samples p50, p90, p95, p99, and mean latency to evaluate in-memory cache hit performance.
3. **Scenario 3: 100 Concurrent Requests for the SAME Uncached URL (Coalescing)**:
   Dispatches 100 simultaneous requests against a single previously uncached URL. Asserts that exactly 1 origin fetch occurs via atomic origin metrics counters, evaluating single-flight collapse mechanics.
4. **Scenario 4: 100 Concurrent DIFFERENT Uncached URLs**:
   Dispatches 100 distinct transformation requests across diverse corpus images and dimension combinations. Measures queue saturation, system throughput (req/s), and latency percentiles.
5. **Scenario 5: Slow Origin Simulation**:
   Routes requests through an origin endpoint with an artificial 2.0s upstream response delay. Validates non-blocking I/O and timeout behavior.
6. **Scenario 6: Failing Origin Simulation**:
   Routes requests through an origin endpoint returning HTTP 500. Validates structured error propagation and RFC 7807 problem details JSON.
7. **Scenario 7: Sustained Load with RSS Memory Sampling**:
   Maintains continuous concurrent transformation load against cached and uncached assets. Samples container RSS memory every interval to verify byte-bounded stability without memory leaks.
8. **Scenario 8: AVIF vs WebP Encode Cost**:
   Performs head-to-head encode benchmarks across five diverse image classes (landscape, night, gradient, UI, grain), measuring encode latency, delivered bytes, and CPU overhead ratios.
9. **Scenario 9: 20-Image Corpus Quality and Compression Benchmark**:
   Transforms all 20 corpus images to standard 800x600 WebP at matched quality settings (q=75 / quality_target=good). Computes Structural Similarity Index (SSIM) and Video Multimethod Assessment Fusion (VMAF) using FFmpeg against original references.

---

## 5. Reproduction Commands

To reproduce the benchmark suite end-to-end on any Darwin or Linux deployment host:

```bash
# 1. Clone repository and navigate to root
cd lumina

# 2. Run master benchmark orchestrator in quick mode (60s sustained load)
./bench/run_all.sh --quick

# 3. Or run the full 30-minute sustained load evaluation
./bench/run_all.sh

# 4. View raw JSON results and CSV quality tables
cat benchmarks/raw_results.json
cat benchmarks/corpus_results.csv
```
