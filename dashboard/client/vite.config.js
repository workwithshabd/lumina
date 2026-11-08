import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
  ],
  server: {
    port: 3000,
    host: '127.0.0.1',
    proxy: {
      '/api': 'http://127.0.0.1:5001',
      '/images': 'http://127.0.0.1:5001',
    },
  },
  build: {
    outDir: '../server/public',
    emptyOutDir: true,
  },
});
