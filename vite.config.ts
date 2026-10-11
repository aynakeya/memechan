import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import ui from '@nuxt/ui/vite'

export default defineConfig({
  plugins: [
    vue(),
    ui({
      router: false,
      colorMode: false,
      autoImport: false,
      components: false,
      dts: false,
      icon: { mode: 'svg', clientBundle: { scan: true } },
      ui: { colors: { primary: 'sage', neutral: 'zinc' } },
    }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Avoid a Vite 8 / Nuxt UI module-ID race during eager transforms on warm starts.
    preTransformRequests: false,
    watch: {
      // Native code is watched by Tauri; models and tool environments are not frontend sources.
      ignored: ['**/src-tauri/**', '**/vendor/**', '**/.local/**', '**/models/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: { target: 'es2022' },
})
