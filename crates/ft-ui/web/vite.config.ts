/// <reference types="vitest" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { TanStackRouterVite } from '@tanstack/router-plugin/vite'
import path from 'node:path'

export default defineConfig({
  plugins: [TanStackRouterVite({ routesDirectory: './src/routes', generatedRouteTree: './src/routeTree.gen.ts' }), react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api/events': {
        target: 'http://127.0.0.1:5174',
        changeOrigin: false,
        ws: false,
        configure: (proxy) => {
          proxy.on('proxyReq', (req) => {
            req.setHeader('Accept', 'text/event-stream')
          })
        },
      },
      '/api': {
        target: 'http://127.0.0.1:5174',
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // Split vendor chunks. W1-C's bundle report flagged a single 500kB+
    // chunk; W2-C breaks the heavyweight deps out so each piece can be
    // cached independently and the initial route doesn't pay for them all.
    rollupOptions: {
      output: {
        manualChunks: (id) => {
          if (!id.includes('node_modules')) return undefined
          if (id.includes('highlight.js') || id.includes('lowlight')) {
            return 'highlight-vendor'
          }
          if (id.includes('@tiptap') || id.includes('tiptap-markdown') || id.includes('prosemirror')) {
            return 'tiptap-vendor'
          }
          if (id.includes('@dnd-kit')) return 'dnd-vendor'
          if (
            id.includes('react-force-graph') ||
            id.includes('force-graph') ||
            // Catch ALL d3-* submodules, not a hand-picked subset. d3 packages
            // depend on each other (d3-zoom -> d3-transition -> d3-color, ...);
            // leaving some in graph-vendor and letting the rest fall through to
            // react-vendor splits that web across chunks and recreates a
            // circular import.
            id.includes('d3-') ||
            id.includes('three')
          ) {
            return 'graph-vendor'
          }
          // React core ONLY (react, react-dom, scheduler) goes in react-vendor.
          // These packages have no third-party runtime imports, so this chunk
          // has zero outgoing cross-chunk edges -- it is a pure sink and can
          // never be part of a circular import.
          //
          // Everything that consumes React's namespace at module-init time
          // (Radix, @floating-ui, cmdk, TanStack, scroll-lock utils) is left to
          // the default/shared chunk on purpose. Those read `React.useLayoutEffect`
          // at the top level; they MUST import React one-directionally from a
          // fully-initialized chunk. The previous config split @radix-ui into its
          // own chunk AND split @floating-ui across radix-vendor/react-vendor,
          // which -- once the redesign added popper-based components and cmdk --
          // made react-vendor and radix-vendor import each other. On that ESM
          // cycle, radix-vendor ran before react-vendor finished initializing,
          // so the React namespace binding was still `undefined`:
          // "Cannot read properties of undefined (reading 'useLayoutEffect')".
          // Keeping react-vendor a pure sink eliminates the whole class of bug.
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id)) {
            return 'react-vendor'
          }
          return undefined
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./tests/setup.ts'],
    css: false,
    exclude: ['node_modules', 'dist', 'tests/e2e/**'],
  },
})
