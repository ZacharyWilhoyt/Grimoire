import { readFile } from 'node:fs/promises'
import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'

// JASSUB 2.4.1 publishes a WASM loader map without its referenced source.
// Intercept just that loader before Vite tries to resolve the broken map.
function jassubWorkerSourceMap(): Plugin {
  return {
    name: 'grimoire-jassub-worker-source-map',
    enforce: 'pre',
    async load(id) {
      const file = id.split('?')[0].replaceAll('\\', '/')
      if (!file.endsWith('/node_modules/jassub/dist/wasm/jassub-worker.js') || [...new URLSearchParams(id.split('?')[1]).keys()].some(key => !['v', 't'].includes(key))) return
      const code = await readFile(file, 'utf8')
      return { code: code.replace(/^\/\/# sourceMappingURL=.*$/gm, ''), map: null }
    },
  }
}

export default defineConfig({
  plugins: [jassubWorkerSourceMap(), react()],
  worker: { plugins: () => [jassubWorkerSourceMap()] },
  server: { port: 5173, strictPort: true, proxy: { '/api': 'http://127.0.0.1:3000' } },
})
