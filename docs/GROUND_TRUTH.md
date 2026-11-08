# Ground Truth Verification Register — Atihsrah

**Date**: 2026-11-08  
**Repository**: Atihsrah (`atihsrah`)  
**Git Commit**: `f3cec1c`  
**Host Architecture**: macOS 15 Darwin / Apple M4 (aarch64)  
**Container Environment**: Colima / Docker Engine v29.9.2 (Linux 6.8.0-117-generic aarch64)  
**Compiler & Tools**:
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- `cargo 1.97.1 (c980f4866 2026-06-30)`
- `cargo-geiger 0.14.0`
- `cargo-audit 0.22.2` (advisory DB: 1422 security advisories)
- `trivy 0.76.1` (Vulnerability DB Updated: 2026-11-07 18:32:58 UTC)

---

## Item 1: WebP Dependency & Linkage Audit

### Exact Command
```bash
grep -in webp Cargo.toml ; grep -in -B1 -A3 'name = "webp\|libwebp' Cargo.lock ; cargo tree -i libwebp-sys 2>&1 | head ; cargo tree | grep -i webp
```

### Raw Output
```text
33:image = { version = "0.25", features = ["jpeg", "png", "webp", "avif"] }
2804-[[package]]
2805:name = "webpki-roots"
2806-version = "1.0.11"
2807-source = "registry+https://github.com/rust-lang/crates.io-index"
2808-checksum = "f5f280bbb2b287bc8e7de7e3852945a0ff50d1bbcb5ed42b6c7a568c546df171"
error: package ID specification `libwebp-sys` did not match any packages
│   ├── image-webp v0.2.4
│   │   │   ├── rustls-webpki v0.103.16
│   │   └── webpki-roots v1.0.11
│   └── webpki-roots v1.0.11 (*)
```

### Plain Statement
**Does the shipping build link libwebp, yes or no?**  
**No.**  
Neither `webp` nor `libwebp-sys` is present in `Cargo.toml` or `Cargo.lock`. WebP encoding and decoding are provided entirely by the pure-Rust `image-webp v0.2.4` crate via `image-rs`. WebP encoding in this configuration is strictly lossless-only; `quality_target` does not affect WebP output size.

---

## Item 2: Unsafe Code Audit & `cargo geiger`

### Exact Command
```bash
git grep -n unsafe src/ ; cargo geiger
```

### Tool Version
`cargo-geiger 0.14.0`

### Raw Output (`git grep -n unsafe src/`)
*(Exit code 1, 0 matches)*

### Raw Output (`cargo geiger`)
```text
Functions  Expressions  Impls  Traits  Methods  Dependency

0/0        0/0          0/0    0/0     0/0      ❓  atihsrah 0.1.0
0/0        0/0          0/0    0/0     0/0      ❓  ├── axum 0.8.9
0/0        0/0          0/0    0/0     0/0      ❓  │   ├── axum-core 0.5.6
40/40      780/826      12/14  1/1     16/20    ☢️ │   │   ├── bytes 1.12.1
16/85      122/622      1/1    0/0     1/1      ☢️ │   │   │   ├── portable-atomic 1.15.0
0/0        0/5          0/0    0/0     0/0      ❓  │   │   │   │   └── serde 1.0.229
0/0        36/36        2/2    0/0     0/0      ☢️ │   │   ├── futures-core 0.3.34
0/0        6/6          0/0    0/0     0/0      ☢️ │   │   ├── http-body-util 0.1.5
...
454/866    40674/74724  1094/1932 86/91   1010/1776
```

### Plain Statement
There is **zero `unsafe`** code in `src/` (0 functions, 0 expressions, 0 impls, 0 traits, 0 methods). The root crate does not declare `#![forbid(unsafe_code)]` at crate root (marked `❓`). Upstream image decoders, cryptographic libraries, and async runtimes (`bytes`, `tokio`, `image`, `rav1e`, `aws-lc-sys`, `zlib-rs`) contain internal `unsafe` usage.

---

## Item 3: Docker Container Build & Sizing Audit

### Dockerfile Base Images (as written)
- Line 8: `FROM rust:1-bookworm AS builder`
- Line 37: `FROM gcr.io/distroless/cc-debian12:latest AS distroless_base`
- Line 39: `FROM debian:bookworm-slim AS rootfs`
- Line 51: `FROM scratch`

### Build Command
```bash
docker build -t atihsrah:truth -f Dockerfile .
```

### Exact Sizing Commands & Raw Output

#### 1. Docker Storage Driver & Image Store
```bash
docker info | grep -i -E "storage driver|containerd|driver status"
```
```text
 Storage Driver: overlayfs
  driver-type: io.containerd.snapshotter.v1
 Runtimes: io.containerd.runc.v2 runc
 containerd version: 193637f7ee8ae5f5aa5248f49e7baa3e6164966e
```
*Note*: The Docker daemon is running with the `io.containerd.snapshotter.v1` image store. Under the containerd image store, `docker image inspect -f '{{.Size}}'` reports the compressed content size of the layer blobs (~13.9 MB), which closely matches the gzipped archive size (~13.7 MB).

#### 2. `docker image inspect -f '{{.Size}}' atihsrah:truth`
```text
13938608
```
*(Compressed blob layer size under containerd store: 13,938,608 bytes / ~13.9 MB)*

#### 3. `docker save atihsrah:truth | gzip | wc -c`
```text
 13738957
```
*(Gzipped container archive transfer size: 13,738,957 bytes / ~13.7 MB)*

#### 4. Unpacked Size Measurement from `docker save`
Summing uncompressed tar layers and unpacked file contents from `docker save`:
```text
Layer 1fe03459f51b: compressed = 175 B, raw tar = 2560 B (0.00 MB), unpacked files sum = 56 B (0.00 MB)
Layer 2d35ae9e848c: compressed = 8752842 B, raw tar = 34448896 B (32.85 MB), unpacked files sum = 33245732 B (31.71 MB)
Layer c360c02b4d94: compressed = 151 B, raw tar = 2560 B (0.00 MB), unpacked files sum = 18 B (0.00 MB)
Layer e33a323d7aa3: compressed = 5181906 B, raw tar = 11553792 B (11.02 MB), unpacked files sum = 11550384 B (11.02 MB)

Total raw uncompressed tar layer sum: 46,007,808 bytes (43.88 MiB / ~46.0 MB)
Total unpacked file content sum: 44,796,190 bytes (42.72 MiB / ~44.8 MB)
```

#### 5. `docker history atihsrah:truth`
```text
IMAGE          CREATED          CREATED BY                                      SIZE      COMMENT
fb433cd26830   17 seconds ago   /bin/sh -c #(nop)  ENTRYPOINT ["/usr/local/b…   0B        
f2437270bc7a   17 seconds ago   /bin/sh -c #(nop)  HEALTHCHECK &{Test:[CMD /…   0B        
de23b5f73413   17 seconds ago   /bin/sh -c #(nop)  EXPOSE 8080 9090             0B        
cca4622dbf8c   17 seconds ago   /bin/sh -c #(nop)  USER 10001:10001             0B        
25d223c80f07   17 seconds ago   /bin/sh -c #(nop)  ENV ATIHSRAH_BIND=0.0.0.0…   0B        
ee790dbd6e41   17 seconds ago   /bin/sh -c #(nop) COPY --chown=10001:10001 f…   11.6MB    
18539626bade   4 days ago       /bin/sh -c #(nop) COPY file:31a10a03c12d6ee3…   12.3kB    
6bee03dd4896   4 days ago       /bin/sh -c #(nop) COPY file:75f9824b472fac3e…   12.3kB    
8825c2dd7de3   4 days ago       /bin/sh -c #(nop) COPY dir:2bd3d6eb7350ac50b…   37.1MB    
```

---

## Item 4: Container Binary Linkage Audit (`file` and `ldd`)

### Extraction Command
```bash
cid=$(docker create atihsrah:truth) && docker cp $cid:/usr/local/bin/atihsrah target/atihsrah_linux && docker rm -v $cid
```

### Exact Commands & Raw Output

#### 1. `file target/atihsrah_linux`
```text
target/atihsrah_linux: ELF 64-bit LSB pie executable, ARM aarch64, version 1 (SYSV), dynamically linked, interpreter /lib/ld-linux-aarch64.so.1, BuildID[sha1]=17560a78b6b4f64fba4243238cf215a3ceff346b, for GNU/Linux 3.7.0, stripped
```

#### 2. `docker run --rm -v "$(pwd)/target/atihsrah_linux:/testbin" debian:bookworm-slim ldd /testbin`
```text
	linux-vdso.so.1 (0x0000fbb0c5740000)
	libgcc_s.so.1 => /lib/aarch64-linux-gnu/libgcc_s.so.1 (0x0000fbb0c4b90000)
	libm.so.6 => /lib/aarch64-linux-gnu/libm.so.6 (0x0000fbb0c4ae0000)
	libc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0000fbb0c4930000)
	/lib/ld-linux-aarch64.so.1 (0x0000fbb0c56f0000)
```

### Plain Statement
**Is the binary static or dynamic?**  
**Dynamic.**  
The binary compiled for the container is an ELF 64-bit PIE dynamically linked executable. It requires the Linux dynamic linker interpreter `/lib/ld-linux-aarch64.so.1` and dynamically links system C libraries (`libc.so.6` glibc, `libm.so.6`, and `libgcc_s.so.1`) provided by the `gcr.io/distroless/cc-debian12` rootfs. It is **not** a statically linked binary.

---

## Item 5: Vulnerability Scans (`trivy image` vs `trivy fs`)

### Tool Version
```bash
trivy --version
```
```text
Version: 0.76.1
Vulnerability DB:
  Version: 2
  UpdatedAt: 2026-11-07 18:32:58.034005789 +0000 UTC
  NextUpdate: 2026-11-08 18:32:58.034005498 +0000 UTC
  DownloadedAt: 2026-11-07 21:44:23.773221 +0000 UTC
```

### Scan 1: Container Image Vulnerability Scan
```bash
trivy image atihsrah:truth --severity HIGH,CRITICAL
```
```text
2026-11-08T09:40:04+05:30	INFO	[vuln] Vulnerability scanning is enabled
2026-11-08T09:40:04+05:30	INFO	[secret] Secret scanning is enabled
2026-11-08T09:40:04+05:30	INFO	[secret] If your scanning is slow, please try '--scanners vuln' to disable secret scanning
2026-11-08T09:40:04+05:30	INFO	[secret] Please see https://trivy.dev/docs/v0.76/guide/scanner/secret#recommendation for faster secret detection
2026-11-08T09:40:04+05:30	INFO	Detected OS	family="debian" version="12.15"
2026-11-08T09:40:04+05:30	INFO	[debian] Detecting vulnerabilities...	os_version="12" pkg_num=10
2026-11-08T09:40:04+05:30	INFO	Number of language-specific files	num=0
2026-11-08T09:40:04+05:30	WARN	Using severities from other vendors for some vulnerabilities. Read https://trivy.dev/docs/v0.76/guide/scanner/vulnerability#severity-selection for details.

Report Summary

┌───────────────────────────────┬────────┬─────────────────┬─────────┐
│            Target             │  Type  │ Vulnerabilities │ Secrets │
├───────────────────────────────┼────────┼─────────────────┼─────────┤
│ atihsrah:truth (debian 12.15) │ debian │        0        │    -    │
└───────────────────────────────┴────────┴─────────────────┴─────────┘
Legend:
- '-': Not scanned
- '0': Clean (no security findings detected)
```

### Scan 2: Filesystem Dependency Vulnerability Scan (Distinct Scan)
```bash
trivy fs . --scanners vuln
```
```text
2026-11-08T09:40:09+05:30	INFO	[vuln] Vulnerability scanning is enabled
2026-11-08T09:40:09+05:30	INFO	Number of language-specific files	num=2
2026-11-08T09:40:09+05:30	INFO	[cargo] Detecting vulnerabilities...

Report Summary

┌─────────────────┬───────┬─────────────────┐
│     Target      │ Type  │ Vulnerabilities │
├─────────────────┼───────┼─────────────────┤
│ Cargo.lock      │ cargo │        0        │
├─────────────────┼───────┼─────────────────┤
│ fuzz/Cargo.lock │ cargo │        0        │
└─────────────────┴───────┴─────────────────┘
Legend:
- '-': Not scanned
- '0': Clean (no security findings detected)
```

---

## Item 6: Live Server Headers in Signed Mode (`curl -sI`)

### Server Startup Environment
```bash
ATIHSRAH_BIND="127.0.0.1:8080"
ATIHSRAH_METRICS_BIND="127.0.0.1:9090"
ATIHSRAH_AUTH_MODE="signed"
ATIHSRAH_SIGNING_KEY="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
ALLOWED_ORIGINS="https://example.com"
ATIHSRAH_PRIVATE_ORIGINS="http://127.0.0.1:9876"
```

### Requests & Headers

#### Case 1: Valid Signed URL WITH `v` Param (`exp = now + 3600`)
**URL**: `http://127.0.0.1:8080/transform?src=http%3A%2F%2F127.0.0.1%3A9876%2Ftest.png&w=1&v=1&exp=1794114814&sig=833380575bc8c0e46862700bba86c3e2d3c3924c87debe81d58df409f3910dbb`

```text
HTTP/1.1 200 OK
x-content-type-options: nosniff
content-type: image/png
x-atihsrah-cache: MISS
cache-control: public, max-age=3600
vary: Accept
x-request-id: 01M6WNS56D5JW5EBKF6VAJANGW
content-length: 73
date: Sun, 08 Nov 2026 04:13:34 GMT
```
- **Cache-Control**: `public, max-age=3600`
- **Vary**: `Accept`

#### Case 2: Valid Signed URL WITHOUT `v` Param (`exp = now + 3600`)
**URL**: `http://127.0.0.1:8080/transform?src=http%3A%2F%2F127.0.0.1%3A9876%2Ftest.png&w=1&exp=1794114814&sig=6a7b6e97679c8abc42cf74c5ad6568d3a9fce9be4cc3ba7e680745e4f849bcef`

```text
HTTP/1.1 200 OK
x-content-type-options: nosniff
content-type: image/png
x-atihsrah-cache: MISS
cache-control: public, max-age=3600
vary: Accept
x-request-id: 01M6WNS56T1FBN4SXHQWJJWXCD
content-length: 73
date: Sun, 08 Nov 2026 04:13:34 GMT
```
- **Cache-Control**: `public, max-age=3600`
- **Vary**: `Accept`

#### Case 3: URL Whose `exp` Is About to Expire (`exp = now + 4`)
**URL**: `http://127.0.0.1:8080/transform?src=http%3A%2F%2F127.0.0.1%3A9876%2Ftest.png&w=1&exp=1794111218&sig=bdb906d1ee7039fea12312b789484ce5b11ff5ae75449fa6a16bde7385d64d32`

```text
HTTP/1.1 200 OK
x-content-type-options: nosniff
content-type: image/png
x-atihsrah-cache: HIT
cache-control: public, max-age=4
vary: Accept
x-request-id: 01M6WNS571YBEXTYNPYDAVBBVG
content-length: 73
date: Sun, 08 Nov 2026 04:13:34 GMT
```
- **Cache-Control**: `public, max-age=4`
- **Vary**: `Accept`

---

## Item 7: Test Suite, Clippy & Audit Verification

### Exact Command
```bash
cargo test --all-targets 2>&1 | tail -30 ; cargo clippy --all-targets -- -D warnings 2>&1 | tail -10 ; cargo audit 2>&1 | tail -15
```

### Raw Output
```text
test test_legacy_client_without_modern_accept_gets_jpeg_for_photo_and_png_for_graphic ... ok
test test_graphic_content_heuristic_and_smaller_selection ... ok
test test_explicit_format_webp_serves_lossless_webp ... ok
test test_larger_than_source_guard_increments_metric_and_does_not_leak_source ... ok
test test_quality_target_strictly_increases_size_for_jpeg_and_avif ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.98s

     Running tests/stage2_signing_vectors.rs (target/debug/deps/stage2_signing_vectors-8e5dea8ead9f691e)

running 1 test
test test_all_signing_vectors ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/transform_pipeline.rs (target/debug/deps/transform_pipeline-10c8e4f294716483)

running 9 tests
test test_reject_duplicate_and_unknown_params ... ok
test test_estimated_processing_memory_limit_rejected ... ok
test test_corrupt_truncated_image_returns_422 ... ok
test test_decompression_bomb_rejected_header_only ... ok
test test_byte_weighted_cache_eviction ... ok
test test_cache_hit_and_miss_lifecycle ... ok
test test_100_concurrent_identical_requests_single_flight ... ok
test test_oversize_entry_not_cached ... ok
test test_immutable_cache_control_with_version_param ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1422 security advisories (from /Users/shabd/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (317 crate dependencies)
Crate:     paste
Version:   1.0.15
Warning:   unmaintained
Title:     paste - no longer maintained
Date:      2024-10-07
ID:        RUSTSEC-2024-0436
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0436

warning: 1 allowed warning found
```

---

## Item 8: Key Test Files & Status on Master

| File Path | Status | Tests Contained | Result on Master |
| :--- | :---: | :--- | :---: |
| `tests/phase_a.rs` | **EXISTS** | `test_a1_leader_cancellation_mid_fetch_and_next_request_succeeds`<br>`test_a1_100_request_herd_with_cancelled_leader`<br>`test_a2_unsigned_claim_kid_cannot_consume_budget`<br>`test_a2_many_fake_kids_do_not_grow_limiter_state`<br>`test_a3_accept_wildcard_second_request_is_hit`<br>`test_a4_avif_and_non_avif_clients_concurrent_get_acceptable_formats`<br>`test_a4_q_zero_exclusion_respected` | **ALL 7 PASS** |
| `tests/stage2_signing_vectors.rs` | **EXISTS** | `test_all_signing_vectors` (20 positive & negative vectors) | **PASS** |
| `tests/security/ssrf.rs` | **EXISTS** *(submodule in `tests/security/main.rs`)* | `test_redirect_rejection`<br>`test_lookalike_hostnames_rejected`<br>`test_ssrf_dns_rebinding_blocked`<br>`test_url_credentials_fragment_port_tricks_rejected`<br>`test_ssrf_all_ip_classes_and_ipv4_mapped_ipv6_blocked`<br>*(Note: `tests/security/` as a whole contains exactly 18 tests across 7 security modules; all 18 pass)* | **ALL 5 PASS** (all 18 in `tests/security/` pass) |
| `tests/audit_remediation.rs` | **EXISTS** | `test_finding_a_trusted_proxy_traversal_prevents_spoofing`<br>`test_finding_a_unauthenticated_kid_spray_blocked_by_ip_limiter`<br>`test_finding_b_cache_key_includes_allow_upscale`<br>`test_finding_c_leader_abort_triggers_follower_takeover`<br>`test_finding_d_queue_cancellation_no_counter_leak`<br>`test_finding_e_private_networks_does_not_disable_tls_validation`<br>`test_finding_f_signed_url_exp_clamps_cache_control_max_age`<br>`test_finding_g_webp_lossless_option_a_invariant_under_quality_target`<br>`test_finding_i_ipv6_special_addresses_blocked`<br>`test_finding_j_accept_negotiation_respects_q_values`<br>`test_finding_k_inflight_key_different_accept_headers`<br>`test_finding_l_exif_orientation_applied_by_image_rs` | **ALL 12 PASS** |

---

## CONTRADICTIONS FOUND

The following contradictions exist between the verified ground truth facts above and existing documentation:

### 1. Static Binary vs. Dynamically Linked Container Binary (`README.md`, `docs/CLAIMS.md`)
- **Documentation Claim**:
  - `README.md:66`: *"Single-binary, zero-C-dependency deployment: You want a statically linked binary or scratch container without system glibc or libvips dependencies."*
  - `README.md:264`: *"Atihsrah is built strictly with pure-Rust image codecs (image-rs and ravif), compiling to a single statically linked binary with zero C library dependencies."*
  - `docs/CLAIMS.md:35-43`: *"Claim 3: Single statically linked binary with zero system dependencies — Status: VERIFIED"*.
- **Ground Truth Fact (Item 4)**:
  - Running `file` on `/usr/local/bin/atihsrah` extracted directly from the container outputs: `ELF 64-bit LSB pie executable ... dynamically linked, interpreter /lib/ld-linux-aarch64.so.1`.
  - Running `ldd` inside the Linux container outputs dynamic dependencies: `libc.so.6` (glibc), `libm.so.6`, `libgcc_s.so.1`, and `/lib/ld-linux-aarch64.so.1`.
  - The shipping binary in the container is **dynamically linked to glibc** provided by Debian/Distroless `cc-debian12`, NOT statically linked.

### 2. WebP C-Library Linkage and Lossy Encoding Claims (`CHANGELOG.md`, `BENCHMARKS.md`)
- **Documentation Claim**:
  - `CHANGELOG.md:73-75`: *"Finding G (WebP Lossy Encoding & Quality Mapping): Integrated `webp = "0.3"` crate (Google `libwebp` bindings). Implemented lossy WebP encoding for `quality < 100`..."*
  - `BENCHMARKS.md:28-60`: Tabulates benchmark metrics across 20 corpus images claiming Atihsrah uses `webp = "0.3.1"` and `libwebp`.
- **Ground Truth Fact (Item 1)**:
  - Neither `webp` nor `libwebp-sys` is present in `Cargo.toml` or `Cargo.lock`.
  - Atihsrah links pure-Rust `image-webp v0.2.4` via `image-rs`, which is strictly lossless-only. `quality_target` does not affect WebP size.

### 3. Container Disk Size Accounting Discrepancy (`docs/CLAIMS.md`, `README.md`)
- **Documentation Claim**:
  - `docs/CLAIMS.md:88-92`: Quotes uncompressed virtual disk size as *"~63 MB (measured via `docker image inspect -f '{{.Size}}' atihsrah:latest`)"* and compressed download size as *"~13.9 MB"*.
  - `README.md:11`: Mentions *(~63 MB uncompressed on disk)*.
- **Ground Truth Fact (Item 3)**:
  - Under Docker Engine's containerd image store (`io.containerd.snapshotter.v1`), `docker image inspect -f '{{.Size}}'` reports the compressed blob layer size of `13938608` bytes (~13.9 MB), which mirrors the gzipped archive size (`13738957` bytes / ~13.7 MB).
  - The actual uncompressed layer size measured by extracting and summing layers from `docker save` is **46,007,808 bytes (43.88 MiB / ~46.0 MB)** for the raw tar layers, and **44,796,190 bytes (42.72 MiB / ~44.8 MB)** for unpacked file contents.
  - The ~63 MB figure in previous documentation reflects legacy graphdriver rootfs allocation or older multi-layer inspect accounting, whereas the current containerd store inspect reports compressed size (~13.9 MB) and unpacked content measures ~44.8–46.0 MB. Both figures must be clearly defined with their exact measurement methods.

### 4. Cache-Control `immutable` and `max-age` in Signed Mode (`docs/SIGNING.md`, `README.md`)
- **Documentation Claim**:
  - `docs/SIGNING.md:29-30` & `README.md:288`: *"When the `v` (cache version) parameter is included in a transformation request, Atihsrah emits: `Cache-Control: public, max-age=31536000, immutable`"*.
  - `CHANGELOG.md:63-65`: Claims Finding F clamped `max-age` by signature expiration `exp`.
- **Ground Truth Fact (Item 6)**:
  - In signed mode, `exp` is required on all requests. In `src/http/transform.rs:487-490`, when both `v` and `remaining_ttl` are present, Atihsrah emits `format!("public, max-age={max_age}")` where `max_age = min(31536000, remaining_ttl)`.
  - It does **NOT** include `immutable` when `exp` is present.
  - Furthermore, `max-age` is clamped by `remaining_ttl` (for example, `exp = now + 3600` emitted `max-age=3600`, not `max-age=31536000`).
  - `immutable` is emitted only when `exp` is absent (which only occurs in unsigned/public mode).

### 5. Non-Existent Configuration Flag (`CHANGELOG.md`)
- **Documentation Claim**:
  - `CHANGELOG.md:60-62`: *"Added independent configuration `ATIHSRAH_DANGER_ACCEPT_INVALID_CERTS` (default false) with explicit security warning logs."*
- **Ground Truth Fact**:
  - `ATIHSRAH_DANGER_ACCEPT_INVALID_CERTS` does not exist in `src/`. The actual configuration implemented in Phase B is `ATIHSRAH_PRIVATE_ORIGINS` and `ATIHSRAH_PRIVATE_CA_FILE`.

### 6. Misleading Test Name vs. Actual Assertion (`tests/audit_remediation.rs`)
- **Documentation / Test Identifier**:
  - `tests/audit_remediation.rs:388` was originally named `test_finding_g_webp_quality_produces_distinct_sizes`.
- **Ground Truth Fact (Item 8)**:
  - The test actually asserts:
    `assert_eq!(out_draft.bytes.len(), out_high.bytes.len(), "Under Option A, WebP is lossless-only and invariant under quality_target");`
  - The test asserts that WebP sizes are identical because WebP is lossless-only under Option A.
  - **Resolution**: Renamed to `test_finding_g_webp_lossless_option_a_invariant_under_quality_target` to accurately describe the asserted behavior.

### 7. Historical Intermediate Benchmark Results (`BENCHMARKS.md`)
- **Documentation Claim**:
  - `BENCHMARKS.md` previously published numbers and latencies for an intermediate build testing C `libwebp`.
- **Ground Truth Fact & Current State**:
  - Those unverified intermediate numbers were quarantined. The benchmark suite was rebuilt from scratch (`bench/run_all.sh`) and executed directly on local Apple Silicon macOS hardware (Apple M4, 10 cores, macOS 27.2).
  - All empirical ground truth is recorded in [`benchmarks/raw_results.json`](../benchmarks/raw_results.json), [`benchmarks/results.csv`](../benchmarks/results.csv), and [`benchmarks/analysis.md`](../benchmarks/analysis.md). Zero fabricated or estimated numbers are retained.

### 8. Absolute Links in Documentation (`docs/CLAIMS.md`)
- **Documentation Defect**:
  - `docs/CLAIMS.md` previously contained absolute local filesystem paths with `file://` schemes.
- **Ground Truth Requirement**:
  - All documentation links must use relative markdown paths (e.g. `../Cargo.toml`, `../src/cache/mod.rs`). Remediation replaced all absolute paths with relative paths.

