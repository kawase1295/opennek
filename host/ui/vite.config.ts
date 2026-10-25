import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

// The host embeds dist/ with include_str! (opennek-core/src/server.rs), so the files keep fixed names
export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsDir: '',
    modulePreload: false,
    // the fonts go into app.css, since the host embeds only these three files
    assetsInlineLimit: (file: string) => (file.endsWith('.woff2') ? true : undefined),
    rolldownOptions: {
      output: {
        entryFileNames: 'app.js',
        assetFileNames: 'app[extname]',
        codeSplitting: false,
      },
    },
  },
  // `npm run dev` serves the UI with hot reload and passes the API to a host started with `opennek`
  server: {
    proxy: { '/api': 'http://127.0.0.1:8085' },
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
})
