import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  return {
  plugins: [
    react(),
    tailwindcss(),
  ],
  build: {
    outDir: env.STATIC_DIR || 'dist',
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules')) {
            if (id.includes('react') || id.includes('react-dom')) return 'vendor-react'
            if (id.includes('@lottiefiles')) return 'vendor-lottie'
            if (id.includes('lucide-react')) return 'vendor-icons'
            if (id.includes('@base-ui') || id.includes('@radix-ui') || id.includes('class-variance-authority')) return 'vendor-ui'
            return 'vendor'
          }
        },
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:15000',
        changeOrigin: true,
      },
    },
  },
  }
})
