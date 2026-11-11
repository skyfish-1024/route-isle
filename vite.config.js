import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'
import { presetWind3 } from 'unocss'

export default defineConfig({
  plugins: [vue(), UnoCSS({ presets: [presetWind3()] })],
  optimizeDeps: { include: ['@lucide/vue'] },
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
})
