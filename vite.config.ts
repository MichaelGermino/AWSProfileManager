import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  root: 'src/renderer',
  base: './',
  build: {
    outDir: '../../dist/renderer',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        // Split the big third-party libs out of the app chunk. These change far less often
        // than app code, so they also stay cached across updates.
        //
        // Do NOT add react-markdown/remark-gfm back here. A named manualChunk gets a
        // <link rel="modulepreload"> in index.html, so listing them pulled 165 kB into the
        // INITIAL load even though their only importer (AIMarkdown -> AIAssistantPanel ->
        // TerminalScreen) is lazy. Left unlisted, Rollup folds them into the TerminalScreen
        // chunk, which is fetched on first visit to the terminal. Initial payload went from
        // ~945 kB to ~403 kB. xterm stays listed because it correctly lands outside the
        // initial graph and benefits from separate caching.
        manualChunks: {
          react: ['react', 'react-dom'],
          xterm: ['@xterm/xterm', '@xterm/addon-fit'],
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src/renderer'),
    },
  },
  server: {
    port: 5173,
  },
});
