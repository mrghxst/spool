import { defineConfig, devices } from '@playwright/test';

// Screenshots of every screen in both themes; run with `bash e2e/run.sh --screens`.
export default defineConfig({
  testDir: '.',
  testMatch: /screens\.shots\.ts$/,
  outputDir: './test-results',
  timeout: 180_000,
  workers: 1,
  reporter: 'list',
  use: { baseURL: 'http://127.0.0.1:4173' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
