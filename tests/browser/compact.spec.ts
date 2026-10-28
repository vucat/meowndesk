import { expect, test } from '@playwright/test';
import { MockTransport } from '../../src/test/mock-transport';
import { formatBytes, formatPercent } from '../../src/telemetry/format';
import { sharePercent } from '../../src/telemetry/display';

const coreCards = ['CPU', 'Memory', 'GPU & VRAM', 'Disk', 'Network', 'Power/Thermals'];

test('compact display mode unmounts charts and swaps in the Top Processes strip', async ({
  page,
}, testInfo) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  await page.evaluate(() => document.fonts.ready);
  const mode = page.getByRole('radiogroup', { name: 'Display mode' });
  await expect(mode.getByRole('radio', { name: 'Charts' })).toBeChecked();
  await expect(page.locator('.dot-chart canvas')).toHaveCount(6);
  await expect(page.locator('.bento-card')).toHaveCount(9);

  await mode.getByRole('radio', { name: 'Charts' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(mode.getByRole('radio', { name: 'Compact' })).toBeChecked();

  await expect(page.getByRole('radiogroup', { name: 'Time range' })).toHaveCount(0);
  await expect(page.locator('.dot-chart canvas')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Pause', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Top Processes' })).toBeVisible();
  for (const name of ['Memory by Type', 'Memory by App', 'Resource Load'])
    await expect(page.getByRole('region', { name })).toHaveCount(0);

  for (const name of coreCards) {
    const card = page.getByRole('region', { name, exact: true });
    await expect(card).toHaveClass(/bento-card-compact/);
    await expect(card.locator('.micro-meter')).toHaveCount(1);
    await expect(card.locator('.secondary-stat')).toHaveCount(3);
    const meter = card.locator('.micro-meter');
    expect((await meter.boundingBox())!.height).toBe(4);
    await expect(card.locator('.secondary-stats')).toHaveCSS('border-top-width', '1px');
    await expect(card.locator('.secondary-stats')).toHaveCSS('border-top-color', 'rgb(30, 30, 35)');
    await expect(card.locator('.secondary-stat').first()).toHaveCSS('border-top-width', '0px');
  }
  const strip = page.getByRole('region', { name: 'Top Processes' });
  await expect(strip.locator('.top-process-item')).toHaveCount(2);
  await expect(strip.getByText('Sorted by memory')).toBeVisible();
  await expect(strip).toContainText(/Combined Memory & CPU Load/);
  await expect(strip.locator('.top-process-name')).toHaveCount(2);
  await expect(strip.locator('.top-process-track').first()).toHaveCSS('height', '4px');
  const columns = await strip
    .locator('.top-processes-strip')
    .evaluate((grid) => getComputedStyle(grid).gridTemplateColumns.split(' ').length);
  expect(columns).toBe(2);

  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  // Sub-pixel rounding may add a pixel; no real overflow is allowed at either viewport.
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight - innerHeight),
  ).toBeLessThan(1);
  if (testInfo.project.name === 'chromium-reference') {
    const contained = await page.locator('.bento-card').evaluateAll((cards) => {
      const bottom = innerHeight;
      return cards.every((card) => card.getBoundingClientRect().bottom <= bottom + 1);
    });
    expect(contained).toBe(true);
  }
  await page.screenshot({
    path: `artifacts/validation/screenshots/compact-${testInfo.project.name}.png`,
    fullPage: false,
  });

  await mode.getByRole('radio', { name: 'Charts' }).click();
  await expect(page.locator('.bento-card')).toHaveCount(9);
  await expect(page.locator('.dot-chart canvas')).toHaveCount(6);
  await expect(page.getByRole('radiogroup', { name: 'Time range' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Top Processes' })).toHaveCount(0);
});

test('compact display mode keeps zero-width meters and unavailable values honest', async ({
  page,
}) => {
  await page.goto('/tests/browser/support/overview.html?scenario=no_gpu');
  const mode = page.getByRole('radiogroup', { name: 'Display mode' });
  await mode.getByRole('radio', { name: 'Compact' }).click();
  const card = page.getByRole('region', { name: 'GPU & VRAM', exact: true });
  await expect(card.locator('.micro-meter')).toHaveAttribute('data-fill', '0');
  await expect(card).toContainText(/unsupported|unavailable|No GPU/i);
  await expect(page.getByRole('region', { name: 'CPU', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('Top Processes caps the strip at five app columns with RAM and share', async ({ page }) => {
  await page.goto('/tests/browser/support/overview.html?scenario=memory_bento');
  await page.getByRole('radio', { name: 'Compact' }).click();
  const strip = page.getByRole('region', { name: 'Top Processes' });
  await expect(strip.locator('.top-process-item')).toHaveCount(5);
  const columns = await strip
    .locator('.top-processes-strip')
    .evaluate((grid) => getComputedStyle(grid).gridTemplateColumns.split(' ').length);
  expect(columns).toBe(5);
  await expect(strip.locator('.top-process-name').first()).toHaveText('WSL2');
  await expect(strip.locator('.top-process-ram').first()).toHaveText(/GB/);
  await expect(strip.locator('.top-process-share').first()).toHaveText(/%/);
  const summaries = new MockTransport('memory_bento').currentView.processSummaries!;
  const total = BigInt(summaries.displayedMemoryTotalBytes);
  await expect(strip).toContainText('Memory list total');
  await expect(strip.locator('.top-processes-total')).toHaveText(formatBytes(total));
  await expect(strip).not.toContainText('across 5 active apps');
  for (const [index, entry] of summaries.memoryTop.slice(0, 5).entries()) {
    const share = sharePercent(BigInt(entry.residentBytes.value!), total)!;
    const row = strip.locator('.top-process-item').nth(index);
    await expect(row.locator('.top-process-share')).toContainText(formatPercent(share));
    expect(await row.locator('.top-process-bar').evaluate((bar) => bar.style.width)).toBe(
      `${share}%`,
    );
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('CPU fallback keeps unreadable resident totals and shares unavailable', async ({ page }) => {
  await page.goto('/tests/browser/support/overview.html?scenario=process_memory_unavailable');
  await page.getByRole('radio', { name: 'Compact' }).click();
  const strip = page.getByRole('region', { name: 'Top Processes' });
  await expect(strip.getByText('Sorted by CPU')).toBeVisible();
  await expect(strip.locator('.top-processes-total .metric-number')).toHaveText('—');
  await expect(strip.locator('.top-processes-total')).toContainText('Unavailable');
  const rows = strip.locator('.top-process-item');
  expect(await rows.count()).toBeGreaterThan(0);
  for (const row of await rows.all()) {
    await expect(row.locator('.top-process-ram')).toContainText('—');
    await expect(row.locator('.top-process-share')).toContainText('—');
    await expect(row.locator('.top-process-cpu')).toContainText('%');
    expect(await row.locator('.top-process-bar').evaluate((bar) => bar.style.width)).toBe('0%');
  }
});

test('Compact retained values are marked last-valid during silence and recover on a new sample', async ({
  page,
}, testInfo) => {
  await page.clock.install();
  await page.goto(
    '/tests/browser/support/overview.html?scenario=full&advance=true&resumeAfterMs=4000',
  );
  await page.getByRole('radio', { name: 'Compact' }).click();
  const strip = page.getByRole('region', { name: 'Top Processes' });
  const values = strip.locator('.metric-number');
  const retained = await values.allTextContents();
  await page.clock.runFor(2001);
  await expect(page.getByText('Live updates interrupted')).toBeVisible();
  await expect(strip.getByText('· Last valid process readings')).toBeVisible();
  expect(await values.allTextContents()).toEqual(retained);
  for (const selector of [
    '.top-processes-total',
    '.top-process-cpu',
    '.top-process-ram',
    '.top-process-share',
  ]) {
    for (const value of await strip.locator(selector).all()) {
      await expect(value).toContainText('Last valid');
      await expect(value.locator('time')).toHaveAttribute('datetime', /T/);
      await expect(value.locator('.metric-number')).toHaveCSS('opacity', '0.8');
    }
  }
  for (const meter of await page.locator('.micro-meter').all())
    await expect(meter).toHaveAttribute('data-fill', '0');
  for (const bar of await strip.locator('.top-process-bar').all())
    expect(await bar.evaluate((element) => element.style.width)).toBe('0%');
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight - innerHeight),
  ).toBeLessThan(1);
  await page.screenshot({
    path: `artifacts/validation/screenshots/compact-interrupted-${testInfo.project.name}.png`,
    fullPage: true,
  });
  await page.clock.runFor(3000);
  await expect(page.getByText('Live updates interrupted')).toHaveCount(0);
  await expect(strip.locator('.metric-stale')).toHaveCount(0);
  expect(await values.allTextContents()).toEqual(retained);
  expect(
    parseFloat(
      await strip
        .locator('.top-process-bar')
        .first()
        .evaluate((element) => element.style.width),
    ),
  ).toBeGreaterThan(0);
});
