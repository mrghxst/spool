import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { addProvider, open, PORTS, WORK } from './helpers';

for (const net of [1, 2, 4]) {
  test(`throughput: 256 MB through the relay, ${net} net workers`, async ({ page }) => {
    await open(page, '', `&net=${net}`);
    await addProvider(page, { port: PORTS.full, name: 'Local', connections: 16 });
    await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', 'perf.nzb'));
    const start = Date.now();
    await page.getByRole('button', { name: 'Download', exact: true }).click();
    await expect(page.getByText(/^Done\./)).toBeVisible({ timeout: 240_000 });
    const total = (Date.now() - start) / 1000;
    // "Average speed" covers the time spent receiving articles.
    const speed = (await page.getByTestId('average-speed').textContent())?.trim();
    console.log(`${net} net workers: transfer ${speed}, whole job ${total.toFixed(1)} s`);
  });
}
