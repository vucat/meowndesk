import { expect, test } from '@playwright/test';

test('the local shell boots with bundled assets and fits the window', async ({ page, baseURL }) => {
  const remoteRequests: string[] = [];
  page.on('request', (request) => {
    if (!request.url().startsWith(baseURL ?? '') && !request.url().startsWith('data:')) {
      remoteRequests.push(request.url());
    }
  });
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));

  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  expect(await page.evaluate(() => document.fonts.check('16px Inter'))).toBe(true);
  expect(
    await page
      .locator('.app-mark')
      .evaluate((element) => (element as HTMLImageElement).naturalWidth),
  ).toBeGreaterThan(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  expect(
    await page.locator('body').evaluate((element) => getComputedStyle(element).backgroundColor),
  ).toBe('rgb(11, 11, 12)');
  expect(remoteRequests).toEqual([]);
  expect(errors).toEqual([]);
});
