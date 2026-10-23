/// <reference types="mocha" />
import { browser, expect } from '@wdio/globals';
import type { View } from '../../src/telemetry/types';
import { advancingSample, bootstrap, command, evidence } from './support/commands';

describe('Phase 7 native lifecycle recovery', () => {
  it('keeps paused values frozen through range changes and delivery hide, then resumes current data', async () => {
    let boot = await bootstrap();
    boot = await advancingSample(
      boot.view.sample?.sequence ?? 0,
      2 * boot.view.preferences.intervalMs,
    );
    await browser.$('#history-pause').click();
    const frozen = (await bootstrap()).view;
    expect(frozen.mode).toBe('paused');
    const { view: hidden } = await command<{ view: View }>('set_delivery_visibility', {
      visible: false,
    });
    expect(hidden.visible).toBe(false);
    await browser.waitUntil(
      async () => (await bootstrap()).view.endpointElapsedMs === frozen.endpointElapsedMs,
      { timeout: 1000 },
    );
    // Wait externally while the actual collector advances; no fake clock or fixture command.
    await new Promise((resolve) => setTimeout(resolve, 2 * boot.view.preferences.intervalMs + 100));
    const { view: restored } = await command<{ view: View }>('set_delivery_visibility', {
      visible: true,
    });
    expect(restored.sample).toEqual(frozen.sample);
    expect(restored.endpointElapsedMs).toBe(frozen.endpointElapsedMs);
    await browser.$('#range-1h').click();
    const ranged = (await bootstrap()).view;
    expect(ranged.endpointElapsedMs).toBe(frozen.endpointElapsedMs);
    expect(ranged.sample).toEqual(frozen.sample);
    const start = Date.now();
    await browser.$('#history-pause').click();
    const resumed = await advancingSample(
      frozen.sample!.sequence,
      2 * boot.view.preferences.intervalMs,
    );
    await browser.waitUntil(async () => (await browser.$('.live-state').getText()) === 'Live', {
      timeout: 2 * boot.view.preferences.intervalMs,
    });
    const resumeMs = Date.now() - start;
    expect(resumed.view.sample!.sequence).toBeGreaterThan(frozen.sample!.sequence);
    expect(resumed.view.endpointElapsedMs).toBeGreaterThan(frozen.endpointElapsedMs);
    evidence('recovery', {
      frozenSequence: frozen.sample!.sequence,
      resumedSequence: resumed.view.sample!.sequence,
      resumeMs,
      limitMs: 2 * boot.view.preferences.intervalMs,
      deliveryHide: 'passed; explicit IPC visibility, native minimize tested separately',
      suspend: 'pending real OS suspend/resume',
      processExit: 'pending native process churn; Rust system_processes tests separate',
    });
    expect(resumeMs).toBeLessThanOrEqual(2 * boot.view.preferences.intervalMs);
  });

  it('minimizes/restores the real window in live and paused modes when the platform driver supports it', async () => {
    const results: object[] = [];
    for (const paused of [false, true]) {
      if ((await bootstrap()).view.mode !== (paused ? 'paused' : 'live'))
        await browser.$('#history-pause').click();
      const before = (await bootstrap()).view;
      try {
        await browser.minimizeWindow();
      } catch (error) {
        if (!/unsupported|unknown command|not implemented/i.test(String(error))) throw error;
        evidence('minimize', { status: 'pending', reason: String(error), completed: results });
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 2 * before.preferences.intervalMs + 100));
      const minimized = (await bootstrap()).view;
      expect(minimized.visible).toBe(false);
      await browser.maximizeWindow();
      await browser.setWindowSize(1440, 1000);
      const after = (await bootstrap()).view;
      expect(after.visible).toBe(true);
      if (paused) {
        expect(after.sample).toEqual(before.sample);
        expect(after.endpointElapsedMs).toBe(before.endpointElapsedMs);
      } else expect(after.sample!.sequence).toBeGreaterThan(before.sample!.sequence);
      results.push({
        paused,
        beforeSequence: before.sample?.sequence,
        afterSequence: after.sample?.sequence,
      });
    }
    if ((await bootstrap()).view.mode === 'paused') await browser.$('#history-pause').click();
    evidence('minimize', { status: 'passed', results });
  });

  it('persists interval/GPU preferences across a real application restart', async () => {
    const original = (await bootstrap()).view.preferences;
    try {
      const changed = original.intervalMs === 2000 ? 1000 : 2000;
      const saved = await command<{ persisted: boolean }>('set_preferences', {
        intervalMs: changed,
        selectedGpuId: original.selectedGpuId,
      });
      expect(saved.persisted).toBe(true);
      await browser.reloadSession();
      const restarted = await advancingSample(0, 2 * changed);
      expect(restarted.view.preferences).toEqual({
        intervalMs: changed,
        selectedGpuId: original.selectedGpuId,
      });
      evidence('restart', {
        status: 'passed',
        preferences: restarted.view.preferences,
        multiGpu: 'pending real second GPU and removal/fallback',
      });
    } finally {
      await command('set_preferences', original);
    }
  });
});
