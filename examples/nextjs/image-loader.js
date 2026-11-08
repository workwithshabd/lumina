/**
 * Next.js Custom Image Loader for Atihsrah
 *
 * IMPORTANT ARCHITECTURAL PATTERN:
 * Next.js `next/image` executes custom loaders on the client-side at runtime,
 * calling `loader({ src, width, quality })`.
 *
 * In signed mode (`ATIHSRAH_AUTH_MODE=signed`), you CANNOT sign URLs in the loader
 * function on the client, because doing so would leak your master signing key!
 *
 * Recommended Pattern:
 * 1. Generate a pre-signed width map during SSR or Static Generation (e.g. `getStaticProps`,
 *    `getServerSideProps`, or React Server Component).
 * 2. Pass the pre-signed map or lookup key to your component.
 *
 * If you must use a dynamic client-side loader without server pre-signing:
 * - The proxy must be running with `ATIHSRAH_AUTH_MODE=public` (which disables cryptographic
 *   signatures). See below for public mode configuration and risks.
 */

/**
 * Pre-signed URL Map Loader (Production - Signed Mode)
 *
 * Maps a width to an already signed URL produced by a server-side route or Server Component.
 *
 * @param {object} options
 * @param {Record<number, string>} options.signedUrls Map of width -> signed URL
 */
export function createPresignedLoader(signedUrls) {
  return function presignedLoader({ width }) {
    if (signedUrls[width]) {
      return signedUrls[width];
    }
    // Fall back to nearest available signed width
    const availableWidths = Object.keys(signedUrls).map(Number).sort((a, b) => a - b);
    const closest = availableWidths.find(w => w >= width) || availableWidths[availableWidths.length - 1];
    return signedUrls[closest];
  };
}

/**
 * Public Mode Loader (Development / Unsigned Environments Only)
 *
 * WARNING: Requires `ATIHSRAH_AUTH_MODE=public`.
 * Running public mode in production leaves your proxy vulnerable to cache churn
 * and unauthorized CPU exhaustion by unauthenticated clients.
 */
export function publicAtihsrahLoader({ src, width, quality }) {
  const proxyBase = process.env.NEXT_PUBLIC_ATIHSRAH_URL || 'https://images.example.com';
  const qTarget = quality && quality >= 85 ? 'high' : quality && quality <= 65 ? 'draft' : 'good';
  const encodedSrc = encodeURIComponent(src);

  return `${proxyBase}/transform?src=${encodedSrc}&w=${width}&format=auto&quality_target=${qTarget}`;
}

