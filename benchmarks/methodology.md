# Reproducible Benchmark Methodology: Atihsrah vs imgproxy vs Thumbor

Generated automatically by the benchmark harness on 2026-11-08 19:45:05 UTC.

---

## 1. System Environment & Specifications

- **Active Benchmark Profile**: Production Default (speed=6, max_entry=10MB)
- **Evaluation Host**: Mac16,10 (Apple Silicon)
- **CPU**: Apple M4 (10 logical cores, architecture `arm64`)
- **Memory**: 16.0 GB Unified Memory
- **Operating System**: macOS 27.2 (Build `26B123`)
- **Kernel**: `Darwin Mac-mini.local 27.2.0 Darwin Kernel Version 27.2.0: Wed Oct 14 23:47:07 PDT 2026; root:xnu-13432.41.4~25/RELEASE_ARM64_T8132 arm64`
- **Compiler / Toolchain**:
  - Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
  - Python: `3.12.14 (main, Sep 24 2026, 17:46:32) [Clang 22.1.3 ]`
  - Python Executable: `/Users/shabd/Desktop/dev/lumina/.venv_bench/bin/python3`
  - Docker: `Docker version 29.9.2, build a82e528901` (Context: `colima`)
  - Docker Container VM: `Ubuntu 24.04.4 LTS | Arch: aarch64 | CPUs: 2 | Mem: 2054844416`
  - Git Commit: `a2aa92b47ee9040f178f806c7299633bd3b550cd` (Dirty worktree: `True`)
  - FFmpeg / libvmaf: Available `True`, libvmaf `True`

---

## 2. Competitor Software Versions & Deployment Topology

| Competitor | Software Version | Execution Environment | Addressing & Security |
| :--- | :--- | :--- | :--- |
| **Atihsrah Native** | `v0.1.0` (pure-Rust) | Native macOS arm64 binary (`target/release/atihsrah`) | `http://127.0.0.1:8080`, metrics on `:9090` |
| **Atihsrah Docker** | `v0.1.0` (pure-Rust) | Docker container (`atihsrah:latest`) in Colima VM | `http://127.0.0.1:8084`, metrics on `:9094` |
| **imgproxy OSS** | `4.0.21` | Docker container (`darthsim/imgproxy:4.0.21`) in Colima VM | `http://127.0.0.1:8081` |
| **imgproxy Cached**| `4.0.21 + nginx/alpine` | Nginx (`nginx:alpine`) reverse proxy caching imgproxy | `http://127.0.0.1:8082` |
| **Thumbor Native** | `7.8.0` | Native macOS application via local venv (Python 3.12) | `http://127.0.0.1:8888` |
| **Mock Origin** | Custom HTTP/HTTPS server | Docker container (`bench_origin`) with self-signed CA | `https://127.0.0.1:8000` (host) / `https://origin:8000` (VM) |

### Virtualization Overhead Disclosure
On macOS, Docker containers execute within a virtualized Linux VM (Colima aarch64). Containerized workloads incur VM kernel scheduling, virtual NIC NAT routing, and hypervisor overhead. Native services (Atihsrah Native and Thumbor) run directly on host Apple Silicon. To isolate virtualization overhead fairly, Atihsrah is measured in both Native and Docker configurations.

### Port Binding & Isolation
All published ports across native processes and Docker containers are bound strictly to `127.0.0.1`. No services are exposed to external or private networks.

---

## 3. Reproduction Commands

To replicate this exact benchmark run locally on this Mac:

```bash
# 1. Clone repository and navigate to root
cd /Users/shabd/Desktop/dev/lumina

# 2. Build optimized native release binary
cargo build --release

# 3. Verify corpus images against manifest
./bench/download_corpus.sh

# 4. Run the full reproducible benchmark suite (production default profile)
./bench/run_all.sh --profile=default

# Or run with throughput benchmark profile
./bench/run_all.sh --profile=throughput
```

To run offline validation only (checks decoders, metric fixtures, manifest, and environment):
```bash
./bench/run_all.sh --validate-only
```

---

## 4. Scenario Descriptions

1. **Cold Requests**: Requests for JPEG, PNG, WebP, and AVIF measuring cold decode/encode latency over N=5 runs, reporting median and spread.
2. **Warm Cache**: 100 sequential requests measuring sub-millisecond in-process cache hits against un-cached proxies and Nginx proxy_cache (p50, p90, p95, p99, min, max, mean, stddev).
3. **Single-Flight Coalescing**: 100 simultaneous concurrent requests for the exact same uncached URL, verifying thundering-herd protection and exactly 1 origin fetch.
4. **Different-URL Concurrency**: 100 concurrent requests across distinct corpus assets evaluating multi-worker throughput and tail latencies.
5. **Slow Origin**: Validates non-blocking async worker behavior during simulated 2.0s upstream origin latency.
6. **Failing Origin**: Validates structured error mapping (HTTP 502 Bad Gateway) when the origin returns HTTP 500.
7. **Sustained Load & Memory RSS**: Measures memory footprint over time to verify strict LRU memory bounding.
8. **AVIF vs WebP Encode Cost**: Quantifies CPU processing duration and file size delta between AVIF and WebP encoders.
9. **20-Image Real-World Calibration Corpus**:
   - Evaluates standard 11x11 local-window SSIM (Wang et al. 2004), exact PSNR, and delivered bytes across real photos, graphics, and screenshots.
10. **20-Image Held-Out Synthetic Stress Suite**:
   - Evaluates procedural edge cases (gradients, high-contrast text, 4K resolution, checkerboards, alpha drop-shadows).
