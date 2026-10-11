import { defineConfig, devices } from '@playwright/test';

/** Static files and browser-local streams only. No application server is started. */
export default defineConfig({
  testDir: './src/console',
  outputDir: './test-output/console',
  reporter: 'list',
  workers: 1,
  retries: 0,
  use: {
    baseURL: 'http://anima-console.test',
    serviceWorkers: 'block',
    // The fake HTTP origin needs the same secure browser APIs as local/TLS deployments.
    launchOptions: {
      args: [
        '--unsafely-treat-insecure-origin-as-secure=http://anima-console.test',
      ],
    },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    channel: process.platform === 'win32' ? 'msedge' : undefined,
  },
  projects: [
    {
      name: 'console',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1280, height: 900 },
      },
    },
  ],
});
