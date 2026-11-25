import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';

// Em dev, a interface roda no Vite e fala com o servidor Rust na 7340.
// Duas páginas: o console (index.html) e a tela do Sona Bridge (bridge.html).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    rollupOptions: { input: { main: fileURLToPath(new URL('./index.html', import.meta.url)), bridge: fileURLToPath(new URL('./bridge.html', import.meta.url)) } },
  },
  server: {
    port: 5174,
    proxy: { '/api': { target: 'http://127.0.0.1:7340', ws: true } },
  },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
