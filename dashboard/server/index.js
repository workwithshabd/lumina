/**
 * Express Server for Atihsrah MERN Dashboard
 *
 * Provides REST APIs, image source hosting, signature computation,
 * live proxy testing, telemetry scraping, and preset/history persistence.
 */

const express = require('express');
const cors = require('cors');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const multer = require('multer');
const { performance } = require('perf_hooks');
const { execSync } = require('child_process');

const { initDb, dbService } = require('./db');
const {
  rfc3986Encode,
  buildCanonicalString,
  computeSignature,
  signAtihsrahUrl,
} = require('../../examples/node/signer');

const app = express();
const PORT = process.env.PORT || 5001;
const ATIHSRAH_PROXY_URL = process.env.ATIHSRAH_PROXY_URL || 'http://127.0.0.1:8080';
const ATIHSRAH_METRICS_URL = process.env.ATIHSRAH_METRICS_URL || 'http://127.0.0.1:9090';

// Default signing secret (32-byte hex generated via openssl rand -hex 32)
const DEFAULT_SIGNING_KEY =
  process.env.ATIHSRAH_SIGNING_KEY ||
  'b87ee72d9bdae9a943c0ee2b51d296df78ed27534bace77c2d93e30f13344f4d';

// Directories
const CORPUS_DIR = path.resolve(__dirname, '../../benchmarks/corpus');
const MANIFEST_PATH = path.resolve(__dirname, '../../bench/corpus_manifest.csv');
const UPLOADS_DIR = path.resolve(__dirname, 'uploads');
const PUBLIC_DIR = path.resolve(__dirname, 'public');

if (!fs.existsSync(UPLOADS_DIR)) {
  fs.mkdirSync(UPLOADS_DIR, { recursive: true });
}

// Multer Storage for image uploads
const storage = multer.diskStorage({
  destination: (req, file, cb) => cb(null, UPLOADS_DIR),
  filename: (req, file, cb) => {
    const ext = path.extname(file.originalname).toLowerCase();
    const safeName = path.basename(file.originalname, ext).replace(/[^a-zA-Z0-9_-]/g, '_');
    cb(null, `${Date.now()}_${safeName}${ext}`);
  },
});
const upload = multer({
  storage,
  limits: { fileSize: 25 * 1024 * 1024 }, // 25 MB max
});

app.use(cors());
app.use(express.json());

// Serve Static Assets with on-the-fly HEIC/RAW transcoding fallback
app.use('/images/corpus', express.static(CORPUS_DIR));

app.use('/images/uploads/:filename', (req, res, next) => {
  const fname = req.params.filename;
  const ext = path.extname(fname).toLowerCase();
  const filePath = path.join(UPLOADS_DIR, fname);

  // If a HEIC, HEIF, TIFF, or RAW file is requested, ensure a PNG converted version is served
  if (['.heic', '.heif', '.tiff', '.tif', '.bmp', '.raw', '.dng', '.cr2', '.nef'].includes(ext)) {
    const pngName = `${path.basename(fname, ext)}.png`;
    const pngTranscodedName = `${path.basename(fname, ext)}_transcoded.png`;
    const pngPath = path.join(UPLOADS_DIR, pngName);
    const pngTranscodedPath = path.join(UPLOADS_DIR, pngTranscodedName);

    if (fs.existsSync(pngTranscodedPath)) {
      return res.sendFile(pngTranscodedPath);
    }
    if (fs.existsSync(pngPath)) {
      return res.sendFile(pngPath);
    }
    if (fs.existsSync(filePath)) {
      try {
        execSync(`sips -s format png "${filePath}" --out "${pngTranscodedPath}"`, { stdio: 'pipe' });
        if (fs.existsSync(pngTranscodedPath)) {
          return res.sendFile(pngTranscodedPath);
        }
      } catch (e) {
        console.warn(`sips transcoding failed for ${fname}:`, e.message);
      }
    }
  }
  next();
});

app.use('/images/uploads', express.static(UPLOADS_DIR));
if (fs.existsSync(PUBLIC_DIR)) {
  app.use(express.static(PUBLIC_DIR));
}

// -----------------------------------------------------------------------------
// Helper: Parse CSV manifest
// -----------------------------------------------------------------------------
function loadCorpusManifest() {
  if (!fs.existsSync(MANIFEST_PATH)) {
    return [];
  }
  const lines = fs.readFileSync(MANIFEST_PATH, 'utf8').split('\n').filter(Boolean);
  const items = [];
  const header = lines[0].split(',');
  for (let i = 1; i < lines.length; i++) {
    const cols = lines[i].split(',');
    if (cols.length >= 7) {
      const filename = cols[1];
      const filePath = path.join(CORPUS_DIR, filename);
      let sizeBytes = 0;
      if (fs.existsSync(filePath)) {
        sizeBytes = fs.statSync(filePath).size;
      }
      items.push({
        id: cols[0],
        filename: cols[1],
        format: cols[2],
        dimensions: cols[3],
        category: cols[4],
        sha256: cols[5],
        license: cols[6],
        sizeBytes,
        localUrl: `http://127.0.0.1:${PORT}/images/corpus/${filename}`,
      });
    }
  }
  return items;
}

// -----------------------------------------------------------------------------
// Helper: Parse Prometheus Metrics text
// -----------------------------------------------------------------------------
function parsePrometheusText(text) {
  const metrics = {
    cacheHits: 0,
    cacheMisses: 0,
    cacheHitRatioPct: 0,
    cacheBytesCurrent: 0,
    cacheBytesMax: 500 * 1024 * 1024,
    cacheEntries: 0,
    inflightCurrent: 0,
    requestsTotal: 0,
    cacheHitBytes: 0,
    responseBytesTotal: 0,
    requestDurationMsAvg: 0,
    raw: text,
  };

  let reqSum = 0;
  let reqCount = 0;

  const lines = text.split('\n');
  for (const line of lines) {
    if (line.startsWith('#') || !line.trim()) continue;
    const [keyPart, valStr] = line.split(/\s+/);
    const val = parseFloat(valStr) || 0;

    if (keyPart === 'atihsrah_cache_hits_total') {
      metrics.cacheHits = val;
    } else if (keyPart === 'atihsrah_cache_misses_total') {
      metrics.cacheMisses = val;
    } else if (keyPart === 'atihsrah_cache_hit_bytes_total') {
      metrics.cacheHitBytes = val;
    } else if (keyPart === 'atihsrah_response_bytes_total') {
      metrics.responseBytesTotal = val;
    } else if (keyPart === 'atihsrah_cache_size_bytes') {
      metrics.cacheBytesCurrent = val;
    } else if (keyPart === 'atihsrah_cache_entries') {
      metrics.cacheEntries = val;
    } else if (keyPart === 'atihsrah_inflight_requests') {
      metrics.inflightCurrent = val;
    } else if (keyPart === 'atihsrah_request_duration_seconds_sum') {
      reqSum = val;
    } else if (keyPart === 'atihsrah_request_duration_seconds_count') {
      reqCount = val;
    }
  }

  const totalReqs = metrics.cacheHits + metrics.cacheMisses;
  metrics.requestsTotal = totalReqs;
  if (totalReqs > 0) {
    metrics.cacheHitRatioPct = Number(((metrics.cacheHits / totalReqs) * 100).toFixed(2));
  }
  if (reqCount > 0) {
    metrics.requestDurationMsAvg = Number(((reqSum / reqCount) * 1000).toFixed(2));
  }

  return metrics;
}

// -----------------------------------------------------------------------------
// API Routes
// -----------------------------------------------------------------------------

// 1. System & Proxy Status
app.get('/api/status', async (req, res) => {
  const dbStatus = dbService.getStatus();
  let proxyStatus = 'offline';
  let proxyLatency = null;
  let metricsAvailable = false;
  let serverHeaders = {};

  try {
    const t0 = performance.now();
    const resp = await axios.get(`${ATIHSRAH_PROXY_URL}/ready`, { timeout: 1500 });
    proxyLatency = Number((performance.now() - t0).toFixed(2));
    proxyStatus = resp.status === 200 ? 'ready' : `status_${resp.status}`;
    serverHeaders = resp.headers;
  } catch (err) {
    proxyStatus = 'offline';
  }

  try {
    const mResp = await axios.get(`${ATIHSRAH_METRICS_URL}/metrics`, { timeout: 1500 });
    metricsAvailable = mResp.status === 200;
  } catch (e) {
    metricsAvailable = false;
  }

  res.json({
    proxy: {
      url: ATIHSRAH_PROXY_URL,
      status: proxyStatus,
      latencyMs: proxyLatency,
      headers: serverHeaders,
    },
    metrics: {
      url: ATIHSRAH_METRICS_URL,
      available: metricsAvailable,
    },
    database: dbStatus,
    dashboardPort: PORT,
    defaultKeyHex: DEFAULT_SIGNING_KEY,
  });
});

// 2. Metrics Telemetry
app.get('/api/metrics', async (req, res) => {
  try {
    const resp = await axios.get(`${ATIHSRAH_METRICS_URL}/metrics`, { timeout: 2000 });
    const parsed = parsePrometheusText(resp.data);
    res.json({ success: true, metrics: parsed });
  } catch (err) {
    res.status(502).json({
      success: false,
      error: `Failed to fetch Prometheus metrics from ${ATIHSRAH_METRICS_URL}: ${err.message}`,
      fallbackMetrics: {
        cacheHits: 0,
        cacheMisses: 0,
        cacheHitRatioPct: 0,
        cacheBytesCurrent: 0,
        cacheBytesMax: 500 * 1024 * 1024,
        cacheEntries: 0,
        inflightCurrent: 0,
        requestsTotal: 0,
      },
    });
  }
});

// 3. Corpus Inventory
app.get('/api/corpus', (req, res) => {
  try {
    const items = loadCorpusManifest();
    res.json({ success: true, count: items.length, images: items });
  } catch (err) {
    res.status(500).json({ success: false, error: err.message });
  }
});

// 4. Image Upload with Automatic Transcoding for HEIC/RAW/TIFF
app.post('/api/upload', upload.single('image'), (req, res) => {
  if (!req.file) {
    return res.status(400).json({ success: false, error: 'No image file uploaded' });
  }

  let finalFilename = req.file.filename;
  let finalMime = req.file.mimetype;
  let finalSize = req.file.size;
  const ext = path.extname(req.file.originalname).toLowerCase();

  // If format is HEIC, HEIF, TIFF, BMP, or Apple ProRAW, transcode to high-res PNG via macOS native sips
  if (['.heic', '.heif', '.tiff', '.tif', '.bmp', '.raw', '.dng', '.cr2', '.nef'].includes(ext)) {
    const inputPath = req.file.path;
    const pngFilename = `${path.basename(req.file.filename, ext)}_transcoded.png`;
    const outputPath = path.join(UPLOADS_DIR, pngFilename);

    try {
      execSync(`sips -s format png "${inputPath}" --out "${outputPath}"`, { stdio: 'pipe' });
      if (fs.existsSync(outputPath)) {
        finalFilename = pngFilename;
        finalMime = 'image/png';
        finalSize = fs.statSync(outputPath).size;
        console.log(`[Upload] Transcoded ${ext} image to PNG: ${pngFilename} (${finalSize} bytes)`);
      }
    } catch (err) {
      console.warn(`[Upload] sips transcoding failed for ${ext}:`, err.message);
    }
  }

  const fileUrl = `http://127.0.0.1:${PORT}/images/uploads/${finalFilename}`;
  
  // Extract width and height if possible via sips
  let imgWidth = null;
  let imgHeight = null;
  try {
    const finalFullPath = path.join(UPLOADS_DIR, finalFilename);
    const sipsRes = execSync(`sips -g pixelWidth -g pixelHeight "${finalFullPath}"`, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'ignore'] });
    const wMatch = sipsRes.match(/pixelWidth:\s*(\d+)/);
    const hMatch = sipsRes.match(/pixelHeight:\s*(\d+)/);
    if (wMatch) imgWidth = parseInt(wMatch[1], 10);
    if (hMatch) imgHeight = parseInt(hMatch[1], 10);
  } catch (dimErr) {
    // Sips dimension query optional
  }

  res.json({
    success: true,
    file: {
      filename: finalFilename,
      originalName: req.file.originalname,
      sizeBytes: finalSize,
      mimetype: finalMime,
      localUrl: fileUrl,
      width: imgWidth,
      height: imgHeight,
    },
  });
});

// 5. URL Signing Tool
app.post('/api/sign', (req, res) => {
  try {
    const { baseUrl, params, options } = req.body;
    const proxyBase = baseUrl || ATIHSRAH_PROXY_URL;
    const keyHex = (options && options.keyHex) || DEFAULT_SIGNING_KEY;
    const exp = options && options.exp ? Number(options.exp) : Math.floor(Date.now() / 1000) + 3600;
    const kid = (options && options.kid) || null;

    const canonical = buildCanonicalString(exp, params, kid);
    const signature = computeSignature(canonical, keyHex);
    const signedUrl = signAtihsrahUrl(proxyBase, params, {
      keyHex,
      exp,
      kid,
    });

    res.json({
      success: true,
      canonicalString: canonical,
      signature,
      signedUrl,
      exp,
      kid,
    });
  } catch (err) {
    res.status(400).json({ success: false, error: err.message });
  }
});

// 6. Test Transformation Execution
app.post('/api/test-transform', async (req, res) => {
  try {
    const rawSrc = req.body.src || req.body.url;
    const rawFormat = req.body.format || req.body.f;
    const {
      w,
      h,
      fit,
      q,
      qt,
      allow_upscale,
      v,
      acceptHeader,
      isSigned = true, // default to signed mode if not specified
      keyHex,
      kid,
      customExp,
    } = req.body;

    const src = rawSrc;
    const format = rawFormat;

    if (!src) {
      return res.status(400).json({ success: false, error: 'src (or url) parameter is required' });
    }

    // Build params object strictly adhering to Atihsrah query schema
    const params = { src };
    if (w && Number(w) > 0) params.w = Number(w);
    if (h && Number(h) > 0) params.h = Number(h);
    if (fit && ['cover', 'contain', 'fill'].includes(fit)) {
      params.fit = fit;
    } else {
      params.fit = 'contain';
    }
    if (format && format !== 'auto') params.format = format;
    if (allow_upscale) params.allow_upscale = 1;
    if (v) params.v = String(v);

    // Atihsrah uses 'quality_target' (draft, good, high, lossless) and strictly rejects 'q'
    const resolvedQualityTarget = qt || (q ? (q <= 55 ? 'draft' : q <= 80 ? 'good' : q <= 90 ? 'high' : 'lossless') : null);
    if (resolvedQualityTarget && ['draft', 'good', 'high', 'lossless'].includes(resolvedQualityTarget)) {
      params.quality_target = resolvedQualityTarget;
    }

    let targetUrl = '';
    let canonicalString = '';
    let usedSig = '';

    if (isSigned) {
      const activeKey = keyHex || DEFAULT_SIGNING_KEY;
      const exp = customExp ? Number(customExp) : Math.floor(Date.now() / 1000) + 3600;
      canonicalString = buildCanonicalString(exp, params, kid || null);
      usedSig = computeSignature(canonicalString, activeKey);
      targetUrl = signAtihsrahUrl(ATIHSRAH_PROXY_URL, params, {
        keyHex: activeKey,
        exp,
        kid: kid || null,
      });
    } else {
      const qp = Object.keys(params)
        .map(k => `${rfc3986Encode(k)}=${rfc3986Encode(params[k])}`)
        .join('&');
      targetUrl = `${ATIHSRAH_PROXY_URL}/transform?${qp}`;
    }

    // Measure request latency
    const headers = {};
    if (acceptHeader) {
      headers['Accept'] = acceptHeader;
    }

    const t0 = performance.now();
    let proxyResponse;
    try {
      proxyResponse = await axios.get(targetUrl, {
        headers,
        responseType: 'arraybuffer',
        timeout: 10000,
        validateStatus: () => true, // capture all status codes
      });
    } catch (fetchErr) {
      return res.status(502).json({
        success: false,
        error: `Failed to connect to Atihsrah proxy at ${ATIHSRAH_PROXY_URL}: ${fetchErr.message}`,
        targetUrl,
      });
    }
    const latencyMs = Number((performance.now() - t0).toFixed(2));

    const status = proxyResponse.status;
    const resHeaders = proxyResponse.headers;
    const transformedBuffer = Buffer.from(proxyResponse.data);
    const transformedBytes = transformedBuffer.length;
    const contentType = resHeaders['content-type'] || 'unknown';
    const cacheHeader = resHeaders['x-atihsrah-cache'] || 'NONE';

    // Measure original source bytes if accessible
    let originalBytes = 0;
    try {
      if (src.startsWith(`http://127.0.0.1:${PORT}/images/corpus/`)) {
        const fname = path.basename(src);
        const fpath = path.join(CORPUS_DIR, fname);
        if (fs.existsSync(fpath)) originalBytes = fs.statSync(fpath).size;
      } else if (src.startsWith(`http://127.0.0.1:${PORT}/images/uploads/`)) {
        const fname = path.basename(src);
        const fpath = path.join(UPLOADS_DIR, fname);
        if (fs.existsSync(fpath)) originalBytes = fs.statSync(fpath).size;
      } else {
        const headResp = await axios.head(src, { timeout: 3000 });
        originalBytes = parseInt(headResp.headers['content-length'], 10) || 0;
      }
    } catch (e) {
      originalBytes = 0;
    }

    const byteDeltaPct = originalBytes > 0
      ? Number((((transformedBytes - originalBytes) / originalBytes) * 100).toFixed(2))
      : 0;

    const base64Data = status === 200
      ? `data:${contentType};base64,${transformedBuffer.toString('base64')}`
      : null;

    let errorDetails = null;
    if (status !== 200) {
      try {
        errorDetails = JSON.parse(transformedBuffer.toString('utf8'));
      } catch (e) {
        errorDetails = transformedBuffer.toString('utf8');
      }
    }

    const historyRecord = {
      testUrl: targetUrl,
      params,
      status,
      latencyMs,
      cacheStatus: cacheHeader,
      contentType,
      originalBytes,
      transformedBytes,
      byteDeltaPct,
    };
    await dbService.addHistory(historyRecord);

    res.json({
      success: status === 200,
      status,
      targetUrl,
      latencyMs,
      headers: resHeaders,
      cacheStatus: cacheHeader,
      contentType,
      originalBytes,
      transformedBytes,
      byteDeltaPct,
      isLargerThanSource: originalBytes > 0 && transformedBytes > originalBytes,
      previewBase64: base64Data,
      errorDetails,
      canonicalString,
    });
  } catch (err) {
    res.status(500).json({ success: false, error: err.message });
  }
});

// Helper: Single transformation execution with timing and response decoding
async function executeSingleTransform({
  src,
  w,
  h,
  fit = 'contain',
  format,
  q,
  qt,
  allow_upscale = 0,
  isSigned = false,
  keyHex,
  acceptHeader,
  originalBytes = 0,
  label = '',
}) {
  const itemParams = {
    src,
    w,
    h,
    fit,
    allow_upscale,
  };
  if (format && format !== 'auto') itemParams.format = format;
  if (q) itemParams.q = q;
  if (qt) itemParams.qt = qt;

  let targetUrl = '';
  if (isSigned) {
    const exp = Math.floor(Date.now() / 1000) + 3600;
    targetUrl = signAtihsrahUrl(ATIHSRAH_PROXY_URL, itemParams, {
      keyHex: keyHex || DEFAULT_SIGNING_KEY,
      exp,
    });
  } else {
    const qp = Object.keys(itemParams)
      .filter(k => itemParams[k] !== undefined && itemParams[k] !== null && itemParams[k] !== '')
      .map(k => `${rfc3986Encode(k)}=${rfc3986Encode(itemParams[k])}`)
      .join('&');
    targetUrl = `${ATIHSRAH_PROXY_URL}/transform?${qp}`;
  }

  const headers = {};
  if (acceptHeader) headers['Accept'] = acceptHeader;

  const t0 = performance.now();
  try {
    const resp = await axios.get(targetUrl, {
      headers,
      responseType: 'arraybuffer',
      timeout: 10000,
    });
    const latencyMs = Number((performance.now() - t0).toFixed(2));
    const buf = Buffer.from(resp.data);
    const transformedBytes = buf.length;
    const orig = originalBytes || transformedBytes;
    const deltaPct = Number((((transformedBytes - orig) / orig) * 100).toFixed(2));
    const contentType = resp.headers['content-type'] || 'image/jpeg';
    const base64Data = `data:${contentType};base64,${buf.toString('base64')}`;

    return {
      success: true,
      label,
      targetUrl,
      status: resp.status,
      latencyMs,
      cache: resp.headers['x-atihsrah-cache'] || 'MISS',
      contentType,
      originalBytes: orig,
      transformedBytes,
      deltaPct,
      largerThanSource: orig > 0 && transformedBytes > orig,
      previewBase64: base64Data,
      w,
      h,
      format: format || 'auto',
      q: q || 65,
    };
  } catch (e) {
    const latencyMs = Number((performance.now() - t0).toFixed(2));
    return {
      success: false,
      label,
      targetUrl,
      status: e.response ? e.response.status : 502,
      latencyMs,
      error: e.response && e.response.data ? e.response.data.toString() : e.message,
      originalBytes,
      transformedBytes: 0,
      deltaPct: 0,
      w,
      h,
      format: format || 'auto',
      q: q || 65,
    };
  }
}

// 7. Parallel Batch Corpus Runner (All Images and Variants at Once)
app.post('/api/batch-test', async (req, res) => {
  try {
    const { params = {}, isSigned = false, keyHex, acceptHeader, testAllVariants = false } = req.body;
    const manifest = loadCorpusManifest();
    const wallStart = performance.now();

    const tasks = [];
    if (testAllVariants) {
      // Full matrix across all corpus images in parallel
      const formats = ['auto', 'avif', 'webp', 'jpeg', 'png'];
      const sizes = [{ w: 400, h: 300 }, { w: 800, h: 600 }];
      const qualities = [40, 65, 85];
      for (const img of manifest) {
        for (const f of formats) {
          for (const s of sizes) {
            for (const q of qualities) {
              tasks.push({
                src: img.localUrl,
                originalBytes: img.sizeBytes,
                filename: img.filename,
                category: img.category,
                w: s.w,
                h: s.h,
                fit: params.fit || 'contain',
                format: f,
                q,
                label: `${img.filename} • ${f.toUpperCase()} • ${s.w}×${s.h} • q=${q}`,
              });
            }
          }
        }
      }
    } else {
      // Standard batch: all 20 corpus images run simultaneously in parallel
      for (const img of manifest) {
        tasks.push({
          src: img.localUrl,
          originalBytes: img.sizeBytes,
          filename: img.filename,
          category: img.category,
          w: params.w || 800,
          h: params.h || 600,
          fit: params.fit || 'contain',
          format: params.format,
          q: params.q,
          qt: params.qt,
          label: img.filename,
        });
      }
    }

    // Execute all conversions concurrently
    const rawResults = await Promise.all(
      tasks.map(t =>
        executeSingleTransform({
          ...t,
          isSigned,
          keyHex,
          acceptHeader,
        }).then(r => ({ ...r, id: t.filename, filename: t.filename, category: t.category }))
      )
    );

    const wallClockMs = Number((performance.now() - wallStart).toFixed(2));
    const successful = rawResults.filter(r => r.status === 200);
    const sumLatencyMs = Number(successful.reduce((acc, r) => acc + r.latencyMs, 0).toFixed(2));
    const avgLatency = successful.length > 0
      ? Number((sumLatencyMs / successful.length).toFixed(2))
      : 0;
    const largerCount = successful.filter(r => r.largerThanSource).length;
    const speedup = wallClockMs > 0 ? Number((sumLatencyMs / wallClockMs).toFixed(1)) : 1;

    res.json({
      success: true,
      totalCount: rawResults.length,
      successCount: successful.length,
      largerCount,
      avgLatencyMs: avgLatency,
      wallClockMs,
      sumLatencyMs,
      speedupMultiplier: `${speedup}x`,
      parallelMode: true,
      results: rawResults,
    });
  } catch (err) {
    res.status(500).json({ success: false, error: err.message });
  }
});

// 7b. Multi-Variant Parallel Testing (All Qualities, Sizes & Formats at Once)
app.post('/api/multi-variant-test', async (req, res) => {
  try {
    const {
      src,
      fit = 'contain',
      formats = ['auto', 'avif', 'webp', 'jpeg', 'png'],
      sizes = [
        { label: 'Thumbnail', w: 200, h: 150 },
        { label: 'Small', w: 400, h: 300 },
        { label: 'Medium', w: 800, h: 600 },
        { label: 'Large', w: 1200, h: 900 },
      ],
      qualities = [40, 65, 85],
      isSigned = false,
      keyHex,
      acceptHeader,
      originalBytes = 0,
    } = req.body;

    if (!src) {
      return res.status(400).json({ success: false, error: 'src parameter is required' });
    }

    const wallStart = performance.now();
    const tasks = [];

    for (const f of formats) {
      for (const s of sizes) {
        for (const q of qualities) {
          tasks.push({
            src,
            w: s.w,
            h: s.h,
            fit,
            format: f,
            q,
            originalBytes,
            label: `${f.toUpperCase()} • ${s.label} (${s.w}×${s.h}) • q=${q}`,
            sizeLabel: s.label,
          });
        }
      }
    }

    // Execute ALL variant conversions at once in parallel!
    const results = await Promise.all(
      tasks.map(t =>
        executeSingleTransform({
          ...t,
          isSigned,
          keyHex,
          acceptHeader,
        }).then(r => ({ ...r, sizeLabel: t.sizeLabel }))
      )
    );

    const wallClockMs = Number((performance.now() - wallStart).toFixed(2));
    const successful = results.filter(r => r.status === 200);
    const sumLatencyMs = Number(successful.reduce((acc, r) => acc + r.latencyMs, 0).toFixed(2));
    const speedup = wallClockMs > 0 ? Number((sumLatencyMs / wallClockMs).toFixed(1)) : 1;

    res.json({
      success: true,
      totalVariants: tasks.length,
      successCount: successful.length,
      wallClockMs,
      sumLatencyMs,
      speedupMultiplier: `${speedup}x`,
      variants: results,
    });
  } catch (err) {
    res.status(500).json({ success: false, error: err.message });
  }
});

// 8. Presets CRUD
app.get('/api/presets', async (req, res) => {
  const presets = await dbService.getPresets();
  res.json({ success: true, presets });
});

app.post('/api/presets', async (req, res) => {
  try {
    const { name, description, params, headers } = req.body;
    if (!name || !params) {
      return res.status(400).json({ success: false, error: 'name and params are required' });
    }
    const created = await dbService.createPreset({ name, description, params, headers });
    res.json({ success: true, preset: created });
  } catch (e) {
    res.status(500).json({ success: false, error: e.message });
  }
});

app.delete('/api/presets/:id', async (req, res) => {
  const deleted = await dbService.deletePreset(req.params.id);
  res.json({ success: !!deleted, deleted });
});

// 9. History CRUD
app.get('/api/history', async (req, res) => {
  const history = await dbService.getHistory(req.query.limit || 50);
  res.json({ success: true, history });
});

app.delete('/api/history', async (req, res) => {
  await dbService.clearHistory();
  res.json({ success: true, message: 'History cleared' });
});

// Catch-all for React Single Page Application (SPA) routing
if (fs.existsSync(PUBLIC_DIR)) {
  app.use((req, res, next) => {
    if (req.method === 'GET' && !req.path.startsWith('/api') && !req.path.startsWith('/images')) {
      return res.sendFile(path.join(PUBLIC_DIR, 'index.html'));
    }
    next();
  });
}

// Start Server
async function start() {
  await initDb();
  app.listen(PORT, '127.0.0.1', () => {
    console.log(`=======================================================`);
    console.log(`🚀 Atihsrah MERN Dashboard Backend running!`);
    console.log(`   Address:       http://127.0.0.1:${PORT}`);
    console.log(`   Atihsrah Proxy: ${ATIHSRAH_PROXY_URL}`);
    console.log(`   Metrics URL:    ${ATIHSRAH_METRICS_URL}`);
    console.log(`=======================================================`);
  });
}

start();
