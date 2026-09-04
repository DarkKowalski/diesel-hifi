import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// A relative base makes one build work at a domain root and at any subpath.
// Set VITE_BASE (for example `/diesel-hifi/`) for hosts that need an absolute
// prefix; `pnpm build:subpath` does exactly that.
const base = process.env.VITE_BASE ?? './';

export default defineConfig({
  base,
  plugins: [svelte()],
  build: {
    target: 'es2022',
    // Everything must be emitted locally: no CDN, no runtime fetch.
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  worker: {
    format: 'es',
  },
  server: {
    port: 5173,
  },
  preview: {
    port: 4173,
  },
});
