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
    watch: {
      // Native code is watched by Tauri; models and tool environments are not frontend sources.
      ignored: ['**/src-tauri/**', '**/vendor/**', '**/.local/**', '**/models/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: { target: 'es2022' },
})
