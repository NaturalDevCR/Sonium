import vue     from '@vitejs/plugin-vue';
import tailwind from '@tailwindcss/vite';
import { VitePWA } from 'vite-plugin-pwa';
import { defineConfig } from 'vite';
import { fileURLToPath, URL } from 'node:url';

export default defineConfig({
  plugins: [
    tailwind(),
    vue(),
    VitePWA({
      registerType:  'autoUpdate',
      includeAssets: ['favicon.png', 'apple-touch-icon.png', 'pwa-512x512.png'],
      manifest: {
        name:             'Sonium',
        short_name:       'Sonium',
        description:      'Multiroom audio control',
        theme_color:      '#0f172a',
        background_color: '#0f172a',
        display:          'standalone',
        start_url:        '/',
        icons: [
          { src: 'pwa-192x192.png', sizes: '192x192', type: 'image/png' },
          { src: 'pwa-512x512.png', sizes: '512x512', type: 'image/png', purpose: 'any maskable' },
        ],
      },
      workbox: {
        navigateFallback:        'index.html',
        navigateFallbackDenylist: [/^\/api/],
      },
    }),
  ],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  build: {
    outDir: 'dist',
  },
  server: {
    proxy: {
      '/api': {
        target:       'http://localhost:1711',
        ws:           true,
        changeOrigin: true,
      },
    },
  },
});
