import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig(({ isSsrBuild }) => {
  if (isSsrBuild) {
    return {
      plugins: [react()],
      ssr: {
        target: 'webworker',
        noExternal: true,
        resolve: {
          conditions: ['edge-light', 'module', 'browser', 'development'],
        },
      },
      build: {
        ssr: true,
        outDir: 'dist/server',
        rollupOptions: {
          input: 'src/entry-server.ts',
        },
      },
    }
  }

  return {
    plugins: [react()],
    build: {
      manifest: true,
      outDir: 'dist/client',
      rollupOptions: {
        input: 'src/entry-client.ts',
      },
    },
  }
})
