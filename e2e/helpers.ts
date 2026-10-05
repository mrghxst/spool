import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, type Download, type Page } from '@playwright/test';

export const WORK = join(dirname(fileURLToPath(import.meta.url)), '.work');
export const RELAY = 'ws://127.0.0.1:18080';

export const PORTS = { full: 15631, sparse: 15632, partialA: 15633, partialB: 15634 } as const;

export type TestProvider = {
  port: number;
  name: string;
  password?: string;
  backup?: boolean;
  connections?: number;
};

export function sha256(buf: Buffer): string {
  return createHash('sha256').update(buf).digest('hex');
}

export function expectedFiles(job: string): Map<string, string> {
  const dir = join(WORK, 'expected', job);
  return new Map(readdirSync(dir).map((f) => [f, sha256(readFileSync(join(dir, f)))]));
}

/** Opens the app in E2E mode with the mock CA trusted. */
export async function open(page: Page) {
  const ca = readFileSync(join(WORK, 'mock', 'ca.der')).toString('base64');
  await page.addInitScript((b64) => {
    (window as unknown as { __SPOOL_TEST_CA__: string }).__SPOOL_TEST_CA__ = b64;
  }, ca);
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  await page.goto('/?e2e');
  await expect(page.getByText('Drop an .nzb file')).toBeVisible();
  return errors;
}

/** Adds a provider through the UI. */
export async function addProvider(page: Page, p: TestProvider, test = false) {
  await page.getByRole('button', { name: 'Providers' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Add provider' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add provider' });
  await dialog.getByLabel('Server').fill('127.0.0.1');
  await dialog.getByLabel('Port').fill(String(p.port));
  await dialog.getByLabel('Connections').fill(String(p.connections ?? 4));
  await dialog.getByLabel('Username').fill('spool');
  await dialog.getByLabel('Password').fill(p.password ?? 'secret');
  await dialog.getByLabel('Name (optional)').fill(p.name);
  if (p.backup) await dialog.getByLabel('Use only for missing articles').check();
  let message: string | null = null;
  if (test) {
    await dialog.getByRole('button', { name: 'Test connection' }).click();
    const status = dialog.getByRole('status');
    await expect(status).toBeVisible();
    message = await status.textContent();
  }
  await dialog.getByRole('button', { name: 'Save provider' }).click();
  await page.getByRole('dialog', { name: 'Providers' }).getByRole('button', { name: 'Close' }).click();
  return message;
}

/** Chooses an NZB, downloads it and waits until the job is done. */
export async function download(page: Page, job: string) {
  await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', `${job}.nzb`));
  await page.getByRole('button', { name: 'Download', exact: true }).click();
  const card = page.locator('section.job').first();
  await expect(card.getByText(/^(Done\.|Finished with problems\.)/)).toBeVisible({ timeout: 60_000 });
  return card;
}

/** Clicks "Save files" and returns name -> sha256 of every download. */
export async function saveFiles(page: Page, count: number): Promise<Map<string, string>> {
  const downloads: Download[] = [];
  const onDownload = (d: Download) => downloads.push(d);
  page.on('download', onDownload);
  await page.getByRole('button', { name: 'Save files' }).click();
  await expect.poll(() => downloads.length, { timeout: 30_000 }).toBe(count);
  page.off('download', onDownload);
  const out = new Map<string, string>();
  for (const d of downloads) {
    const path = await d.path();
    out.set(d.suggestedFilename(), sha256(readFileSync(path)));
  }
  return out;
}
