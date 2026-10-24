import { defineConfig } from 'vitest/config';

export default defineConfig({
  root: import.meta.dirname,
  test: {
    coverage: { reportsDirectory: 'test-output/coverage' },
    projects: [
      {
        test: {
          name: 'village-web',
          environment: 'jsdom',
          include: ['web/src/**/*.test.{ts,tsx}'],
          setupFiles: ['web/src/test-setup.ts'],
        },
      },
      {
        test: {
          name: 'village-tools',
          environment: 'node',
          include: ['tools/**/*.test.ts'],
        },
      },
    ],
  },
});
