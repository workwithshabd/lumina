# Using Atihsrah with Next.js (`next/image`)

This guide explains how to integrate Atihsrah with Next.js applications while preserving security and image performance.

---

## 1. Architectural Constraint: Server-Side vs Client-Side

Next.js `next/image` invokes custom loaders on the client side at runtime (`loader({ src, width, quality })`).

### Security Rule
> [!CAUTION]
> **NEVER expose your `ATIHSRAH_SIGNING_KEY` to the client or browser bundle.**
> Do not prefix your signing key with `NEXT_PUBLIC_`.
> Signing must only take place on the server (e.g. in React Server Components, `getServerSideProps`, `getStaticProps`, or API routes).

---

## 2. Recommended Approach: Server-Side Pre-Signed Map

When generating pages on the server (React Server Components in Next.js 13/14/15 App Router):

```jsx
// app/page.jsx (Server Component)
import Image from 'next/image';
import { signAtihsrahUrl, computeExpiryBucket } from '@/lib/signer';
import { createPresignedLoader } from '@/lib/image-loader';

export default async function Page() {
  const sourceImage = 'https://images.example.com/hero.jpg';
  const widths = [640, 750, 828, 1080, 1200, 1920];
  const now = Math.floor(Date.now() / 1000);
  const exp = computeExpiryBucket(now, 300, 3600); // 1-hour cache bucket

  const signedMap = {};
  for (const w of widths) {
    signedMap[w] = signAtihsrahUrl(process.env.ATIHSRAH_URL, {
      src: sourceImage,
      w,
      format: 'auto',
      quality_target: 'good',
    }, {
      keyHex: process.env.ATIHSRAH_SIGNING_KEY,
      exp,
    });
  }

  const loader = createPresignedLoader(signedMap);

  return (
    <main>
      <h1>Product Showcase</h1>
      {/*
        Hero image: use priority={true} (or loading="eager")
        Non-hero images: use loading="lazy" (the default in Next.js)
      */}
      <Image
        loader={loader}
        src={sourceImage}
        alt="Hero Image"
        width={1200}
        height={800}
        priority={true}
        sizes="(max-width: 768px) 100vw, (max-width: 1200px) 50vw, 33vw"
      />
    </main>
  );
}
```

---

## 3. Public Auth Mode (Unsigned) & Security Risks

If your application architecture strictly requires a purely client-side dynamic loader:
1. Configure Atihsrah with `ATIHSRAH_AUTH_MODE=public`.
2. Use `publicAtihsrahLoader` from `examples/nextjs/image-loader.js`.

### Security Notice:
> [!WARNING]
> In public mode, cryptographic HMAC verification is completely disabled.
> - While `ALLOWED_ORIGINS` and IP-based rate limiting remain enforced, any external caller can request arbitrary resizing and format conversions.
> - An attacker can generate requests for millions of distinct width and format combinations, causing cache thrashing and saturating CPU resources.
> - Always prefer signed mode with server-side pre-signing for production workloads.

