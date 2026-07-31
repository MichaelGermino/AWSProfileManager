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
        manualChunks: {
          react: ['react', 'react-dom'],
          markdown: ['react-markdown', 'remark-gfm'],
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
