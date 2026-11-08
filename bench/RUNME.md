# Atihsrah Benchmark Reproduction Guide (macOS Local Execution)

This guide provides exact step-by-step instructions to execute the reproducible benchmark suite locally on Apple Silicon macOS hardware.

---

## 1. System Requirements & Prerequisites

- **Host Machine**: Apple Mac (Apple Silicon M-series recommended: M1/M2/M3/M4) running macOS 14+.
- **Software Prerequisites**:
  - `rustc` & `cargo` 1.80+ (Rust toolchain)
  - `docker` (Docker Desktop or Colima virtualized container engine)
  - `uv` (Fast Python package manager)
  - `curl`, `openssl`, `git`
  - Optional: `ffmpeg` with `libvmaf` filter (for optional video multi-method assessment fusion)

---

## 2. One-Command Local Execution

The benchmark pipeline is fully automated and self-contained:

```bash
# Run full benchmark suite locally on Mac
./bench/run_all.sh
```

For quick smoke testing (15-second sustained load instead of standard soak):
```bash
./bench/run_all.sh --quick
```

To run offline validation checks only (verifies image decoders, checksums, deterministic fixtures, and environment without spinning up background servers):
```bash
./bench/run_all.sh --validate-only
```

---

## 3. Test Corpus Composition & Licensing

The benchmark corpus consists of 20 real images defined in [`bench/corpus_manifest.csv`](./corpus_manifest.csv):
- **14 Photographic images** from the standard Eastman Kodak True Color Lossless Suite (Public Domain).
- **1 High-resolution nature photograph** (`moraine_lake.jpg`, Public Domain via Wikimedia Commons, Gorgo).
- **2 Real UI screenshots**:
  - `kde_spectacle.png`: LGPL-2.0-or-later (KDE Project)
  - `training_module.png`: CC-BY-SA-4.0 (Wikimedia Foundation)
- **3 Transparent Alpha & Vector Graphics**:
  - `python_logo_alpha.png`: Python Software Foundation trademark policy / open web asset
  - `alacritty_logo_alpha.png`: Apache-2.0 (Alacritty project)
  - `world_map_graphic.png`: Public Domain (Wikimedia Commons vector projection)

Checksums are verified automatically against SHA-256 before any benchmark execution.

---

## 4. Fair Format Comparison & Competitor Settings

### Competitor Topology
- **Atihsrah Native**: Pure-Rust release binary (`target/release/atihsrah`) running natively on macOS host silicon (`http://127.0.0.1:8080`, metrics on `:9090`).
- **Atihsrah Docker**: Containerized pure-Rust build running in the Colima Linux VM (`http://127.0.0.1:8084`, metrics on `:9094`).
- **imgproxy OSS**: Docker container (`darthsim/imgproxy:4.0.21@sha256:4dfe68cc26ea5cf010318d03818d4cd30f71fd33fa7e149c4be19adafc20c52e`) running in Colima VM (`http://127.0.0.1:8081`).
- **imgproxy Cached**: Nginx reverse proxy caching imgproxy (`http://127.0.0.1:8082`).
- **Thumbor Native**: Native macOS process via pinned Python 3.12 virtualenv (`http://127.0.0.1:8888`).
- **Mock Origin**: Custom HTTPS origin with self-signed test CA (`https://127.0.0.1:8000`).

### Format Parity & Capability Disclosures
- **Matched AVIF**: Evaluates Atihsrah (q=65) vs imgproxy (q=65). Both proxies encode lossy AVIF at matched target quality.
- **Matched WebP**:
  - Atihsrah generates lossless WebP via pure-Rust `image-rs` (`WebPEncoder::new_lossless`).
  - imgproxy and Thumbor generate lossy WebP at q=75 via C `libwebp` / Pillow.
  - *Disclosure*: Atihsrah's pure-Rust build does not link C `libwebp` and therefore does not support lossy WebP. WebP comparisons are clearly labeled as encoding mode trade-offs (lossless fidelity vs lossy compression).
- **Client Auto Negotiation**: Tested as a distinct scenario (`Accept: image/avif,image/webp,*/*`) to evaluate format selection policies rather than compression efficiency at identical format.

---

## 5. Metrics & Decision Rules

1. **Structural Similarity (SSIM)**:
   - Standard 11x11 circular symmetric Gaussian window ($\sigma = 1.5$) via `scikit-image` (Wang et al. 2004).
   - Alpha transparency composited over standard opaque white matte `(255, 255, 255)` to prevent hidden variations in $A=0$ pixels from corrupting RGB covariance.
2. **Peak Signal-to-Noise Ratio (PSNR)**:
   - Exact mathematical $10 \log_{10}(255^2 / MSE)$. Returns `inf` on exact pixel identity.
3. **Lossless Verification**:
   - `np.array_equal` exact pixel equality enforced for lossless transformations where dimensions match.
4. **Decision Thresholds**:
   - Minimum SSIM for lossy compression: $\ge 0.90$.
   - A smaller file **cannot win** if its quality falls below the minimum threshold.
   - Byte size difference $\le 1.0\%$ is recorded as a **Tie**.

---

## 6. Output Directory Structure & Interpretation

Every execution creates a unique, timestamped output directory (`benchmarks/runs/run_<YYYYMMDD_HHMMSS>/`):

```text
benchmarks/runs/run_<timestamp>/
├── system_info.json        # Machine-readable host, OS, CPU, RAM, Docker, Git metadata
├── raw_results.json        # Machine-readable latencies, RSS samples, HTTP statuses, and metrics
├── results.csv             # Per-image CSV breakdown of bytes, SSIM, PSNR, and deltas
├── methodology.md          # Environmental record and reproduction commands
├── analysis.md             # Empirical comparative analysis and matrix
├── validation.log          # Offline validation and health check log
└── output/                 # Decoded and transformed image outputs
    ├── reference/          # Ground-truth Pillow Lanczos reference PNGs
    ├── atihsrah_native/    # Atihsrah Native transformed images
    ├── atihsrah_docker/    # Atihsrah Docker transformed images
    ├── imgproxy/           # imgproxy transformed images
    └── thumbor/            # Thumbor transformed images
```

Existing prior runs in `benchmarks/` are never overwritten or deleted.

---

## 7. Security & Localhost Isolation

- **Localhost-Only Bindings**: Every published service port binds strictly to `127.0.0.1`. No services are accessible from external or local area networks.
- **Guaranteed Cleanup**: Bash `trap cleanup EXIT INT TERM` guarantees that background processes (`target/release/atihsrah`, `thumbor`) and Docker containers are terminated upon completion, failure, or interrupt (`Ctrl-C`).

---

## 8. Troubleshooting

- **Missing Container Runtime**: If Docker is not running, start Colima (`colima start`) or Docker Desktop.
- **Python Dependencies**: All dependencies are locked in `bench/requirements.txt` and installed into `.venv_bench`. Run `uv pip install -r bench/requirements.txt --python .venv_bench` to refresh.
- **Port Conflicts**: Ensure ports 8000, 8080, 8081, 8082, 8084, 8888, 9090, 9094 are free before starting.
- **Optional VMAF**: If FFmpeg lacks `libvmaf`, VMAF is safely omitted and recorded as `None` without failing the run.
