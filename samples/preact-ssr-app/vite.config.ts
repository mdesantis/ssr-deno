import { defineConfig } from 'vite'

export default defineConfig({
  resolve: {
    alias: {
      'react-dom/server': 'preact-render-to-string',
      'react-dom': 'preact/compat',
      'react': 'preact/compat',
      'react/jsx-runtime': 'preact/jsx-runtime',
    },
  },
  ssr: {
    target: 'webworker',
    noExternal: true,
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: {
      input: 'src/entry-server.tsx',
    },
  },
})
