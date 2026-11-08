# Atihsrah vs imgproxy vs Thumbor: Empirical Benchmark Analysis

> [!NOTE]
> **REPRODUCIBLE MACOS HARDWARE MEASUREMENTS**:
> All figures below were generated directly on Apple Silicon macOS hardware via the automated benchmark suite (`bench/run_all.sh`).
> Every number reflects reproducible command output recorded in `raw_results.json` and `results.csv`.
> Results are partitioned by execution tier (Native vs Docker) and target format (Matched AVIF, Matched WebP, Auto Negotiation).

---

## 1. Hardware & Environment Specifications

- **Evaluation Date**: 2026-11-08 15:28:30 UTC
- **Hardware Platform**: Mac16,10 (Apple M4, 10 logical cores, 16.0 GB Unified Memory, `arm64`)
- **Operating System**: macOS 27.2 (Build 26B123)
- **Kernel**: `Darwin Mac-mini.local 27.2.0 Darwin Kernel Version 27.2.0: Wed Oct 14 23:47:07 PDT 2026; root:xnu-13432.41.4~25/RELEASE_ARM64_T8132 arm64`
- **Compiler / Toolchain**: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, Python 3.12.14, `Docker version 29.9.2, build a82e528901` (`colima`)
- **Git Commit**: `b507d0b6` (Full: `b507d0b6330ab5701cee730c7c9cb75763e50fd6`, Dirty: `False`)
- **Active Profile**: `Production Default (speed=6, max_entry=10MB)`
- **Competitors Tested**:
  - **Atihsrah Native**: `v0.1.0` (pure-Rust native binary `target/release/atihsrah`)
  - **Atihsrah Docker**: `v0.1.0` (`atihsrah:latest` inside Colima Linux VM)
  - **imgproxy OSS**: `4.0.21` (`darthsim/imgproxy@sha256:4dfe68cc26ea5cf010318d03818d4cd30f71fd33fa7e149c4be19adafc20c52e`)
  - **imgproxy + Nginx**: `4.0.21 + nginx/alpine` (`nginx@sha256:713fc2ebfd92e8217db451477182ee91944b123d2b1ee9fd5a8c6e91ca6c3a11`)
  - **Thumbor Native**: `7.8.0` (Native macOS via Python 3.12 / uv)

---

## 2. Comparative Matrix: Native & Containerized Performance

| Scenario | Metric | Atihsrah Native | Atihsrah Docker | imgproxy OSS | imgproxy + Nginx | Thumbor Native | Winner | Notes |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :--- |
| **Cold Request (JPEG)** | Median ± Spread (N=5) | 52.9 ± 17.3 ms | 55.4 ± 26.3 ms | 23.2 ± 26.4 ms | N/A | 23.1 ± 12.3 ms | Thumbor Native | Cold decode/encode |
| **Cold Request (PNG)** | Median ± Spread (N=5) | 9.2 ± 2.7 ms | 10.4 ± 2.2 ms | 33.2 ± 4.6 ms | N/A | 27.7 ± 13.5 ms | Atihsrah Native | Lossless encoding |
| **Cold Request (WebP)** | Median ± Spread (N=5) | 10.5 ± 4.0 ms | 10.2 ± 2.1 ms | 52.3 ± 15.8 ms | N/A | 40.2 ± 26.0 ms | Atihsrah Native | Pure Rust vs libwebp |
| **Cold Request (AVIF)** | Median ± Spread (N=5) | 139.2 ± 14.0 ms | 308.6 ± 41.4 ms | 44.5 ± 15.9 ms | N/A | 59.4 ± 18.0 ms | imgproxy | ravif vs libheif |
| **Warm Cache (p50)** | Median Latency | 0.32 ms | 1.17 ms | 20.38 ms | 0.67 ms | 15.23 ms | **Atihsrah Native** | In-process Moka LRU cache |
| **Warm Cache (p90)** | 90th Percentile | 0.38 ms | 1.89 ms | 22.54 ms | 0.77 ms | 16.49 ms | **Atihsrah Native** | Tail latency (N=100) |
| **Warm Cache (p95)** | 95th Percentile | 0.41 ms | 2.32 ms | 24.92 ms | 0.79 ms | 16.91 ms | **Atihsrah Native** | Sub-millisecond memory hits |
| **Warm Cache (p99)** | 99th Percentile | 0.6 ms | 2.54 ms | 27.53 ms | 1.03 ms | 20.55 ms | **Atihsrah Native** | Zero disk / reverse-proxy hop |
| **Warm Cache (Mean ± Std)** | Distribution | 0.33 ± 0.05 ms | 1.31 ± 0.37 ms | 20.73 ± 1.84 ms | 0.69 ± 0.06 ms | 15.58 ± 0.84 ms | **Atihsrah Native** | Full distribution (N=100) |
| **100 Concurrent (Same URL)** | Origin Fetches | **1 req** | N/A | 100 req | N/A | N/A | **Atihsrah** | Single-flight request coalescing |
| **100 Concurrent (Same URL)** | Total Duration | 36.2 ms | N/A | 1714.7 ms | N/A | N/A | **Atihsrah** | Thundering-herd protection |
| **100 Concurrent (Diff URLs)**| Throughput | 96.1 req/s | 201.3 req/s | 103.6 req/s | N/A | N/A | imgproxy | Multi-worker parallel load |
| **100 Concurrent (Diff URLs)**| Tail Latency p95 | 33.7 ms | 70.5 ms | 118.9 ms | N/A | N/A | Atihsrah Native | Tail latency under saturation |
| **Slow Origin (2.0s delay)** | Elapsed Duration | 2.01 s | N/A | 2.02 s | N/A | N/A | Tie | Non-blocking async fetch |
| **Failing Origin (HTTP 500)** | Status Code | HTTP 502 | N/A | HTTP 502 | N/A | N/A | **Atihsrah** | Maps to 502 Bad Gateway |
| **Sustained RSS Memory** | Start -> End RSS | 125.94 -> 84.77 MB | 183.1 -> 182.8 MB | 112.9 -> 99.94 MB | N/A | 73.56 -> 68.84 MB | N/A | Observed RSS stability (10 cached assets; does not verify 500MB LRU limit) |

---

## 3. Matched Format Calibration Corpus Analysis (20 Real Benchmark Images)

Target transformation: `w=800, h=600, fit=cover`.
Visual quality metrics computed against ground truth Pillow `Image.Resampling.LANCZOS` reference using standard 11x11 Gaussian local-window SSIM (Wang et al. 2004) and exact PSNR.

### 3.1 Matched AVIF Compression & Quality (Nominal q=90 Setting Comparison)

- **Total Calibration Corpus Images Evaluated**: 20
- **Quality Gate Threshold**: Category-specific: SSIM $\ge 0.95$ for photos, SSIM $\ge 0.98$ for graphics/UI/alpha
- **Matched Pairs Meeting Quality Threshold**: 2 pairs
- **Disqualified Pairs Failing Quality Gate**: 18 pairs
- **Quality Failures**:
  - Atihsrah Quality Gate Failures: 1 (img_18: SSIM 0.9799 < 0.98)
  - imgproxy Quality Gate Failures: 17 (img_01: SSIM 0.8372 < 0.95, img_02: SSIM 0.8985 < 0.95, img_03: SSIM 0.9427 < 0.95, img_04: SSIM 0.9375 < 0.95, img_05: SSIM 0.8669 < 0.95, img_06: SSIM 0.9343 < 0.95, img_07: SSIM 0.8206 < 0.95, img_08: SSIM 0.7923 < 0.95, img_09: SSIM 0.8644 < 0.95, img_10: SSIM 0.9006 < 0.95, img_11: SSIM 0.9311 < 0.95, img_12: SSIM 0.9195 < 0.95, img_13: SSIM 0.9175 < 0.95, img_14: SSIM 0.9408 < 0.95, img_16: SSIM 0.9347 < 0.98, img_17: SSIM 0.9533 < 0.98, img_20: SSIM 0.8923 < 0.98)
- **Quality-Gated Wins (Denominator = 2 eligible pairs)**:
  - Atihsrah Wins: 2
  - imgproxy Wins: 0
  - Ties (|Δ| ≤ 1.0%): 0
- **Median Byte Delta (Eligible Pairs)**: -21.2%
- **Mean Byte Delta (Eligible Pairs)**: -33.9%
- **Average SSIM (All Samples)**: Atihsrah 0.9807 vs imgproxy 0.9110
- **Optimization Hypothesis Verdict**: **NOT SUPPORTED / QUALITY GATE FAILURE**: Atihsrah failed the minimum SSIM quality gate on 1 asset(s): img_18 (python_logo_alpha.png: SSIM 0.9799 < 0.98). Compression wins cannot be certified when visual quality gates are breached.
- **Encoder Scale Disclosure**: Comparing nominal q=90 is a configuration comparison, not an equivalent perceptual quality match. Upstream encoders (`ravif` vs `libheif`/`libaom`) have distinct rate-distortion curves.

### 3.2 Matched WebP Analysis (Capability & Encoding Trade-off)

- **Atihsrah WebP**: Pure-Rust `image-rs` lossless encoding (Average SSIM: 0.9980)
- **imgproxy & Thumbor WebP**: `libwebp` / Pillow lossy encoding at q=75 (Average SSIM: 0.8874)
- **WebP File Size Comparison**: imgproxy delivers smaller files on **20 of 20 images** (delta range -80% to -96% vs Atihsrah lossless WebP).
- *Capability Disclosure*: Under Option A (pure Rust), Atihsrah generates lossless WebP for graphics and alpha assets. Because it does not link the C `libwebp` library, it cannot produce lossy WebP at q=75. Consequently, comparing Atihsrah lossless WebP directly to lossy WebP reflects an encoding mode trade-off (lossless visual fidelity vs lossy compression).

---

## 4. Per-Image Results: 20-Image Real-World Calibration Corpus

| ID | Filename | Category | SSIM Gate | Orig Bytes | Atihsrah AVIF Bytes | Atihsrah AVIF SSIM | Atihsrah AVIF PSNR | imgproxy AVIF Bytes | imgproxy AVIF SSIM | imgproxy AVIF PSNR | AVIF Byte Δ (%) | Quality Status | Atihsrah WebP (Lossless) | imgproxy WebP (q75) |
| :--- | :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| img_01 | kodim01.png | photo | 0.95 | 736501 | 125449 | 0.9838 | 39.95 | 166705 | 0.8372 | 26.16 | -24.7% | At:PASS / img:**FAIL** | 724544 (lossless) | 75868 (q75) |
| img_02 | kodim02.png | photo | 0.95 | 617995 | 90031 | 0.9683 | 41.48 | 129005 | 0.8985 | 32.56 | -30.2% | At:PASS / img:**FAIL** | 633064 (lossless) | 34898 (q75) |
| img_03 | kodim03.png | photo | 0.95 | 502888 | 51233 | 0.9763 | 42.87 | 87022 | 0.9427 | 34.31 | -41.1% | At:PASS / img:**FAIL** | 574162 (lossless) | 24086 (q75) |
| img_04 | kodim04.png | photo | 0.95 | 637432 | 58350 | 0.9775 | 43.4 | 94178 | 0.9375 | 35.89 | -38.0% | At:PASS / img:**FAIL** | 538996 (lossless) | 25776 (q75) |
| img_05 | kodim05.png | photo | 0.95 | 785610 | 131351 | 0.9842 | 39.51 | 171843 | 0.8669 | 25.85 | -23.6% | At:PASS / img:**FAIL** | 793316 (lossless) | 82502 (q75) |
| img_06 | kodim07.png | photo | 0.95 | 566322 | 60205 | 0.9800 | 42.2 | 94587 | 0.9343 | 31.0 | -36.3% | At:PASS / img:**FAIL** | 647450 (lossless) | 32544 (q75) |
| img_07 | kodim08.png | photo | 0.95 | 788470 | 133006 | 0.9831 | 39.02 | 170838 | 0.8206 | 22.81 | -22.1% | At:PASS / img:**FAIL** | 756508 (lossless) | 84050 (q75) |
| img_08 | kodim13.png | photo | 0.95 | 822712 | 164655 | 0.9839 | 38.62 | 213191 | 0.7923 | 23.94 | -22.8% | At:PASS / img:**FAIL** | 811068 (lossless) | 106608 (q75) |
| img_09 | kodim14.png | photo | 0.95 | 692201 | 123626 | 0.9790 | 40.15 | 165403 | 0.8644 | 28.05 | -25.3% | At:PASS / img:**FAIL** | 744284 (lossless) | 67872 (q75) |
| img_10 | kodim15.png | photo | 0.95 | 612582 | 77857 | 0.9721 | 41.78 | 117619 | 0.9006 | 31.31 | -33.8% | At:PASS / img:**FAIL** | 617990 (lossless) | 34142 (q75) |
| img_11 | kodim17.png | photo | 0.95 | 602078 | 68721 | 0.9794 | 42.51 | 106398 | 0.9311 | 32.96 | -35.4% | At:PASS / img:**FAIL** | 585220 (lossless) | 35130 (q75) |
| img_12 | kodim19.png | photo | 0.95 | 671476 | 68968 | 0.9804 | 41.8 | 105522 | 0.9195 | 28.16 | -34.6% | At:PASS / img:**FAIL** | 619426 (lossless) | 38606 (q75) |
| img_13 | kodim20.png | photo | 0.95 | 492462 | 59747 | 0.9718 | 41.7 | 87140 | 0.9175 | 29.96 | -31.4% | At:PASS / img:**FAIL** | 555142 (lossless) | 29850 (q75) |
| img_14 | kodim23.png | photo | 0.95 | 557596 | 57220 | 0.9696 | 42.36 | 97705 | 0.9408 | 32.75 | -41.4% | At:PASS / img:**FAIL** | 597764 (lossless) | 25472 (q75) |
| img_15 | moraine_lake.jpg | photo | 0.95 | 1520767 | 134470 | 0.9625 | 38.66 | 170574 | 0.9652 | 38.51 | -21.2% | At:PASS / img:PASS | 845412 (lossless) | 62652 (q75) |
| img_16 | kde_spectacle.png | ui_screenshot | 0.98 | 83699 | 44810 | 0.9964 | 44.5 | 52748 | 0.9347 | 27.35 | -15.0% | At:PASS / img:**FAIL** | 158446 (lossless) | 22054 (q75) |
| img_17 | training_module.png | ui_screenshot | 0.98 | 107821 | 45668 | 0.9971 | 44.2 | 53650 | 0.9533 | 27.75 | -14.9% | At:PASS / img:**FAIL** | 134476 (lossless) | 19514 (q75) |
| img_18 | python_logo_alpha.png | alpha | 0.98 | 15770 | 13950 | 0.9799 | 41.43 | 22764 | 0.9897 | 45.37 | -38.7% | At:**FAIL** / img:PASS | 54606 (lossless) | 19156 (q75) |
| img_19 | alacritty_logo_alpha.png | alpha | 0.98 | 3612 | 10477 | 0.9931 | 49.31 | 19633 | 0.9800 | 39.06 | -46.6% | At:PASS / img:PASS | 163650 (lossless) | 7494 (q75) |
| img_20 | world_map_graphic.png | graphic | 0.98 | 87924 | 60061 | 0.9963 | 47.57 | 51768 | 0.8923 | 19.2 | +16.0% | At:PASS / img:**FAIL** | 82416 (lossless) | 65118 (q75) |

---

## 5. Held-Out Synthetic Stress Suite (20 Procedural Test Assets)

> [!WARNING]
> **SYNTHETIC STRESS SUITE DISCLOSURE**:
> The 20 synthetic stress assets below evaluate procedural edge cases (gradients, high-contrast text, 4K resolution, checkerboards, alpha drop-shadows).
> **Procedural stress assets do not measure real-world photographic generalization.** Results on synthetic patterns must not be extrapolated to claim real-world web delivery superiority.

| ID | Filename | Category | SSIM Gate | Orig Bytes | Atihsrah AVIF Bytes | Atihsrah AVIF SSIM | Atihsrah AVIF PSNR | imgproxy AVIF Bytes | imgproxy AVIF SSIM | imgproxy AVIF PSNR | AVIF Byte Δ (%) | Quality Status | Atihsrah WebP (Lossless) | imgproxy WebP (q75) |
| :--- | :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| synth_01 | 01_landscape.png | photo | 0.95 | 1206358 | 14702 | 0.9849 | 46.76 | 25439 | 0.9497 | 41.2 | -42.2% | At:PASS / img:**FAIL** | 453734 (lossless) | 3888 (q75) |
| synth_02 | 02_portrait.jpg | photo | 0.95 | 66761 | 6853 | 0.9944 | 50.0 | 14517 | 0.9924 | 46.71 | -52.8% | At:PASS / img:PASS | 50018 (lossless) | 3904 (q75) |
| synth_03 | 03_macro_texture.png | photo | 0.95 | 3072094 | 28478 | 0.9978 | 45.17 | 77834 | 0.9978 | 45.73 | -63.4% | At:PASS / img:PASS | 534942 (lossless) | 17844 (q75) |
| synth_04 | 04_night_skyline.jpg | photo | 0.95 | 237866 | 80347 | 0.9926 | 43.57 | 123967 | 0.7972 | 22.55 | -35.2% | At:PASS / img:**FAIL** | 252096 (lossless) | 37754 (q75) |
| synth_05 | 05_architecture.png | photo | 0.95 | 2035919 | 93428 | 0.9948 | 40.0 | 105152 | 0.9948 | 39.97 | -11.1% | At:PASS / img:PASS | 348984 (lossless) | 59972 (q75) |
| synth_06 | 06_vector_flat.png | vector_flat | 0.98 | 32167 | 9069 | 0.9991 | 49.99 | 10403 | 0.9983 | 50.35 | -12.8% | At:PASS / img:PASS | 28926 (lossless) | 6180 (q75) |
| synth_07 | 07_smooth_gradient.png | smooth_gradient | 0.98 | 364394 | 6013 | 0.9931 | 50.23 | 13455 | 0.9927 | 49.92 | -55.3% | At:PASS / img:PASS | 112624 (lossless) | 4450 (q75) |
| synth_08 | 08_typography_poster.png | typography_poster | 0.98 | 222246 | 26968 | 0.9962 | 47.27 | 34347 | 0.9170 | 25.05 | -21.5% | At:PASS / img:**FAIL** | 68804 (lossless) | 14164 (q75) |
| synth_09 | 09_ui_desktop.png | ui_desktop | 0.98 | 33439 | 3340 | 0.9975 | 49.39 | 4125 | 0.9967 | 49.34 | -19.0% | At:PASS / img:PASS | 1160 (lossless) | 1944 (q75) |
| synth_10 | 10_ui_mobile.png | ui_mobile | 0.98 | 18535 | 1383 | 0.9996 | 55.35 | 1564 | 0.9971 | 51.09 | -11.6% | At:PASS / img:PASS | 556 (lossless) | 1554 (q75) |
| synth_11 | 11_document_text.png | document_text | 0.98 | 4725797 | 87337 | 0.9987 | 43.01 | 83638 | 0.7992 | 16.03 |  +4.4% | At:PASS / img:**FAIL** | 200876 (lossless) | 52626 (q75) |
| synth_12 | 12_transparent_icon.png | transparent_icon | 0.98 | 13040 | 11511 | 0.9952 | 45.04 | 14247 | 0.9965 | 39.49 | -19.2% | At:PASS / img:PASS | 32340 (lossless) | 10972 (q75) |
| synth_13 | 13_transparent_shadow.png | transparent_shadow | 0.98 | 24390 | 2773 | 0.9995 | 60.69 | 2161 | 0.9992 | 59.01 | +28.3% | At:PASS / img:PASS | 2778 (lossless) | 3070 (q75) |
| synth_14 | 14_hdr_highcontrast.jpg | hdr_highcontrast | 0.95 | 60780 | 2298 | 0.9981 | 57.67 | 5290 | 0.9982 | 51.7 | -56.6% | At:PASS / img:PASS | 16372 (lossless) | 1880 (q75) |
| synth_15 | 15_lowcontrast_fog.jpg | lowcontrast_fog | 0.95 | 54118 | 2768 | 0.9988 | 57.03 | 4213 | 0.9984 | 56.86 | -34.3% | At:PASS / img:PASS | 5944 (lossless) | 2058 (q75) |
| synth_16 | 16_pattern_checkerboard.png | pattern_checkerboard | 0.98 | 42529 | 114611 | 0.9988 | 42.85 | 86481 | 0.9993 | 46.59 | +32.5% | At:PASS / img:PASS | 49312 (lossless) | 53150 (q75) |
| synth_17 | 17_noise_grain.png | noise_grain | 0.95 | 2203506 | 124231 | 0.9856 | 41.22 | 161609 | 0.5029 | 25.4 | -23.1% | At:PASS / img:**FAIL** | 270374 (lossless) | 73676 (q75) |
| synth_18 | 18_photo_4k_large.jpg | photo_4k_large | 0.95 | 364042 | 15106 | 0.9905 | 47.97 | 34130 | 0.9885 | 44.71 | -55.7% | At:PASS / img:PASS | 317710 (lossless) | 12642 (q75) |
| synth_19 | 19_photo_medium.jpg | photo | 0.95 | 36512 | 8456 | 0.9889 | 48.15 | 16049 | 0.9906 | 49.24 | -47.3% | At:PASS / img:PASS | 42920 (lossless) | 4434 (q75) |
| synth_20 | 20_thumbnail_small.png | thumbnail_small | 0.95 | 129403 | 33651 | 0.9760 | 43.78 | 56351 | 0.9580 | 34.53 | -40.3% | At:PASS / img:PASS | 327140 (lossless) | 9200 (q75) |

---

## 6. Operational Scope & Production Qualification

> [!NOTE]
> **LOCAL MACOS BENCHMARK DISCLOSURE & PRODUCTION QUALIFICATION**:
> All benchmarks in this report were executed locally on Apple Silicon macOS hardware (Mac16,10).
> Local execution validates developer ergonomics, cache hit concurrency, single-flight request coalescing, and relative codec compression efficiency under identical CPU architectures.
> **However, local macOS results do not certify Linux production latency SLAs or multi-tenant production stability.**
> Production deployment requires testing under target Linux server environments (e.g. Ubuntu LTS on AMD64/Graviton) with production network topographies.
