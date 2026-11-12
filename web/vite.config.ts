import { defineConfig } from 'vitest/config';

export default defineConfig({
  // Relative base so the same build works on GitHub Pages under /silicon/.
  base: './',
  worker: { format: 'es' },
  build: { target: 'es2022', sourcemap: true },
  test: { include: ['src/**/*.test.ts'], environment: 'node' },
});
