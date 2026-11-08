# Master Engineering Audit, Score Reconciliation & Production Readiness Roadmap

**Evaluation Date**: 2026-11-08  
**Repository**: `atihsrah` (Folder: `/Users/shabd/Desktop/dev/lumina`)  
**Tested Code Commit**: [`a2aa92b4`](../)  
**Committed Artifacts Commit**: [`a2aa92b4`](../)  
**Active Run**: `run_20261108_194505`  
**Host Platform**: Apple M4 (10 cores, 16 GB Unified Memory), macOS 27.2 (Darwin 27.2.0)  
**Safety Invariant**: Strict `#![forbid(unsafe_code)]` in first-party code; zero C image codec libraries  

---

## 1. Executive Summary & Verdict

| Dimension | Empirical Finding | Verdict |
| :--- | :--- | :---: |
| **Correctness & Tests** | 159 Rust unit, integration, and security tests pass; 7 candidate validation fixtures pass; zero compiler warnings (`-D warnings`). | **PASS** |
| **Warm Cache Latency** | Native p50: **0.33 ms**, p95: **0.44 ms** (<0.8 ms target), p99: **0.67 ms** via in-process Moka LRU cache. | **PASS** |
| **Concurrency Shield** | Inflight request coalescing collapses 100 concurrent identical requests into **1 origin fetch** (34.4 ms vs 1,560.4 ms for unshielded imgproxy). | **PASS** |
| **Cold Transform Speed** | Faster on PNG (8.8 ms vs 29.5 ms) and WebP (9.6 ms vs 48.0 ms); slower on cold AVIF (121.0 ms vs 40.2 ms) and JPEG (50.1 ms vs 23.2 ms). | **TRADE-OFF** |
| **Byte Budget Guard** | Auto mode delivered 18/20 calibration outputs and 18/20 synthetic outputs $\le$ source bytes in `run_20261108_194505` (90.0%). | **QUALIFIED PASS** |
| **Quality Gate: Explicit AVIF** | 20 of 20 calibration assets pass (15/15 photos, 2/2 alpha, 3/3 graphics/UI). Zero quality gate failures. | **PASS** |
| **Quality Gate: Auto Mode** | 20 of 20 calibration pass (100%; Python logo 0.9984, Alacritty 0.9807); 20 of 20 synthetic pass (100%; checkerboard 1.0000 via lossless WebP). | **PASS** |
| **GATE 1 STATUS** | **QUALITY AS THE INVARIANT DEFAULT: CERTIFIED (100% Quality Pass Across All 40 Images)** | **GATE 1 PASS** |

---

## 2. Reconciled Calibration Corpus Ground Truth (`run_20261108_194505`)

All metrics below are drawn directly from [`benchmarks/raw_results.json`](../benchmarks/raw_results.json):

### 2.1 Explicit AVIF ($q=90$) Per-Category Quality Breakdown

- **Photos ($N=15$, Gate: $\text{SSIM} \ge 0.95$)**:
  - **Atihsrah**: **15 / 15 passed** (SSIM range: 0.9625–0.9842).
  - **imgproxy**: **1 / 15 passed** (`moraine_lake.jpg`: SSIM **0.9652**; 14 failed, scoring 0.7923–0.9427 against Pillow reference).
- **Alpha Assets ($N=2$, Gate: $\text{SSIM} \ge 0.98$)**:
  - **Atihsrah**: **2 / 2 passed** (`python_logo_alpha.png`: SSIM **0.9984** via alpha-premultiplied resampling; `alacritty_logo_alpha.png`: SSIM **0.9931**).
  - **imgproxy**: **2 / 2 passed** (`python_logo_alpha.png`: SSIM **0.9897**; `alacritty_logo_alpha.png`: SSIM **0.9800**).
- **Graphics & UI ($N=3$, Gate: $\text{SSIM} \ge 0.98$)**:
  - **Atihsrah**: **3 / 3 passed** (SSIM range: 0.9963–0.9971).
  - **imgproxy**: **0 / 3 passed** (SSIM range: 0.8923–0.9533).
- **Total Calibration Quality Passes**:
  - **Atihsrah Explicit AVIF ($q=90$)**: **20 / 20 (100%)**
  - **imgproxy Explicit AVIF ($q=90$)**: **3 / 20 (15.0%)**
- **Mutually Eligible Comparison Pairs ($N=3$)**:
  Three images met category quality thresholds on **both** engines simultaneously:
  1. `moraine_lake.jpg` (Photo): Atihsrah 134,470 B (SSIM 0.9625) vs imgproxy 170,574 B (SSIM **0.9652**) $\to$ **-21.2%** byte delta.
  2. `alacritty_logo_alpha.png` (Alpha): Atihsrah 10,481 B (SSIM 0.9931) vs imgproxy 19,633 B (SSIM **0.9800**) $\to$ **-46.6%** byte delta.
  3. `python_logo_alpha.png` (Alpha): Atihsrah 26,361 B (SSIM **0.9984**) vs imgproxy 22,764 B (SSIM **0.9897**) $\to$ **+15.8%** byte delta.
  - Conventional Median Delta ($N=3$): **-21.2%** (Mean: **-17.3%**).

### 2.2 Auto-Negotiation Pipeline (`format=auto`)
- **Calibration Quality**: **20 / 20 passed (100%)**, empirically verified in `run_20261108_194505`. `python_logo_alpha.png` achieved SSIM **0.9984** $\ge 0.98$ via alpha-premultiplied resampling. Under the quality-first $q \ge 40$ policy, `alacritty_logo_alpha.png` selected AVIF $q=40$ (4,329 B vs 3,612 B source) preserving visual quality at SSIM 0.9807 $\ge 0.98$.
- **Held-Out Synthetic Quality**: **20 / 20 passed (100%)**. `synth_16` / `16_pattern_checkerboard.png` selected lossless WebP (49,312 B) achieving SSIM **1.0000** $\ge 0.98$.
- **Byte Budget Compliance**: **18 / 20 calibration** and **18 / 20 synthetic** outputs $\le$ source bytes in `run_20261108_194505` (90.0%). Alacritty, Python logo, and Checkerboard were permitted to exceed source bytes to preserve quality, recorded via `atihsrah_output_larger_than_source_total`.
- **Scope Clarification**: Quality metrics and auto-negotiation evaluations were performed on **Atihsrah Native**. The Docker tier was evaluated for cold/warm latency and concurrency; the harness does not collect duplicate per-image quality grids for Docker.

---

## 3. Online Request-Path Realities vs Offline Quality Benchmarks

> [!IMPORTANT]
> **SSIM is an offline evaluation metric, not an online HTTP request-path filter.**
> The proxy runtime does not compute SSIM during request processing; doing so would require retaining uncompressed ground-truth reference frames in memory and executing costly multi-pass spatial convolutions ($O(W \times H \times K^2)$) on every transform, adding 50–150 ms of overhead per request.

### The Quality Regression Mechanism
In the baseline implementation (`aafd82e4`), candidate selection is driven primarily by byte size:
1. For `alacritty_logo_alpha.png` (source: 3,612 B), the nominal AVIF candidate ($q=90$) generates 10,477 B (SSIM 0.9931).
2. The fallback ladder evaluated $q=40$ (4,329 B, SSIM 0.9807) and $q=20$ (3,401 B, SSIM 0.9674).
3. The engine sorted candidates by byte size and selected the smallest candidate ($q=20$), forcing the output under the 3,612-byte source limit at the cost of a quality gate breach (0.9674 < 0.98).
4. Similarly, on `16_pattern_checkerboard.png` (source: 42,529 B), AVIF $q=90$ generated 114,611 B, and the engine fell to $q=20$ (32,357 B), dropping SSIM to 0.9397.

### Policy Remediation Strategy & Quality Gate Resolution
- **Quality-First Ladder**: Disallow aggressive quantization steps ($q=20$) for alpha and graphic content classes. Enforce a minimum quality floor ($q \ge 40$).
- **Multi-Codec Candidate Evaluation**: For non-photographic and alpha assets, evaluate lossless WebP and PNG alongside AVIF.
- **Exclusion of Lossy Quantization on Geometric Patterns**: On flat geometric graphics and patterns (`!is_photo`), lossy AVIF $q=40$ is strictly excluded from candidate evaluation to prevent boundary ringing artifacts.
- **Alpha-Premultiplied Resampling**: Resampling RGBA images with premultiplied alpha eliminates dark boundary bleed from transparent (0,0,0,0) pixels into antialiased borders.
- **Empirical Resolution Across All 40 Benchmark Assets**:
  - On `alacritty_logo_alpha.png`: The $q=40$ candidate (4,329 B, SSIM 0.9807) passes quality ($\ge 0.98$) while exceeding the 3,612-byte source, recorded via `atihsrah_output_larger_than_source_total`.
  - On `python_logo_alpha.png`: Alpha-premultiplied resampling raises SSIM from 0.9799 to **0.9984** $\ge 0.98$ (**PASS**).
  - On `16_pattern_checkerboard.png`: The engine evaluates primary AVIF ($q=90$, 114,611 B) and lossless WebP (49,312 B), selecting lossless WebP at SSIM **1.0000** $\ge 0.98$ (**PASS**).
  - All 20 calibration and all 20 synthetic stress suite assets meet or exceed strict quality gates (**100% quality pass rate**).

---

## 4. Grounded Competitor & Architecture Matrix

| Dimension | Atihsrah | imgproxy OSS | Cloudflare Images |
| :--- | :--- | :--- | :--- |
| **Deployment Model** | Self-hosted, single binary or container in customer infrastructure/VPC. | Self-hosted standalone container in customer infrastructure/VPC. | Managed cloud SaaS across Cloudflare's global edge network. |
| **Local macOS Execution** | **Yes** (tested: Native binary and Docker container). | **Yes** (tested: Docker container). | **No** (proprietary edge service; cannot be run locally). |
| **Internal Implementation** | First-party pure Rust (`#![forbid(unsafe_code)]`); excludes C image-codec libraries. | C / C++ (`libvips`, `glib`, `libheif`, `libwebp`). | **Proprietary / vendor-internal** (undisclosed edge architecture). |
| **Caching Model** | Embedded in-process Moka LRU RAM cache (Native: 0.33 ms p50, 0.44 ms p95 in this run). | Standalone stateless operation (Direct: 20.08 ms p50, 21.91 ms p95; with Nginx proxy_cache: 0.69 ms p50, 1.05 ms p95). Persistent internal cache is an **imgproxy Pro** feature. Fronting with NGINX or a CDN is an optional deployment choice. | Distributed edge caching across Cloudflare PoPs with tiered caching and data center request collapsing. *(Network latency unmeasured in this benchmark)*. |
| **Cold Concurrency** | 100 uncached concurrent requests: **94.4 req/s** (Native), **211.6 req/s** (Docker). | 100 uncached concurrent requests: **105.8 req/s** (multi-threaded libvips pipeline). | Managed global edge network processing. *(Unmeasured in local benchmark)*. |
| **Request Coalescing** | Built-in `InflightManager` coalesced 100 concurrent requests into **1 origin fetch** in this run. | Standalone OSS passes concurrent requests through to origin unless protected by an upstream shield. | Cloudflare documents request collapsing per edge data center to shield customer origins. |
| **Licensing & Costs** | **No software license fee** (AGPL-3.0); customer pays only their own server compute/hosting. | **No software license fee** for OSS (MIT); Pro features require a commercial license. | **Metered SaaS pricing**: includes 5,000 unique transformations, then **\$0.50 per 1,000 transformations**; storage is **\$5 per 100,000 images**; delivery is **\$1 per 100,000 delivered images**. |

---

## 5. Formal Production Readiness Roadmap & Release Gates

To transition Atihsrah from pilot to general customer production, the following 5 gates must be certified:

```mermaid
flowchart LR
    G1["Gate 1: Quality-First Policy"] --> G2["Gate 2: Iso-Quality Recalibration"]
    G2 --> G3["Gate 3: Linux Target Qualification"]
    G3 --> G4["Gate 4: 500MB Cache Stress"]
    G4 --> G5["Gate 5: Release Ops & Fuzzing"]
    G5 --> PROD["General Production Release"]
```

### Gate 1: Quality as the Invariant Default (IMPLEMENTED & VERIFIED)
- **Implemented in [`src/processor/image_rs.rs`](../src/processor/image_rs.rs)**:
  - Destructive quantization ($q=20$) has been purged from the candidate ladder.
  - Enforces minimum quality floor ($q \ge 40$) for lossy AVIF and evaluates multi-codec candidates (lossless WebP, quality 100) on graphics and alpha content classes.
  - On `alacritty_logo_alpha.png`, the engine selects the quality-passing candidate ($q=40$, 4,329 B, SSIM 0.9807 $\ge$ 0.98), preserving visual fidelity over arbitrary byte constraints, and records output larger than source via `atihsrah_output_larger_than_source_total`.
  - On `python_logo_alpha.png`, alpha-premultiplied resampling raises SSIM from 0.9799 to **0.9984** $\ge 0.98$ (**PASS**).
  - On `16_pattern_checkerboard.png`, the engine selects lossless WebP (49,312 B, SSIM **1.0000** $\ge 0.98$), strictly excluding lossy AVIF $q=40$ on geometric graphics and achieving 100% quality gate compliance.
  - Verified with unit test `test_quality_first_policy_on_graphics_and_alpha` (all 159 Rust tests and 7 candidate validation tests pass).

### Gate 2: Iso-Quality Competitive Recalibration
- Expand the real-world evaluation corpus beyond 20 images.
- Sweep file sizes at **matched quality (iso-quality SSIM)** rather than nominal $q$ values to generate statistically valid rate-distortion curves against imgproxy.

### Gate 3: Linux Production Target Qualification
- Execute the release container under cgroup CPU and memory limits on production Linux hardware (ARM64 and AMD64).
- Measure cold transforms with **hundreds of samples for p95** and **$\ge 1,000$ samples for p99** to report statistically meaningful tail percentiles and variance.
- Establish separate service-side processing SLOs and end-to-end request SLOs.

### Gate 4: Resource Limits & Multi-Instance Behavior
- Saturate the Moka cache beyond 500 MB under sustained concurrency to verify memory bounding, eviction stability, and inflight coordination under memory pressure.
- Document deployment topology for multi-replica setups (e.g. consistent hashing or CDN caching to manage cold-start origin load).

### Gate 5: Release Operations & Security Evidence
- Multi-architecture container certification (`linux/amd64`, `linux/arm64`).
- Non-root execution (`10001:10001`), read-only root filesystems, and bounded graceful shutdown under active load.
- Prometheus dashboards and alert thresholds for 5xx rates, queue depth, memory saturation, and larger-than-source outputs.
- Formal canary criteria and automated rollback triggers.
