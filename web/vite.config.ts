import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig(({ mode }) => {
  // In dev the UI runs on Vite and proxies to the Rust server, so application
  // code can always use same-origin relative URLs.
  const env = loadEnv(mode, process.cwd(), 'YK_')
  const api = env.YK_API ?? 'http://127.0.0.1:23130'

  return {
    plugins: [react()],
    build: {
      outDir: 'dist',
      // The workbench is served from the user's own machine; readable stack
      // traces in bug reports are worth more than the extra kilobytes.
      sourcemap: true,
      chunkSizeWarningLimit: 900,
    },
    server: {
      port: 5273,
      proxy: {
        '/api': { target: api, changeOrigin: true, ws: true },
      },
    },
  }
})
