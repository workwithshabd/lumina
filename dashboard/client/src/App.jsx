import React, { useState, useEffect, useRef } from 'react';
import {
  Zap,
  Sliders,
  Image as ImageIcon,
  Activity,
  BarChart3,
  Key,
  Bookmark,
  RefreshCw,
  Copy,
  Check,
  Upload,
  ArrowRight,
  TrendingDown,
  TrendingUp,
  Layers,
  ShieldCheck,
  AlertTriangle,
  Play,
  RotateCcw,
  Sparkles,
  Server,
  Database,
  ExternalLink,
  Split,
  Eye,
  Columns,
  Clock,
  Gauge,
  HardDrive,
  FileCheck,
  Filter,
  CheckCircle2,
  XCircle,
  HelpCircle,
  Maximize2,
  Lock,
  Unlock,
  Link2,
  Crop
} from 'lucide-react';

export default function App() {
  const [activeTab, setActiveTab] = useState('studio'); // studio | batch | telemetry | signer | presets
  const [status, setStatus] = useState(null);
  const [corpusImages, setCorpusImages] = useState([]);
  const [presets, setPresets] = useState([]);
  const [history, setHistory] = useState([]);
  const [telemetry, setTelemetry] = useState(null);

  // Studio Form State
  const [selectedSourceType, setSelectedSourceType] = useState('corpus'); // corpus | upload | url
  const [selectedCorpusImg, setSelectedCorpusImg] = useState('');
  const [customUrl, setCustomUrl] = useState('https://images.unsplash.com/photo-1579783900882-c0d3dad7b119');
  const [uploadedImageUrl, setUploadedImageUrl] = useState('');

  const [w, setW] = useState(800);
  const [h, setH] = useState(600);
  const [autoHeight, setAutoHeight] = useState(true); // Default: auto height preserves natural aspect ratio without cropping
  const [lockAspect, setLockAspect] = useState(true);
  const [fit, setFit] = useState('contain'); // Default: contain (no crop)
  const [sourceMeta, setSourceMeta] = useState(null); // { w, h, ratio, label, isPortrait }
  const [transformedMeta, setTransformedMeta] = useState(null); // { w, h, ratio }
  const [format, setFormat] = useState('auto');
  const [q, setQ] = useState(65);
  const [qt, setQt] = useState('');
  const [allowUpscale, setAllowUpscale] = useState(false);
  const [v, setV] = useState('');
  const [acceptHeader, setAcceptHeader] = useState('image/avif,image/webp,*/*');

  // Security / Signing State
  const [isSigned, setIsSigned] = useState(true);
  const [secretKey, setSecretKey] = useState('');
  const [kid, setKid] = useState('');
  const [ttlSecs, setTtlSecs] = useState(3600);

  // Transform Result & UI State
  const [loading, setLoading] = useState(false);
  const [transformResult, setTransformResult] = useState(null);
  const [splitPos, setSplitPos] = useState(50);
  const [viewMode, setViewMode] = useState('split'); // split | side-by-side | single
  const [copiedUrl, setCopiedUrl] = useState(false);
  const [copiedCurl, setCopiedCurl] = useState(false);

  // Batch Test State
  const [batchLoading, setBatchLoading] = useState(false);
  const [batchData, setBatchData] = useState(null);
  const [batchSweepMode, setBatchSweepMode] = useState(false);

  // Multi-Variant Parallel Testing State
  const [matrixLoading, setMatrixLoading] = useState(false);
  const [matrixData, setMatrixData] = useState(null);
  const [matrixFilterFormat, setMatrixFilterFormat] = useState('ALL');

  // Preset Modal State
  const [presetName, setPresetName] = useState('');
  const [presetDesc, setPresetDesc] = useState('');
  const [showPresetModal, setShowPresetModal] = useState(false);

  // Initial Data Fetching
  useEffect(() => {
    fetchStatus();
    fetchCorpus();
    fetchPresets();
    fetchHistory();
    fetchTelemetry();

    const interval = setInterval(() => {
      fetchStatus();
      fetchTelemetry();
    }, 4000);
    return () => clearInterval(interval);
  }, []);

  const fetchStatus = async () => {
    try {
      const res = await fetch('/api/status');
      const data = await res.json();
      setStatus(data);
      if (!secretKey && data.defaultKeyHex) {
        setSecretKey(data.defaultKeyHex);
      }
    } catch (e) {
      console.error('Failed to fetch status:', e);
    }
  };

  const fetchCorpus = async () => {
    try {
      const res = await fetch('/api/corpus');
      const data = await res.json();
      if (data.success && data.images.length > 0) {
        setCorpusImages(data.images);
        if (!selectedCorpusImg) {
          setSelectedCorpusImg(data.images[0].localUrl);
        }
      }
    } catch (e) {
      console.error('Failed to fetch corpus:', e);
    }
  };

  const fetchPresets = async () => {
    try {
      const res = await fetch('/api/presets');
      const data = await res.json();
      if (data.success) setPresets(data.presets);
    } catch (e) {
      console.error('Failed to fetch presets:', e);
    }
  };

  const fetchHistory = async () => {
    try {
      const res = await fetch('/api/history');
      const data = await res.json();
      if (data.success) setHistory(data.history);
    } catch (e) {
      console.error('Failed to fetch history:', e);
    }
  };

  const fetchTelemetry = async () => {
    try {
      const res = await fetch('/api/metrics');
      const data = await res.json();
      if (data.success) setTelemetry(data.metrics);
    } catch (e) {
      console.error('Failed to fetch metrics:', e);
    }
  };

  const getActiveSourceUrl = () => {
    if (selectedSourceType === 'corpus') return selectedCorpusImg;
    if (selectedSourceType === 'upload') return uploadedImageUrl;
    return customUrl;
  };

  // Detect natural dimensions and aspect ratio of current source image
  useEffect(() => {
    const src = getActiveSourceUrl();
    if (!src) return;
    const img = new Image();
    img.src = src;
    img.onload = () => {
      const naturalW = img.naturalWidth;
      const naturalH = img.naturalHeight;
      const ratio = naturalW / naturalH;
      const isPortrait = naturalH > naturalW;
      const isSquare = Math.abs(ratio - 1) < 0.05;
      const label = isSquare
        ? '1:1 Square'
        : isPortrait
        ? `Portrait (${(naturalW / naturalH).toFixed(2)})`
        : `Landscape (${(naturalW / naturalH).toFixed(2)})`;

      setSourceMeta({
        w: naturalW,
        h: naturalH,
        ratio,
        label,
        isPortrait,
      });

      if (lockAspect && w) {
        const propH = Math.round(w / ratio);
        setH(propH);
      }
    };
  }, [selectedSourceType, selectedCorpusImg, uploadedImageUrl, customUrl]);

  const handleWChange = (newW) => {
    setW(newW);
    if (lockAspect && sourceMeta?.ratio) {
      const propH = Math.round(newW / sourceMeta.ratio);
      setH(propH);
    }
  };

  const handleHChange = (newH) => {
    setH(newH);
    setAutoHeight(false);
    if (lockAspect && sourceMeta?.ratio) {
      const propW = Math.round(newH * sourceMeta.ratio);
      setW(propW);
    }
  };

  const executeTransform = async () => {
    const src = getActiveSourceUrl();
    if (!src) {
      alert('Please select or enter a valid image source.');
      return;
    }

    setLoading(true);
    setTransformResult(null);

    try {
      const payload = {
        src,
        w: w ? Number(w) : undefined,
        h: autoHeight ? undefined : (h ? Number(h) : undefined),
        fit,
        format,
        q: q || undefined,
        qt: qt || undefined,
        allow_upscale: allowUpscale,
        v: v || undefined,
        acceptHeader,
        isSigned,
        keyHex: secretKey || undefined,
        kid: kid || undefined,
      };

      const res = await fetch('/api/test-transform', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      const data = await res.json();
      setTransformResult(data);
      fetchHistory();
      fetchTelemetry();
    } catch (err) {
      alert('Failed to execute transformation: ' + err.message);
    } finally {
      setLoading(false);
    }
  };

  const handleFileUpload = async (e) => {
    const file = e.target.files[0];
    if (!file) return;

    const formData = new FormData();
    formData.append('image', file);

    try {
      const res = await fetch('/api/upload', {
        method: 'POST',
        body: formData,
      });
      const data = await res.json();
      if (data.success) {
        setUploadedImageUrl(data.file.localUrl);
        setSelectedSourceType('upload');
        if (data.file.width && data.file.height) {
          const nw = data.file.width;
          const nh = data.file.height;
          const ratio = nw / nh;
          setSourceMeta({
            w: nw,
            h: nh,
            ratio,
            label: nh > nw ? `Portrait (${(nw / nh).toFixed(2)})` : `Landscape (${(nw / nh).toFixed(2)})`,
            isPortrait: nh > nw,
          });
          if (lockAspect && w) {
            setH(Math.round(w / ratio));
          }
        }
      } else {
        alert('Upload failed: ' + data.error);
      }
    } catch (err) {
      alert('Upload error: ' + err.message);
    }
  };

  const runBatchTest = async () => {
    setBatchLoading(true);
    setBatchData(null);
    try {
      const res = await fetch('/api/batch-test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          params: { w, h, fit, format, q, qt },
          isSigned,
          keyHex: secretKey,
          acceptHeader,
          testAllVariants: batchSweepMode,
        }),
      });
      const data = await res.json();
      setBatchData(data);
    } catch (e) {
      alert('Batch test failed: ' + e.message);
    } finally {
      setBatchLoading(false);
    }
  };

  const runMultiVariantTest = async () => {
    const src = getActiveSourceUrl();
    if (!src) {
      alert('Please select or upload an image first.');
      return;
    }
    setMatrixLoading(true);
    setMatrixData(null);

    try {
      const res = await fetch('/api/multi-variant-test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          src,
          fit,
          formats: ['auto', 'avif', 'webp', 'jpeg', 'png'],
          sizes: [
            { label: 'Thumbnail', w: 200, h: 150 },
            { label: 'Small', w: 400, h: 300 },
            { label: 'Medium', w: 800, h: 600 },
            { label: 'Large', w: 1200, h: 900 },
          ],
          qualities: [40, 65, 85],
          isSigned,
          keyHex: secretKey,
          acceptHeader,
          originalBytes: sourceMeta?.sizeBytes || 0,
        }),
      });
      const data = await res.json();
      if (!data.success) {
        alert('Multi-variant test error: ' + (data.error || 'Unknown error'));
      } else {
        setMatrixData(data);
      }
    } catch (e) {
      alert('Failed to run multi-variant test: ' + e.message);
    } finally {
      setMatrixLoading(false);
    }
  };

  const saveCurrentPreset = async () => {
    if (!presetName) {
      alert('Please enter a preset name');
      return;
    }
    const payload = {
      name: presetName,
      description: presetDesc,
      params: { w, h, fit, format, q, qt, allow_upscale: allowUpscale ? 1 : 0 },
      headers: { accept: acceptHeader },
    };

    try {
      const res = await fetch('/api/presets', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      const data = await res.json();
      if (data.success) {
        fetchPresets();
        setShowPresetModal(false);
        setPresetName('');
        setPresetDesc('');
      }
    } catch (e) {
      alert('Failed to save preset: ' + e.message);
    }
  };

  const applyPreset = (preset) => {
    if (preset.params.w) setW(preset.params.w);
    if (preset.params.h) setH(preset.params.h);
    if (preset.params.fit) setFit(preset.params.fit);
    if (preset.params.format) setFormat(preset.params.format);
    if (preset.params.q) setQ(preset.params.q);
    if (preset.params.qt !== undefined) setQt(preset.params.qt);
    if (preset.params.allow_upscale !== undefined) setAllowUpscale(!!preset.params.allow_upscale);
    if (preset.headers && preset.headers.accept) setAcceptHeader(preset.headers.accept);
    setActiveTab('studio');
  };

  const copyToClipboard = (text, isCurl = false) => {
    navigator.clipboard.writeText(text);
    if (isCurl) {
      setCopiedCurl(true);
      setTimeout(() => setCopiedCurl(false), 2000);
    } else {
      setCopiedUrl(true);
      setTimeout(() => setCopiedUrl(false), 2000);
    }
  };

  const formatBytes = (bytes) => {
    if (!bytes || bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  return (
    <div className="min-h-screen bg-[#070b14] text-slate-100 font-sans flex flex-col">
      {/* Top Header Bar */}
      <header className="border-b border-slate-800/80 bg-[#0c1222]/90 backdrop-blur-xl sticky top-0 z-50 px-8 py-4">
        <div className="max-w-[1600px] mx-auto flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="w-11 h-11 rounded-2xl bg-gradient-to-tr from-indigo-600 via-indigo-500 to-cyan-400 flex items-center justify-center shadow-lg shadow-indigo-500/25 ring-1 ring-white/20">
              <Zap className="w-6 h-6 text-white" />
            </div>
            <div>
              <div className="flex items-center gap-2.5">
                <h1 className="font-extrabold text-xl tracking-tight bg-gradient-to-r from-white via-slate-100 to-slate-400 bg-clip-text text-transparent">
                  Atihsrah Studio
                </h1>
                <span className="text-[11px] px-2.5 py-0.5 rounded-full bg-indigo-500/15 text-indigo-300 font-semibold border border-indigo-500/30">
                  Pure-Rust v0.1.0
                </span>
              </div>
              <p className="text-xs text-slate-400 mt-0.5">High-Performance Image Optimization & Benchmark Lab</p>
            </div>
          </div>

          {/* Service Status Badges */}
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 px-3.5 py-1.5 rounded-xl bg-slate-900/80 border border-slate-800 text-xs shadow-sm">
              <span className={`w-2.5 h-2.5 rounded-full ${status?.proxy.status === 'ready' ? 'bg-emerald-400 animate-pulse' : 'bg-rose-500'}`} />
              <span className="text-slate-400">Proxy:</span>
              <span className={status?.proxy.status === 'ready' ? 'text-emerald-400 font-semibold' : 'text-rose-400 font-semibold'}>
                {status?.proxy.status === 'ready' ? '127.0.0.1:8080 (Ready)' : 'Offline'}
              </span>
              {status?.proxy.latencyMs && (
                <span className="text-slate-500 font-mono text-[11px]">({status.proxy.latencyMs}ms)</span>
              )}
            </div>

            <div className="flex items-center gap-2 px-3.5 py-1.5 rounded-xl bg-slate-900/80 border border-slate-800 text-xs shadow-sm">
              <Database className="w-3.5 h-3.5 text-cyan-400" />
              <span className="text-slate-400">Store:</span>
              <span className="text-cyan-300 font-semibold">
                {status?.database?.connected ? 'MongoDB' : 'In-Memory / JSON'}
              </span>
            </div>

            <div className="flex items-center gap-2 px-3.5 py-1.5 rounded-xl bg-slate-900/80 border border-slate-800 text-xs shadow-sm">
              <Activity className="w-3.5 h-3.5 text-indigo-400" />
              <span className="text-slate-400">Metrics:</span>
              <span className={status?.metrics?.available ? 'text-indigo-300 font-semibold' : 'text-slate-500'}>
                {status?.metrics?.available ? ':9090 (Active)' : 'Inactive'}
              </span>
            </div>
          </div>
        </div>
      </header>

      {/* Main Container */}
      <div className="flex-1 max-w-[1600px] w-full mx-auto px-8 py-8 space-y-8">
        {/* KPI Summary Cards Grid (4 across) */}
        <section className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6">
          {/* Card 1: Cache Hit Ratio */}
          <div className="bg-[#0f172a]/80 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-lg shadow-black/20 flex flex-col justify-between">
            <div className="flex items-center justify-between mb-4">
              <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Cache Hit Ratio</span>
              <div className="w-9 h-9 rounded-xl bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center">
                <TrendingUp className="w-4 h-4 text-indigo-400" />
              </div>
            </div>
            <div>
              <div className="text-3xl font-extrabold font-mono text-indigo-400">
                {telemetry ? `${telemetry.cacheHitRatioPct}%` : '0%'}
              </div>
              <p className="text-xs text-slate-400 mt-1.5 flex items-center gap-1.5">
                <span className="text-emerald-400 font-medium">{telemetry?.cacheHits || 0} hits</span> /{' '}
                <span className="text-amber-400 font-medium">{telemetry?.cacheMisses || 0} misses</span>
              </p>
            </div>
          </div>

          {/* Card 2: Warm Response Latency */}
          <div className="bg-[#0f172a]/80 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-lg shadow-black/20 flex flex-col justify-between">
            <div className="flex items-center justify-between mb-4">
              <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Median Request Latency</span>
              <div className="w-9 h-9 rounded-xl bg-cyan-500/10 border border-cyan-500/20 flex items-center justify-center">
                <Clock className="w-4 h-4 text-cyan-400" />
              </div>
            </div>
            <div>
              <div className="text-3xl font-extrabold font-mono text-cyan-400">
                {telemetry?.requestDurationMsP50 !== undefined ? `${telemetry.requestDurationMsP50} ms` : '0.34 ms'}
              </div>
              <p className="text-xs text-slate-400 mt-1.5">
                <span className="text-cyan-300 font-medium">Sub-millisecond</span> in-process Moka hits
              </p>
            </div>
          </div>

          {/* Card 3: Memory Footprint */}
          <div className="bg-[#0f172a]/80 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-lg shadow-black/20 flex flex-col justify-between">
            <div className="flex items-center justify-between mb-4">
              <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Cache Memory (LRU)</span>
              <div className="w-9 h-9 rounded-xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center">
                <HardDrive className="w-4 h-4 text-emerald-400" />
              </div>
            </div>
            <div>
              <div className="text-3xl font-extrabold font-mono text-emerald-400">
                {telemetry ? `${(telemetry.cacheBytesCurrent / (1024 * 1024)).toFixed(1)} MB` : '0 MB'}
              </div>
              <p className="text-xs text-slate-400 mt-1.5">
                Capacity: 500 MB max ({telemetry?.cacheEntries || 0} active assets)
              </p>
            </div>
          </div>

          {/* Card 4: Single-Flight Coalescing */}
          <div className="bg-[#0f172a]/80 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-lg shadow-black/20 flex flex-col justify-between">
            <div className="flex items-center justify-between mb-4">
              <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Total Throughput</span>
              <div className="w-9 h-9 rounded-xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-center">
                <Layers className="w-4 h-4 text-amber-400" />
              </div>
            </div>
            <div>
              <div className="text-3xl font-extrabold font-mono text-amber-400">
                {telemetry?.requestsTotal || 0} reqs
              </div>
              <p className="text-xs text-slate-400 mt-1.5">
                <span className="text-amber-300 font-medium">Single-Flight</span> thundering-herd immune
              </p>
            </div>
          </div>
        </section>

        {/* Navigation Tabs Bar */}
        <div className="flex items-center justify-between border-b border-slate-800 pb-3">
          <div className="flex items-center gap-2">
            {[
              { id: 'studio', label: 'Interactive Studio', icon: Sliders },
              { id: 'batch', label: 'Batch Benchmark (20 Images)', icon: Sparkles },
              { id: 'telemetry', label: 'Prometheus Gauges', icon: BarChart3 },
              { id: 'signer', label: 'HMAC Signer Lab', icon: Key },
              { id: 'presets', label: 'Presets & History', icon: Bookmark, badge: history.length },
            ].map((tab) => {
              const Icon = tab.icon;
              const isActive = activeTab === tab.id;
              return (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`flex items-center gap-2.5 px-4 py-2.5 rounded-xl text-sm font-semibold transition-all ${
                    isActive
                      ? 'bg-indigo-600 text-white shadow-md shadow-indigo-600/30 ring-1 ring-indigo-400/40'
                      : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/60'
                  }`}
                >
                  <Icon className="w-4 h-4" />
                  {tab.label}
                  {tab.badge !== undefined && tab.badge > 0 && (
                    <span className="ml-1 text-[11px] px-1.5 py-0.5 rounded-full bg-slate-800 text-slate-300 font-mono">
                      {tab.badge}
                    </span>
                  )}
                </button>
              );
            })}
          </div>

          <button
            onClick={() => setShowPresetModal(true)}
            className="flex items-center gap-2 px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 text-xs font-semibold transition-all shadow-sm"
          >
            <Bookmark className="w-3.5 h-3.5 text-amber-400" />
            Save Configuration Preset
          </button>
        </div>

        {/* Tab 1: Interactive Studio Layout Grid */}
        {activeTab === 'studio' && (
          <div className="space-y-8">
            <div className="grid grid-cols-12 gap-8 items-start">
            {/* Left Column: Input Source & Controls (5 of 12 columns) */}
            <div className="col-span-12 lg:col-span-5 space-y-6">
              {/* Card 1: Image Source Selector */}
              <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 space-y-5">
                <div className="flex items-center justify-between border-b border-slate-800/80 pb-3">
                  <h3 className="text-sm font-bold text-slate-200 flex items-center gap-2.5">
                    <ImageIcon className="w-4 h-4 text-indigo-400" />
                    Select Image Source
                  </h3>
                  <span className="text-[11px] font-mono text-slate-400">20 Corpus Assets</span>
                </div>

                {/* Source Mode Switcher */}
                <div className="grid grid-cols-3 gap-1.5 p-1 bg-slate-900 rounded-xl border border-slate-800">
                  <button
                    onClick={() => setSelectedSourceType('corpus')}
                    className={`py-2 text-xs font-semibold rounded-lg transition-all ${
                      selectedSourceType === 'corpus'
                        ? 'bg-indigo-600 text-white shadow-sm'
                        : 'text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    Corpus Gallery
                  </button>
                  <button
                    onClick={() => setSelectedSourceType('upload')}
                    className={`py-2 text-xs font-semibold rounded-lg transition-all ${
                      selectedSourceType === 'upload'
                        ? 'bg-indigo-600 text-white shadow-sm'
                        : 'text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    Upload Image
                  </button>
                  <button
                    onClick={() => setSelectedSourceType('url')}
                    className={`py-2 text-xs font-semibold rounded-lg transition-all ${
                      selectedSourceType === 'url'
                        ? 'bg-indigo-600 text-white shadow-sm'
                        : 'text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    HTTPS URL
                  </button>
                </div>

                {/* Corpus Gallery Visual Grid */}
                {selectedSourceType === 'corpus' && (
                  <div className="space-y-3">
                    <div className="grid grid-cols-2 gap-2.5 max-h-[220px] overflow-y-auto pr-1">
                      {corpusImages.map((img) => {
                        const isSelected = selectedCorpusImg === img.localUrl;
                        return (
                          <div
                            key={img.id}
                            onClick={() => setSelectedCorpusImg(img.localUrl)}
                            className={`p-2.5 rounded-xl border cursor-pointer transition-all flex items-center gap-2.5 ${
                              isSelected
                                ? 'bg-indigo-600/20 border-indigo-500 shadow-sm ring-1 ring-indigo-500'
                                : 'bg-slate-900/60 border-slate-800 hover:border-slate-700 hover:bg-slate-800/50'
                            }`}
                          >
                            <div className="w-10 h-10 rounded-lg bg-slate-950 flex items-center justify-center overflow-hidden shrink-0 border border-slate-800">
                              <img src={img.localUrl} alt={img.filename} className="w-full h-full object-cover" />
                            </div>
                            <div className="truncate">
                              <p className="text-xs font-semibold text-slate-200 truncate">{img.filename}</p>
                              <p className="text-[10px] text-slate-400 font-mono">
                                {img.category} • {formatBytes(img.sizeBytes)}
                              </p>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}

                {/* Upload Image View */}
                {selectedSourceType === 'upload' && (
                  <div className="space-y-3">
                    <label className="border-2 border-dashed border-slate-700 hover:border-indigo-500 rounded-xl p-6 text-center cursor-pointer bg-slate-900/40 hover:bg-slate-900 transition-all flex flex-col items-center justify-center gap-2">
                      <Upload className="w-6 h-6 text-slate-400" />
                      <span className="text-xs text-slate-200 font-medium">Click to select local image file</span>
                      <span className="text-[11px] text-slate-500">PNG, JPEG, WebP, AVIF up to 25 MB</span>
                      <input type="file" accept="image/*" onChange={handleFileUpload} className="hidden" />
                    </label>
                    {uploadedImageUrl && (
                      <p className="text-xs text-emerald-400 font-mono truncate bg-slate-900 p-2 rounded-lg border border-slate-800">
                        Uploaded: {uploadedImageUrl}
                      </p>
                    )}
                  </div>
                )}

                {/* Custom URL Input */}
                {selectedSourceType === 'url' && (
                  <div className="space-y-2">
                    <label className="text-xs text-slate-400">External HTTPS Source URL</label>
                    <input
                      type="url"
                      value={customUrl}
                      onChange={(e) => setCustomUrl(e.target.value)}
                      placeholder="https://images.unsplash.com/..."
                      className="w-full bg-slate-900 border border-slate-700 rounded-xl px-4 py-2.5 text-xs text-slate-200 font-mono focus:outline-none focus:border-indigo-500"
                    />
                  </div>
                )}
              </div>

              {/* Card 2: Transformation Parameters Grid */}
              <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 space-y-6">
                <div className="flex items-center justify-between border-b border-slate-800/80 pb-3">
                  <h3 className="text-sm font-bold text-slate-200 flex items-center gap-2.5">
                    <Sliders className="w-4 h-4 text-cyan-400" />
                    Transformation Parameters
                  </h3>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => setLockAspect(!lockAspect)}
                      className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium border transition-all cursor-pointer ${
                        lockAspect
                          ? 'bg-indigo-600/20 text-indigo-300 border-indigo-500/40'
                          : 'bg-slate-900 text-slate-400 border-slate-800 hover:text-slate-200'
                      }`}
                      title="Lock aspect ratio so width and height scale proportionally"
                    >
                      {lockAspect ? <Lock className="w-3 h-3 text-indigo-400" /> : <Unlock className="w-3 h-3 text-slate-400" />}
                      <span>{lockAspect ? 'Aspect Locked' : 'Aspect Free'}</span>
                    </button>
                    <span className="text-[11px] font-mono text-cyan-400">CatmullRom Filter</span>
                  </div>
                </div>

                {/* Source Resolution Metadata Badge */}
                {sourceMeta && (
                  <div className="flex items-center justify-between px-3.5 py-2.5 rounded-xl bg-slate-900/90 border border-slate-800 text-xs">
                    <span className="text-slate-400 flex items-center gap-1.5">
                      <ImageIcon className="w-3.5 h-3.5 text-cyan-400" />
                      Original Dimensions:
                    </span>
                    <span className="font-mono text-cyan-300 font-semibold">
                      {sourceMeta.w} × {sourceMeta.h} px ({sourceMeta.label})
                    </span>
                  </div>
                )}

                {/* Dimensions Grid (2 columns) */}
                <div className="grid grid-cols-2 gap-4">
                  {/* Width */}
                  <div className="space-y-2">
                    <div className="flex justify-between items-center text-xs">
                      <span className="text-slate-400 font-medium">Width (w)</span>
                      <span className="font-mono text-indigo-300 font-bold">{w} px</span>
                    </div>
                    <input
                      type="range"
                      min="50"
                      max="2400"
                      step="10"
                      value={w}
                      onChange={(e) => handleWChange(Number(e.target.value))}
                    />
                    <div className="flex gap-1.5 pt-1">
                      {[200, 400, 600, 800, 1200].map((val) => (
                        <button
                          key={val}
                          type="button"
                          onClick={() => handleWChange(val)}
                          className={`flex-1 py-1 text-[10px] rounded-md font-mono border transition-all cursor-pointer ${
                            w === val
                              ? 'bg-indigo-600 text-white border-indigo-500'
                              : 'bg-slate-900 hover:bg-slate-800 border-slate-800 text-slate-300'
                          }`}
                        >
                          {val}
                        </button>
                      ))}
                    </div>
                  </div>

                  {/* Height with Auto (No Crop) Option */}
                  <div className="space-y-2">
                    <div className="flex justify-between items-center text-xs">
                      <div className="flex items-center gap-1.5">
                        <span className="text-slate-400 font-medium">Height (h)</span>
                        <label className="flex items-center gap-1 cursor-pointer text-[10px] text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded border border-emerald-500/30">
                          <input
                            type="checkbox"
                            checked={autoHeight}
                            onChange={(e) => setAutoHeight(e.target.checked)}
                            className="w-3 h-3 rounded border-slate-700 bg-slate-900 text-emerald-500"
                          />
                          Auto (No Crop)
                        </label>
                      </div>
                      <span className={`font-mono font-bold ${autoHeight ? 'text-emerald-400' : 'text-indigo-300'}`}>
                        {autoHeight
                          ? `Auto (${sourceMeta?.ratio ? Math.round(w / sourceMeta.ratio) : 'Prop'} px)`
                          : `${h} px`}
                      </span>
                    </div>
                    <input
                      type="range"
                      min="50"
                      max="1800"
                      step="10"
                      value={h}
                      disabled={autoHeight}
                      onChange={(e) => handleHChange(Number(e.target.value))}
                      className={autoHeight ? 'opacity-40 cursor-not-allowed' : ''}
                    />
                    <div className="flex gap-1.5 pt-1">
                      <button
                        type="button"
                        onClick={() => setAutoHeight(true)}
                        className={`flex-1 py-1 text-[10px] rounded-md font-mono border transition-all cursor-pointer ${
                          autoHeight
                            ? 'bg-emerald-600 text-white border-emerald-500'
                            : 'bg-slate-900 hover:bg-slate-800 border-slate-800 text-emerald-300'
                        }`}
                      >
                        Auto
                      </button>
                      {[300, 450, 600, 900].map((val) => (
                        <button
                          key={val}
                          type="button"
                          onClick={() => handleHChange(val)}
                          className={`flex-1 py-1 text-[10px] rounded-md font-mono border transition-all cursor-pointer ${
                            !autoHeight && h === val
                              ? 'bg-indigo-600 text-white border-indigo-500'
                              : 'bg-slate-900 hover:bg-slate-800 border-slate-800 text-slate-300'
                          }`}
                        >
                          {val}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>

                {/* Fit Mode Grid (3 pills across) */}
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-slate-400 font-medium block">Fit Mode (fit)</span>
                    <span className="text-[11px] text-slate-400 font-medium">
                      {fit === 'contain'
                        ? 'Preserves 100% of image without cropping'
                        : fit === 'cover'
                        ? 'Crops image edges to fill exact frame'
                        : 'Stretches image without cropping'}
                    </span>
                  </div>
                  <div className="grid grid-cols-3 gap-1.5 p-1 bg-slate-900 rounded-xl border border-slate-800">
                    {[
                      { id: 'contain', label: 'Contain', badge: 'No Crop (Safe)' },
                      { id: 'cover', label: 'Cover', badge: 'Crops Edges' },
                      { id: 'fill', label: 'Fill', badge: 'Stretch' },
                    ].map((m) => (
                      <button
                        key={m.id}
                        type="button"
                        onClick={() => setFit(m.id)}
                        className={`py-2 px-2 text-center rounded-lg transition-all cursor-pointer ${
                          fit === m.id
                            ? 'bg-indigo-600 text-white shadow-sm'
                            : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/40'
                        }`}
                      >
                        <span className="block text-xs font-bold">{m.label}</span>
                        <span className="block text-[10px] text-slate-300 opacity-80">{m.badge}</span>
                      </button>
                    ))}
                  </div>

                  {fit === 'cover' && !autoHeight && sourceMeta?.ratio && (
                    <div className="flex items-start gap-2 p-2.5 rounded-xl bg-amber-500/10 border border-amber-500/30 text-amber-300 text-[11px]">
                      <AlertTriangle className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
                      <span>
                        <strong>Cover mode active:</strong> Top/bottom or left/right edges will be cropped if target dimensions ({w}×{h}) differ from source ({sourceMeta.w}×{sourceMeta.h}). Switch to <strong>Contain</strong> or check <strong>Auto (No Crop)</strong> to preserve the full image.
                      </span>
                    </div>
                  )}
                </div>

                {/* Output Format Grid (5 pills across) */}
                <div className="space-y-2">
                  <span className="text-xs text-slate-400 font-medium block">Target Format (format)</span>
                  <div className="grid grid-cols-5 gap-1.5 p-1 bg-slate-900 rounded-xl border border-slate-800">
                    {['auto', 'avif', 'webp', 'jpeg', 'png'].map((fmt) => (
                      <button
                        key={fmt}
                        onClick={() => setFormat(fmt)}
                        className={`py-1.5 text-[11px] font-semibold rounded-lg uppercase transition-all ${
                          format === fmt ? 'bg-indigo-600 text-white shadow-sm' : 'text-slate-400 hover:text-slate-200'
                        }`}
                      >
                        {fmt}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Quality & Quality Target Subgrid */}
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <div className="flex justify-between items-center text-xs">
                      <span className="text-slate-400 font-medium">Quality (q)</span>
                      <span className="font-mono text-cyan-300 font-bold">{q}</span>
                    </div>
                    <input
                      type="range"
                      min="1"
                      max="100"
                      value={q}
                      onChange={(e) => setQ(Number(e.target.value))}
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-xs text-slate-400 font-medium block">Quality Target (qt)</label>
                    <select
                      value={qt}
                      onChange={(e) => setQt(e.target.value)}
                      className="w-full bg-slate-900 border border-slate-700/80 rounded-xl px-3 py-2 text-xs text-slate-200 focus:outline-none focus:border-indigo-500 font-medium"
                    >
                      <option value="">Auto (derived from slider)</option>
                      <option value="draft">draft (JPEG 50 / AVIF 45)</option>
                      <option value="good">good (JPEG 75 / AVIF 65)</option>
                      <option value="high">high (JPEG 85 / AVIF 75)</option>
                      <option value="lossless">lossless (max fidelity)</option>
                    </select>
                  </div>
                </div>

                {/* Client Accept Header Simulation */}
                <div className="space-y-2">
                  <label className="text-xs text-slate-400 font-medium block">Client Accept Header Simulation</label>
                  <select
                    value={acceptHeader}
                    onChange={(e) => setAcceptHeader(e.target.value)}
                    className="w-full bg-slate-900 border border-slate-700/80 rounded-xl px-3.5 py-2.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500 font-medium"
                  >
                    <option value="image/avif,image/webp,*/*">Modern Browser (AVIF + WebP accepted)</option>
                    <option value="image/webp,*/*">WebP Only (Safari 14-15 / Older Chrome)</option>
                    <option value="*/*">Legacy Client (JPEG / PNG only)</option>
                  </select>
                </div>

                {/* Toggles & Options Grid */}
                <div className="flex items-center justify-between pt-2 border-t border-slate-800">
                  <label className="flex items-center gap-2.5 cursor-pointer text-xs text-slate-300 font-medium">
                    <input
                      type="checkbox"
                      checked={allowUpscale}
                      onChange={(e) => setAllowUpscale(e.target.checked)}
                      className="w-4 h-4 rounded border-slate-700 bg-slate-900 text-indigo-500 focus:ring-0"
                    />
                    Allow Upscale (allow_upscale=1)
                  </label>

                  <div className="flex items-center gap-2">
                    <span className="text-xs text-slate-400">Cache Version:</span>
                    <input
                      type="text"
                      value={v}
                      onChange={(e) => setV(e.target.value)}
                      placeholder="v=1"
                      className="w-20 bg-slate-900 border border-slate-700 rounded-lg px-2.5 py-1 text-xs text-slate-200 font-mono text-center"
                    />
                  </div>
                </div>
              </div>

              {/* Card 3: Security & Signing */}
              <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 space-y-4">
                <div className="flex items-center justify-between border-b border-slate-800/80 pb-3">
                  <h3 className="text-sm font-bold text-slate-200 flex items-center gap-2.5">
                    <ShieldCheck className="w-4 h-4 text-emerald-400" />
                    HMAC-SHA256 URL Signature
                  </h3>
                  <label className="flex items-center gap-2 cursor-pointer text-xs text-slate-300 font-medium">
                    <input
                      type="checkbox"
                      checked={isSigned}
                      onChange={(e) => setIsSigned(e.target.checked)}
                      className="w-4 h-4 rounded border-slate-700 bg-slate-900 text-indigo-500 focus:ring-0"
                    />
                    Signed Mode
                  </label>
                </div>

                {isSigned && (
                  <div className="space-y-3">
                    <div>
                      <label className="text-[11px] text-slate-400 mb-1 block">32-Byte Hex Secret Key</label>
                      <input
                        type="text"
                        value={secretKey}
                        onChange={(e) => setSecretKey(e.target.value)}
                        className="w-full bg-slate-900 border border-slate-700 rounded-xl px-3.5 py-2 text-xs text-slate-300 font-mono focus:outline-none focus:border-indigo-500 truncate"
                      />
                    </div>

                    <div className="grid grid-cols-2 gap-3">
                      <div>
                        <label className="text-[11px] text-slate-400 mb-1 block">Key ID (kid)</label>
                        <input
                          type="text"
                          value={kid}
                          onChange={(e) => setKid(e.target.value)}
                          placeholder="optional kid"
                          className="w-full bg-slate-900 border border-slate-700 rounded-xl px-3.5 py-2 text-xs text-slate-300 font-mono"
                        />
                      </div>
                      <div>
                        <label className="text-[11px] text-slate-400 mb-1 block">TTL Window (seconds)</label>
                        <input
                          type="number"
                          value={ttlSecs}
                          onChange={(e) => setTtlSecs(Number(e.target.value))}
                          className="w-full bg-slate-900 border border-slate-700 rounded-xl px-3.5 py-2 text-xs text-slate-300 font-mono"
                        />
                      </div>
                    </div>
                  </div>
                )}
              </div>

              {/* Action Buttons: Single and All-at-Once Multi-Variant */}
              <div className="space-y-3">
                <button
                  onClick={executeTransform}
                  disabled={loading || matrixLoading}
                  className="w-full py-3.5 rounded-2xl bg-gradient-to-r from-indigo-600 via-indigo-500 to-indigo-600 hover:from-indigo-500 hover:to-indigo-500 text-white font-bold text-sm tracking-wide shadow-xl shadow-indigo-600/30 transition-all flex items-center justify-center gap-2.5 cursor-pointer disabled:opacity-50"
                >
                  {loading ? (
                    <>
                      <RefreshCw className="w-5 h-5 animate-spin" />
                      Executing Transformation on Proxy...
                    </>
                  ) : (
                    <>
                      <Play className="w-4 h-4 fill-white" />
                      Execute Single Transformation
                    </>
                  )}
                </button>

                <button
                  onClick={runMultiVariantTest}
                  disabled={loading || matrixLoading}
                  className="w-full py-3.5 rounded-2xl bg-gradient-to-r from-cyan-600 via-teal-500 to-emerald-600 hover:from-cyan-500 hover:to-emerald-500 text-white font-bold text-sm tracking-wide shadow-xl shadow-cyan-600/30 transition-all flex items-center justify-center gap-2.5 cursor-pointer disabled:opacity-50"
                >
                  {matrixLoading ? (
                    <>
                      <RefreshCw className="w-5 h-5 animate-spin" />
                      Running All Conversions at Once in Parallel...
                    </>
                  ) : (
                    <>
                      <Zap className="w-4 h-4 fill-amber-300 text-amber-300" />
                      ⚡ Run All Qualities, Sizes & Formats at Once
                    </>
                  )}
                </button>
              </div>
            </div>

            {/* Right Column: Visual Preview Canvas & Telemetry (7 of 12 columns) */}
            <div className="col-span-12 lg:col-span-7 space-y-6">
              {/* Telemetry Header Bar */}
              {transformResult && (
                <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-5 shadow-xl shadow-black/20 flex flex-wrap items-center justify-between gap-4">
                  <div className="flex items-center gap-3">
                    <span
                      className={`px-3 py-1 rounded-full text-xs font-bold border ${
                        transformResult.status === 200
                          ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30'
                          : 'bg-rose-500/15 text-rose-400 border-rose-500/30'
                      }`}
                    >
                      HTTP {transformResult.status}
                    </span>

                    <span
                      className={`px-3 py-1 rounded-full text-xs font-bold border ${
                        transformResult.cacheStatus === 'HIT'
                          ? 'bg-emerald-500/15 text-emerald-300 border-emerald-500/30'
                          : 'bg-amber-500/15 text-amber-300 border-amber-500/30'
                      }`}
                    >
                      CACHE: {transformResult.cacheStatus}
                    </span>

                    <span className="px-3 py-1 rounded-full text-xs font-mono font-bold bg-indigo-500/15 text-indigo-300 border border-indigo-500/30">
                      {transformResult.latencyMs} ms
                    </span>

                    <span className="px-3 py-1 rounded-full text-xs font-mono bg-slate-800 text-slate-300 border border-slate-700">
                      {transformResult.contentType}
                    </span>
                  </div>

                  {transformResult.originalBytes > 0 && (
                    <div className="flex items-center gap-3">
                      <span className="text-xs text-slate-400 font-mono">
                        {formatBytes(transformResult.originalBytes)} → {formatBytes(transformResult.transformedBytes)}
                      </span>
                      <span
                        className={`text-xs font-extrabold font-mono px-2.5 py-1 rounded-lg border ${
                          transformResult.byteDeltaPct < 0
                            ? 'bg-emerald-500/20 text-emerald-300 border-emerald-500/40'
                            : 'bg-rose-500/20 text-rose-300 border-rose-500/40'
                        }`}
                      >
                        {transformResult.byteDeltaPct}%
                      </span>
                    </div>
                  )}
                </div>
              )}

              {/* Main Visual Preview Panel */}
              <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 flex flex-col min-h-[520px]">
                {/* View Mode Bar */}
                <div className="flex items-center justify-between mb-4 border-b border-slate-800/80 pb-3">
                  <h3 className="text-sm font-bold text-slate-200 flex items-center gap-2.5">
                    <Eye className="w-4 h-4 text-indigo-400" />
                    Visual Inspector & Quality Comparison
                  </h3>

                  <div className="flex items-center gap-2 bg-slate-900 rounded-xl p-1 border border-slate-800">
                    <button
                      onClick={() => setViewMode('split')}
                      className={`px-3 py-1.5 text-xs font-semibold rounded-lg flex items-center gap-1.5 transition-all ${
                        viewMode === 'split' ? 'bg-indigo-600 text-white shadow-sm' : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      <Split className="w-3.5 h-3.5" />
                      Split Curtain
                    </button>
                    <button
                      onClick={() => setViewMode('side-by-side')}
                      className={`px-3 py-1.5 text-xs font-semibold rounded-lg flex items-center gap-1.5 transition-all ${
                        viewMode === 'side-by-side' ? 'bg-indigo-600 text-white shadow-sm' : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      <Columns className="w-3.5 h-3.5" />
                      Side-by-Side
                    </button>
                  </div>
                </div>

                {/* Canvas Viewport */}
                <div className="flex-1 flex items-center justify-center relative min-h-[420px] bg-slate-950 rounded-xl border border-slate-800/80 overflow-hidden">
                  {loading ? (
                    <div className="flex flex-col items-center gap-3 text-slate-400 py-12">
                      <RefreshCw className="w-10 h-10 animate-spin text-indigo-500" />
                      <p className="text-xs font-medium">Transforming pixels on Atihsrah proxy...</p>
                    </div>
                  ) : transformResult?.previewBase64 ? (
                    viewMode === 'split' ? (
                      /* Split Curtain Slider */
                      <div
                        className="split-viewer-container w-full h-[450px] relative cursor-ew-resize"
                        onMouseMove={(e) => {
                          const rect = e.currentTarget.getBoundingClientRect();
                          const pos = ((e.clientX - rect.left) / rect.width) * 100;
                          setSplitPos(Math.max(5, Math.min(95, pos)));
                        }}
                      >
                        {/* Transformed Image (Bottom Layer) */}
                        <img
                          src={transformResult.previewBase64}
                          alt="Transformed"
                          className="absolute inset-0 w-full h-full object-contain pointer-events-none"
                          onLoad={(e) => {
                            setTransformedMeta({
                              w: e.target.naturalWidth,
                              h: e.target.naturalHeight,
                              ratio: e.target.naturalWidth / e.target.naturalHeight,
                            });
                          }}
                        />
                        <div className="absolute bottom-4 right-4 px-3 py-1 rounded-lg bg-black/80 backdrop-blur text-xs font-mono text-emerald-300 z-10 border border-emerald-500/30 flex items-center gap-1.5">
                          <span>Optimized ({transformResult.contentType})</span>
                          {transformedMeta?.w && (
                            <span className="text-slate-300">• {transformedMeta.w}×{transformedMeta.h} px</span>
                          )}
                        </div>

                        {/* Original Image (Top Layer clipped by slider) */}
                        <div
                          className="absolute inset-0 overflow-hidden pointer-events-none"
                          style={{ width: `${splitPos}%` }}
                        >
                          <img
                            src={getActiveSourceUrl()}
                            alt="Original"
                            className="absolute inset-0 w-full h-full object-contain"
                            style={{ width: '100%', minWidth: '100%' }}
                            onLoad={(e) => {
                              const nw = e.target.naturalWidth;
                              const nh = e.target.naturalHeight;
                              setSourceMeta((prev) => ({
                                ...prev,
                                w: nw,
                                h: nh,
                                ratio: nw / nh,
                                label: nh > nw ? 'Portrait' : 'Landscape',
                                isPortrait: nh > nw,
                              }));
                            }}
                          />
                          <div className="absolute bottom-4 left-4 px-3 py-1 rounded-lg bg-black/80 backdrop-blur text-xs font-mono text-amber-300 z-10 border border-amber-500/30 flex items-center gap-1.5">
                            <span>Original Source</span>
                            {sourceMeta?.w && (
                              <span className="text-slate-300">• {sourceMeta.w}×{sourceMeta.h} px</span>
                            )}
                          </div>
                        </div>

                        {/* Slider Divider Bar */}
                        <div className="split-divider" style={{ left: `${splitPos}%` }}>
                          <div className="split-handle">↔</div>
                        </div>
                      </div>
                    ) : (
                      /* Side-by-Side View */
                      <div className="grid grid-cols-2 gap-4 w-full h-[450px] p-4">
                        {/* Original Source Card */}
                        <div className="flex flex-col items-center justify-center border border-slate-800 rounded-xl p-3 bg-slate-900/40 relative">
                          <span className="absolute top-3 left-3 text-[11px] font-mono bg-amber-500/20 text-amber-300 px-2.5 py-0.5 rounded-md border border-amber-500/30 flex items-center gap-1.5">
                            <span>Original Source</span>
                            {sourceMeta?.w && (
                              <span className="text-amber-200/80 font-normal">({sourceMeta.w}×{sourceMeta.h} px)</span>
                            )}
                          </span>
                          <img
                            src={getActiveSourceUrl()}
                            alt="Original"
                            className="max-h-[340px] object-contain"
                            onLoad={(e) => {
                              const nw = e.target.naturalWidth;
                              const nh = e.target.naturalHeight;
                              setSourceMeta((prev) => ({
                                ...prev,
                                w: nw,
                                h: nh,
                                ratio: nw / nh,
                                label: nh > nw ? 'Portrait' : 'Landscape',
                                isPortrait: nh > nw,
                              }));
                            }}
                          />
                          <span className="text-xs font-mono text-slate-400 mt-2">
                            {formatBytes(transformResult.originalBytes)}
                          </span>
                        </div>

                        {/* Optimized Card */}
                        <div className="flex flex-col items-center justify-center border border-slate-800 rounded-xl p-3 bg-slate-900/40 relative">
                          <div className="absolute top-3 left-3 flex items-center gap-2">
                            <span className="text-[11px] font-mono bg-emerald-500/20 text-emerald-300 px-2.5 py-0.5 rounded-md border border-emerald-500/30">
                              Optimized ({transformResult.contentType})
                            </span>
                            {transformedMeta?.w && (
                              <span className="text-[11px] font-mono bg-slate-800 text-slate-300 px-2 py-0.5 rounded-md border border-slate-700">
                                {transformedMeta.w}×{transformedMeta.h} px
                              </span>
                            )}
                          </div>
                          <img
                            src={transformResult.previewBase64}
                            alt="Optimized"
                            className="max-h-[340px] object-contain"
                            onLoad={(e) => {
                              const nw = e.target.naturalWidth;
                              const nh = e.target.naturalHeight;
                              setTransformedMeta({
                                w: nw,
                                h: nh,
                                ratio: nw / nh,
                              });
                            }}
                          />
                          <div className="flex items-center gap-2 mt-2">
                            <span className="text-xs font-mono text-emerald-400 font-bold">
                              {formatBytes(transformResult.transformedBytes)} ({transformResult.byteDeltaPct}%)
                            </span>
                            {transformedMeta?.ratio && sourceMeta?.ratio && (
                              Math.abs(transformedMeta.ratio - sourceMeta.ratio) < 0.05 ? (
                                <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                                  Full Image Preserved (No Crop)
                                </span>
                              ) : (
                                <span className="text-[10px] font-mono text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full border border-amber-500/20">
                                  Cropped by Cover
                                </span>
                              )
                            )}
                          </div>
                        </div>
                      </div>
                    )
                  ) : transformResult?.status && transformResult.status !== 200 ? (
                    <div className="flex flex-col items-center gap-3 p-8 text-center">
                      <AlertTriangle className="w-12 h-12 text-rose-500" />
                      <h4 className="text-sm font-bold text-rose-400">
                        HTTP Error {transformResult.status} from Atihsrah Proxy
                      </h4>
                      <pre className="text-xs font-mono bg-slate-900 p-4 rounded-xl border border-slate-800 max-w-lg text-left text-slate-300 overflow-x-auto whitespace-pre-wrap">
                        {JSON.stringify(transformResult.errorDetails, null, 2)}
                      </pre>
                    </div>
                  ) : (
                    <div className="flex flex-col items-center gap-3 text-slate-500 py-16">
                      <ImageIcon className="w-16 h-16 stroke-[1.2]" />
                      <p className="text-sm font-medium">Select parameters and click Execute to test Atihsrah</p>
                    </div>
                  )}
                </div>

                {/* URL Export Bar */}
                {transformResult?.targetUrl && (
                  <div className="mt-4 pt-4 border-t border-slate-800/80 flex items-center justify-between gap-4">
                    <div className="flex-1 truncate font-mono text-xs text-slate-400 bg-slate-900/90 px-3.5 py-2.5 rounded-xl border border-slate-800">
                      {transformResult.targetUrl}
                    </div>

                    <div className="flex items-center gap-2.5 shrink-0">
                      <button
                        onClick={() => copyToClipboard(transformResult.targetUrl, false)}
                        className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 text-xs font-semibold transition-all cursor-pointer"
                      >
                        {copiedUrl ? <Check className="w-4 h-4 text-emerald-400" /> : <Copy className="w-4 h-4" />}
                        {copiedUrl ? 'Copied' : 'Copy URL'}
                      </button>

                      <button
                        onClick={() =>
                          copyToClipboard(
                            `curl -sI -H "Accept: ${acceptHeader}" "${transformResult.targetUrl}"`,
                            true
                          )
                        }
                        className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 text-xs font-semibold transition-all cursor-pointer"
                      >
                        {copiedCurl ? <Check className="w-4 h-4 text-emerald-400" /> : <Copy className="w-4 h-4" />}
                        {copiedCurl ? 'Copied cURL' : 'cURL'}
                      </button>
                    </div>
                  </div>
                )}
              </div>

              {/* Detailed Headers & Signature Sub-Grid (2 cards) */}
              {transformResult && (
                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  {/* Card 1: HTTP Response Headers */}
                  <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-5 shadow-xl shadow-black/20 space-y-3">
                    <h4 className="text-xs font-bold uppercase tracking-wider text-slate-400">Response Headers</h4>
                    <div className="space-y-1.5 font-mono text-[11px] text-slate-300 max-h-[160px] overflow-y-auto">
                      {Object.entries(transformResult.headers || {}).map(([k, val]) => (
                        <div key={k} className="flex justify-between border-b border-slate-800/60 pb-1">
                          <span className="text-indigo-300">{k}:</span>
                          <span className="text-slate-400 truncate max-w-[200px]">{val}</span>
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* Card 2: Canonical HMAC String */}
                  <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-5 shadow-xl shadow-black/20 space-y-3">
                    <h4 className="text-xs font-bold uppercase tracking-wider text-slate-400">Canonical String (HMAC)</h4>
                    <pre className="bg-slate-950 p-3 rounded-xl font-mono text-[11px] text-emerald-300 border border-slate-800/80 whitespace-pre-wrap max-h-[160px] overflow-y-auto">
                      {transformResult.canonicalString || 'No canonical string'}
                    </pre>
                  </div>
                </div>
              )}
            </div>
          </div>

          {/* Parallel Multi-Variant Testing Results Section */}
          {matrixData && (
            <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-2xl space-y-6">
              <div className="flex flex-wrap items-center justify-between gap-4 border-b border-slate-800/80 pb-4">
                <div>
                  <h3 className="text-base font-bold text-slate-100 flex items-center gap-2.5">
                    <Zap className="w-5 h-5 text-amber-400 fill-amber-400" />
                    Multi-Variant Parallel Test Results (All Formats, Sizes & Qualities at Once)
                  </h3>
                  <p className="text-xs text-slate-400 mt-1">
                    Executed {matrixData.totalVariants} image transformations concurrently across processor worker threads.
                  </p>
                </div>

                <div className="flex items-center gap-3">
                  {/* Format Filter Pills */}
                  <div className="flex bg-slate-900 p-1 rounded-xl border border-slate-800 text-xs">
                    {['ALL', 'AVIF', 'WEBP', 'JPEG', 'PNG', 'AUTO'].map((fmt) => (
                      <button
                        key={fmt}
                        onClick={() => setMatrixFilterFormat(fmt)}
                        className={`px-3 py-1 rounded-lg font-semibold transition-all cursor-pointer ${
                          matrixFilterFormat === fmt
                            ? 'bg-indigo-600 text-white shadow-sm'
                            : 'text-slate-400 hover:text-slate-200'
                        }`}
                      >
                        {fmt}
                      </button>
                    ))}
                  </div>

                  <button
                    onClick={() => setMatrixData(null)}
                    className="text-xs px-3 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 font-medium cursor-pointer"
                  >
                    Clear Results
                  </button>
                </div>
              </div>

              {/* Parallel Speedup Stat Cards */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
                <div className="bg-slate-900/80 border border-slate-800 p-4 rounded-xl">
                  <span className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider">Completed Variants</span>
                  <p className="text-xl font-extrabold font-mono text-slate-100 mt-1">
                    {matrixData.successCount} / {matrixData.totalVariants}
                  </p>
                </div>

                <div className="bg-slate-900/80 border border-slate-800 p-4 rounded-xl">
                  <span className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider">Parallel Wall Time</span>
                  <p className="text-xl font-extrabold font-mono text-emerald-400 mt-1 flex items-center gap-1.5">
                    <Clock className="w-4 h-4 text-emerald-400" />
                    {matrixData.wallClockMs} ms
                  </p>
                </div>

                <div className="bg-slate-900/80 border border-slate-800 p-4 rounded-xl">
                  <span className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider">Serial Sum Latency</span>
                  <p className="text-xl font-extrabold font-mono text-cyan-400 mt-1">
                    {matrixData.sumLatencyMs} ms
                  </p>
                </div>

                <div className="bg-slate-900/80 border border-slate-800 p-4 rounded-xl">
                  <span className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider">Concurrency Boost</span>
                  <p className="text-xl font-extrabold font-mono text-amber-300 mt-1 flex items-center gap-1.5">
                    <Zap className="w-4 h-4 fill-amber-300 text-amber-300" />
                    {matrixData.speedupMultiplier} FASTER
                  </p>
                </div>
              </div>

              {/* Matrix Cards Grid */}
              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-4 gap-4 max-h-[500px] overflow-y-auto pr-1">
                {matrixData.variants
                  .filter((v) => matrixFilterFormat === 'ALL' || v.format.toUpperCase() === matrixFilterFormat)
                  .map((item, idx) => (
                    <div
                      key={idx}
                      className="bg-slate-900/90 border border-slate-800/80 hover:border-slate-700 rounded-xl p-3.5 space-y-3 transition-all flex flex-col justify-between"
                    >
                      <div className="space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-bold uppercase font-mono px-2 py-0.5 bg-indigo-950/70 border border-indigo-700/50 rounded-md text-indigo-300">
                            {item.format}
                          </span>
                          <span className="text-[11px] text-cyan-300 font-mono font-bold">
                            {item.latencyMs} ms
                          </span>
                        </div>

                        <div className="w-full h-32 bg-slate-950 rounded-lg overflow-hidden flex items-center justify-center border border-slate-800 relative group">
                          {item.previewBase64 ? (
                            <img
                              src={item.previewBase64}
                              alt={item.label}
                              className="w-full h-full object-contain"
                            />
                          ) : (
                            <div className="text-xs text-rose-400">Failed (HTTP {item.status})</div>
                          )}
                          <div className="absolute top-1.5 right-1.5 px-1.5 py-0.5 rounded text-[10px] font-mono font-bold bg-black/70 text-slate-300 border border-white/10">
                            {item.w}×{item.h}
                          </div>
                        </div>

                        <div className="space-y-1 text-[11px] font-mono">
                          <div className="flex justify-between text-slate-400">
                            <span>Size / Qual:</span>
                            <span className="text-slate-200">{item.sizeLabel || `${item.w}×${item.h}`} • q={item.q}</span>
                          </div>
                          <div className="flex justify-between text-slate-400">
                            <span>Delivered:</span>
                            <span className="text-slate-100 font-bold">{formatBytes(item.transformedBytes)}</span>
                          </div>
                          <div className="flex justify-between items-center">
                            <span className="text-slate-400">Savings:</span>
                            <span className={`font-bold px-1.5 py-0.2 rounded text-[10px] ${item.deltaPct < 0 ? 'bg-emerald-500/20 text-emerald-300' : 'bg-rose-500/20 text-rose-300'}`}>
                              {item.deltaPct}%
                            </span>
                          </div>
                        </div>
                      </div>

                      <button
                        onClick={() => {
                          setTransformResult(item);
                          window.scrollTo({ top: 0, behavior: 'smooth' });
                        }}
                        className="w-full py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 text-[11px] font-semibold transition-all cursor-pointer text-center"
                      >
                        Inspect in Viewer
                      </button>
                    </div>
                  ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Tab 2: Batch Benchmark */}
      {activeTab === 'batch' && (
        <div className="space-y-6">
          <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 flex flex-wrap items-center justify-between gap-4">
            <div>
              <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2.5">
                <Sparkles className="w-5 h-5 text-indigo-400" />
                Automated Parallel Batch Benchmark Runner
              </h2>
              <p className="text-xs text-slate-400 mt-1">
                Dispatches image transformations concurrently across processor worker threads with zero larger-than-source verification.
              </p>
            </div>

            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={() => setBatchSweepMode(!batchSweepMode)}
                className={`px-3.5 py-2.5 rounded-xl text-xs font-semibold border transition-all cursor-pointer ${
                  batchSweepMode
                    ? 'bg-amber-500/20 text-amber-300 border-amber-500/40 shadow-sm'
                    : 'bg-slate-900 text-slate-400 border-slate-800 hover:text-slate-200'
                }`}
              >
                {batchSweepMode ? 'Mode: Full Matrix Sweep (All Formats × Sizes × Qualities)' : 'Mode: 20-Corpus Parallel (All at Once)'}
              </button>

              <button
                onClick={runBatchTest}
                disabled={batchLoading}
                className="px-6 py-3 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white font-bold text-xs tracking-wide shadow-lg shadow-indigo-600/30 transition-all flex items-center gap-2 cursor-pointer disabled:opacity-50"
              >
                {batchLoading ? (
                  <>
                    <RefreshCw className="w-4 h-4 animate-spin" />
                    Executing Parallel Conversions...
                  </>
                ) : (
                  <>
                    <Play className="w-4 h-4 fill-white" />
                    {batchSweepMode ? 'Run Parallel Matrix Sweep' : 'Run 20-Image Parallel Test'}
                  </>
                )}
              </button>
            </div>
          </div>

          {batchData && (
            <div className="space-y-6">
              {/* 5 Cards Grid */}
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-6">
                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Success Rate</span>
                  <p className="text-2xl font-extrabold font-mono text-slate-100 mt-2">
                    {batchData.successCount} / {batchData.totalCount}
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Wall Clock Time</span>
                  <p className="text-2xl font-extrabold font-mono text-emerald-400 mt-2 flex items-center gap-1.5">
                    <Clock className="w-5 h-5 text-emerald-400" />
                    {batchData.wallClockMs ? `${batchData.wallClockMs} ms` : `${batchData.avgLatencyMs} ms`}
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Parallel Boost</span>
                  <p className="text-2xl font-extrabold font-mono text-amber-300 mt-2 flex items-center gap-1.5">
                    <Zap className="w-5 h-5 fill-amber-300 text-amber-300" />
                    {batchData.speedupMultiplier || '1x'}
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Larger Than Source</span>
                  <p className={`text-2xl font-extrabold font-mono mt-2 ${batchData.largerCount === 0 ? 'text-emerald-400' : 'text-rose-400'}`}>
                    {batchData.largerCount} ({batchData.largerCount === 0 ? 'PASS' : 'FAIL'})
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Format Policy</span>
                  <p className="text-2xl font-extrabold font-mono text-indigo-300 mt-2">
                    Pure Rust Option A
                  </p>
                </div>
              </div>

                {/* Table */}
                <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl overflow-hidden shadow-xl shadow-black/20">
                  <table className="w-full text-left text-xs border-collapse">
                    <thead>
                      <tr className="bg-slate-900 border-b border-slate-800 text-slate-400 font-mono">
                        <th className="py-3 px-5">ID</th>
                        <th className="py-3 px-5">Filename</th>
                        <th className="py-3 px-5">Category</th>
                        <th className="py-3 px-5">Original</th>
                        <th className="py-3 px-5">Delivered</th>
                        <th className="py-3 px-5">Delta (%)</th>
                        <th className="py-3 px-5">Latency</th>
                        <th className="py-3 px-5">Cache</th>
                        <th className="py-3 px-5">Format</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-slate-800 font-mono">
                      {batchData.results.map((r) => (
                        <tr key={r.id} className="hover:bg-slate-800/40 transition-colors">
                          <td className="py-3 px-5 text-slate-500">{r.id}</td>
                          <td className="py-3 px-5 text-slate-200 font-semibold">{r.filename}</td>
                          <td className="py-3 px-5 text-slate-400">{r.category}</td>
                          <td className="py-3 px-5 text-slate-400">{formatBytes(r.originalBytes)}</td>
                          <td className="py-3 px-5 text-slate-200 font-medium">{formatBytes(r.transformedBytes)}</td>
                          <td className="py-3 px-5">
                            <span
                              className={`px-2 py-0.5 rounded font-bold ${
                                r.deltaPct < 0
                                  ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                                  : 'bg-rose-500/20 text-rose-300 border border-rose-500/30'
                              }`}
                            >
                              {r.deltaPct}%
                            </span>
                          </td>
                          <td className="py-3 px-5 text-cyan-300">{r.latencyMs} ms</td>
                          <td className="py-3 px-5">
                            <span
                              className={`text-[11px] px-2 py-0.5 rounded font-bold ${
                                r.cache === 'HIT' ? 'bg-emerald-500/20 text-emerald-300' : 'bg-amber-500/20 text-amber-300'
                              }`}
                            >
                              {r.cache}
                            </span>
                          </td>
                          <td className="py-3 px-5 text-slate-400">{r.contentType}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </div>
        )}

        {/* Tab 3: Prometheus Telemetry */}
        {activeTab === 'telemetry' && (
          <div className="space-y-6">
            <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-6 shadow-xl shadow-black/20 flex items-center justify-between">
              <div>
                <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2.5">
                  <BarChart3 className="w-5 h-5 text-indigo-400" />
                  Live Prometheus Metrics (:9090/metrics)
                </h2>
                <p className="text-xs text-slate-400 mt-1">
                  Real-time telemetry scraped directly from Atihsrah's isolated metrics listener.
                </p>
              </div>
              <button
                onClick={fetchTelemetry}
                className="flex items-center gap-2 px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 text-xs font-semibold transition-all cursor-pointer"
              >
                <RefreshCw className="w-3.5 h-3.5" />
                Scrape Now
              </button>
            </div>

            {telemetry && (
              <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
                <div className="bg-[#0f172a]/90 border border-indigo-500/30 rounded-2xl p-6 shadow-xl shadow-indigo-500/5">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Cache Hit Ratio</span>
                  <p className="text-4xl font-extrabold font-mono text-indigo-400 mt-3">
                    {telemetry.cacheHitRatioPct}%
                  </p>
                  <p className="text-xs text-slate-400 mt-2">
                    Hits: {telemetry.cacheHits} / Misses: {telemetry.cacheMisses}
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-cyan-500/30 rounded-2xl p-6 shadow-xl shadow-cyan-500/5">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Moka LRU Memory</span>
                  <p className="text-4xl font-extrabold font-mono text-cyan-400 mt-3">
                    {(telemetry.cacheBytesCurrent / (1024 * 1024)).toFixed(2)} MB
                  </p>
                  <p className="text-xs text-slate-400 mt-2">
                    Capacity: {(telemetry.cacheBytesMax / (1024 * 1024)).toFixed(0)} MB ({telemetry.cacheEntries} assets)
                  </p>
                </div>

                <div className="bg-[#0f172a]/90 border border-emerald-500/30 rounded-2xl p-6 shadow-xl shadow-emerald-500/5">
                  <span className="text-xs font-semibold uppercase tracking-wider text-slate-400">Total Proxy Requests</span>
                  <p className="text-4xl font-extrabold font-mono text-emerald-400 mt-3">
                    {telemetry.requestsTotal}
                  </p>
                  <p className="text-xs text-slate-400 mt-2">
                    Active Inflight: {telemetry.inflightCurrent}
                  </p>
                </div>
              </div>
            )}
          </div>
        )}

        {/* Tab 4: HMAC Signer Lab */}
        {activeTab === 'signer' && (
          <div className="space-y-6 max-w-4xl mx-auto">
            <div className="bg-[#0f172a]/90 backdrop-blur-md border border-slate-800 rounded-2xl p-8 shadow-xl shadow-black/20 space-y-6">
              <div>
                <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2.5">
                  <Key className="w-5 h-5 text-indigo-400" />
                  HMAC-SHA256 Signing & Canonical Query String Inspector
                </h2>
                <p className="text-xs text-slate-400 mt-1">
                  Inspect how Atihsrah guarantees constant-time URL verification against parameter tampering and replay attacks.
                </p>
              </div>

              <div className="space-y-4">
                <div>
                  <label className="text-xs font-semibold text-slate-300 mb-1.5 block">Active Canonical String</label>
                  <pre className="bg-slate-950 p-4 rounded-xl font-mono text-xs text-emerald-300 border border-slate-800 whitespace-pre-wrap">
                    {transformResult?.canonicalString || 'Run a transformation in the Studio to inspect its canonical string.'}
                  </pre>
                </div>

                <div>
                  <label className="text-xs font-semibold text-slate-300 mb-1.5 block">Signed Proxy URL</label>
                  <pre className="bg-slate-950 p-4 rounded-xl font-mono text-xs text-cyan-300 border border-slate-800 whitespace-pre-wrap break-all">
                    {transformResult?.targetUrl || 'No active URL.'}
                  </pre>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Tab 5: Presets & History */}
        {activeTab === 'presets' && (
          <div className="space-y-8">
            {/* Presets Grid */}
            <div className="space-y-4">
              <h2 className="text-base font-bold text-slate-100 flex items-center gap-2.5">
                <Bookmark className="w-5 h-5 text-amber-400" />
                Saved Optimization Presets
              </h2>

              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                {presets.map((p) => (
                  <div key={p.id} className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl p-6 hover:border-slate-700 transition-all shadow-lg flex flex-col justify-between">
                    <div>
                      <h4 className="text-sm font-bold text-slate-200">{p.name}</h4>
                      <p className="text-xs text-slate-400 mt-1">{p.description}</p>
                      <div className="flex flex-wrap gap-2 mt-4">
                        <span className="text-[11px] font-mono px-2.5 py-1 bg-slate-900 text-indigo-300 rounded-lg border border-slate-800">
                          {p.params.w}x{p.params.h}
                        </span>
                        <span className="text-[11px] font-mono px-2.5 py-1 bg-slate-900 text-cyan-300 rounded-lg border border-slate-800 uppercase">
                          {p.params.format}
                        </span>
                        <span className="text-[11px] font-mono px-2.5 py-1 bg-slate-900 text-slate-300 rounded-lg border border-slate-800">
                          fit={p.params.fit}
                        </span>
                      </div>
                    </div>

                    <button
                      onClick={() => applyPreset(p)}
                      className="mt-6 w-full py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-semibold border border-slate-700 transition-all cursor-pointer"
                    >
                      Apply to Studio
                    </button>
                  </div>
                ))}
              </div>
            </div>

            {/* History Table */}
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h2 className="text-base font-bold text-slate-100 flex items-center gap-2.5">
                  <Activity className="w-5 h-5 text-cyan-400" />
                  Recent Test Execution History
                </h2>
                {history.length > 0 && (
                  <button
                    onClick={async () => {
                      await fetch('/api/history', { method: 'DELETE' });
                      fetchHistory();
                    }}
                    className="text-xs text-rose-400 hover:text-rose-300 font-semibold cursor-pointer"
                  >
                    Clear History
                  </button>
                )}
              </div>

              <div className="bg-[#0f172a]/90 border border-slate-800 rounded-2xl overflow-hidden shadow-xl shadow-black/20">
                <table className="w-full text-left text-xs border-collapse">
                  <thead>
                    <tr className="bg-slate-900 border-b border-slate-800 text-slate-400 font-mono">
                      <th className="py-3 px-5">Time</th>
                      <th className="py-3 px-5">Status</th>
                      <th className="py-3 px-5">Cache</th>
                      <th className="py-3 px-5">Latency</th>
                      <th className="py-3 px-5">Savings</th>
                      <th className="py-3 px-5">Format</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-800 font-mono">
                    {history.map((h) => (
                      <tr key={h.id} className="hover:bg-slate-800/40 transition-colors">
                        <td className="py-3 px-5 text-slate-500">
                          {new Date(h.timestamp).toLocaleTimeString()}
                        </td>
                        <td className="py-3 px-5">
                          <span
                            className={`px-2 py-0.5 rounded font-bold ${
                              h.status === 200 ? 'bg-emerald-500/20 text-emerald-300' : 'bg-rose-500/20 text-rose-300'
                            }`}
                          >
                            {h.status}
                          </span>
                        </td>
                        <td className="py-3 px-5">
                          <span
                            className={`px-2 py-0.5 rounded font-bold ${
                              h.cacheStatus === 'HIT' ? 'bg-emerald-500/20 text-emerald-300' : 'bg-amber-500/20 text-amber-300'
                            }`}
                          >
                            {h.cacheStatus}
                          </span>
                        </td>
                        <td className="py-3 px-5 text-cyan-300">{h.latencyMs} ms</td>
                        <td className="py-3 px-5 text-emerald-400 font-bold">{h.byteDeltaPct}%</td>
                        <td className="py-3 px-5 text-slate-400">{h.contentType}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Preset Modal */}
      {showPresetModal && (
        <div className="fixed inset-0 bg-black/75 backdrop-blur-md flex items-center justify-center z-50 p-4">
          <div className="bg-[#0f172a] border border-slate-700 rounded-2xl p-8 max-w-md w-full shadow-2xl space-y-6">
            <div>
              <h3 className="text-base font-bold text-slate-100">Save Configuration Preset</h3>
              <p className="text-xs text-slate-400 mt-1">Save current transformation settings to MongoDB for quick recall.</p>
            </div>

            <div className="space-y-4">
              <div>
                <label className="text-xs font-semibold text-slate-300 mb-1.5 block">Preset Name</label>
                <input
                  type="text"
                  value={presetName}
                  onChange={(e) => setPresetName(e.target.value)}
                  placeholder="e.g. Card Thumbnail (AVIF)"
                  className="w-full bg-slate-900 border border-slate-700 rounded-xl px-4 py-2.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                />
              </div>

              <div>
                <label className="text-xs font-semibold text-slate-300 mb-1.5 block">Description</label>
                <input
                  type="text"
                  value={presetDesc}
                  onChange={(e) => setPresetDesc(e.target.value)}
                  placeholder="Brief description of this preset"
                  className="w-full bg-slate-900 border border-slate-700 rounded-xl px-4 py-2.5 text-xs text-slate-200 focus:outline-none focus:border-indigo-500"
                />
              </div>

              <div className="p-4 bg-slate-900/80 rounded-xl border border-slate-800 text-xs font-mono text-slate-400 space-y-1.5">
                <div>Dimensions: {w} × {h} ({fit})</div>
                <div>Format: {format} (q: {q}{qt ? `, qt: ${qt}` : ''})</div>
                <div>Accept: {acceptHeader}</div>
              </div>
            </div>

            <div className="flex justify-end gap-3 pt-2">
              <button
                onClick={() => setShowPresetModal(false)}
                className="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={saveCurrentPreset}
                className="px-5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold cursor-pointer shadow-lg shadow-indigo-600/30"
              >
                Save to Database
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
