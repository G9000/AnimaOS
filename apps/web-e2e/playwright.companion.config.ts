import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';

const workspaceRoot = resolve(__dirname, '../..');

export default defineConfig({
  testDir: './src',
  testMatch: 'companion.spec.ts',
  testIgnore: '**/console/**',
  outputDir: './test-output/companion',
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:4271',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command:
      'bun run --filter @animaos/web preview --host 127.0.0.1 --port 4271',
    url: 'http://127.0.0.1:4271',
    cwd: workspaceRoot,
    reuseExistingServer: false,
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
