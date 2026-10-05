import { defineConfig, devices } from '@playwright/test';

// Throughput benchmark; run with `bash e2e/run.sh --perf`.
export default defineConfig({
  testDir: '.',
  testMatch: /perf\.bench\.ts$/,
  outputDir: './test-results',
  timeout: 300_000,
  workers: 1,
  reporter: 'list',
  use: { baseURL: 'http://127.0.0.1:4173' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
