import { defineConfig } from 'astro/config';
import svelte from '@astrojs/svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  output: 'static',
  base: process.env.VITE_BASE ?? '/',
  integrations: [svelte()],
  devToolbar: { enabled: false },
  vite: {
    plugins: [tailwindcss()],
    build: { target: 'es2022', assetsInlineLimit: 0, sourcemap: false },
    worker: { format: 'es' },
  },
});
