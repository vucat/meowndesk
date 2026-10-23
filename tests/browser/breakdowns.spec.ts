import { expect, test } from '@playwright/test';

test('US3 breakdowns switch denominators, rank metrics and keep textual alternatives', async ({
  page,
}) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  const memoryCard = page.getByRole('region', { name: 'Memory by Type' });
  const memorySelector = memoryCard.getByRole('radiogroup', { name: 'Memory type' });
  await expect(memoryCard.getByRole('img', { name: /System RAM.*in use/i })).toBeVisible();
  await expect(memoryCard.getByText('In Use', { exact: true })).toBeVisible();
  await memorySelector.getByRole('radio', { name: 'Dedicated VRAM' }).click();
  await expect(memoryCard.getByRole('img', { name: /Dedicated VRAM.*in use/i })).toBeVisible();
  await expect(memoryCard.getByText('Unclassified', { exact: true })).toBeVisible();
  await expect(memoryCard.getByText(/Partial coverage/)).toBeVisible();

  const appCard = page.getByRole('region', { name: 'Memory by App' });
  const appItems = appCard.getByRole('listitem');
  await expect(appItems).toHaveCount(2);
  const columns = await appCard.locator('.treemap-block').evaluateAll((blocks) =>
    blocks.map((block) => {
      const { width, height } = block.getBoundingClientRect();
      return { width, height };
    }),
  );
  expect(columns).toHaveLength(2);
  expect(columns[0]!.width / columns[1]!.width).toBeCloseTo(48 / 34, 1);
  expect(columns[0]!.height).toBe(columns[1]!.height);
  await expect(appCard.locator('.card-heading .memory-app-total')).toContainText('in all');
  await expect(appCard.getByTitle(/Shared pages/)).toBeVisible();
  await expect(appCard.locator('.sr-only').filter({ hasText: /Shared pages/ })).toHaveCount(1);

  const loadCard = page.getByRole('region', { name: 'Resource Load' });
  const ranking = loadCard.getByRole('radiogroup', { name: 'Ranking metric' });
  await expect(ranking.getByRole('radio', { name: 'CPU' })).toBeChecked();
  await expect(ranking.getByRole('radio', { name: 'Power' })).toBeDisabled();
  await expect(loadCard.getByText(/per-process power is unavailable/i)).toBeVisible();
  await ranking.getByRole('radio', { name: 'GPU' }).click();
  await expect(loadCard.getByRole('listitem')).toHaveCount(1);
  await expect(loadCard.getByText(/Selected GPU|NVIDIA/)).toBeVisible();
  await expect(page.getByRole('button', { name: /kill|terminate|stop process/i })).toHaveCount(0);
});

test('US3 unavailable VRAM and unsupported GPU ranking stay explicit', async ({ page }) => {
  await page.goto('/tests/browser/support/overview.html?scenario=no_gpu');
  const memoryCard = page.getByRole('region', { name: 'Memory by Type' });
  await memoryCard.getByRole('radio', { name: 'Dedicated VRAM' }).click();
  await expect(memoryCard.getByText(/Dedicated VRAM is unavailable/i)).toBeVisible();
  const loadCard = page.getByRole('region', { name: 'Resource Load' });
  const gpu = loadCard.getByRole('radio', { name: 'GPU' });
  await expect(gpu).toBeDisabled();
  await expect(loadCard.getByRole('radio', { name: 'CPU' })).toBeChecked();
});
