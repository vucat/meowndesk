import { expect, test } from '@playwright/test';

test('Bento controls stay on one row, donut sits beside its legend, diagnostics stay accessible', async ({
  page,
}, testInfo) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  await page.evaluate(() => document.fonts.ready);
  const controls = page.locator('.header-controls');
  const centerLines = await controls
    .locator('.live-state, .range-control, .pause-control, .settings-trigger')
    .evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return rect.y + rect.height / 2;
      }),
    );
  expect(centerLines).toHaveLength(4);
  expect(Math.max(...centerLines) - Math.min(...centerLines)).toBeLessThan(2);
  const card = page.getByRole('region', { name: 'Memory by Type' });
  const donut = await card.getByRole('img').boundingBox();
  const legend = await card.locator('.memory-legend').boundingBox();
  expect(donut).not.toBeNull();
  expect(legend).not.toBeNull();
  expect(donut!.x + donut!.width).toBeLessThan(legend!.x);
  expect(Math.abs(donut!.y + donut!.height / 2 - legend!.y - legend!.height / 2)).toBeLessThan(2);
  await expect(card.getByText('Cached', { exact: true })).toBeVisible();
  for (const caption of await page.locator('.dot-chart figcaption').all()) {
    const details = caption.locator('.sr-only');
    await expect(details).toContainText(/gap columns|60 measured columns/);
    const rect = await details.boundingBox();
    expect(rect!.width).toBeLessThanOrEqual(1);
    await expect(caption).toContainText(/0–[\d.]+ (?:%|W|[KM]B\/s)/);
  }
  await expect(page.getByTestId('history-status')).toHaveText('Live updates');
  await page.screenshot({
    path: `artifacts/validation/screenshots/bento-${testInfo.project.name}.png`,
    fullPage: true,
  });
  await page.getByRole('button', { name: 'Pause', exact: true }).click();
  await expect(page.getByTestId('history-status')).toBeVisible();
  await expect(page.getByTestId('history-status')).toContainText('Frozen at');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('Bento layout expands two columns for three churned apps without overlapping tiles', async ({
  page,
}, testInfo) => {
  await page.clock.install();
  await page.goto('/tests/browser/support/overview.html?scenario=process_churn&advance=true');
  await page.clock.runFor(2100);
  const card = page.getByRole('region', { name: 'Memory by App' });
  await expect(card.getByRole('listitem')).toHaveCount(3);
  const tiles = card.locator('.treemap-block');
  await expect(tiles).toHaveCount(3);
  await expect(tiles.first()).toHaveAttribute('title', 'new-workload · 1.46 GB');
  const rectangles = await tiles.evaluateAll((elements) =>
    elements.map((element) => {
      const { x, y, width, height } = element.getBoundingClientRect();
      return { x, y, width, height };
    }),
  );
  expect(new Set(rectangles.map((rect) => Math.round(rect.x))).size).toBeGreaterThan(1);
  expect(new Set(rectangles.map((rect) => Math.round(rect.y))).size).toBeGreaterThan(1);
  for (let index = 0; index < rectangles.length; index++) {
    const a = rectangles[index]!;
    expect(a.width).toBeGreaterThan(0);
    expect(a.height).toBeGreaterThan(0);
    for (const b of rectangles.slice(index + 1)) {
      expect(
        a.x + a.width <= b.x + 1 ||
          b.x + b.width <= a.x + 1 ||
          a.y + a.height <= b.y + 1 ||
          b.y + b.height <= a.y + 1,
      ).toBe(true);
    }
  }
  await card.screenshot({
    path: `artifacts/validation/screenshots/bento-treemap-${testInfo.project.name}.png`,
  });
});

test('dominant app keeps the 48/34/18 Bento layout and four readable icon squares', async ({
  page,
}, testInfo) => {
  await page.goto('/tests/browser/support/overview.html?scenario=memory_bento');
  await page.evaluate(() => document.fonts.ready);
  const card = page.getByRole('region', { name: 'Memory by App' });
  await expect(card.getByRole('listitem')).toHaveCount(8);
  await expect(card.locator('.treemap-block')).toHaveCount(7);
  await expect(card.locator('.treemap-compact')).toHaveCount(4);
  const grid = card.locator('.memory-app-bento');
  const columns = await grid.evaluate((el) =>
    [...el.children].map((child) => {
      const { x, y, width, height } = child.getBoundingClientRect();
      return { x, y, width, height };
    }),
  );
  expect(columns).toHaveLength(3);
  expect(columns[0]!.height).toBe(200);
  expect(columns[0]!.width / columns[1]!.width).toBeCloseTo(48 / 34, 1);
  expect(columns[1]!.width / columns[2]!.width).toBeCloseTo(34 / 18, 1);
  const middle = await card
    .locator('.memory-app-middle .treemap-block')
    .evaluateAll((tiles) => tiles.map((el) => el.getBoundingClientRect().height));
  expect(middle[0]! / middle[1]!).toBeCloseTo(55 / 45, 1);
  const small = card.locator('.treemap-compact');
  const squares = await small.evaluateAll((tiles) =>
    tiles.map((el) => {
      const { x, y, width, height } = el.getBoundingClientRect();
      const badge = el.querySelector('.treemap-badge')!.getBoundingClientRect();
      return {
        x,
        y,
        width,
        height,
        badgeX: badge.x,
        badgeY: badge.y,
        badgeWidth: badge.width,
        badgeHeight: badge.height,
      };
    }),
  );
  for (const [index, square] of squares.entries()) {
    expect(square.width).toBe(44);
    expect(square.height).toBe(44);
    expect(square.badgeWidth).toBe(28);
    expect(square.badgeHeight).toBe(28);
    expect(square.badgeX - square.x).toBe(8);
    expect(square.badgeY - square.y).toBe(8);
    if (index > 0) expect(square.y - squares[index - 1]!.y).toBe(52);
  }
  const gridBounds = await grid.boundingBox();
  const cardBounds = await card.boundingBox();
  for (const square of squares) {
    expect(square.y + square.height).toBeLessThanOrEqual(gridBounds!.y + gridBounds!.height);
    expect(square.y + square.height).toBeLessThanOrEqual(cardBounds!.y + cardBounds!.height);
  }
  for (const name of [
    'WSL2',
    'VS Code',
    'Chrome',
    'Ollama',
    'Docker',
    'Explorer',
    'OpenCode',
    'tiny-app',
  ]) {
    await expect(card.getByRole('listitem').filter({ hasText: name })).toHaveCount(1);
  }
  await expect(small.first()).toHaveAttribute('title', /Ollama · [\d.]+ MB/);
  await expect(card.locator('.card-note')).toHaveCount(0);
  await expect(card.locator('.treemap-block').first()).toHaveCSS(
    'background-color',
    'rgb(28, 28, 31)',
  );
  await expect(card.locator('.treemap-block').first()).toHaveCSS('border-radius', '12px');
  await expect(card.locator('.treemap-block').first()).toHaveCSS(
    'border-color',
    'rgba(255, 255, 255, 0.07)',
  );
  await expect(grid).toHaveCSS('gap', '8px');
  await expect(card.locator('.treemap-badge').first()).toHaveCSS('border-radius', '8px');
  const heading = await card.locator('.card-heading').boundingBox();
  const total = await card.locator('.memory-app-total').boundingBox();
  expect(total!.x + total!.width).toBeLessThanOrEqual(heading!.x + heading!.width + 1);
  await card.screenshot({
    path: `artifacts/validation/screenshots/memory-app-bento-${testInfo.project.name}.png`,
  });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('Memory by App remains contained at minimum width and short desktop heights', async ({
  page,
}) => {
  for (const viewport of [
    { width: 900, height: 640 },
    { width: 1200, height: 800 },
  ]) {
    await page.setViewportSize(viewport);
    await page.goto('/tests/browser/support/overview.html?scenario=memory_bento');
    const card = page.getByRole('region', { name: 'Memory by App' });
    const bounds = await card.boundingBox();
    const tiles = await card.locator('.treemap-block').evaluateAll((elements) =>
      elements.map((element) => {
        const { x, y, width, height } = element.getBoundingClientRect();
        return { x, y, width, height };
      }),
    );
    expect(tiles).toHaveLength(7);
    for (const tile of tiles) {
      expect(tile.x).toBeGreaterThanOrEqual(bounds!.x);
      expect(tile.y).toBeGreaterThanOrEqual(bounds!.y);
      expect(tile.x + tile.width).toBeLessThanOrEqual(bounds!.x + bounds!.width);
      expect(tile.y + tile.height).toBeLessThanOrEqual(bounds!.y + bounds!.height);
    }
    await expect(card.locator('.treemap-name')).toHaveCount(3);
    const labelsFit = await card.locator('.treemap-label').evaluateAll((elements) =>
      elements.every((element) => {
        const label = element.getBoundingClientRect();
        const badge = element.previousElementSibling!.getBoundingClientRect();
        return label.height > 0 && label.y >= badge.bottom;
      }),
    );
    expect(labelsFit).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
  }
});
