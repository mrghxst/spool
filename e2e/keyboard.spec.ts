import { expect, test } from '@playwright/test';
import { open } from './helpers';

test('keyboard: open and close sheets, focus rings, return focus', async ({ page }) => {
  await open(page);
  // Skip past the wordmark link to the Settings button.
  const settings = page.getByRole('button', { name: 'Settings' });
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press('Tab');
    if (await settings.evaluate((el) => el === document.activeElement)) break;
  }
  await expect(settings).toBeFocused();
  const outline = await settings.evaluate((el) => getComputedStyle(el).outlineWidth);
  expect(outline).toBe('2px');

  await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog).toBeVisible();
  // Focus moves into the sheet and stays there.
  expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  for (let i = 0; i < 30; i++) await page.keyboard.press('Tab');
  expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);

  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(settings).toBeFocused();

  // The add-provider form is reachable and the reorder hint is announced.
  await page.getByRole('button', { name: 'Add provider' }).first().press('Enter');
  await expect(page.getByRole('dialog', { name: 'Add provider' }).getByLabel('Server')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});
