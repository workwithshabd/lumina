#!/usr/bin/env python3
import urllib.request
import urllib.parse
import json
import time
import hmac
import hashlib
import os

KEY_HEX = "b87ee72d9bdae9a943c0ee2b51d296df78ed27534bace77c2d93e30f13344f4d"
KEY_BYTES = bytes.fromhex(KEY_HEX)
PROXY_URL = "http://127.0.0.1:8080/transform"
METRICS_URL = "http://127.0.0.1:9090/metrics"
SRC_URL = "http://127.0.0.1:5001/images/uploads/IMG_0172.jpg"

def rfc3986_quote(val):
    return urllib.parse.quote(str(val), safe="-._~")

def sign_url(params):
    exp = int(time.time()) + 3600
    p = dict(params)
    p["src"] = SRC_URL
    p["exp"] = str(exp)
    
    sorted_keys = sorted(p.keys())
    lines = ["GET", "/transform", str(exp)]
    for k in sorted_keys:
        lines.append(f"{k}={rfc3986_quote(p[k])}")
    canonical = "\n".join(lines)
    sig = hmac.new(KEY_BYTES, canonical.encode("utf-8"), hashlib.sha256).hexdigest()
    
    qp = "&".join([f"{k}={rfc3986_quote(p[k])}" for k in sorted_keys])
    return f"{PROXY_URL}?{qp}&sig={sig}"

def scrape_metrics():
    req = urllib.request.urlopen(METRICS_URL)
    return req.read().decode("utf-8")

def main():
    print("[*] Checking baseline metrics...")
    metrics_before = scrape_metrics()
    
    # Test matrix: Formats x Sizes x Qualities
    test_cases = [
        # Cold test (first run)
        {"name": "AVIF 800x600 (q65)", "params": {"w": 800, "h": 600, "fit": "contain", "format": "avif", "q": 65}},
        # Warm test (identical request - cache hit)
        {"name": "AVIF 800x600 (q65) [Cache Hit]", "params": {"w": 800, "h": 600, "fit": "contain", "format": "avif", "q": 65}},
        # WebP tests
        {"name": "WebP 800x600 (q80)", "params": {"w": 800, "h": 600, "fit": "contain", "format": "webp", "q": 80}},
        {"name": "WebP 400x300 (q65)", "params": {"w": 400, "h": 300, "fit": "contain", "format": "webp", "q": 65}},
        # JPEG tests
        {"name": "JPEG 1200x900 (q85)", "params": {"w": 1200, "h": 900, "fit": "contain", "format": "jpeg", "q": 85}},
        {"name": "JPEG 800x600 (q65)", "params": {"w": 800, "h": 600, "fit": "contain", "format": "jpeg", "q": 65}},
        {"name": "JPEG 200x150 (q40)", "params": {"w": 200, "h": 150, "fit": "contain", "format": "jpeg", "q": 40}},
        # Auto format tests (Client accepts AVIF)
        {"name": "Auto Format 800x600 (Accept: AVIF)", "params": {"w": 800, "h": 600, "fit": "contain", "format": "auto", "q": 65}, "accept": "image/avif,image/webp,*/*"},
        # PNG lossless test
        {"name": "PNG 400x300", "params": {"w": 400, "h": 300, "fit": "contain", "format": "png"}},
    ]
    
    source_bytes = os.path.getsize("dashboard/server/uploads/IMG_0172.jpg")
    results = []
    
    print(f"[*] Executing transformation matrix on IMG_0172 ({source_bytes} bytes, 5712x4284)...")
    
    for tc in test_cases:
        url = sign_url(tc["params"])
        headers = {}
        if "accept" in tc:
            headers["Accept"] = tc["accept"]
        else:
            headers["Accept"] = "image/avif,image/webp,*/*"
            
        t0 = time.perf_counter()
        req = urllib.request.Request(url, headers=headers)
        try:
            resp = urllib.request.urlopen(req, timeout=15)
            data = resp.read()
            elapsed_ms = (time.perf_counter() - t0) * 1000
            
            output_bytes = len(data)
            savings_pct = ((output_bytes - source_bytes) / source_bytes) * 100
            cache_status = resp.headers.get("x-atihsrah-cache", "MISS")
            content_type = resp.headers.get("content-type", "")
            
            res = {
                "name": tc["name"],
                "status": resp.status,
                "contentType": content_type,
                "latencyMs": round(elapsed_ms, 2),
                "cache": cache_status,
                "originalBytes": source_bytes,
                "transformedBytes": output_bytes,
                "savingsPct": round(savings_pct, 2),
                "params": tc["params"],
                "targetUrl": url,
            }
            results.append(res)
            print(f"  [+] {tc['name']}: {resp.status} | {content_type} | {output_bytes/1024:.1f} KB ({savings_pct:.1f}%) | {elapsed_ms:.1f} ms | Cache: {cache_status}")
        except Exception as e:
            elapsed_ms = (time.perf_counter() - t0) * 1000
            print(f"  [-] {tc['name']} FAILED ({elapsed_ms:.1f} ms): {e}")
            results.append({
                "name": tc["name"],
                "status": 500,
                "error": str(e),
                "latencyMs": round(elapsed_ms, 2),
            })
            
    print("\n[*] Scraping final Prometheus metrics...")
    metrics_after = scrape_metrics()
    
    # Generate Output Files
    output_json = {
        "inputImage": {
            "originalFile": "IMG_0172.HEIC",
            "sourceFile": "IMG_0172.jpg",
            "dimensions": "5712x4284 (24.5 MP)",
            "sizeBytes": source_bytes,
            "sizeMB": round(source_bytes / (1024 * 1024), 2),
        },
        "summary": {
            "totalTests": len(results),
            "successfulTests": len([r for r in results if r.get("status") == 200]),
            "avgLatencyMs": round(sum(r["latencyMs"] for r in results) / len(results), 2),
            "cacheHitLatencyMs": next((r["latencyMs"] for r in results if r.get("cache") == "HIT"), None),
            "maxBandwidthSavingsPct": min(r["savingsPct"] for r in results if "savingsPct" in r),
        },
        "results": results,
        "prometheusMetrics": {
            "endpoint": METRICS_URL,
            "raw": metrics_after,
        }
    }
    
    json_path = "monitoring/metrics_IMG_0172.json"
    with open(json_path, "w") as f:
        json.dump(output_json, f, indent=2)
    print(f"[+] Saved structured metrics JSON to: {json_path}")
    
    # Also generate Markdown report
    md_path = "monitoring/metrics_IMG_0172.md"
    with open(md_path, "w") as f:
        f.write("# Atihsrah Benchmark & Metrics Report: IMG_0172\n\n")
        f.write(f"- **Input Image**: `IMG_0172.HEIC` (Transcoded source: `IMG_0172.jpg`)\n")
        f.write(f"- **Original Dimensions**: `5712 × 4284` (24.5 Megapixels)\n")
        f.write(f"- **Original File Size**: `{source_bytes:,} bytes` ({source_bytes / (1024*1024):.2f} MB)\n")
        f.write(f"- **Total Tests Executed**: `{len(results)}`\n")
        f.write(f"- **Average Transformation Latency**: `{output_json['summary']['avgLatencyMs']} ms`\n")
        f.write(f"- **Warm Cache Hit Latency**: `{output_json['summary']['cacheHitLatencyMs']} ms`\n")
        f.write(f"- **Maximum Bandwidth Savings**: `{output_json['summary']['maxBandwidthSavingsPct']}%`\n\n")
        f.write("## 1. Transformation Results Table\n\n")
        f.write("| Test Scenario | Format | Dimensions | Delivered Size | Savings | Latency | Cache |\n")
        f.write("| :--- | :---: | :---: | :---: | :---: | :---: | :---: |\n")
        for r in results:
            if r.get("status") == 200:
                p = r["params"]
                dim = f"{p.get('w', 'auto')}×{p.get('h', 'auto')}"
                delivered = f"{r['transformedBytes'] / 1024:.1f} KB"
                f.write(f"| **{r['name']}** | `{r['contentType']}` | `{dim}` | `{delivered}` | `{r['savingsPct']}%` | `{r['latencyMs']} ms` | `{r['cache']}` |\n")
        
        f.write("\n## 2. Live Prometheus Scrape Snapshot (`/metrics`)\n\n")
        f.write("```prometheus\n")
        f.write(metrics_after)
        f.write("```\n")
        
    print(f"[+] Saved Markdown metrics report to: {md_path}")

if __name__ == "__main__":
    main()

