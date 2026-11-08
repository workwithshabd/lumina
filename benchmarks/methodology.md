# Reproducible Benchmark Methodology: Atihsrah vs imgproxy vs Thumbor

Generated automatically by the benchmark harness on 2026-11-08 06:10:25 UTC.

---

## 1. System Environment & Specifications

- **Evaluation Host**: Mac16,10 (Apple Silicon)
- **CPU**: Apple M4 (10 logical cores, architecture `arm64`)
- **Memory**: 16.0 GB Unified Memory
- **Operating System**: macOS 27.2 (Build `26B123`)
- **Kernel**: `Darwin Mac-mini.local 27.2.0 Darwin Kernel Version 27.2.0: Wed Oct 14 23:47:07 PDT 2026; root:xnu-13432.41.4~25/RELEASE_ARM64_T8132 arm64`
- **Compiler / Toolchain**:
  - Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
  - Python: `3.12.14 (main, Sep 24 2026, 17:46:32) [Clang 22.1.3 ]`
  - Docker: `Docker version 29.9.2, build a82e528901` (Context: `colima`)
  - Git Commit: `f4d83f9`

---

## 2. Competitor Software Versions & Deployment Topology

| Competitor | Software Version | Execution Environment | Addressing & Topology |
| :--- | :--- | :--- | :--- |
| **Atihsrah Native** | `v0.1.0` (pure-Rust) | Native macOS arm64 binary (`target/release/atihsrah`) | `http://127.0.0.1:8080`, metrics on `:9090` |
| **Atihsrah Docker** | `v0.1.0` (pure-Rust) | Docker container (`atihsrah:latest`) in Colima VM | `http://127.0.0.1:8084`, metrics on `:9094` |
| **imgproxy OSS** | `4.0.21` | Docker container (`darthsim/imgproxy:latest`) in Colima VM | `http://127.0.0.1:8081` |
| **imgproxy Cached**| `4.0.21 + nginx/alpine` | Nginx (`nginx:alpine`) reverse proxy caching imgproxy | `http://127.0.0.1:8082` |
| **Thumbor** | `7.8.0` | Native macOS application via `uv run` / Python 3.12 | `http://127.0.0.1:8888` |
| **Mock Origin** | Custom HTTP/HTTPS server | Docker container (`bench_origin`) with self-signed CA | `https://127.0.0.1:8000` (host) / `https://origin:8000` (VM) |

### Virtualization Overhead Disclosure
On macOS, Docker Desktop / Colima executes containers within an arm64 Linux virtual machine. Containerized services (imgproxy, imgproxy_cached, and containerized Atihsrah) incur hypervisor context-switching, virtual network interface NAT translation, and guest kernel scheduling. Native services (Native Atihsrah and Thumbor) execute directly on macOS host silicon. To fairly isolate this virtualization delta, Atihsrah is benchmarked in both native and containerized modes.

---

## 3. Reproduction Commands

To replicate this exact benchmark run locally on this Mac:

```bash
# 1. Clone repository and navigate to root
cd lumina

# 2. Build optimized native release binary
cargo build --release

# 3. Download and verify the 20 real benchmark images
./bench/download_corpus.sh

# 4. Run the full reproducible benchmark suite
./bench/run_all.sh
```

---

## 4. Scenario Descriptions

1. **Cold Requests**: Single requests for JPEG, PNG, WebP, and AVIF to measure first-time processing and pipeline initialization latency.
2. **Warm Cache**: 50 sequential requests for the same image to measure sub-millisecond in-process cache hit latency against un-cached proxies and Nginx proxy_cache.
3. **Single-Flight Coalescing**: 100 simultaneous concurrent requests for the exact same uncached URL, asserting exactly 1 origin fetch on Atihsrah.
4. **Different-URL Concurrency**: 100 concurrent requests across distinct corpus assets to evaluate multi-threaded throughput and tail latencies.
5. **Slow Origin**: Validates handling and timeout bounds when the upstream origin experiences a simulated 2.0s delay.
6. **Failing Origin**: Validates structured error mapping (HTTP 502 Bad Gateway) when the origin returns HTTP 500.
7. **Sustained Load & Memory RSS**: Measures memory footprint and verifies cache bounds during continuous traffic over sustained durations.
8. **AVIF vs WebP Encode Cost**: Quantifies CPU time ratio and file size delta between AVIF and WebP encoders.
9. **20-Image Corpus Quality & Compression**: Evaluates delivered bytes, dimensions, and SSIM against ground-truth Pillow Lanczos references across photos, graphics, text, and transparent assets.
