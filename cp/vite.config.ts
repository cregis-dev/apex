import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const BACKEND = 'http://127.0.0.1:12356'

export default defineConfig({
  plugins: [react()],
  base: '/cp/',
  server: {
    proxy: {
      '/api': BACKEND,
      '/admin': BACKEND,
    },
  },
  build: {
    outDir: '../target/web/cp',
    emptyOutDir: true,
  },
})
