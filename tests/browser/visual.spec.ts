import { expect, test } from '@playwright/test';

for (const scenario of ['full', 'no_gpu', 'denied', 'stale', 'no_active_interface']) {
  test(`visual contract and screenshots: ${scenario}`, async ({ page }, info) => {
    await page.goto(`/tests/browser/support/overview.html?scenario=${scenario}`);
    await page.evaluate(() => document.fonts.ready);
    const cards = page.locator('.bento-card');
    await expect(cards).toHaveCount(9);
    await expect(cards.locator('h2')).toHaveText([
      'CPU',
      'Memory',
      'GPU & VRAM',
      'Disk',
      'Network',
      'Power/Thermals',
      'Memory by Type',
      'Memory by App',
      'Resource Load',
    ]);
    await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(11, 11, 12)');
    await expect(cards.first()).toHaveCSS('background-color', 'rgb(20, 20, 22)');
    const dimensions = await page.evaluate(() => ({
      width: innerWidth,
      height: innerHeight,
      scroll: document.documentElement.scrollWidth,
    }));
    expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.width);
    const positions = await cards.evaluateAll((elements) =>
      elements.map((el) => {
        const { x, y, width } = el.getBoundingClientRect();
        return { x, y, width, scroll: el.scrollWidth, client: el.clientWidth };
      }),
    );
    const columns = dimensions.width >= 1200 ? 3 : 2;
    for (let i = 0; i < positions.length; i++) {
      const item = positions[i]!;
      expect(item.scroll).toBeLessThanOrEqual(item.client);
      expect(item.x + item.width).toBeLessThanOrEqual(dimensions.width);
      if (i >= columns) expect(item.y).toBeGreaterThan(positions[i - columns]!.y);
    }
    await page.screenshot({
      path: `artifacts/validation/screenshots/phase7-${scenario}-${dimensions.width}x${dimensions.height}.png`,
      fullPage: false,
    });
    await page.screenshot({
      path: `artifacts/validation/screenshots/phase7-${scenario}-${dimensions.width}x${dimensions.height}-full.png`,
      fullPage: true,
    });
    if (scenario === 'full') {
      // The shared fixture has a measured disk zero at column 0 and a gap at 16.
      const ink = await page
        .getByRole('region', { name: 'Disk', exact: true })
        .locator('canvas')
        .evaluate((element) => {
          const canvas = element as HTMLCanvasElement;
          const context = canvas.getContext('2d');
          if (!context) throw new Error('Canvas context unavailable');
          return [0, 16].map((column) => {
            const width = canvas.width / 60;
            const pixels = context.getImageData(
              Math.floor(column * width),
              0,
              Math.floor(width),
              canvas.height,
            ).data;
            let tinted = 0;
            for (let i = 0; i < pixels.length; i += 4) {
              if (
                Math.max(pixels[i]!, pixels[i + 1]!, pixels[i + 2]!) -
                  Math.min(pixels[i]!, pixels[i + 1]!, pixels[i + 2]!) >
                60
              )
                tinted++;
            }
            return tinted;
          });
        });
      expect(ink[0]).toBeGreaterThan(0);
      expect(ink[1]).toBe(0);
      await expect(
        page.getByRole('region', { name: 'Disk', exact: true }).locator('figcaption'),
      ).toContainText('measured zero columns');
    }
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.screenshot({
      path: `artifacts/validation/screenshots/phase7-settings-${scenario}-${info.project.name}.png`,
    });
  });
}
