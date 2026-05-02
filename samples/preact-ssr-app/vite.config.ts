import { defineConfig } from 'vite'

export default defineConfig({
  ssr: {
    target: 'webworker',
    noExternal: true,
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: {
      input: 'src/entry-server.ts',
    },
  },
})
