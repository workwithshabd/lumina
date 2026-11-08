# Atihsrah Benchmark Reproduction Guide (Linux VPS)

This guide provides exact step-by-step instructions to execute the reproducible benchmark suite on real deployment hardware (e.g. a dedicated Linux cloud instance or bare-metal server).

---

## 1. System Requirements

- **Target OS**: Linux (Ubuntu 22.04/24.04 LTS or Debian 12 recommended)
- **Architecture**: x86_64 or aarch64
- **Hardware Recommended**: Dedicated instance with at least 4 vCPUs and 8 GB RAM (e.g. Hetzner CPX31 / CCX23, AWS c6i.xlarge, or DigitalOcean CPU-Optimized).
- **Software Dependencies**:
  - Docker Engine 24.0+ & Docker Compose v2.20+ (`docker compose`)
  - Python 3.10+ (with `Pillow` and `numpy`: `pip install Pillow numpy`)
  - `curl` and `git`
  - Optional: `ffmpeg` (compiled with `libvmaf` if computing VMAF scores)

---

## 2. Test Corpus Composition & Licensing Notice

The benchmark corpus consists of 20 real images defined in [`bench/corpus_manifest.csv`](./corpus_manifest.csv):
- **14 Photographic images** from the standard Eastman Kodak True Color Lossless Suite (Public Domain).
- **1 High-resolution nature photograph** (`moraine_lake.jpg`, Public Domain via Wikimedia Commons).
- **2 Real UI screenshots**:
  - `kde_spectacle.png`: LGPL-2.0-or-later (KDE Project)
  - `training_module.png`: CC-BY-SA-4.0 (Wikimedia Foundation)
- **3 Transparent Alpha & Vector Graphics**:
  - `python_logo_alpha.png`: Python Software Foundation trademark policy / open web asset
  - `alacritty_logo_alpha.png`: Apache-2.0 (Alacritty project)
  - `world_map_graphic.png`: Public Domain (Wikimedia Commons vector projection)

> [!IMPORTANT]
> **Licenses to Double-Check**:
> If redistributing or modifying the benchmark package publicly, double-check attribution requirements for:
> - `kde_spectacle.png` (LGPL-2.0)
> - `training_module.png` (CC-BY-SA-4.0)
> - `alacritty_logo_alpha.png` (Apache-2.0)
> All other images (including the 14 Kodak suite images) are Public Domain.

---

## 3. Step-by-Step Execution on VPS

### Step 1: Clone Repository
```bash
git clone https://github.com/your-org/lumina.git
cd lumina
```

### Step 2: Install Python Measurement Dependencies
```bash
python3 -m pip install --upgrade pip
python3 -m pip install Pillow numpy
```

### Step 3: Pin imgproxy Version
Set the exact imgproxy version you wish to evaluate:
```bash
export IMGPROXY_VERSION="v3.27.0"
```

### Step 4: Build Atihsrah Container
Build the pure-Rust Atihsrah container image locally:
```bash
docker build -t atihsrah:latest -f Dockerfile .
```

### Step 5: Download and Verify Corpus Images
Run the corpus downloader to fetch the 20 real benchmark images and verify their SHA256 checksums:
```bash
./bench/download_corpus.sh
```

### Step 6: Execute the Full Benchmark Suite
Execute all benchmark scenarios:
```bash
# Standard run (includes 30-minute sustained load with 60s RSS sampling)
./bench/run_all.sh --sustained-duration 1800

# OR Quick test run (60-second sustained load for smoke testing)
./bench/run_all.sh --quick
```

---

## 4. Benchmark Scenarios Executed

The runner executes 9 scenarios across identical hardware and resource allocations (4 concurrent workers):
1. **Cold single request per image type**: JPEG, PNG, WebP, AVIF latency and delivered bytes.
2. **Warm cache latency distribution**:
   - Atihsrah (in-process Moka LRU cache)
   - imgproxy standalone (no cache, re-decodes/re-encodes on every hit)
   - imgproxy behind Nginx `proxy_cache` (fair comparison with `Vary: Accept`)
3. **100 concurrent requests for SAME uncached URL**: Asserts exactly 1 origin fetch via single-flight request coalescing.
4. **100 concurrent requests for DIFFERENT uncached URLs**: Throughput (req/s) and p50/p95 tail latency under cold concurrency.
5. **Slow origin**: 2.0s upstream latency simulation.
6. **Failing origin**: HTTP 500 upstream error mapping and structured JSON responses.
7. **Sustained load memory stability**: Measures container RSS plateau over 30 minutes (or 60s).
8. **AVIF vs WebP encode cost**: CPU processing duration vs byte compression trade-off.
9. **20-image corpus quality and size comparison**:
   - Builds exactly one ground-truth reference per image using Pillow Lanczos sinc filtering.
   - Asserts identical output dimensions for both proxies.
   - Computes SSIM and VMAF against ground truth.
   - Fails if any lossless output SSIM < 0.99, if any metric is NaN/0.0, or dimensions mismatch.
   - Flags any output larger than the source image.

---

## 5. Artifacts & Reporting Results

Upon completion, the harness generates:
- `benchmarks/raw_results.json`: Full machine-readable timing, RSS samples, and HTTP logs.
- `benchmarks/results.csv`: Per-image byte sizes, SSIM scores, and deltas.
- `benchmarks/methodology.md`: Environmental record (kernel, CPU, RAM, Docker and tool versions).

Copy the template from `bench/analysis_TEMPLATE.md` to `benchmarks/analysis.md` and populate each table with the real measured values from `benchmarks/raw_results.json` and `benchmarks/results.csv`.
Apply the explicit decision rules in `analysis_TEMPLATE.md` to evaluate whether the optimization-layer hypothesis is supported, unsupported, or inconclusive.

