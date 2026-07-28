import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // Svelte 5 runes throughout; no legacy reactive statements anywhere in this app.
    runes: true,
  },
};
