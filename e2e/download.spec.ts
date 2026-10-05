import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { addProvider, download, expectedFiles, open, PORTS, saveFiles, WORK } from './helpers';

test('1. clean download: output SHA-256 matches', async ({ page }) => {
  const errors = await open(page);
  await addProvider(page, { port: PORTS.full, name: 'Full' });
  const card = await download(page, 'clean');
  await expect(card.getByText('Done. 2 files ready.')).toBeVisible();
  const files = await saveFiles(page, 2);
  for (const [name, hash] of expectedFiles('clean')) {
    expect(files.get(name), name).toBe(hash);
  }
  expect(files.has('clean.par2')).toBe(true);
  expect(errors).toEqual([]);
});

test('2. primary misses 10% of articles, the backup has them', async ({ page }) => {
  await open(page);
  await addProvider(page, { port: PORTS.sparse, name: 'Primary' });
  await addProvider(page, { port: PORTS.full, name: 'Backup', backup: true });
  const card = await download(page, 'backup');
  await expect(card.getByText(/^Done\./)).toBeVisible();
  // debian-12.iso has 49 articles; parts 3, 13, 23, 33 and 43 come from the backup.
  await expect(card.getByTestId('backup-count')).toHaveText(/From backup\s*5/);
  const files = await saveFiles(page, 2);
  for (const [name, hash] of expectedFiles('backup')) {
    expect(files.get(name), name).toBe(hash);
  }
});

test('3. both providers miss articles; PAR2 repairs it', async ({ page }) => {
  await open(page);
  await addProvider(page, { port: PORTS.partialA, name: 'Provider A' });
  await addProvider(page, { port: PORTS.partialB, name: 'Provider B', backup: true });
  const card = await download(page, 'repair');
  await expect(card.getByText(/^Done\..*repaired with PAR2/)).toBeVisible();
  await expect(card.getByTestId('repaired-count')).toHaveText(/Repaired\s*2/);
  // The data file was posted under an obfuscated name; PAR2 restores it.
  const files = await saveFiles(page, 3);
  for (const [name, hash] of expectedFiles('repair')) {
    expect(files.get(name), name).toBe(hash);
  }
});

test('5. wrong password shows the 481 message', async ({ page }) => {
  await open(page);
  const message = await addProvider(page, { port: PORTS.full, name: 'Wrong', password: 'nope' }, true);
  expect(message).toBe('Login failed: wrong username or password (481)');
  // A job with only this provider fails with the same reason.
  await page.locator('input[type=file][accept*=nzb]').setInputFiles(join(WORK, 'mock', 'clean.nzb'));
  await page.getByRole('button', { name: 'Download', exact: true }).click();
  await expect(page.getByRole('alert').filter({ hasText: '(481)' }).first()).toBeVisible();
});
