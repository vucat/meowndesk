import { expect, test } from '@playwright/test';

test('keyboard reaches range, Pause and Settings; modal traps and restores focus', async ({
  page,
}) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link', { name: 'Skip to Overview' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('main')).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('radio', { name: '1m', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('radio', { name: '5m', exact: true })).toBeChecked();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Pause', exact: true })).toBeFocused();
  await page.keyboard.press('Space');
  await expect(page.getByTestId('history-status')).toContainText('Frozen at');
  await expect(page.getByRole('button', { name: 'Resume', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.keyboard.press('Tab');
  const settings = page.getByRole('button', { name: 'Settings', exact: true });
  await expect(settings).toBeFocused();
  const outline = await settings.evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(outline).not.toBe('none');
  await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog).toHaveAttribute('aria-modal', 'true');
  await expect(dialog.getByRole('radio', { name: '1 second', exact: true })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.getByRole('button', { name: 'Save', exact: true })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('radio', { name: '1 second', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(dialog.getByRole('radio', { name: '2 seconds' })).toBeChecked();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(settings).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Space');
  await expect(page.getByTestId('history-status')).toHaveText('Live updates');
});

test('reduced motion retains textual charts, sensor reasons and readable controls', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/tests/browser/support/overview.html?scenario=no_gpu');
  expect(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches)).toBe(
    true,
  );
  await expect(page.locator('.bento-card')).toHaveCount(9);
  await expect(page.locator('.dot-chart canvas')).toHaveCount(6);
  for (const caption of await page.locator('.dot-chart figcaption').all()) {
    await expect(caption).toContainText(/gap columns|60 measured columns/);
    await expect(caption).toContainText(/0–/);
  }
  await expect(page.getByRole('region', { name: 'GPU & VRAM' })).toContainText(
    /unsupported|unavailable|No GPU/i,
  );
  const animations = await page.evaluate(
    () => document.getAnimations().filter((animation) => animation.playState === 'running').length,
  );
  expect(animations).toBe(0);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  for (const control of await page.getByRole('dialog').locator('button, input').all()) {
    const box = await control.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
});
