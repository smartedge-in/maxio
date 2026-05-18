import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

const backendPort = process.env.PORT ?? '9000'
const backendTarget = `http://127.0.0.1:${backendPort}`

export default defineConfig({
  plugins: [sveltekit(), tailwindcss()],
  server: {
    proxy: {
      '/api': backendTarget,
      '/healthz': backendTarget,
      '/readyz': backendTarget,
    },
  },
})
