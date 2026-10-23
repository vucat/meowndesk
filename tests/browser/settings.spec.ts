import { expect, test } from '@playwright/test';

test('US4 Settings saves intervals, restores focus and keeps paused GPU changes disabled', async ({
  page,
}) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  const trigger = page.getByRole('button', { name: 'Settings' });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('radio', { name: '1 second' })).toBeChecked();
  await dialog.getByRole('radio', { name: '5 seconds' }).click();
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();

  await trigger.click();
  await expect(page.getByRole('dialog').getByRole('radio', { name: '5 seconds' })).toBeChecked();
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByRole('dialog')).not.toBeVisible();

  await page.getByRole('button', { name: 'Pause' }).click();
  await trigger.click();
  const pausedDialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(pausedDialog.getByRole('radio', { name: /NVIDIA/ })).toBeDisabled();
  await expect(pausedDialog.getByText(/Resume monitoring to change/i)).toBeVisible();
  await pausedDialog.getByRole('radio', { name: '2 seconds' }).click();
  await pausedDialog.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('button', { name: 'Resume' })).toBeVisible();
  await trigger.click();
  await expect(page.getByRole('dialog').getByRole('radio', { name: '2 seconds' })).toBeChecked();
});

test('US4 Settings reports no GPU without inventing a selector', async ({ page }) => {
  await page.goto('/tests/browser/support/overview.html?scenario=no_gpu');
  await page.getByRole('button', { name: 'Settings' }).click();
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByText(/No GPU was discovered/i)).toBeVisible();
  await expect(dialog.getByRole('radio', { name: /NVIDIA/ })).toHaveCount(0);
  await expect(dialog.getByRole('radio', { name: '1 second' })).toBeChecked();
});

test('Settings preserves keyboard focus while live telemetry advances', async ({ page }) => {
  await page.clock.install();
  await page.goto('/tests/browser/support/overview.html?scenario=full&advance=true');
  const trigger = page.getByRole('button', { name: 'Settings' });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await dialog.getByRole('radio', { name: '5 seconds' }).click();
  const save = dialog.getByRole('button', { name: 'Save' });
  await save.focus();
  const observed = await page.locator('.overview-footer').textContent();
  await page.clock.runFor(2100);
  await expect(page.locator('.overview-footer')).not.toHaveText(observed!);
  await expect(save).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('radio', { name: '5 seconds' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();
});
