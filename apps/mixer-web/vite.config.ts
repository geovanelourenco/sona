import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// Em dev, a interface roda no Vite e fala com o servidor Rust na 7340.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5174,
    proxy: { '/api': { target: 'http://127.0.0.1:7340', ws: true } },
  },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
