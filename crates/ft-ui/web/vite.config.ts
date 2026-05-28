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
            id.includes('d3-force') ||
            id.includes('force-graph') ||
            id.includes('d3-quadtree') ||
            id.includes('d3-binarytree') ||
            id.includes('d3-zoom') ||
            id.includes('d3-drag') ||
            id.includes('d3-selection') ||
            id.includes('d3-octree') ||
            id.includes('three')
          ) {
            return 'graph-vendor'
          }
          if (id.includes('@radix-ui')) return 'radix-vendor'
          if (
            id.includes('react-dom') ||
            id.includes('@tanstack/react-query') ||
            id.includes('@tanstack/react-router') ||
            /\/react\//.test(id)
          ) {
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
