import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import { readFile } from 'node:fs/promises'
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
    {
      name: 'font-licenses',
      async generateBundle() {
        const manifest = JSON.parse(await readFile(path.join(__dirname, 'package.json'), 'utf8'))
        const fonts = Object.keys(manifest.dependencies).filter(name => name.startsWith('@fontsource'))
        for (const name of fonts) {
          this.emitFile({
            type: 'asset',
            fileName: `licenses/${name.replace('@', '').replace('/', '-')}.txt`,
            source: await readFile(path.join(__dirname, 'node_modules', name, 'LICENSE'), 'utf8'),
          })
        }
      },
    },
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
