import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

export default defineConfig(async ({ mode }) => {
  const { stdout } = await promisify(execFile)('git', ['rev-parse', '--short', 'HEAD'], {
    cwd: __dirname,
    encoding: 'utf8',
  })

  return {
  define: {
    'import.meta.env.VITE_BUILD_HASH': JSON.stringify(stdout.trim()),
  },
  plugins: [
    react(),
    tailwindcss(),
  ],
  build: {
    outDir: process.env.STATIC_DIR || 'dist',
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
