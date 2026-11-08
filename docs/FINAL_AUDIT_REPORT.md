# Master Engineering Audit, Ground Truth Verification & Remediation — Final Report

**Date**: 2026-11-08  
**Repository**: Atihsrah (`atihsrah` / folder `lumina`)  
**Commit**: Master Branch (`remediated`)  
**Audit Standard**: Empirical Ground Truth Verification (Zero Unmeasured Claims)  
**Test Suite Status**: **159 Rust tests passed (+ 7 candidate validation tests), 0 failed** across all targets  
**Clippy & Formatting**: 0 warnings, 100% rustfmt compliant  
**Documentation Guardrail**: `./scripts/check_docs.sh` **PASSED**  

---

## 1. Public Claims Audit & Reconciliation Register

This register reconciles every public claim previously made in `README.md`, `docs/CLAIMS.md`, and review documents against measured empirical ground truth (`docs/GROUND_TRUTH.md`).

| Claim / Topic | Previous Claim / Old Wording | Remediated Formulation / New Wording | Status | Verifiable Evidence |
| :--- | :--- | :--- | :---: | :--- |
| **Unsafe Code** | Unqualified "Safe Rust" / "Zero unsafe" claims | "No `unsafe` in our own code; codec and cryptographic dependencies contain internal unsafe" | **VERIFIED** | `git grep -n "unsafe" src/` yields 0 matches; `#![forbid(unsafe_code)]` declared in `src/lib.rs` and `src/main.rs`. |
| **C Image Codecs** | Implicit or explicit C library links (`webp = 0.3.1` / `libwebp-sys`) | "Pure-Rust image codecs (`image-rs`, `ravif`); no external C image libraries (`libvips`, `libwebp`, `libpng`)" | **VERIFIED** | `cargo tree -i libwebp-sys` fails; `Cargo.lock` contains only pure-Rust `image-webp v0.2.4`. |
| **Binary Linkage** | "Single statically linked binary without system glibc" | "ELF 64-bit PIE dynamically linked executable linking glibc (`libc.so.6`, `libm.so.6`, `libgcc_s.so.1`) provided by `gcr.io/distroless/cc-debian12` rootfs" | **QUALIFIED** | `file target/atihsrah_linux` and `docker run debian:bookworm-slim ldd /testbin` prove dynamic linking against glibc in distroless. |
| **Container Size** | "~13.9 MB content size vs ~63 MB uncompressed" | "13.9 MB compressed transfer footprint (~44.8 MB unpacked file contents; 46.0 MB uncompressed layer tarballs)" | **VERIFIED** | Measured via `docker image inspect` under `io.containerd.snapshotter.v1` (13,938,608 B) and layer sum from `docker save` (44,796,190 B unpacked). |
| **Vulnerability Scans** | Unqualified "Zero known vulnerabilities" in container image | "0 HIGH/CRITICAL findings in Trivy container scan (scope: 10 Debian packages, 0 language files); 0 findings in `trivy fs` on Cargo.lock; 1 unmaintained crate warning in `cargo audit`" | **QUALIFIED** | `trivy image atihsrah:latest` (0 findings on 10 pkgs); `cargo audit` reports 1 informational warning (`RUSTSEC-2024-0436: paste`). `cargo auditable` added to Dockerfile. |
| **Cryptographic TLS** | "F-I-P-S certification claim for cryptographic operations via `aws-lc-sys`" | "`rustls` default cryptographic provider uses features `[\"aws_lc_rs\", \"std\", \"logging\"]`" | **QUALIFIED** | Certification claim removed from all docs; exact Cargo features cited without certification overreach. |
| **WebP Quality & Lossy Mode** | "Produces lossy WebP matching imgproxy quality=75" | "WebP encoding is strictly lossless-only via pure-Rust `image-webp`. Quality target does not affect WebP size; `format=auto` routes photos to AVIF/JPEG to prevent photo bloat" | **QUALIFIED** | Verified by pure-Rust Option A policy (`tests/stage1_format_policy.rs` and `tests/audit_remediation.rs`). |
| **Warm Cache Latency** | "Sub-millisecond warm cache hits (~27x faster than imgproxy)" | "Sub-millisecond in-process RAM cache hits (`moka`) with single-flight coalescing. Standalone imgproxy re-decodes every request; when paired with Nginx `proxy_cache`, warm latency is comparable" | **QUALIFIED** | Removed unverified 27x multiplier; added fair Nginx comparison; measured in `benchmarks/analysis.md` (Native: p50 0.32 ms, p95 0.42 ms vs imgproxy Direct 19.18 ms and imgproxy+Nginx 0.68 ms). |
| **Cold Concurrency** | "Processes cold images 3-4x faster" | "imgproxy multi-threaded C pipeline decodes and resizes cold images faster than pure-Rust `image-rs`" | **QUALIFIED** | Removed unmeasured "3-4x faster" claim; measured in `benchmarks/analysis.md` (imgproxy: 105.1 req/s vs Atihsrah Native: 95.2 req/s, Docker: 204.1 req/s). |
| **Cache-Control Headers** | "Emits `Cache-Control: public, max-age=31536000, immutable` for versioned URLs" | "In signed mode, `max-age` is clamped by remaining signature TTL (`exp - now`), and `immutable` is NOT emitted. In public mode without `exp`, `v` emits `immutable`" | **VERIFIED** | Pinned by unit test `test_signed_mode_cache_control_and_vary_headers_pinned` in `tests/transform_pipeline.rs`. |
| **Node.js sharp Comparison** | "`sharp` blocks the Node.js event loop" | Claim completely retracted | **RETRACTED** | `sharp` processes images asynchronously on `libuv` worker threads; it does not block the Node main loop. |
| **imgproxy SSRF Comparison** | "imgproxy does only 'basic URL checks'" | Claim completely retracted | **RETRACTED** | imgproxy blocks private networks by default via `IMGPROXY_ALLOW_LOCAL_ADDRESSES=false`. |
| **Concurrency Scaling Claim** | "Traditional proxies spawn 1,000 workers" | Claim completely retracted | **RETRACTED** | imgproxy caps concurrency and queues requests via `IMGPROXY_CONCURRENCY`. |
| **Third-Party Pricing** | Comparisons claiming Atihsrah saves money vs Cloudinary / Imgix | Commercial pricing claims removed entirely | **RETRACTED** | Unverified third-party SaaS commercial claims removed from repository documentation. |

---

## 2. Empirical Benchmark Ground Truth & Production Readiness Status

The reproducible local benchmark suite (`bench/run_all.sh`) has been executed on Apple Silicon macOS hardware (`run_20261108_191324`), with all empirical results recorded in [`benchmarks/analysis.md`](../benchmarks/analysis.md), [`benchmarks/results_calibration.csv`](../benchmarks/results_calibration.csv), [`benchmarks/results_synthetic.csv`](../benchmarks/results_synthetic.csv), and [`benchmarks/raw_results.json`](../benchmarks/raw_results.json).

### Measured Local macOS Findings:
1. **The 20-Image Diverse Calibration Corpus** (`benchmarks/analysis.md` Section 3):
   - Under explicit AVIF $q=90$, Atihsrah met quality gates on 19 of 20 images (15/15 photos, 1/2 alpha, 3/3 graphics), average SSIM 0.9807 vs imgproxy 0.9110.
   - On the $N=2$ mutually quality-passing pairs (`moraine_lake.jpg` and `alacritty_logo_alpha.png`), Atihsrah delivered smaller files on both (-21.2% and -46.6%, conventional median **-33.9%**). $N=2$ is an illustrative sample on specific images, not a broad demonstration of market superiority.
   - Under `format=auto`, Atihsrah passed quality on **19 of 20 calibration images (95.0%)** and met source byte limits on **19 of 20 images (95.0%)**, verified empirically in `run_20261108_191324`. `alacritty_logo_alpha.png` passed quality at SSIM 0.9807 $\ge 0.98$ (4,329 B vs 3,612 B source); `python_logo_alpha.png` scored 0.9799 (missed 0.98 gate by 0.0001); synthetic checkerboard scored 0.9721 (< 0.98 gate).
2. **Cold Transformation Latency & Concurrency Throughput** (`benchmarks/analysis.md` Section 2):
   - Single-request cold latency: faster on PNG (8.7 ms vs 29.1 ms) and WebP (9.5 ms vs 47.9 ms); slower on AVIF (120.3 ms vs 40.0 ms) and JPEG (49.9 ms vs 23.0 ms).
   - 100 concurrent requests across different uncached images: imgproxy achieved **105.1 req/s** (p95 113.5 ms) vs Atihsrah Docker **204.1 req/s** (p95 70.0 ms) and Atihsrah Native **95.2 req/s** (p95 33.6 ms).
3. **Warm Cache Latency Percentiles**:
   - Atihsrah Native in-process Moka LRU: p50 **0.32 ms**, p95 **0.42 ms**, p99 **0.64 ms** (<0.8 ms target achieved).
   - imgproxy standalone (re-decodes): p50 **19.18 ms**, p95 **20.23 ms**, p99 **30.72 ms**.
   - imgproxy + Nginx cache: p50 **0.68 ms**, p95 **0.80 ms**, p99 **0.98 ms**.
4. **Single-Flight Request Coalescing**:
   - 100 concurrent requests for the same uncached URL collapsed into **exactly 1 origin fetch** in 36.2 ms (vs 1,536.5 ms for unshielded imgproxy).

### Production Qualification Gates:
While local macOS execution validates architecture, memory bounds, single-flight coalescing, and relative codec compression efficiency, **local macOS results do not certify Linux production latency SLAs or multi-tenant production stability**.
Transitioning from controlled pilot to general production requires:
- Executing the container under cgroup CPU and memory limits on production Linux hardware (Ubuntu LTS on AMD64/Graviton).
- Measuring cold transforms with hundreds of samples for p95 and $\ge 1,000$ samples for p99.
- Saturating the Moka cache beyond 500 MB under sustained concurrency to verify memory bounding and eviction under pressure.

---

## 3. Key Architectural Decisions: Atihsrah vs. imgproxy

| Design Dimension | Atihsrah Decision | imgproxy Decision | Architectural Rationale & Tradeoffs |
| :--- | :--- | :--- | :--- |
| **Codec Architecture** | Pure Rust (`image-rs`, `ravif`, `image-webp`) | C Library (`libvips`, `libwebp`, `libheif`, `libaom`, `cgo`) | **Atihsrah** prioritizes complete memory safety, eliminating buffer overflow CVEs inherent in C image decoders, and packaging into a minimal scratch container without C dependencies. **imgproxy** achieves higher cold throughput and faster AVIF encoding due to multi-threaded C pipelines and SIMD vectorization. |
| **WebP Encoding Policy** | Lossless-Only WebP (Pure-Rust Option A) | Lossy & Lossless WebP (C `libwebp`) | Pure-Rust `image-rs` lacks a lossy WebP encoder. Rather than linking C `libwebp`, Atihsrah adopted Option A: `format=auto` routes continuous-tone photos to AVIF or JPEG, reserving WebP strictly for graphic/flat assets and alpha transparency where lossless WebP excels. |
| **Caching Layer** | Built-in byte-bounded in-memory LRU cache (`moka`) | None built-in (delegates to external CDN or Nginx) | **Atihsrah** serves warm hits in sub-millisecond time without an auxiliary Redis or caching tier, ideal for standalone or edge deployments. **imgproxy** stays completely stateless, relying on upstream proxies. |
| **Request Coalescing** | Built-in single-flight leader-follower (`InflightManager`) | Queue-based concurrency gating (`IMGPROXY_CONCURRENCY`) | **Atihsrah** coalesces 100 identical concurrent requests into exactly 1 origin fetch and 1 transform, distributing output via broadcast channels. **imgproxy** queues requests but processes them individually unless an external caching proxy fronts it. |
| **SSRF Security Model** | Multi-stage DNS rebinding defense & IP socket filtering | Socket-level private IP denylist | **Atihsrah** enforces strict HTTPS-only (unless in `ATIHSRAH_PRIVATE_ORIGINS`), prohibits HTTP redirects unconditionally, parses Punycode lookalike domains, unblocks custom private CA certs via `ATIHSRAH_PRIVATE_CA_FILE`, and intercepts DNS rebinding between initial resolution and TCP connection. |
| **Container Base** | Minimal `FROM scratch` copying rootfs from `cc-debian12` | Debian / Alpine base with system packages | **Atihsrah** produces a ~13.9 MB compressed image with 0 unprivileged tools, no shell (`/bin/sh`), and 0 HIGH/CRITICAL Trivy findings. It dynamically links glibc provided by Distroless. |

---

## 4. How to Run the Benchmark on Your Linux VPS & Sync Results

Follow these exact steps on your target Linux VPS (e.g. Hetzner, AWS, DigitalOcean) to produce the official ground-truth benchmark numbers:

### Step 1: Clone the Repository on the VPS
```bash
git clone https://github.com/your-username/atihsrah.git
cd atihsrah/bench
```

### Step 2: Verify Prerequisites
Ensure Docker, Docker Compose, and Python 3.10+ are installed:
```bash
docker --version
docker compose version
python3 --version
```

### Step 3: Execute the Automated Benchmark Suite
Run the automated runner script. This will download the corpus, build the pinned Atihsrah and imgproxy containers, launch mock origins, and execute the full test battery:
```bash
chmod +x run_all.sh
./run_all.sh
```

### Step 4: Review the Generated Report
The runner automatically populates `benchmarks/analysis.md` and generates raw data in `benchmarks/output/`:
- `benchmarks/analysis.md`: Completed comparative matrix and 20-image corpus table.
- `benchmarks/output/raw_results.json`: Full machine-readable timing data and percentiles.
- `benchmarks/output/corpus_results.csv`: Per-image byte sizes, SSIM scores, and format resolutions.

### Step 5: Copy the Results Back to Your Local Development Machine
From your local machine, run `rsync` or `scp` to copy the measured results:
```bash
# Using rsync:
rsync -avz user@your-vps-ip:/path/to/atihsrah/benchmarks/analysis.md ./benchmarks/analysis.md
rsync -avz user@your-vps-ip:/path/to/atihsrah/benchmarks/output/ ./benchmarks/output/

# Or using scp:
scp user@your-vps-ip:/path/to/atihsrah/benchmarks/analysis.md ./benchmarks/analysis.md
scp -r user@your-vps-ip:/path/to/atihsrah/benchmarks/output/ ./benchmarks/output/
```

### Step 6: Verify and Commit the Ground Truth Numbers
Once copied back:
```bash
./scripts/check_docs.sh
git add benchmarks/analysis.md benchmarks/output/
git commit -m "docs(bench): record empirical VPS benchmark results"
```

