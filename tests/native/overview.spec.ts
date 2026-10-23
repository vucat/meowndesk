/// <reference types="mocha" />
import { browser, expect } from '@wdio/globals';
import { mkdirSync } from 'node:fs';
import { advancingSample, bootstrap, evidence } from './support/commands';

describe('Phase 7 native Overview acceptance', () => {
  it('records first advancing/visible sample latency, baseline and native screenshots', async () => {
    const initial = await bootstrap();
    // Discovery may still be warming up; latency is recorded against the first actual sample.

    const start = Number(process.env.MEOWNDESK_SESSION_START_MS ?? Date.now());
    const interval = initial.view.preferences.intervalMs;
    const observed = initial.view.sample ? initial : await advancingSample(0, 2 * interval);
    await browser.waitUntil(
      async () => (await browser.$('.overview-footer').getText()).includes('Observed'),
      { timeout: 2 * interval, interval: 50 },
    );
    const launchToVisibleMs = Date.now() - start;
    let warmed = observed;
    await browser.waitUntil(
      async () => {
        warmed = await bootstrap();
        return (warmed.view.sample?.sequence ?? 0) >= 3;
      },
      { timeout: 3 * interval, interval: 50 },
    );
    const sample = warmed.view.sample!;
    const required = [
      sample.cpu.totalPct,
      sample.memory.usedBytes,
      sample.memory.totalBytes,
      sample.disk.freeBytes,
      sample.disk.totalBytes,
      sample.network.receiveBytesPerSecond,
      sample.network.sendBytesPerSecond,
    ];
    const baseline =
      required.every((reading) => reading.status === 'valid') &&
      (warmed.view.processSummaries?.cpuTop.length ?? 0) > 0 &&
      (warmed.view.processSummaries?.memoryTop.length ?? 0) > 0;
    evidence('overview', {
      launchToVisibleMs,
      limitMs: 2 * interval,
      sc002: launchToVisibleMs <= 2 * interval ? 'passed' : 'failed',
      baseline,
      firstSequence: observed.view.sample?.sequence,
      observed: warmed,
    });
    await expect(browser.$$('section.bento-card')).toBeElementsArrayOfSize(9);
    if (sample.host.environment === 'wsl2')
      await expect(browser.$('.guest-label')).toHaveText('WSL2 guest · Guest resources only');
    if (sample.host.environment === 'native') expect(baseline).toBe(true);
    mkdirSync('artifacts/validation/screenshots', { recursive: true });
    for (const [width, height] of [
      [1440, 1000],
      [1024, 768],
    ] as const) {
      await browser.setWindowSize(width, height);
      let viewport = await browser.execute(() => ({
        width: innerWidth,
        height: innerHeight,
        dpr: devicePixelRatio,
      }));
      if (viewport.width !== width || viewport.height !== height) {
        await browser.setWindowSize(
          width + width - viewport.width,
          height + height - viewport.height,
        );
        viewport = await browser.execute(() => ({
          width: innerWidth,
          height: innerHeight,
          dpr: devicePixelRatio,
        }));
      }
      expect(viewport.width).toBe(width);
      expect(viewport.height).toBe(height);
      evidence(`native-viewport-${width}x${height}`, viewport);
      await browser.saveScreenshot(
        `artifacts/validation/screenshots/phase7-native-${width}x${height}.png`,
      );
      expect(await browser.execute(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      );
    }
    expect(launchToVisibleMs).toBeLessThanOrEqual(2 * interval);
  });
});
