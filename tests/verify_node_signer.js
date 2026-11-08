/**
 * Verifies that examples/node/signer.js matches all vectors in docs/signing-vectors.json
 */

const fs = require('fs');
const path = require('path');
const {
  rfc3986Encode,
  computeExpiryBucket,
  buildCanonicalString,
  computeSignature,
  MAX_SIGNATURE_TTL_SECS,
} = require('../examples/node/signer');

const vectorsPath = path.join(__dirname, '..', 'docs', 'signing-vectors.json');
const vectorData = JSON.parse(fs.readFileSync(vectorsPath, 'utf8'));

console.log(`Verifying Node.js signer across ${vectorData.vectors.length} vectors...`);

for (const vec of vectorData.vectors) {
  if (vec.type === 'positive') {
    const canonical = buildCanonicalString(vec.exp, vec.params, vec.kid);
    if (canonical !== vec.canonical_string) {
      console.error(`FAIL [${vec.id}]: canonical mismatch`);
      console.error('Expected:\n' + vec.canonical_string);
      console.error('Actual:\n' + canonical);
      process.exit(1);
    }

    const sig = computeSignature(canonical, vec.key);
    if (sig !== vec.expected_sig) {
      console.error(`FAIL [${vec.id}]: signature mismatch`);
      console.error(`Expected: ${vec.expected_sig}, got: ${sig}`);
      process.exit(1);
    }
  }
}

// Verify expiry bucket logic and edge assertions
const now = 1700000000;
const bucket1 = computeExpiryBucket(now, 300, 3600);
if (bucket1 !== 1700002800 && (bucket1 % 3600 !== 0)) {
  console.error(`FAIL: bucket alignment error: ${bucket1}`);
  process.exit(1);
}

// Assert over-limit bucket + minTtl throws error
let threw = false;
try {
  computeExpiryBucket(now, 50000, 40000); // 90000 > 86400
} catch (e) {
  threw = true;
}
if (!threw) {
  console.error('FAIL: computeExpiryBucket failed to throw on exceeding MAX_SIGNATURE_TTL_SECS');
  process.exit(1);
}

console.log('All Node.js signer verification checks passed!');

