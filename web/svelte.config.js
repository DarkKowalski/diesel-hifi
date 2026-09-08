import { vitePreprocess } from '@astrojs/svelte';

// Svelte runs the interactive simulator inside Astro's static page.
export default {
  preprocess: vitePreprocess(),
};
