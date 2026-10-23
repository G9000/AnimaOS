import { defineConfig } from 'vitest/config';

export default defineConfig({
  root: import.meta.dirname,
  test: {
    name: '@animaos/playground',
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    reporters: ['default'],
  },
});
