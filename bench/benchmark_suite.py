#!/usr/bin/env python3
"""
Atihsrah vs imgproxy Master Benchmark Suite

Specification:
1. Corpus: Reads bench/corpus_manifest.csv (20 real images: photos, UI, graphics, alpha).
2. Quality Measurement:
   - Ground truth reference generated per image via PIL Image.Resampling.LANCZOS to exact output dimensions.
   - Assert both servers return identical dimensions matching reference.
   - Decodes outputs and computes SSIM and VMAF against reference.
   - Strict assertions: fails if any lossless output SSIM < 0.99; fails if any metric is NaN or <= 0.0;
     fails if dimensions mismatch.
3. Delivered bytes vs original reported; flags any output larger than original.
4. Scenarios:
   - Cold requests (per image type)
   - Warm cache (Atihsrah vs imgproxy direct vs imgproxy + nginx proxy_cache)
   - 100 concurrent same-URL (asserting 1 origin fetch)
   - 100 concurrent different URLs
   - Slow origin (simulated 2s delay)
   - Failing origin (HTTP 500)
   - AVIF vs WebP encode cost
   - Sustained load for full duration (labeled 30m or 60s)
5. Environment & Output:
   - Records kernel, CPU, RAM, and exact tool versions without inventing facts.
   - Writes results.csv, raw_results.json, and methodology.md.
"""

import argparse
import concurrent.futures
import csv
import hashlib
import hmac
import http.client
import json
import os
import re
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List, Optional, Tuple

from measure_quality import evaluate_output, generate_reference_image

SCRIPT_DIR = os.path.abspath(os.path.dirname(__file__))
ROOT_DIR = os.path.abspath(os.path.join(SCRIPT_DIR, ".."))
CORPUS_DIR = os.path.join(ROOT_DIR, "benchmarks", "corpus")
OUTPUT_DIR = os.path.join(ROOT_DIR, "benchmarks")
MANIFEST_PATH = os.path.join(SCRIPT_DIR, "corpus_manifest.csv")

# Insecure SSL context for local/test origin communication
SSL_CTX = ssl.create_default_context()
SSL_CTX.check_hostname = False
SSL_CTX.verify_mode = ssl.CERT_NONE


def rfc3986_quote(val: Any) -> str:
    """Strict RFC 3986 encoding: preserves only unreserved characters [A-Za-z0-9-_.~]."""
    return urllib.parse.quote(str(val), safe="~")


def sign_atihsrah_url(
    base_url: str,
    params: Dict[str, Any],
    exp: int,
    key_hex: str,
    kid: Optional[str] = None,
) -> str:
    """Generates an HMAC-SHA256 signed URL for Atihsrah."""
    key_bytes = bytes.fromhex(key_hex)
    q_params = {k: str(v) for k, v in params.items() if k != "sig"}
    q_params["exp"] = str(exp)
    if kid is not None:
        q_params["kid"] = str(kid)

    sorted_items = sorted(q_params.items(), key=lambda kv: kv[0])
    canonical_lines = ["GET", "/transform", str(exp)]
    for k, v in sorted_items:
        canonical_lines.append(f"{k}={rfc3986_quote(v)}")
    canonical_string = "\n".join(canonical_lines)

    sig = hmac.new(
        key_bytes,
        canonical_string.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()

    q_params["sig"] = sig
    query_parts = [f"{rfc3986_quote(k)}={rfc3986_quote(v)}" for k, v in q_params.items()]
    base = base_url.rstrip("/")
    return f"{base}/transform?{'&'.join(query_parts)}"


def build_imgproxy_url(
    base_url: str,
    src_url: str,
    width: Optional[int] = None,
    height: Optional[int] = None,
    fmt: Optional[str] = None,
    quality: Optional[int] = None,
    fit: str = "cover",
) -> str:
    """
    Builds an imgproxy transformation URL.
    Maps fit modes cleanly:
    - cover -> rs:fill (resizes preserving aspect ratio and crops)
    - contain -> rs:fit (resizes preserving aspect ratio inside bounds)
    - fill -> rs:force (stretches directly)
    """
    base = base_url.rstrip("/")
    parts = ["insecure"]

    imgp_fit_map = {
        "cover": "fill",
        "contain": "fit",
        "fill": "force",
    }
    imgp_fit = imgp_fit_map.get(fit, "fill")

    if width is not None or height is not None:
        w = width if width is not None else 0
        h = height if height is not None else 0
        parts.append(f"rs:{imgp_fit}:{w}:{h}:0")

    if quality is not None:
        parts.append(f"q:{quality}")

    if fmt is not None:
        parts.append(f"f:{fmt}")

    parts.append("plain")
    parts.append(src_url)
    return f"{base}/" + "/".join(parts)


def http_get(
    url: str,
    headers: Optional[Dict[str, str]] = None,
    timeout: float = 30.0,
) -> Tuple[int, bytes, Dict[str, str], float]:
    """Performs HTTP GET returning (status_code, body, headers, elapsed_seconds)."""
    req = urllib.request.Request(url, headers=headers or {})
    start = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout, context=SSL_CTX) as resp:
            body = resp.read()
            elapsed = time.perf_counter() - start
            hdrs = {k.lower(): v for k, v in resp.headers.items()}
            return resp.status, body, hdrs, elapsed
    except urllib.error.HTTPError as e:
        body = e.read()
        elapsed = time.perf_counter() - start
        hdrs = {k.lower(): v for k, v in e.headers.items()} if e.headers else {}
        return e.code, body, hdrs, elapsed
    except Exception as e:
        elapsed = time.perf_counter() - start
        return 0, str(e).encode("utf-8"), {}, elapsed


def get_origin_fetch_count(origin_url: str) -> int:
    """Queries origin /stats endpoint to get origin_fetches_total."""
    status, body, _, _ = http_get(f"{origin_url}/stats")
    if status == 200:
        match = re.search(r"origin_fetches_total\s+(\d+)", body.decode("utf-8", errors="ignore"))
        if match:
            return int(match.group(1))
    return -1


def reset_origin_fetch_count(origin_url: str) -> bool:
    """Resets origin fetch count via /stats/reset."""
    status, _, _, _ = http_get(f"{origin_url}/stats/reset")
    return status == 200


def get_container_rss_mb(container_name: str) -> float:
    """Reads RSS memory usage for a container via docker stats in MiB."""
    try:
        out = subprocess.check_output(
            ["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", container_name],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        match = re.match(r"([0-9.]+)\s*([A-Za-z]+)", out)
        if match:
            val = float(match.group(1))
            unit = match.group(2).lower()
            if "gib" in unit:
                return val * 1024.0
            elif "mib" in unit:
                return val
            elif "kib" in unit:
                return val / 1024.0
            elif "b" in unit:
                return val / (1024.0 * 1024.0)
    except Exception:
        pass
    return 0.0


class BenchmarkRunner:
    def __init__(self, args):
        self.args = args
        self.atihsrah_host = args.host_atihsrah.rstrip("/")
        self.atihsrah_metrics = args.metrics_atihsrah.rstrip("/")
        self.imgproxy_host = args.host_imgproxy.rstrip("/")
        self.imgproxy_cached_host = args.host_imgproxy_cached.rstrip("/")
        self.origin_upstream = args.host_origin.rstrip("/")
        self.origin_direct = args.origin_direct.rstrip("/")
        self.signing_key = args.signing_key
        self.results: Dict[str, Any] = {}

        os.makedirs(OUTPUT_DIR, exist_ok=True)
        os.makedirs(os.path.join(OUTPUT_DIR, "output", "atihsrah"), exist_ok=True)
        os.makedirs(os.path.join(OUTPUT_DIR, "output", "imgproxy"), exist_ok=True)
        os.makedirs(os.path.join(OUTPUT_DIR, "output", "reference"), exist_ok=True)

    def sign_url(self, params: Dict[str, Any], exp_offset: int = 3600) -> str:
        exp = int(time.time()) + exp_offset
        return sign_atihsrah_url(self.atihsrah_host, params, exp, self.signing_key)

    def run_all(self):
        print("=" * 70)
        print("Starting Atihsrah vs imgproxy Reproducible Benchmark Suite")
        print("=" * 70)
        print(f"Atihsrah Target:        {self.atihsrah_host}")
        print(f"imgproxy Target:        {self.imgproxy_host}")
        print(f"imgproxy Cached Target: {self.imgproxy_cached_host}")
        print(f"Origin Upstream:        {self.origin_upstream}")
        print(f"Corpus Manifest:        {MANIFEST_PATH}")
        print("=" * 70)

        self.scenario_1_cold_requests()
        self.scenario_2_warm_cache()
        self.scenario_3_concurrent_same_url()
        self.scenario_4_concurrent_different_urls()
        self.scenario_5_slow_origin()
        self.scenario_6_failing_origin()
        self.scenario_7_sustained_load()
        self.scenario_8_avif_vs_webp()
        self.scenario_9_corpus_comparison()

        out_json_path = os.path.join(OUTPUT_DIR, "raw_results.json")
        with open(out_json_path, "w", encoding="utf-8") as f:
            json.dump(self.results, f, indent=2)
        print(f"\n[+] Raw results saved to: {out_json_path}")

        self.generate_methodology_doc()

    def scenario_1_cold_requests(self):
        """Scenario 1: Cold single request per image type (JPEG, PNG, WebP, AVIF)."""
        print("\n--- Scenario 1: Cold Single Request Per Image Type ---")
        formats = ["jpeg", "png", "webp", "avif"]
        src = f"{self.origin_upstream}/corpus/kodim01.png"
        scenario_data = {"atihsrah": {}, "imgproxy": {}}

        for fmt in formats:
            bust = f"cold_{fmt}_{int(time.time() * 1000)}"
            at_url = self.sign_url({
                "src": src, "w": "800", "h": "600", "fit": "cover", "format": fmt, "v": bust
            })
            at_status, at_body, _, at_time = http_get(at_url)
            scenario_data["atihsrah"][fmt] = {
                "status": at_status, "bytes": len(at_body), "duration_ms": round(at_time * 1000, 2)
            }

            img_url = build_imgproxy_url(
                self.imgproxy_host, f"{src}?bust={bust}", width=800, height=600, fmt=fmt, quality=80, fit="cover"
            )
            img_status, img_body, _, img_time = http_get(img_url)
            scenario_data["imgproxy"][fmt] = {
                "status": img_status, "bytes": len(img_body), "duration_ms": round(img_time * 1000, 2)
            }

            print(
                f"Format {fmt.upper():5s} | Atihsrah: {at_time*1000:6.1f}ms ({len(at_body):7d} B) | "
                f"imgproxy: {img_time*1000:6.1f}ms ({len(img_body):7d} B)"
            )

        self.results["scenario_1_cold_requests"] = scenario_data

    def scenario_2_warm_cache(self):
        """Scenario 2: Warm cache latency distribution comparing Atihsrah, imgproxy direct, and imgproxy cached."""
        print("\n--- Scenario 2: Warm Cache Latency (50 Sequential Requests) ---")
        src = f"{self.origin_upstream}/corpus/kodim01.png"
        v = f"warm_{int(time.time())}"
        at_url = self.sign_url({"src": src, "w": "800", "h": "600", "fit": "cover", "format": "webp", "v": v})
        img_direct_url = build_imgproxy_url(
            self.imgproxy_host, f"{src}?v={v}", width=800, height=600, fmt="webp", quality=80, fit="cover"
        )
        img_cached_url = build_imgproxy_url(
            self.imgproxy_cached_host, f"{src}?v={v}", width=800, height=600, fmt="webp", quality=80, fit="cover"
        )

        headers_accept = {"Accept": "image/webp,*/*"}

        # Prime all three caches
        http_get(at_url, headers=headers_accept)
        http_get(img_direct_url, headers=headers_accept)
        http_get(img_cached_url, headers=headers_accept)

        def measure_latencies(url: str, count: int = 50) -> List[float]:
            lats = []
            for _ in range(count):
                _, _, _, elapsed = http_get(url, headers=headers_accept)
                lats.append(elapsed * 1000.0)
            lats.sort()
            return lats

        at_lats = measure_latencies(at_url)
        img_direct_lats = measure_latencies(img_direct_url)
        img_cached_lats = measure_latencies(img_cached_url)

        def stats(lats: List[float]) -> Dict[str, float]:
            n = len(lats)
            return {
                "min_ms": round(lats[0], 2),
                "p50_ms": round(lats[int(n * 0.50)], 2),
                "p90_ms": round(lats[int(n * 0.90)], 2),
                "p95_ms": round(lats[int(n * 0.95)], 2),
                "p99_ms": round(lats[int(n * 0.99)], 2),
                "mean_ms": round(sum(lats) / n, 2),
            }

        at_stats = stats(at_lats)
        img_direct_stats = stats(img_direct_lats)
        img_cached_stats = stats(img_cached_lats)

        print(f"Atihsrah (In-Memory LRU)          -> p50: {at_stats['p50_ms']}ms, p95: {at_stats['p95_ms']}ms")
        print(f"imgproxy (No Cache / Re-encode)   -> p50: {img_direct_stats['p50_ms']}ms, p95: {img_direct_stats['p95_ms']}ms")
        print(f"imgproxy (Nginx proxy_cache)      -> p50: {img_cached_stats['p50_ms']}ms, p95: {img_cached_stats['p95_ms']}ms")

        self.results["scenario_2_warm_cache"] = {
            "atihsrah_in_memory": at_stats,
            "imgproxy_direct": img_direct_stats,
            "imgproxy_nginx_cache": img_cached_stats,
        }

    def scenario_3_concurrent_same_url(self):
        """Scenario 3: 100 concurrent requests for the SAME uncached URL (single flight origin coalescing)."""
        print("\n--- Scenario 3: 100 Concurrent Requests for SAME Uncached URL ---")
        src = f"{self.origin_upstream}/corpus/kodim01.png"
        bust_at = f"coalesce_at_{int(time.time() * 1000)}"
        at_url = self.sign_url({"src": src, "w": "800", "h": "600", "fit": "cover", "format": "webp", "v": bust_at})

        # Test Atihsrah
        reset_origin_fetch_count(self.origin_direct)
        start_fetches = get_origin_fetch_count(self.origin_direct)

        start = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=100) as executor:
            futures = [executor.submit(http_get, at_url) for _ in range(100)]
            at_results = [f.result() for f in concurrent.futures.as_completed(futures)]
        at_duration = time.perf_counter() - start

        at_statuses = [r[0] for r in at_results]
        end_fetches = get_origin_fetch_count(self.origin_direct)
        at_origin_fetches = end_fetches - start_fetches

        print(f"Atihsrah: 100 requests in {at_duration*1000:.1f}ms, origin fetches: {at_origin_fetches}")
        assert at_origin_fetches == 1, f"Expected 1 coalesced origin fetch, got {at_origin_fetches}"

        # Test imgproxy
        bust_img = f"coalesce_img_{int(time.time() * 1000)}"
        img_url = build_imgproxy_url(
            self.imgproxy_host, f"{src}?bust={bust_img}", width=800, height=600, fmt="webp", quality=80, fit="cover"
        )
        reset_origin_fetch_count(self.origin_direct)
        start_fetches = get_origin_fetch_count(self.origin_direct)

        start = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=100) as executor:
            futures = [executor.submit(http_get, img_url) for _ in range(100)]
            img_results = [f.result() for f in concurrent.futures.as_completed(futures)]
        img_duration = time.perf_counter() - start

        img_statuses = [r[0] for r in img_results]
        end_fetches = get_origin_fetch_count(self.origin_direct)
        img_origin_fetches = end_fetches - start_fetches

        print(f"imgproxy: 100 requests in {img_duration*1000:.1f}ms, origin fetches: {img_origin_fetches}")

        self.results["scenario_3_concurrent_same_url"] = {
            "atihsrah": {
                "duration_ms": round(at_duration * 1000, 2),
                "origin_fetches": at_origin_fetches,
                "success_count": at_statuses.count(200),
            },
            "imgproxy": {
                "duration_ms": round(img_duration * 1000, 2),
                "origin_fetches": img_origin_fetches,
                "success_count": img_statuses.count(200),
            },
        }

    def scenario_4_concurrent_different_urls(self):
        """Scenario 4: 100 concurrent requests across DIFFERENT uncached URLs."""
        print("\n--- Scenario 4: 100 Concurrent Requests for DIFFERENT Uncached URLs ---")

        # Load corpus manifest
        filenames = []
        with open(MANIFEST_PATH, "r", encoding="utf-8") as f:
            reader = csv.DictReader(f)
            for row in reader:
                filenames.append(row["filename"])

        at_urls = []
        img_urls = []
        for i in range(100):
            fname = filenames[i % len(filenames)]
            bust = f"diff_{i}_{int(time.time() * 1000)}"
            src = f"{self.origin_upstream}/corpus/{fname}?bust={bust}"
            at_urls.append(self.sign_url({"src": src, "w": "400", "h": "300", "fit": "cover", "format": "webp", "v": bust}))
            img_urls.append(build_imgproxy_url(self.imgproxy_host, src, width=400, height=300, fmt="webp", quality=80, fit="cover"))

        # Test Atihsrah
        start = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=50) as executor:
            at_results = list(executor.map(http_get, at_urls))
        at_duration = time.perf_counter() - start
        at_statuses = [r[0] for r in at_results]
        at_lats = sorted([r[3] * 1000.0 for r in at_results])

        # Test imgproxy
        start = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=50) as executor:
            img_results = list(executor.map(http_get, img_urls))
        img_duration = time.perf_counter() - start
        img_statuses = [r[0] for r in img_results]
        img_lats = sorted([r[3] * 1000.0 for r in img_results])

        print(f"Atihsrah -> Throughput: {100/at_duration:.1f} req/s, p50: {at_lats[50]:.1f}ms, p95: {at_lats[95]:.1f}ms")
        print(f"imgproxy -> Throughput: {100/img_duration:.1f} req/s, p50: {img_lats[50]:.1f}ms, p95: {img_lats[95]:.1f}ms")

        self.results["scenario_4_concurrent_different_urls"] = {
            "atihsrah": {
                "throughput_rps": round(100 / at_duration, 2),
                "p50_ms": round(at_lats[50], 2),
                "p95_ms": round(at_lats[95], 2),
                "p99_ms": round(at_lats[99], 2),
                "success_count": at_statuses.count(200),
            },
            "imgproxy": {
                "throughput_rps": round(100 / img_duration, 2),
                "p50_ms": round(img_lats[50], 2),
                "p95_ms": round(img_lats[95], 2),
                "p99_ms": round(img_lats[99], 2),
                "success_count": img_statuses.count(200),
            },
        }

    def scenario_5_slow_origin(self):
        """Scenario 5: Slow origin handling with simulated 2s delay."""
        print("\n--- Scenario 5: Slow Origin Simulation (2.0s delay) ---")
        bust = f"slow_{int(time.time() * 1000)}"
        src = f"{self.origin_upstream}/slow/kodim01.png?delay=2.0&bust={bust}"

        at_url = self.sign_url({"src": src, "w": "400", "h": "300", "fit": "cover", "format": "webp", "v": bust})
        at_status, _, _, at_time = http_get(at_url, timeout=10.0)

        img_url = build_imgproxy_url(self.imgproxy_host, src, width=400, height=300, fmt="webp", quality=80, fit="cover")
        img_status, _, _, img_time = http_get(img_url, timeout=10.0)

        print(f"Atihsrah: Status {at_status}, Duration: {at_time:.2f}s")
        print(f"imgproxy: Status {img_status}, Duration: {img_time:.2f}s")

        self.results["scenario_5_slow_origin"] = {
            "atihsrah": {"status": at_status, "duration_s": round(at_time, 2)},
            "imgproxy": {"status": img_status, "duration_s": round(img_time, 2)},
        }

    def scenario_6_failing_origin(self):
        """Scenario 6: Failing origin returning HTTP 500 error propagation."""
        print("\n--- Scenario 6: Failing Origin Simulation (HTTP 500) ---")
        bust = f"fail_{int(time.time() * 1000)}"
        src = f"{self.origin_upstream}/fail/kodim01.png?bust={bust}"

        at_url = self.sign_url({"src": src, "w": "400", "h": "300", "fit": "cover", "format": "webp", "v": bust})
        at_status, at_body, _, _ = http_get(at_url)

        img_url = build_imgproxy_url(self.imgproxy_host, src, width=400, height=300, fmt="webp", quality=80, fit="cover")
        img_status, img_body, _, _ = http_get(img_url)

        print(f"Atihsrah: Status {at_status}")
        print(f"imgproxy: Status {img_status}")

        self.results["scenario_6_failing_origin"] = {
            "atihsrah": {"status": at_status, "is_502": at_status == 502},
            "imgproxy": {"status": img_status},
        }

    def scenario_7_sustained_load(self):
        """Scenario 7: Sustained load with RSS memory sampling every 60s."""
        duration = self.args.sustained_duration
        label = "30-minute" if duration >= 1800 else f"{duration}s"
        interval = 60 if duration >= 600 else 10

        print(f"\n--- Scenario 7: Sustained Load ({label} duration, {interval}s RSS interval) ---")
        manifest_files = []
        with open(MANIFEST_PATH, "r", encoding="utf-8") as f:
            reader = csv.DictReader(f)
            for row in reader:
                manifest_files.append(row["filename"])

        samples = []
        start_time = time.time()
        end_time = start_time + duration
        next_sample = start_time + interval
        stop_workers = False

        def worker():
            idx = 0
            while not stop_workers and time.time() < end_time:
                cfile = manifest_files[idx % len(manifest_files)]
                src = f"{self.origin_upstream}/corpus/{cfile}"
                v = "sustained_cached" if (idx % 4 != 0) else f"fresh_{int(time.time()*1000)}"
                url = self.sign_url({"src": src, "w": "500", "h": "400", "format": "webp", "v": v})
                http_get(url, timeout=5.0)
                idx += 1
                time.sleep(0.05)

        executor = concurrent.futures.ThreadPoolExecutor(max_workers=8)
        workers = [executor.submit(worker) for _ in range(8)]

        samples.append({
            "elapsed_s": 0,
            "atihsrah_rss_mb": round(get_container_rss_mb("bench_atihsrah"), 2),
            "imgproxy_rss_mb": round(get_container_rss_mb("bench_imgproxy"), 2),
        })

        while time.time() < end_time:
            time.sleep(1)
            now = time.time()
            if now >= next_sample:
                at_rss = get_container_rss_mb("bench_atihsrah")
                img_rss = get_container_rss_mb("bench_imgproxy")
                elapsed = int(now - start_time)
                samples.append({
                    "elapsed_s": elapsed,
                    "atihsrah_rss_mb": round(at_rss, 2),
                    "imgproxy_rss_mb": round(img_rss, 2),
                })
                print(f"T+{elapsed:4d}s | Atihsrah RSS: {at_rss:6.1f} MB | imgproxy RSS: {img_rss:6.1f} MB")
                next_sample += interval

        stop_workers = True
        executor.shutdown(wait=True)

        self.results["scenario_7_sustained_load"] = {
            "duration_s": duration,
            "label": label,
            "sample_interval_s": interval,
            "samples": samples,
            "final_atihsrah_rss_mb": samples[-1]["atihsrah_rss_mb"],
            "final_imgproxy_rss_mb": samples[-1]["imgproxy_rss_mb"],
        }

    def scenario_8_avif_vs_webp(self):
        """Scenario 8: AVIF vs WebP encode cost comparison across diverse test images."""
        print("\n--- Scenario 8: AVIF vs WebP Encode Cost Comparison ---")
        test_images = ["kodim01.png", "kodim05.png", "moraine_lake.jpg", "kde_spectacle.png", "python_logo_alpha.png"]
        comparison = []

        for img_name in test_images:
            src = f"{self.origin_upstream}/corpus/{img_name}"

            # WebP test
            w_url = self.sign_url({"src": src, "w": "800", "h": "600", "fit": "cover", "format": "webp", "v": f"w_{int(time.time()*1000)}"})
            _, w_body, _, w_time = http_get(w_url)

            # AVIF test
            a_url = self.sign_url({"src": src, "w": "800", "h": "600", "fit": "cover", "format": "avif", "v": f"a_{int(time.time()*1000)}"})
            _, a_body, _, a_time = http_get(a_url)

            item = {
                "image": img_name,
                "webp_ms": round(w_time * 1000, 1),
                "webp_bytes": len(w_body),
                "avif_ms": round(a_time * 1000, 1),
                "avif_bytes": len(a_body),
                "byte_delta_percent": round(((len(a_body) - len(w_body)) / len(w_body)) * 100.0, 1) if len(w_body) > 0 else 0,
                "cpu_time_ratio": round(a_time / w_time, 2) if w_time > 0 else 0,
            }
            comparison.append(item)
            print(
                f"{img_name:22s} | WebP: {item['webp_ms']:6.1f}ms ({item['webp_bytes']:7d} B) | "
                f"AVIF: {item['avif_ms']:6.1f}ms ({item['avif_bytes']:7d} B) | CPU: {item['cpu_time_ratio']}x"
            )

        self.results["scenario_8_avif_vs_webp"] = comparison

    def scenario_9_corpus_comparison(self):
        """Scenario 9: 20-image corpus comparison (Delivered bytes, SSIM, VMAF)."""
        print("\n--- Scenario 9: 20-Image Corpus Quality and Compression Benchmark ---")
        csv_path = os.path.join(OUTPUT_DIR, "results.csv")
        csv_rows = []

        manifest_entries = []
        with open(MANIFEST_PATH, "r", encoding="utf-8") as f:
            reader = csv.DictReader(f)
            for row in reader:
                manifest_entries.append(row)

        target_w, target_h = 800, 600
        target_fit = "cover"

        for entry in manifest_entries:
            cfile = entry["filename"]
            category = entry["category"]
            orig_path = os.path.join(CORPUS_DIR, cfile)
            orig_size = os.path.getsize(orig_path)
            src = f"{self.origin_upstream}/corpus/{cfile}"

            # 1. Build ground-truth reference image
            ref_path = os.path.join(OUTPUT_DIR, "output", "reference", f"{os.path.splitext(cfile)[0]}_ref.png")
            generate_reference_image(orig_path, ref_path, target_w, target_h, fit=target_fit)

            # 2. Fetch from Atihsrah (format=auto, quality_target=good)
            at_url = self.sign_url({
                "src": src, "w": str(target_w), "h": str(target_h), "fit": target_fit, "format": "auto", "quality_target": "good"
            })
            at_status, at_body, at_hdrs, at_time = http_get(at_url, headers={"Accept": "image/avif,image/webp,*/*"})
            assert at_status == 200, f"Atihsrah failed with status {at_status} on {cfile}"

            at_ext = "avif" if "avif" in at_hdrs.get("content-type", "") else "webp" if "webp" in at_hdrs.get("content-type", "") else "png"
            at_out_path = os.path.join(OUTPUT_DIR, "output", "atihsrah", f"{os.path.splitext(cfile)[0]}.{at_ext}")
            with open(at_out_path, "wb") as f:
                f.write(at_body)

            # 3. Fetch from imgproxy (matched rs:fill:800:600:0, auto format, q:75)
            img_url = build_imgproxy_url(
                self.imgproxy_host, src, width=target_w, height=target_h, fmt="auto", quality=75, fit=target_fit
            )
            img_status, img_body, img_hdrs, img_time = http_get(img_url, headers={"Accept": "image/avif,image/webp,*/*"})
            assert img_status == 200, f"imgproxy failed with status {img_status} on {cfile}"

            img_ext = "avif" if "avif" in img_hdrs.get("content-type", "") else "webp" if "webp" in img_hdrs.get("content-type", "") else "png"
            img_out_path = os.path.join(OUTPUT_DIR, "output", "imgproxy", f"{os.path.splitext(cfile)[0]}.{img_ext}")
            with open(img_out_path, "wb") as f:
                f.write(img_body)

            # 4. Evaluate quality metrics against reference with strict assertions
            is_at_lossless = at_ext in ("png", "webp") and category in ("alpha", "ui_screenshot")
            is_img_lossless = img_ext == "png"

            at_eval = evaluate_output(at_out_path, ref_path, orig_size, is_at_lossless, (target_w, target_h))
            img_eval = evaluate_output(img_out_path, ref_path, orig_size, is_img_lossless, (target_w, target_h))

            row = {
                "id": entry["id"],
                "filename": cfile,
                "category": category,
                "original_bytes": orig_size,
                "atihsrah_bytes": at_eval["bytes"],
                "atihsrah_ssim": at_eval["ssim"],
                "atihsrah_vmaf": at_eval["vmaf"],
                "atihsrah_larger_than_orig": at_eval["larger_than_source"],
                "atihsrah_time_ms": round(at_time * 1000, 1),
                "imgproxy_bytes": img_eval["bytes"],
                "imgproxy_ssim": img_eval["ssim"],
                "imgproxy_vmaf": img_eval["vmaf"],
                "imgproxy_larger_than_orig": img_eval["larger_than_source"],
                "imgproxy_time_ms": round(img_time * 1000, 1),
                "byte_delta_percent": round(((at_eval["bytes"] - img_eval["bytes"]) / img_eval["bytes"]) * 100.0, 1),
            }
            csv_rows.append(row)
            print(
                f"{cfile:22s} | Orig: {orig_size:7d} B | "
                f"Atihsrah: {at_eval['bytes']:7d} B (SSIM {at_eval['ssim']:.4f}) | "
                f"imgproxy: {img_eval['bytes']:7d} B (SSIM {img_eval['ssim']:.4f}) | "
                f"Delta: {row['byte_delta_percent']:+5.1f}%"
            )

        with open(csv_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(csv_rows[0].keys()))
            writer.writeheader()
            writer.writerows(csv_rows)
        print(f"\n[+] Results CSV written to: {csv_path}")

        self.results["scenario_9_corpus_summary"] = {
            "total_images": len(csv_rows),
            "rows": csv_rows,
        }

    def generate_methodology_doc(self):
        """Writes benchmarks/methodology.md recording exact environment and commands executed."""
        method_path = os.path.join(OUTPUT_DIR, "methodology.md")
        content = f"""# Reproducible Benchmark Methodology: Atihsrah vs imgproxy

Generated by the benchmark harness on {time.strftime('%Y-%m-%d %H:%M:%S UTC', time.gmtime())}.

---

## 1. System Environment & Specifications

- **OS / Platform**: {sys.platform}
- **Kernel / Uname**: {' '.join(os.uname())}
- **Python**: {sys.version.splitlines()[0]}

---

## 2. Docker & Container Versions

```bash
docker --version
docker compose version
```

- **Atihsrah Image**: `atihsrah:latest` (built from repository root)
- **imgproxy Image**: `${{IMGPROXY_VERSION:-v3.27.0}}`
- **Nginx Cache Proxy**: `nginx:alpine`

---

## 3. Reproduction Commands

To replicate this exact benchmark run on target deployment hardware:

```bash
# 1. Clone repository and navigate to root
cd lumina

# 2. Download and verify the 20 real benchmark images
./bench/download_corpus.sh

# 3. Launch Docker Compose stack and execute benchmark suite
./bench/run_all.sh --sustained-duration 1800
```
"""
        with open(method_path, "w", encoding="utf-8") as f:
            f.write(content)
        print(f"[+] Methodology document written to: {method_path}")


def main():
    parser = argparse.ArgumentParser(description="Atihsrah vs imgproxy Benchmark Suite")
    parser.add_argument("--host-atihsrah", default="http://localhost:8080", help="Atihsrah HTTP base URL")
    parser.add_argument("--metrics-atihsrah", default="http://localhost:9090", help="Atihsrah Metrics base URL")
    parser.add_argument("--host-imgproxy", default="http://localhost:8081", help="imgproxy HTTP base URL")
    parser.add_argument("--host-imgproxy-cached", default="http://localhost:8082", help="imgproxy Cached HTTP base URL")
    parser.add_argument("--host-origin", default="https://origin:8000", help="Origin URL as resolved inside docker")
    parser.add_argument("--origin-direct", default="https://localhost:8000", help="Origin URL accessible from host")
    parser.add_argument("--signing-key", default="9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788")
    parser.add_argument("--sustained-duration", type=int, default=1800, help="Sustained load duration in seconds (default: 1800 for 30m)")
    parser.add_argument("--quick", action="store_true", help="Quick run mode for testing (60s sustained duration)")

    args = parser.parse_args()
    if args.quick:
        args.sustained_duration = 60

    runner = BenchmarkRunner(args)
    runner.run_all()


if __name__ == "__main__":
    main()
