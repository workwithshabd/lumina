# Atihsrah Image Quality, Latency & Metrics Report: IMG_0172

> **Audit Context & Scope**: Evaluates transformation quality, file size reduction, cold transformation latency, and warm cache latency for high-resolution photo `IMG_0172.HEIC` on an Apple Silicon local host. This report validates processing fidelity for this specific asset on this test run and does not claim general superiority or cross-environment comparative performance.

## 1. Input Baselines & Pipeline Boundary Clarification

> [!IMPORTANT]
> **Pipeline Processing Boundary**: Atihsrah processes the HTTP origin source (`IMG_0172.jpg`, 7.29 MB) delivered to it over HTTP. It does **not** natively decode Apple HEIC files. The conversion from raw iPhone HEIC (`1.59 MB`) to origin JPEG (`6.96 MB`) or lossless PNG (`18.17 MB`) occurs upstream at upload time via Apple's native `sips` CLI utility.

| Input Stream | File Path | File Size | Spatial Dimensions | Role in Pipeline |
| :--- | :--- | :---: | :---: | :--- |
| **Raw Original** | `IMG_0172.HEIC` | **1,664,401 B** (1.59 MB) | 4284 × 5712 (24.5 MP) | Original user upload from iPhone |
| **HTTP Origin Source** | `IMG_0172.jpg` | **7,293,138 B** (6.96 MB) | 4284 × 5712 (24.5 MP) | Upstream `sips` transcode fetched and processed by Atihsrah |
| **Lossless Reference** | `IMG_0172_transcoded.png` | **19,055,939 B** (18.17 MB) | 4284 × 5712 (24.5 MP) | Upstream `sips` 24-bit RGB raster |

*Pure-Rust Architectural Reason*: Atihsrah enforces `#![forbid(unsafe_code)]` with zero C dependencies. Because ISO BMFF HEVC intra decoding currently lacks a pure-safe-Rust implementation on crates.io, the upstream mock/dashboard origin performs this step via macOS `sips` so Atihsrah decodes standard JPEG/PNG inputs without C libraries (`libheif`/`libde265`).

## 2. Image Quality, Cold Transformation & Warm Cache Matrix

- **Requested Box vs Rendered Dimensions**: The requested dimensions (e.g. `800×600`) represent an outer bounding box under `fit=contain`. The actual output dimensions (e.g. `450×600`) preserve the portrait 3:4 aspect ratio without cropping.
- **Scope of "Lossless" Encoding**: The terms "lossless WebP" and "lossless PNG" describe the **codec compression stage**, not the end-to-end transformation. Resampling from 24.5 MP down to smaller canvas sizes modifies pixel values; the reported SSIM (0.9989–0.9991) and PSNR (52–54 dB) reflect the combined effect of Catmull-Rom resampling and lossless container encoding.
- **Pure-Rust WebP Encoder**: Under `image-rs 0.25`, `WebPEncoder::new_lossless` operates in lossless VP8L mode. The quality parameter `q` is not used by the encoder, resulting in near-lossless fidelity at larger file sizes.
- **Cold Latency Sample Context**: Cold transformation latencies are single observations ($N=1$) for this image and run, subject to disk I/O and process scheduling variance, whereas warm-cache latencies are derived from a distribution ($N=100$).
- **Verifiable Artifacts**: Output images and corresponding Pillow Lanczos ground-truth reference images are saved to `monitoring/evaluation_IMG_0172/`.

| Test Scenario | Requested Box | Rendered Dimensions | Delivered Size | SSIM | PSNR | Reduction vs Raw HEIC | Reduction vs Origin JPEG | Cold Latency (MISS, N=1) | Warm Latency (HIT) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **AVIF 800x600 (q=65, Balanced)** | `800×600 (fit=contain)` | `450×600 (3:4 preserved)` | `10.2 KB` | **0.9714** | 38.74 dB | **99.37%** | 99.86% | **333.16 ms** | 0.42 ms |
| **AVIF 800x600 (q=85, High Fidelity)** | `800×600 (fit=contain)` | `450×600 (3:4 preserved)` | `17.1 KB` | **0.9816** | 41.69 dB | **98.95%** | 99.76% | **289.02 ms** | 0.73 ms |
| **WebP 800x600 (Lossless VP8L)** | `800×600 (fit=contain)` | `450×600 (3:4 preserved)` | `253.3 KB` | **0.9989** | 54.15 dB | **84.41%** | 96.44% | **253.71 ms** | 0.39 ms |
| **WebP 400x300 (Lossless VP8L Thumbnail)** | `400×300 (fit=contain)` | `225×300 (3:4 preserved)` | `80.0 KB` | **0.9991** | 52.13 dB | **95.08%** | 98.88% | **248.0 ms** | 0.42 ms |
| **JPEG 1200x900 (q=85, High Fidelity)** | `1200×900 (fit=contain)` | `675×900 (3:4 preserved)` | `75.6 KB` | **0.9856** | 43.92 dB | **95.35%** | 98.94% | **259.67 ms** | 0.40 ms |
| **JPEG 800x600 (q=65, Balanced)** | `800×600 (fit=contain)` | `450×600 (3:4 preserved)` | `26.8 KB` | **0.9724** | 39.47 dB | **98.35%** | 99.62% | **256.68 ms** | 0.51 ms |
| **JPEG 200x150 (q=40, Thumbnail)** | `200×150 (fit=contain)` | `113×150 (3:4 preserved)` | `3.1 KB` | **0.9281** | 31.66 dB | **99.81%** | 99.96% | **243.43 ms** | 0.36 ms |
| **PNG 400x300 (Lossless RGB)** | `400×300 (fit=contain)` | `225×300 (3:4 preserved)` | `96.8 KB` | **0.9991** | 52.13 dB | **94.04%** | 98.64% | **242.38 ms** | 0.38 ms |

## 3. Warm Cache Hit Latency Distribution (N=100 requests)

Evaluated across 100 consecutive requests for cached asset `AVIF 800×600` on local hardware:

| Metric | Latency |
| :--- | :--- |
| **Sample Count** | 100 requests |
| **Median (p50)** | **0.18 ms** |
| **p90** | **0.22 ms** |
| **p95** | **0.24 ms** |
| **p99** | **0.38 ms** |
| **Min / Max** | 0.14 ms / 0.53 ms |
| **Mean ± Std Dev** | 0.19 ms ± 0.05 ms |

> *Scope Qualification*: This distribution reflects in-memory warm-cache lookups via the `moka` cache on local hardware. It does not reflect network latency, TLS termination, or distributed edge CDN performance.

## 4. Isolated Test Run vs Cumulative Prometheus Counters

Isolated counter deltas captured during this specific evaluation run:

| Metric | Test Run Delta (Δ) | Cumulative Service Total |
| :--- | :---: | :---: |
| **Cache Hits** | `108` | `286` |
| **Cache Misses** | `8` | `127` |
| **Response Bytes** | `2,198,078 B` | `19,934,481 B` |
| **Cache Hit Bytes** | `1,621,489 B` | `10,529,023 B` |
| **Active Cache Entries** | — | `127` |
| **Active Cache Memory** | — | `9,405,458 B` (8.97 MB) |

## 5. Independent Metric Verification (SSIM & PSNR)

To independently confirm both the SSIM and PSNR calculations without relying on pre-computed values, execute [`monitoring/verify_quality.py`](verify_quality.py):

```bash
./.venv_bench/bin/python monitoring/verify_quality.py
```

### Execution Transcript

```
==========================================================================================
        Independent Quality Verification: SSIM & PSNR for IMG_0172
==========================================================================================
Test Scenario                    | SSIM (Rep / Recomp)    | PSNR (Rep / Recomp)    | Status
------------------------------------------------------------------------------------------
AVIF 800x600 (q=65, Balanced)    | 0.9714 / 0.9714        | 38.74 dB / 38.74 dB    | MATCH (at displayed precision)
AVIF 800x600 (q=85, High Fidelity)| 0.9816 / 0.9816       | 41.69 dB / 41.69 dB    | MATCH (at displayed precision)
WebP 800x600 (Lossless VP8L)     | 0.9989 / 0.9989        | 54.15 dB / 54.15 dB    | MATCH (at displayed precision)
WebP 400x300 (Lossless VP8L)     | 0.9991 / 0.9991        | 52.13 dB / 52.13 dB    | MATCH (at displayed precision)
JPEG 1200x900 (q=85, High Fid.)  | 0.9856 / 0.9856        | 43.92 dB / 43.92 dB    | MATCH (at displayed precision)
JPEG 800x600 (q=65, Balanced)    | 0.9724 / 0.9724        | 39.47 dB / 39.47 dB    | MATCH (at displayed precision)
JPEG 200x150 (q=40, Thumbnail)   | 0.9281 / 0.9281        | 31.66 dB / 31.66 dB    | MATCH (at displayed precision)
PNG 400x300 (Lossless RGB)       | 0.9991 / 0.9991        | 52.13 dB / 52.13 dB    | MATCH (at displayed precision)
==========================================================================================
✓ All 8 scenario outputs match reported SSIM (4 decimals) and PSNR (2 decimals) at displayed precision.
```
