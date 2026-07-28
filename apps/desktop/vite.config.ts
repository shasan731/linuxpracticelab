import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri sets this when running a debug build; read via globalThis so this config needs no
// Node type definitions of its own.
const debugBuild =
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env
    ?.TAURI_ENV_DEBUG === 'true';

// Tauri serves the built assets from the app bundle, so nothing here may assume a web origin.
export default defineConfig({
  plugins: [svelte()],
  // Tauri watches this port; failing loudly beats silently moving to another one.
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // src-tauri changes trigger a Rust rebuild, not a frontend reload.
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    // Matches the Edge WebView2 runtime the app ships against.
    target: 'chrome110',
    outDir: 'dist',
    emptyOutDir: true,
    // Debug builds keep sourcemaps; release builds do not ship them.
    sourcemap: debugBuild,
    minify: debugBuild ? false : 'esbuild',
  },
});
