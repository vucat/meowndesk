import { expect, test } from '@playwright/test';
import { MockTransport } from '../../src/test/mock-transport';
import { expectedSummaries } from '../../src/test/overview-expectations';
const cards = [
  'CPU',
  'Memory',
  'GPU & VRAM',
  'Disk',
  'Network',
  'Power/Thermals',
  'Memory by Type',
  'Memory by App',
  'Resource Load',
];
for (const scenario of [
  'full',
  'no_gpu',
  'denied',
  'stale',
  'no_active_interface',
  'wsl2',
  'initializing',
]) {
  test(`US1 Overview ${scenario}`, async ({ page }) => {
    await page.goto(`/tests/browser/support/overview.html?scenario=${scenario}`);
    for (const name of cards)
      await expect(page.getByRole('region', { name, exact: true })).toBeVisible();
    if (scenario === 'initializing') await expect(page.getByText(/Initializing/i)).toBeVisible();
    const expectations = expectedSummaries(new MockTransport(scenario).currentView);
    for (const [resource, expected] of Object.entries(expectations)) {
      await expect(page.getByTestId(`${resource}-summary`)).toContainText(expected);
    }
    await expect(page.getByRole('button', { name: /kill|terminate|stop process/i })).toHaveCount(0);
    await page.keyboard.press('Tab');
    await expect(page.locator(':focus')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    if (scenario === 'wsl2') await expect(page.getByText(/WSL2.*guest|guest.*WSL2/i)).toBeVisible();
    if (scenario === 'denied') await expect(page.getByText(/Degraded monitoring/i)).toBeVisible();
    if (scenario === 'no_active_interface')
      await expect(page.getByText(/No active interfaces/i)).toBeVisible();
  });
}
test('stopped collection shows a freshness warning without synthesizing samples', async ({
  page,
}) => {
  await page.clock.install();
  await page.goto('/tests/browser/support/overview.html?scenario=full&silence=true');
  await page.clock.fastForward(2001);
  await expect(page.getByText('Live updates interrupted')).toBeVisible();
});
