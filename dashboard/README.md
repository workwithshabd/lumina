# Atihsrah Studio — MERN Dashboard

A modern, full-stack **MERN (MongoDB, Express, React, Node.js)** interactive studio and testing lab for the **Atihsrah** pure-Rust high-performance image optimization proxy.

---

## 🌟 Key Features

1. **Interactive Studio Playground**:
   - Live visual comparison with an interactive **Split Curtain Slider** (before & after comparison).
   - Side-by-side and single-view visual inspection.
   - Real-time telemetry: Latency (ms), Cache state (`HIT` / `MISS`), Output format, and Byte savings (%).
   - Sliders and inputs for dimensions (`w`, `h`), `fit` (`cover`, `contain`, `fill`, `inside`, `outside`), `format` (`auto`, `avif`, `webp`, `jpeg`, `png`), `q` (quality 1–100), and `qt` (quality target).
   - Client `Accept` header switcher: Modern (`AVIF + WebP`), WebP-only, or Legacy (`JPEG/PNG`).
   - Integrated HMAC-SHA256 signing using Atihsrah's reference RFC 3986 canonical query string builder.

2. **Corpus & Custom Image Manager**:
   - Direct access to all 20 images from the benchmark corpus (Kodak suite, nature photographs, UI screenshots, and alpha logos).
   - Support for custom image file uploads.
   - Support for external HTTPS image URLs (e.g. Unsplash, Wikimedia).

3. **Automated Batch Benchmark**:
   - 1-click execution across all 20 benchmark images.
   - Live table displaying per-image latency, cache state, original vs delivered bytes, and larger-than-source guard verification.

4. **Live Prometheus Telemetry**:
   - Real-time scraping of Atihsrah's isolated metrics listener (`:9090/metrics`).
   - Visual gauges for Cache Hit Ratio (%), Moka LRU memory consumption (MB / 500 MB limit), in-flight single-flight requests, and total requests served.

5. **Presets & History Persistence (MongoDB + Embedded Fallback)**:
   - Save custom transformation configs as reusable presets.
   - Automatic logging of all test runs with latency, cache hit/miss status, and byte savings.
   - Connects to MongoDB via Mongoose when available; automatically falls back to an embedded persistent JSON store if MongoDB is not running locally.

---

## 🚀 Quick Start (1 Command)

To launch both the Atihsrah proxy server and the MERN dashboard simultaneously:

```bash
./dashboard/start.sh
```

Then open your browser at:
👉 **`http://127.0.0.1:5001`**

---

## 🏗️ Architecture

```
┌────────────────────────────────────────────────────────┐
│                   Browser / Client                     │
│          React UI (Vite + Tailwind CSS / Lucide)       │
└───────────────────────────▲────────────────────────────┘
                            │ HTTP
┌───────────────────────────▼────────────────────────────┐
│               Express.js API Server (:5001)            │
│  - Serves static React production bundle               │
│  - Hosts local corpus & uploaded images                │
│  - Computes HMAC-SHA256 signatures                    │
│  - MongoDB (Mongoose) / Embedded JSON Store           │
│  - Scrapes & parses Prometheus metrics (:9090)         │
└───────────────────────────▲────────────────────────────┘
                            │ HTTP /transform
┌───────────────────────────▼────────────────────────────┐
│            Atihsrah Pure-Rust Proxy (:8080)            │
│  - Moka in-memory LRU cache (0.34 ms warm hits)        │
│  - InflightManager single-flight request coalescing    │
│  - CatmullRom resampling & ravif AVIF encoder          │
│  - Multi-stage SSRF DNS rebinding security guards      │
└────────────────────────────────────────────────────────┘
```

---

## ⚙️ Ports & Endpoints

| Service | Address | Purpose |
| :--- | :--- | :--- |
| **Dashboard UI** | `http://127.0.0.1:5001/` | React Single-Page Application |
| **Dashboard API** | `http://127.0.0.1:5001/api/...` | Status, transforms, signing, presets, history |
| **Corpus Image Origin** | `http://127.0.0.1:5001/images/corpus/...` | Test image source for Atihsrah |
| **Atihsrah Proxy** | `http://127.0.0.1:8080/transform` | Core image optimization proxy |
| **Atihsrah Ready Check** | `http://127.0.0.1:8080/ready` | Health probe |
| **Atihsrah Metrics** | `http://127.0.0.1:9090/metrics` | Prometheus metrics scrape endpoint |
| **MongoDB (Optional)** | `mongodb://127.0.0.1:27017/atihsrah_dashboard` | Persistent store (falls back to `server/data/store.json` if absent) |

---

## 🛠️ Development & Manual Execution

### Running the Backend Only
```bash
cd dashboard
node server/index.js
```

### Running Vite Development Server (with Hot Reload)
```bash
cd dashboard/client
npm run dev
# Accessible at http://127.0.0.1:3000 (proxies /api to :5001)
```

### Rebuilding Client Production Bundle
```bash
cd dashboard/client
npm run build
# Outputs directly to dashboard/server/public
```

