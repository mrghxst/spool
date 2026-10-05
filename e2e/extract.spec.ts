import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { addProvider, download, expectedFiles, open, PORTS, saveFiles, WORK } from './helpers';

test('4. multi-volume 7z extracts and the parts are cleaned up', async ({ page }) => {
  await open(page);
  await addProvider(page, { port: PORTS.full, name: 'Full' });
  const card = await download(page, 'archive');
  await expect(card.getByText('Done. 2 files ready.')).toBeVisible();
  await expect(card.getByText('Archive password from the NZB:')).toBeVisible();
  const files = await saveFiles(page, 2);
  const expected = expectedFiles('archive');
  expect([...files.keys()].sort()).toEqual([...expected.keys()].sort());
  for (const [name, hash] of expected) {
    expect(files.get(name), name).toBe(hash);
  }
});

// RAR archives can only be created with RARLAB's proprietary `rar` tool, which
// CI doesn't install. See docs/decisions.md.
test('4b. multi-volume RAR extracts', async ({ page }) => {
  test.skip(!existsSync(join(WORK, 'fixtures', 'rar')), 'needs a local `rar` binary to build the fixture');
  await open(page);
  await addProvider(page, { port: PORTS.full, name: 'Full' });
  const card = await download(page, 'rar');
  await expect(card.getByText(/^Done\./)).toBeVisible();
  const files = await saveFiles(page, 2);
  for (const [name, hash] of expectedFiles('archive')) {
    expect(files.get(name), name).toBe(hash);
  }
});
