import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// package.json is type: module, so the vite config loads as ESM — no __dirname
const root = dirname(fileURLToPath(import.meta.url));

// Shared-library allowlist (architecture.md "Plane A contract"): externalized
// at build time, the bundle keeps the bare import and the runtime resolves it
// via the shell index.html import map into the shell's vendor output —
// react/rxjs/i18next are app-wide singletons (two instances would crash with
// Invalid hook call).
// '@shell/sdk' (api entry) and '@shell/sdk/ui' (ui entry) are both import map
// keys: component source imports by these names (the shell's official SDK
// facade), and resolve.alias maps them to the slsdk-ts package in
// node_modules for type and build-time resolution;
// external is decided by exact match on the original import specifier, so the
// bundle keeps '@shell/sdk(/ui)' verbatim — missing the ui key would inline
// the SDK facility source (103KB) into every component bundle, breaking the
// shared singleton.
// react-dom/client and jsx-runtime(-dev-runtime) are internal subpath
// mappings of allowlisted packages; the allowlist set is not widened further,
// and the react-dom family is listed in full to align with the shell's import
// map key set.
const SHARED_IMPORTS = [
  'react',
  'react/jsx-runtime',
  'react/jsx-dev-runtime',
  'react-dom',
  'react-dom/client',
  'rxjs',
  'i18next',
  'react-i18next',
  '@shell/sdk',
  '@shell/sdk/ui',
];

export default defineConfig({
  // VITE_BASE: still required in standalone deployment when mounted under a
  // path prefix (e.g. /apps/xatodo/); the component bundle entry.js uses
  // relative imports internally and is unaffected by base
  base: process.env.VITE_BASE || '/',
  plugins: [react()],
  resolve: {
    alias: {
      '@shell/sdk': 'slsdk-ts',
    },
  },
  server: {
    proxy: {
      // standalone dev debugging: /api goes straight to the local server
      // (the component mounts /api/v1/... itself, no rewrite needed)
      '/api': {
        target: 'http://localhost:8600',
        changeOrigin: true,
      },
    },
  },
  build: {
    rollupOptions: {
      // Single build entry: the component ESM (dynamically imported by the
      // shell in module mode, default export being the component root).
      // index.html + main.tsx only serve vite dev standalone debugging and
      // never enter the build output.
      input: resolve(root, 'src/entry.tsx'),
      // The entry signature must be preserved: the consumer (shell
      // ComponentHost) imports it dynamically at runtime, so rollup sees no
      // static consumer at build time — without preserveEntrySignatures the
      // default export (the App component tree) would be tree-shaken away,
      // leaving only side effects from top-level service instantiation
      // (verified empty shell: no component code, no inlined CSS, no default
      // export — the component bundle would be unusable).
      // vite lib mode defaults to strict; declared explicitly here in
      // bundler form as the same semantics.
      preserveEntrySignatures: 'strict',
      external: (source) => SHARED_IMPORTS.includes(source),
      output: {
        // Fixed file names (no hash): manifest.ui.entry references entry.js
        // statically
        entryFileNames: 'entry.js',
        chunkFileNames: 'assets/chunk-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
  },
});
