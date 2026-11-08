/**
 * Database Module for Atihsrah MERN Dashboard
 *
 * Connects to MongoDB via Mongoose when available.
 * If MongoDB is not running locally, seamlessly falls back to an
 * embedded JSON-file persistent store so the dashboard works out of the box.
 */

const mongoose = require('mongoose');
const fs = require('fs');
const path = require('path');

const STORE_PATH = path.join(__dirname, 'data', 'store.json');

// Initial default seed presets
const DEFAULT_PRESETS = [
  {
    id: 'preset_avif_hero',
    name: 'AVIF High-Res Hero',
    description: 'Compress high-resolution hero photo into AVIF with cover crop',
    params: {
      w: 800,
      h: 600,
      fit: 'cover',
      format: 'avif',
      q: 65,
      allow_upscale: 0,
    },
    headers: {
      accept: 'image/avif,image/webp,*/*',
    },
    createdAt: new Date().toISOString(),
  },
  {
    id: 'preset_auto_negotiation',
    name: 'Modern Browser Auto-Negotiation',
    description: 'Automatic format selection (AVIF for modern, WebP/JPEG fallback)',
    params: {
      w: 600,
      h: 400,
      fit: 'cover',
      format: 'auto',
      allow_upscale: 0,
    },
    headers: {
      accept: 'image/avif,image/webp,*/*',
    },
    createdAt: new Date().toISOString(),
  },
  {
    id: 'preset_alpha_logo',
    name: 'Transparent Icon & Logo',
    description: 'Contain fit preserving transparency with alpha optimization',
    params: {
      w: 250,
      h: 250,
      fit: 'contain',
      format: 'auto',
      allow_upscale: 1,
    },
    headers: {
      accept: 'image/avif,image/webp,*/*',
    },
    createdAt: new Date().toISOString(),
  },
  {
    id: 'preset_thumbnail_fast',
    name: 'Fast Compact Thumbnail',
    description: 'Small preview thumbnail at quality 50',
    params: {
      w: 150,
      h: 150,
      fit: 'cover',
      format: 'auto',
      q: 50,
      allow_upscale: 0,
    },
    headers: {
      accept: 'image/avif,image/webp,*/*',
    },
    createdAt: new Date().toISOString(),
  },
  {
    id: 'preset_retina_lossless',
    name: 'Retina Crisp Lossless Graphic',
    description: 'Lossless target for UI screenshots and diagrams',
    params: {
      w: 1200,
      h: 800,
      fit: 'inside',
      qt: 'lossless',
      allow_upscale: 1,
    },
    headers: {
      accept: 'image/webp,*/*',
    },
    createdAt: new Date().toISOString(),
  },
];

// Fallback Embedded Store
class EmbeddedStore {
  constructor(filepath) {
    this.filepath = filepath;
    this.data = { presets: [...DEFAULT_PRESETS], history: [] };
    this.load();
  }

  load() {
    try {
      if (fs.existsSync(this.filepath)) {
        const raw = fs.readFileSync(this.filepath, 'utf8');
        this.data = JSON.parse(raw);
        if (!this.data.presets || this.data.presets.length === 0) {
          this.data.presets = [...DEFAULT_PRESETS];
          this.save();
        }
      } else {
        this.save();
      }
    } catch (e) {
      console.warn('Failed to read local store, initializing fresh store:', e.message);
      this.save();
    }
  }

  save() {
    try {
      const dir = path.dirname(this.filepath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      fs.writeFileSync(this.filepath, JSON.stringify(this.data, null, 2), 'utf8');
    } catch (e) {
      console.error('Failed to save local store:', e.message);
    }
  }

  // Presets
  getPresets() {
    return this.data.presets;
  }

  getPresetById(id) {
    return this.data.presets.find(p => p.id === id);
  }

  createPreset(doc) {
    const item = {
      ...doc,
      id: doc.id || 'preset_' + Date.now() + '_' + Math.random().toString(36).substr(2, 5),
      createdAt: new Date().toISOString(),
    };
    this.data.presets.unshift(item);
    this.save();
    return item;
  }

  deletePreset(id) {
    const idx = this.data.presets.findIndex(p => p.id === id);
    if (idx !== -1) {
      const removed = this.data.presets.splice(idx, 1)[0];
      this.save();
      return removed;
    }
    return null;
  }

  // History
  getHistory(limit = 50) {
    return this.data.history.slice(0, limit);
  }

  addHistory(doc) {
    const item = {
      ...doc,
      id: 'hist_' + Date.now() + '_' + Math.random().toString(36).substr(2, 5),
      timestamp: new Date().toISOString(),
    };
    this.data.history.unshift(item);
    if (this.data.history.length > 200) {
      this.data.history = this.data.history.slice(0, 200);
    }
    this.save();
    return item;
  }

  clearHistory() {
    this.data.history = [];
    this.save();
    return true;
  }
}

let isMongoConnected = false;
let mongoMode = 'initializing';
const embeddedStore = new EmbeddedStore(STORE_PATH);

// Mongoose Schema Definitions
const PresetSchema = new mongoose.Schema({
  name: { type: String, required: true },
  description: String,
  params: { type: Object, required: true },
  headers: { type: Object, default: {} },
  createdAt: { type: Date, default: Date.now },
});

const HistorySchema = new mongoose.Schema({
  testUrl: String,
  params: Object,
  status: Number,
  latencyMs: Number,
  cacheStatus: String,
  contentType: String,
  originalBytes: Number,
  transformedBytes: Number,
  byteDeltaPct: Number,
  timestamp: { type: Date, default: Date.now },
});

let PresetModel = null;
let HistoryModel = null;

async function initDb(uri) {
  const mongoUri = uri || process.env.MONGODB_URI || 'mongodb://127.0.0.1:27017/atihsrah_dashboard';
  try {
    mongoose.set('strictQuery', false);
    await mongoose.connect(mongoUri, {
      serverSelectionTimeoutMS: 2000,
      connectTimeoutMS: 2000,
    });
    isMongoConnected = true;
    mongoMode = 'mongodb';
    PresetModel = mongoose.model('Preset', PresetSchema);
    HistoryModel = mongoose.model('History', HistorySchema);
    console.log(`[DB] Connected to MongoDB at ${mongoUri}`);

    // Seed presets in Mongo if empty
    const count = await PresetModel.countDocuments();
    if (count === 0) {
      for (const p of DEFAULT_PRESETS) {
        await PresetModel.create({
          name: p.name,
          description: p.description,
          params: p.params,
          headers: p.headers,
        });
      }
      console.log('[DB] Seeded initial presets into MongoDB.');
    }
  } catch (err) {
    isMongoConnected = false;
    mongoMode = 'embedded_fallback';
    console.log(`[DB] MongoDB not detected (${err.message}). Active mode: Embedded JSON fallback store (${STORE_PATH}).`);
  }
}

// Unified CRUD Service
const dbService = {
  getStatus() {
    return {
      connected: isMongoConnected,
      mode: mongoMode,
      storePath: isMongoConnected ? 'mongodb' : STORE_PATH,
    };
  },

  async getPresets() {
    if (isMongoConnected && PresetModel) {
      const items = await PresetModel.find().sort({ createdAt: -1 });
      return items.map(i => ({
        id: i._id.toString(),
        name: i.name,
        description: i.description,
        params: i.params,
        headers: i.headers,
        createdAt: i.createdAt,
      }));
    }
    return embeddedStore.getPresets();
  },

  async createPreset(doc) {
    if (isMongoConnected && PresetModel) {
      const created = await PresetModel.create(doc);
      return {
        id: created._id.toString(),
        name: created.name,
        description: created.description,
        params: created.params,
        headers: created.headers,
        createdAt: created.createdAt,
      };
    }
    return embeddedStore.createPreset(doc);
  },

  async deletePreset(id) {
    if (isMongoConnected && PresetModel) {
      const res = await PresetModel.findByIdAndDelete(id);
      return res ? { id } : null;
    }
    return embeddedStore.deletePreset(id);
  },

  async getHistory(limit = 50) {
    if (isMongoConnected && HistoryModel) {
      const items = await HistoryModel.find().sort({ timestamp: -1 }).limit(limit);
      return items.map(i => ({
        id: i._id.toString(),
        testUrl: i.testUrl,
        params: i.params,
        status: i.status,
        latencyMs: i.latencyMs,
        cacheStatus: i.cacheStatus,
        contentType: i.contentType,
        originalBytes: i.originalBytes,
        transformedBytes: i.transformedBytes,
        byteDeltaPct: i.byteDeltaPct,
        timestamp: i.timestamp,
      }));
    }
    return embeddedStore.getHistory(limit);
  },

  async addHistory(entry) {
    if (isMongoConnected && HistoryModel) {
      const created = await HistoryModel.create(entry);
      return {
        id: created._id.toString(),
        ...entry,
        timestamp: created.timestamp,
      };
    }
    return embeddedStore.addHistory(entry);
  },

  async clearHistory() {
    if (isMongoConnected && HistoryModel) {
      await HistoryModel.deleteMany({});
      return true;
    }
    return embeddedStore.clearHistory();
  },
};

module.exports = {
  initDb,
  dbService,
};

