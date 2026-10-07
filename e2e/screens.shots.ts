import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import { addProvider, open, PORTS, WORK } from './helpers';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = join(WORK, 'screens');
const ASSETS = join(ROOT, 'docs', 'assets');
mkdirSync(OUT, { recursive: true });
mkdirSync(ASSETS, { recursive: true });

const SLOW = { primary: 15635, backup: 15636 };

async function shot(page: Page, name: string) {
  await page.waitForTimeout(250); // let sheet transitions settle
  await page.screenshot({ path: join(OUT, `${name}.png`), fullPage: true });
}

async function articles(page: Page): Promise<[number, number]> {
  const text = (await page.locator('section.job dd').nth(3).textContent()) ?? '';
  const m = /([\d,]+) of ([\d,]+)/.exec(text);
  return m ? [Number(m[1].replace(/,/g, '')), Number(m[2].replace(/,/g, ''))] : [0, 1];
}

for (const theme of ['dark', 'light'] as const) {
  test.describe(theme, () => {
    test.use({ colorScheme: theme, viewport: { width: 1280, height: 860 }, deviceScaleFactor: 2 });

    test(`all screens (${theme})`, async ({ page }) => {
      await open(page);
      await shot(page, `${theme}-01-first-visit`);

      // Add provider sheet with a test result.
      await page.getByRole('button', { name: 'Add provider' }).first().click();
      const sheet = page.getByRole('dialog', { name: 'Add provider' });
      await sheet.getByLabel('Server').fill('127.0.0.1');
      await sheet.getByLabel('Port').fill(String(SLOW.primary));
      await sheet.getByLabel('Username').fill('spool');
      await sheet.getByLabel('Password').fill('secret');
      await sheet.getByLabel('Name (optional)').fill('Primary');
      await sheet.getByRole('button', { name: 'Test connection' }).click();
      await expect(sheet.getByRole('status')).toContainText('Logged in');
      await shot(page, `${theme}-02-add-provider`);
      await sheet.getByRole('button', { name: 'Save provider' }).click();
      await page.getByRole('dialog', { name: 'Providers' }).getByRole('button', { name: 'Close' }).click();
      await addProvider(page, { port: SLOW.backup, name: 'Backup', backup: true });

      await page.getByRole('button', { name: 'Providers' }).click();
      await shot(page, `${theme}-03-providers`);
      await page.keyboard.press('Escape');

      await page.getByRole('button', { name: 'Settings' }).click();
      await page.getByRole('button', { name: 'Test relay' }).click();
      await expect(page.getByRole('dialog', { name: 'Settings' }).getByRole('status')).toContainText('works');
      await shot(page, `${theme}-04-settings`);
      await page.keyboard.press('Escape');

      await shot(page, `${theme}-05-ready`);

      await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', 'ubuntu-24.04-desktop.nzb'));
      await expect(page.getByRole('button', { name: 'Download', exact: true })).toBeVisible();
      await shot(page, `${theme}-06-summary`);

      await page.getByRole('button', { name: 'Download', exact: true }).click();
      await expect
        .poll(async () => {
          const [done, total] = await articles(page);
          return done / total;
        }, { timeout: 60_000, intervals: [100] })
        .toBeGreaterThan(0.45);
      // Crop to the top bar and the job card.
      const card = await page.locator('section.job').boundingBox();
      await page.screenshot({
        path: join(ASSETS, `hero-${theme}.png`),
        clip: { x: 0, y: 0, width: 1280, height: Math.ceil((card?.y ?? 0) + (card?.height ?? 700) + 16) },
      });
      await shot(page, `${theme}-07-downloading`);

      await expect(page.getByText(/^Done\./)).toBeVisible({ timeout: 120_000 });
      await page.mouse.move(0, 0);
      await shot(page, `${theme}-08-done`);
    });

    test(`errors (${theme})`, async ({ page }) => {
      await open(page);
      await addProvider(page, { port: PORTS.full, name: 'Wrong password', password: 'nope' });
      await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', 'clean.nzb'));
      await page.getByRole('button', { name: 'Download', exact: true }).click();
      await expect(page.getByRole('alert').first()).toBeVisible();
      await shot(page, `${theme}-09-error`);
    });

    test(`mobile (${theme})`, async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 780 });
      await open(page);
      await shot(page, `${theme}-10-mobile-first-visit`);
      await addProvider(page, { port: SLOW.primary, name: 'Primary' });
      await addProvider(page, { port: SLOW.backup, name: 'Backup', backup: true });
      await page.getByRole('button', { name: 'Settings' }).click();
      await shot(page, `${theme}-11-mobile-settings`);
      await page.keyboard.press('Escape');
      await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', 'ubuntu-24.04-desktop.nzb'));
      await shot(page, `${theme}-12-mobile-summary`);
      await page.getByRole('button', { name: 'Download', exact: true }).click();
      await expect(page.getByText(/^Done\./)).toBeVisible({ timeout: 120_000 });
      await shot(page, `${theme}-13-mobile-done`);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, 'no horizontal scroll at 360 px').toBeLessThanOrEqual(0);
    });
  });
}
