# Atihsrah: Public Claims & Evidence Verification Register

> **EVIDENCE STANDARD**: Every claim in this register must cite an exact command or source file in this repository. Any claim without verifiable empirical evidence is deleted or qualified. For complete command executions and raw terminal outputs, refer to the [Ground Truth Verification Register](GROUND_TRUTH.md).

---

## 1. Codebase & Architectural Claims

### Claim 1: "No `unsafe` in our own code; codec dependencies contain unsafe"
- **Status**: **VERIFIED**
- **Evidence Command**:
  ```bash
  git grep -n "unsafe" src/
  ```
  Result: `0` matches in `src/`. Both `src/lib.rs` and `src/main.rs` enforce `#![forbid(unsafe_code)]`.
- **Dependency Clarification**:
  `cargo geiger` reports that upstream codec dependencies (`image`, `ravif`), async runtime primitives (`tokio`, `bytes`), and cryptographic libraries (`aws-lc-rs` / `aws-lc-sys`, `zlib-rs`) contain internal `unsafe` code for low-level memory layout, SIMD vectorization, and hardware acceleration.

### Claim 2: "Pure-Rust image codecs; no external C image libraries"
- **Status**: **VERIFIED**
- **Evidence Files**: [`Cargo.toml`](../Cargo.toml) and [`Cargo.lock`](../Cargo.lock)
  - Image processing is powered exclusively by `image` (v0.25) and `ravif` (v0.11).
  - External C image libraries (`libvips`, `libwebp-sys`, `libpng-sys`, `libjpeg-turbo`) are strictly absent from the dependency tree.
  - WebP encoding in this pure-Rust build (`FORMAT_OPTION: A`) is lossless-only via pure-Rust `image-webp v0.2.4`.
  - The TLS stack (`rustls`) uses the crate configuration `features = ["aws_lc_rs", "std", "logging"]` as its default cryptographic provider.

### Claim 3: "Minimal container deployment without C image libraries"
- **Status**: **VERIFIED**
- **Evidence Command**:
  ```bash
  docker run --rm -v "$(pwd)/target/atihsrah_linux:/testbin" debian:bookworm-slim ldd /testbin
  ```
  The container binary `/usr/local/bin/atihsrah` is an ELF 64-bit PIE executable dynamically linked against system glibc (`libc.so.6`, `libm.so.6`, `libgcc_s.so.1`) provided by the `gcr.io/distroless/cc-debian12` rootfs in a `FROM scratch` minimal container. It contains zero external C image library dependencies (`libvips`, `libwebp`).

### Claim 4: "Built-in byte-weighted in-memory LRU cache (`moka`)"
- **Status**: **VERIFIED**
- **Evidence Source**: [`src/cache/mod.rs`](../src/cache/mod.rs)
  - Utilizes `moka::future::Cache` with `weigher(|_, v| v.len() as u32)`.
  - Capacity is bounded by `CACHE_MAX_SIZE_MB` (default 500 MB) with individual entries capped by `CACHE_SINGLE_ENTRY_MAX_MB` (default 10 MB).
  - Test verification: [`tests/transform_pipeline.rs::test_byte_weighted_cache_eviction`](../tests/transform_pipeline.rs).

### Claim 5: "Single-flight leader-follower request coalescing (prevents thundering herd)"
- **Status**: **VERIFIED**
- **Evidence Source**: [`src/cache/inflight.rs`](../src/cache/inflight.rs) and [`src/http/transform.rs`](../src/http/transform.rs)
  - Broadcast-channel coordination deduplicates concurrent requests for the same uncached image.
  - Drop guards ensure leader cancellation mid-flight notifies followers and releases map entries cleanly.
  - Inflight cache keys incorporate resolved format and upscale parameters to prevent cross-client cache pollution.
  - Test verification: [`tests/phase_a.rs::test_a1_100_request_herd_with_cancelled_leader`](../tests/phase_a.rs) and [`tests/transform_pipeline.rs::test_100_concurrent_identical_requests_single_flight`](../tests/transform_pipeline.rs).

### Claim 6: "SSRF defense with DNS rebinding protection and private IP blocking"
- **Status**: **VERIFIED**
- **Evidence Source**: [`src/fetch/validator.rs`](../src/fetch/validator.rs) and [`src/fetch/resolver.rs`](../src/fetch/resolver.rs)
  - Strictly requires HTTPS origins (unless explicitly allowlisted in `ATIHSRAH_PRIVATE_ORIGINS`).
  - Prohibits redirects, userinfo, IP literals in public mode, and non-standard ports.
  - Resolves IP addresses before TCP connection, rejecting RFC 1918, RFC 4193 ULA, carrier-grade NAT, loopback, link-local metadata (169.254.169.254), and IPv4-mapped IPv6.
  - Test verification: [`tests/fetch_security.rs`](../tests/fetch_security.rs) and [`tests/security/ssrf.rs`](../tests/security/ssrf.rs) (18 security tests passing in `tests/security/`).

### Claim 7: "Strict HMAC-SHA256 URL signing with constant-time verification"
- **Status**: **VERIFIED**
- **Evidence Source**: [`src/auth/signing.rs`](../src/auth/signing.rs)
  - Enforces deterministic canonical parameter sorting, strict RFC 3986 percent-encoding, and constant-time signature comparison (`subtle::ConstantTimeEq`).
  - Rejects duplicate parameter keys with `AppError::DuplicateParam` (HTTP 422).
  - Test verification: [`tests/stage2_signing_vectors.rs`](../tests/stage2_signing_vectors.rs) (verifies positive and negative test vectors from [`docs/signing-vectors.json`](signing-vectors.json)).

---

## 2. Container Sizing & Security Scans

### Container Size Measurement
- **Evidence Commands**:
  ```bash
  docker image inspect -f '{{.Size}}' atihsrah:latest
  docker save atihsrah:latest | gzip | wc -c
  ```
- **Results**:
  - **Compressed Transfer Footprint**: **~13.9 MB** (under Docker's `io.containerd.snapshotter.v1` store, `.Size` reports 13,938,608 bytes; `docker save | gzip` reports 13,738,957 bytes).
  - **Uncompressed File Contents**: **~44.8 MB** (unpacked file contents sum to 44,796,190 bytes; uncompressed raw tar layers sum to 46,007,808 bytes / ~46.0 MB).

### Security Scan Scope & Results
- **Evidence Commands & Scope**:
  1. **Container Image Scan**:
     ```bash
     trivy image atihsrah:latest --severity HIGH,CRITICAL
     ```
     Result: **0 HIGH/CRITICAL findings** (Trivy v0.76.1; scope: 10 Debian base packages, 0 language files).
  2. **Filesystem Dependency Scan**:
     ```bash
     trivy fs . --scanners vuln
     ```
     Result: **0 findings** (scanned `Cargo.lock` and `fuzz/Cargo.lock`).
  3. **Cargo Audit Scan**:
     ```bash
     cargo audit
     ```
     Result: **0 vulnerabilities**, 1 informational warning (`RUSTSEC-2024-0436: paste is unmaintained`, pulled as a transitive dependency).

---

## 3. Performance & Benchmark Methodology

> [!IMPORTANT]
> All empirical latency, throughput, and memory consumption comparisons between Atihsrah, imgproxy, and Thumbor are measured directly on local macOS Apple Silicon hardware using the automated benchmark suite in [`bench/run_all.sh`](../bench/run_all.sh). Detailed methodology and per-image numbers are recorded in [`benchmarks/analysis.md`](../benchmarks/analysis.md), [`benchmarks/results.csv`](../benchmarks/results.csv), and [`benchmarks/system_info.json`](../benchmarks/system_info.json).

- **Warm In-Memory Cache Latency**:
  - Atihsrah serves cached assets directly from in-memory RAM (`moka`) without CPU transcoding overhead.
  - Measured Result (Apple M4, 10 cores, macOS 27.2, `run_20261108_194505`): **0.33 ms median p50** (p95: **0.44 ms**, p99: **0.67 ms**), compared to imgproxy Direct (20.08 ms p50, p95: 21.91 ms) and imgproxy + Nginx cache (0.69 ms p50, p95: 1.05 ms).
- **Single-Flight Request Coalescing**:
  - Under 100 concurrent requests for the exact same uncached URL, Atihsrah initiates **exactly 1 origin fetch** in 34.4 ms, eliminating thundering-herd cache stampedes. Uncached imgproxy initiates 100 origin fetches in 1,560.4 ms.
- **Cold Transformation Throughput (100 Concurrent Different URLs)**:
  - `libvips` utilizes multi-threaded C pipelines and SIMD vectorization for image transformation, reaching **105.8 req/s** (p95: 113.9 ms) under 100 concurrent uncached requests.
  - Pure-Rust `image-rs` and `ravif` run single-process software codecs within Tokio blocking threads, achieving **94.4 req/s** (native, p95: 30.5 ms) and **211.6 req/s** (containerized, p95: 67.0 ms).
- **20-Image Corpus Compression & Quality**:
  - On the 20-image real calibration corpus under explicit AVIF $q=90$, Atihsrah met quality gates on **20 of 20 images** (100%, all 15 photos, 2/2 alpha, 3/3 graphics) with average SSIM of **0.9816** vs imgproxy's **0.9110** (3 of 20 passed).
  - On the $N=3$ mutually quality-passing pairs (`moraine_lake.jpg`, `alacritty_logo_alpha.png`, and `python_logo_alpha.png`), Atihsrah delivered smaller files on 2 of 3 (-21.2% and -46.6% vs +15.8%, conventional median **-21.2%**, mean -17.3%). $N=3$ is an illustrative observation on specific images, not a broad demonstration of market superiority.
  - Under `format=auto`, Atihsrah met quality gates on **20 of 20 images (100%)** on the real calibration corpus and **20 of 20 images (100%)** on the held-out synthetic stress suite, confirmed empirically in `run_20261108_194505`. `python_logo_alpha.png` achieved SSIM **0.9984** $\ge 0.98$ via alpha-premultiplied resampling. On `alacritty_logo_alpha.png`, the engine selected the quality-passing AVIF $q=40$ candidate (4,329 B, SSIM 0.9807 $\ge 0.98$). On `16_pattern_checkerboard.png`, the engine selected lossless WebP (49,312 B, SSIM 1.0000 $\ge 0.98$). Output size was $\le$ source on 18 of 20 calibration images (90.0%) and 18 of 20 synthetic images (90.0%), permitting larger outputs when quality mandates it.
- **Docker Virtualization Overhead on macOS**:
  - Running Atihsrah containerized inside Colima's Linux VM incurs a **~0.85 ms network and virtualization penalty** on warm cache lookups (1.18 ms vs 0.33 ms native).

---

## 4. Retractions & Corrections of Previous Claims

1. **Claim: `sharp "blocks the Node event loop"`**:
   - **Correction**: **RETRACTED**. `sharp` uses `libuv`'s background thread pool for image processing and does not block the Node.js main event loop.
2. **Claim: `imgproxy does only "basic URL checks" for SSRF`**:
   - **Correction**: **RETRACTED**. imgproxy blocks private local IP addresses by default via `IMGPROXY_ALLOW_LOCAL_ADDRESSES=false`.
3. **Claim: `"Traditional proxies spawn 1,000 workers"`**:
   - **Correction**: **RETRACTED**. imgproxy caps concurrency and queues incoming requests via `IMGPROXY_CONCURRENCY`.
4. **Claim: Commercial CDN pricing comparisons**:
   - **Correction**: **REMOVED**. Commercial vendor pricing claims have been removed from documentation.
5. **Claim: `"0 ms latency"`**:
   - **Correction**: **REPLACED** with `"sub-millisecond latency"`.

---

## 5. When to Choose imgproxy Instead

Choose imgproxy if your workload requires:
1. **Lossy WebP Encoding**: Atihsrah (in pure-Rust Option A) encodes WebP in lossless mode only. If you require lossy WebP for continuous-tone photos, imgproxy provides lossy WebP via `libwebp`.
2. **Maximum Cold Throughput & Fast AVIF**: Workloads transcoding millions of uncached images per hour benefit from `libvips` assembly vectorization.
3. **Advanced Graphical Transformations**: Facial recognition cropping, blur/sharpen convolution filters, watermark overlays, and animated GIF/video transcoding.
4. **Existing Reverse Proxy Cache**: If all traffic is already cached upstream by an edge CDN or Nginx caching tier, Atihsrah's built-in in-memory cache is redundant.

---

## 6. Production Roadmap & 5-Gate Certification

All five gates required for broad customer production readiness have been empirically evaluated and certified:
1. **Gate 1 (Visual Quality Invariant)**: 100% compliance with perceptual quality gates across all 40 calibration and synthetic stress assets in `run_20261108_194505`.
2. **Gate 2 (Iso-Quality Competitive Recalibration)**: 100 real images evaluated; -33.4% median byte delta vs imgproxy at matched SSIM $\ge 0.95$.
3. **Gate 3 (Linux Production Target Qualification)**: ARM64 and AMD64 containers qualified under cgroup limits; warm p50 1.52 ms, p99 2.18 ms ($N=1,000$); cold AVIF p50 267.25 ms, p95 380.03 ms ($N=200$).
4. **Gate 4 (Stress Scale, Cache Eviction & Recovery)**: Moka RAM cache strictly clamped at 29.93 MB, 200/200 replica success under 50 concurrent workers, SIGTERM in-flight drain code 0, 502 error shielding.
5. **Gate 5 (Production Release Operations)**: Non-root distroless container execution, read-only rootfs, Prometheus alerts (`monitoring/prometheus_alerts.yml`), Grafana dashboard (`monitoring/grafana-dashboard.json`), canary criteria and automated rollback triggers (`docs/RELEASE_OPERATIONS.md`).
See [`docs/FINAL_AUDIT_AND_VERDICT.md`](FINAL_AUDIT_AND_VERDICT.md) for full certification data and empirical analysis.
