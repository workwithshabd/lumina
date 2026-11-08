/**
 * MERN Architecture: React Client Consumer
 *
 * Consumes pre-signed responsive image payloads produced by the Node backend.
 * Notice:
 * 1. ZERO signing keys exist on the frontend.
 * 2. `loading` is dynamic: "eager" for hero elements (preventing LCP delay),
 *    and "lazy" for below-the-fold content.
 */

import React, { useEffect, useState } from 'react';
import { ResponsiveImage } from '../react/Image';

export function ProductCatalog() {
  const [products, setProducts] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch('/api/products')
      .then(res => res.json())
      .then(data => {
        setProducts(data.products || []);
        setLoading(false);
      })
      .catch(err => {
        console.error('Failed to load products:', err);
        setLoading(false);
      });
  }, []);

  if (loading) return <div>Loading catalog...</div>;

  return (
    <main className="catalog-container">
      {products.map(prod => (
        <article key={prod.id} className="product-card">
          {/*
            Pass pre-signed src and srcset directly.
            Hero images receive loading="eager" to maximize Largest Contentful Paint (LCP).
          */}
          <ResponsiveImage
            src={prod.image.src}
            srcSet={prod.image.srcset}
            sizes="(max-width: 768px) 100vw, (max-width: 1200px) 50vw, 33vw"
            alt={prod.title}
            loading={prod.isHero ? 'eager' : 'lazy'}
            width={800}
            height={600}
          />
          <h2>{prod.title}</h2>
        </article>
      ))}
    </main>
  );
}

