/**
 * MERN Architecture: Express.js Backend Image Signing Service
 *
 * CRITICAL SECURITY INVARIANT:
 * - Signing keys NEVER reach the client browser.
 * - Every image width in a responsive `srcset` is signed individually on the server.
 * - NEVER append `&w=...` to an existing signed URL on the frontend; every parameter
 *   is covered by the HMAC signature, so appending query parameters causes HTTP 401.
 */

const express = require('express');
const { signAtihsrahUrl, computeExpiryBucket } = require('../node/signer');

const app = express();
const PORT = process.env.PORT || 3000;

const PROXY_BASE_URL = process.env.ATIHSRAH_URL || 'https://images.example.com';
const SIGNING_KEY = process.env.ATIHSRAH_SIGNING_KEY;

if (!SIGNING_KEY) {
  console.error('ERROR: ATIHSRAH_SIGNING_KEY environment variable is required.');
  console.error('Generate one with: openssl rand -hex 32');
  process.exit(1);
}

/**
 * Generates an image descriptor object with pre-signed URLs for responsive rendering.
 *
 * @param {string} sourceUrl Target image URL on allowlisted HTTPS origin
 * @param {number[]} widths Array of target widths, e.g. [320, 640, 1024, 1920]
 * @param {object} [options] Format, fit, quality target, version
 * @returns {object} { src, srcset, width, height }
 */
function generateResponsiveImage(sourceUrl, widths, options = {}) {
  // Use a 1-hour expiry bucket to maximize CDN and browser cache hit ratio
  const now = Math.floor(Date.now() / 1000);
  const exp = computeExpiryBucket(now, 300, 3600);

  const format = options.format || 'auto';
  const fit = options.fit || 'cover';
  const qualityTarget = options.qualityTarget || 'good';
  const v = options.v;

  const signedEntries = widths.map(w => {
    const params = {
      src: sourceUrl,
      w,
      format,
      fit,
      quality_target: qualityTarget,
    };
    if (v) params.v = v;

    const url = signAtihsrahUrl(PROXY_BASE_URL, params, {
      keyHex: SIGNING_KEY,
      exp,
    });
    return `${url} ${w}w`;
  });

  // Default fallback image is the middle or largest width
  const defaultWidth = widths[Math.floor(widths.length / 2)] || widths[0];
  const defaultParams = {
    src: sourceUrl,
    w: defaultWidth,
    format,
    fit,
    quality_target: qualityTarget,
  };
  if (v) defaultParams.v = v;

  const defaultSrc = signAtihsrahUrl(PROXY_BASE_URL, defaultParams, {
    keyHex: SIGNING_KEY,
    exp,
  });

  return {
    src: defaultSrc,
    srcset: signedEntries.join(', '),
  };
}

// Example API endpoint: GET /api/products
app.get('/api/products', (req, res) => {
  const products = [
    {
      id: 'prod_1',
      title: 'Premium Mountain Landscape',
      isHero: true, // Hero image on product detail page
      image: generateResponsiveImage(
        'https://images.example.com/photos/mountain.jpg',
        [320, 640, 960, 1280, 1920],
        { v: '1.0' }
      ),
    },
    {
      id: 'prod_2',
      title: 'Ergonomic Desk Chair',
      isHero: false,
      image: generateResponsiveImage(
        'https://images.example.com/products/chair.jpg',
        [240, 480, 720],
        { v: '1.0' }
      ),
    },
  ];

  res.json({ products });
});

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`Server listening on http://localhost:${PORT}`);
  });
}

module.exports = { app, generateResponsiveImage };

