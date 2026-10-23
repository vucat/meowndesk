import { expect, test, type Page } from '@playwright/test';

async function feedbackMs(
  page: Page,
  selector: string,
  marker: string,
  kind: 'text' | 'mode',
): Promise<number> {
  return page.evaluate(
    async ({ selector, marker, kind }) => {
      const element = document.querySelector(selector);
      if (!(element instanceof HTMLElement)) return Infinity;
      const start = performance.now();
      element.click();
      for (let frame = 0; frame < 60; frame += 1) {
        await new Promise((resolve) => requestAnimationFrame(resolve));
        const mode = document.querySelector('#history-controls')?.getAttribute('data-mode');
        const ready =
          kind === 'mode' ? mode === marker : (document.body.textContent ?? '').includes(marker);
        if (ready) return performance.now() - start;
      }
      return Infinity;
    },
    { selector, marker, kind },
  );
}

function nearestRankP95(samples: number[]): number {
  const sorted = [...samples].sort((a, b) => a - b);
  return sorted[Math.ceil(0.95 * sorted.length) - 1]!;
}

test('US2 ranges and Pause are keyboard operable and label the frozen endpoint', async ({
  page,
}) => {
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  const group = page.getByRole('radiogroup', { name: 'Time range' });
  await expect(group).toBeVisible();
  await expect(group.getByRole('radio', { name: '1m', exact: true })).toBeChecked();
  await group.getByRole('radio', { name: '1m', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(group.getByRole('radio', { name: '5m', exact: true })).toBeChecked();
  await expect(page.locator('figure figcaption').first()).toContainText('5m ·');

  const pause = page.getByRole('button', { name: 'Pause' });
  await pause.click();
  const resume = page.getByRole('button', { name: 'Resume' });
  await expect(resume).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('history-status')).toContainText('Frozen at');
  await expect(page.locator('#history-controls')).toHaveAttribute('data-mode', 'paused');
  await expect(page.locator('figure figcaption').first()).toContainText(
    /gap columns|Partial history/,
  );

  const frozen = await page.getByTestId('history-status').textContent();
  await resume.click();
  await expect(page.getByRole('button', { name: 'Pause' })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await expect(page.getByTestId('history-status')).toHaveText('Live updates');
  expect(frozen).toMatch(/Frozen at/);
});

test('SC-002 range, Pause and Resume feedback p95 stays at or below 200 ms', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/tests/browser/support/overview.html?scenario=full');
  const ranges = ['#range-5m', '#range-15m', '#range-1h', '#range-1m'];
  const rangeSamples: number[] = [];
  for (let trial = 0; trial < 20; trial += 1) {
    const selector = ranges[trial % ranges.length]!;
    const marker = `${selector.replace('#range-', '')} ·`;
    rangeSamples.push(await feedbackMs(page, selector, marker, 'text'));
  }

  const resumeSamples: number[] = [];
  for (let trial = 0; trial < 20; trial += 1) {
    await page.locator('#history-pause').click();
    await expect(page.locator('#history-controls')).toHaveAttribute('data-mode', 'paused');
    resumeSamples.push(await feedbackMs(page, '#history-pause', 'live', 'mode'));
  }

  expect(rangeSamples.every(Number.isFinite)).toBe(true);
  expect(resumeSamples.every(Number.isFinite)).toBe(true);
  // The pause measurement targets the live->paused transition.
  const pausedSamples = await page.evaluate(async () => {
    const samples: number[] = [];
    const control = document.querySelector('#history-pause');
    if (!(control instanceof HTMLElement)) return samples;
    for (let trial = 0; trial < 20; trial += 1) {
      const start = performance.now();
      control.click();
      for (let frame = 0; frame < 60; frame += 1) {
        await new Promise((resolve) => requestAnimationFrame(resolve));
        if (document.querySelector('#history-controls')?.getAttribute('data-mode') === 'paused') {
          samples.push(performance.now() - start);
          break;
        }
      }
      control.click();
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
    return samples;
  });
  expect(pausedSamples).toHaveLength(20);

  const r = nearestRankP95(rangeSamples);
  const p = nearestRankP95(pausedSamples);
  const s = nearestRankP95(resumeSamples);
  console.log(
    JSON.stringify({
      sc002P95Ms: { range: r, pause: p, resume: s },
      trials: 20,
      samples: { range: rangeSamples, pause: pausedSamples, resume: resumeSamples },
    }),
  );
  expect(r).toBeLessThanOrEqual(200);
  expect(p).toBeLessThanOrEqual(200);
  expect(s).toBeLessThanOrEqual(200);
});
