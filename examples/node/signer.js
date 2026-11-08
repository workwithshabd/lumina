/**
 * Atihsrah Reference URL Signer (Node.js)
 *
 * Implements strict RFC 3986 canonical query parameter encoding and HMAC-SHA256
 * signature generation for the Atihsrah image proxy.
 */

const crypto = require('crypto');

const MAX_SIGNATURE_TTL_SECS = 86400; // 24 hours

/**
 * Strict RFC 3986 percent-encoding.
 * Only unreserved characters [A-Za-z0-9\-_.~] are preserved unescaped.
 * Characters like [!'()*] are explicitly percent-encoded.
 *
 * @param {string|number} val
 * @returns {string}
 */
function rfc3986Encode(val) {
  return encodeURIComponent(String(val)).replace(
    /[!'()*]/g,
    c => '%' + c.charCodeAt(0).toString(16).toUpperCase()
  );
}

/**
 * Computes an expiration timestamp snapped to a discrete time bucket.
 * This guarantees that repeated requests for the same image within a time window
 * yield identical signed URLs, enabling upstream CDN and browser cache hits.
 *
 * @param {number} nowSecs Unix timestamp in seconds
 * @param {number} minTtlSecs Minimum lifetime required from now
 * @param {number} bucketSecs Granularity of the time bucket
 * @returns {number} Expiration timestamp in seconds
 */
function computeExpiryBucket(nowSecs, minTtlSecs, bucketSecs) {
  if (bucketSecs <= 0 || minTtlSecs <= 0) {
    throw new Error('minTtlSecs and bucketSecs must be positive integers');
  }
  if (bucketSecs + minTtlSecs > MAX_SIGNATURE_TTL_SECS) {
    throw new Error(
      `bucketSecs (${bucketSecs}) + minTtlSecs (${minTtlSecs}) exceeds MAX_SIGNATURE_TTL_SECS (${MAX_SIGNATURE_TTL_SECS})`
    );
  }
  return Math.ceil((nowSecs + minTtlSecs) / bucketSecs) * bucketSecs;
}

/**
 * Assembles the canonical string for HMAC-SHA256 computation.
 *
 * Format:
 * GET\n
 * /transform\n
 * {exp}\n
 * {param1_key}={param1_val}\n
 * ...
 * {paramN_key}={paramN_val}
 *
 * Rules:
 * - Sorted alphabetically by ASCII key name.
 * - 'sig' is excluded.
 * - 'exp' is included both as the header line and in the sorted parameter list.
 *
 * @param {number} exp Expiration timestamp
 * @param {Record<string, string|number>} params Request parameters
 * @param {string|null} [kid] Optional key ID for key rotation
 * @returns {string}
 */
function buildCanonicalString(exp, params, kid = null) {
  const q = { ...params };
  delete q.sig;
  q.exp = String(exp);
  if (kid !== null && kid !== undefined) {
    q.kid = String(kid);
  }

  const sortedKeys = Object.keys(q).sort();
  const lines = ['GET', '/transform', String(exp)];

  for (const k of sortedKeys) {
    lines.push(`${k}=${rfc3986Encode(q[k])}`);
  }

  return lines.join('\n');
}

/**
 * Computes the HMAC-SHA256 signature hex string for a canonical string.
 *
 * @param {string} canonical
 * @param {string} keyHex 64-character hexadecimal key
 * @returns {string}
 */
function computeSignature(canonical, keyHex) {
  const keyBytes = Buffer.from(keyHex, 'hex');
  if (keyBytes.length < 32) {
    throw new Error('Signing key must be at least 32 bytes (64 hex characters)');
  }
  return crypto.createHmac('sha256', keyBytes).update(canonical, 'utf8').digest('hex');
}

/**
 * Signs an Atihsrah transformation URL.
 *
 * NOTE: Every parameter sent to Atihsrah MUST be included in the signature.
 * Appending query parameters (e.g. &w=400) after signing will invalidate
 * the signature and result in HTTP 401 Unauthorized.
 *
 * @param {string} baseUrl Proxy base URL (e.g. 'https://images.example.com')
 * @param {Record<string, string|number>} params Query parameters (src, w, h, format, v, etc.)
 * @param {object} options
 * @param {string} options.keyHex 64-char hex key
 * @param {string|null} [options.kid] Optional key ID for key rotation
 * @param {number} [options.exp] Explicit expiration timestamp
 * @param {number} [options.minTtlSecs=300] Minimum TTL if computing bucket (default 5m)
 * @param {number} [options.bucketSecs=3600] Bucket interval if computing bucket (default 1h)
 * @param {number} [options.nowSecs] Current time override (for testing)
 * @returns {string} Fully signed URL
 */
function signAtihsrahUrl(baseUrl, params, options) {
  if (!options || !options.keyHex) {
    throw new Error('options.keyHex is required');
  }

  let exp = options.exp;
  if (exp === undefined || exp === null) {
    const now = options.nowSecs !== undefined ? options.nowSecs : Math.floor(Date.now() / 1000);
    const minTtl = options.minTtlSecs !== undefined ? options.minTtlSecs : 300;
    const bucket = options.bucketSecs !== undefined ? options.bucketSecs : 3600;
    exp = computeExpiryBucket(now, minTtl, bucket);
  }

  const kid = options.kid || null;
  const canonical = buildCanonicalString(exp, params, kid);
  const sig = computeSignature(canonical, options.keyHex);

  const queryParams = { ...params };
  delete queryParams.sig;
  queryParams.exp = String(exp);
  if (kid) {
    queryParams.kid = String(kid);
  }
  queryParams.sig = sig;

  // Preserve deterministic query parameter ordering
  const queryParts = Object.keys(queryParams).map(
    k => `${rfc3986Encode(k)}=${rfc3986Encode(queryParams[k])}`
  );

  const base = baseUrl.replace(/\/+$/, '');
  return `${base}/transform?${queryParts.join('&')}`;
}

module.exports = {
  MAX_SIGNATURE_TTL_SECS,
  rfc3986Encode,
  computeExpiryBucket,
  buildCanonicalString,
  computeSignature,
  signAtihsrahUrl,
};

