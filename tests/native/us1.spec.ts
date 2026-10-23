/// <reference types="mocha" />
import { browser, expect } from '@wdio/globals';
import { mkdirSync, readFileSync, readdirSync, statfsSync, writeFileSync } from 'node:fs';
import { platform, release } from 'node:os';
import { validateBootstrap } from '../../src/telemetry/validate';
import type { Bootstrap, CommandResult, View } from '../../src/telemetry/types';

const evidenceName = process.env.MEOWNDESK_NATIVE_EVIDENCE ?? 'us1';

async function command<T>(name: string, args = {}): Promise<T> {
  const result = (await browser.execute(
    async (commandName, commandArgs) => {
      const api = (
        window as unknown as {
          __TAURI_INTERNALS__: { invoke(name: string, args: object): Promise<unknown> };
        }
      ).__TAURI_INTERNALS__;
      return api.invoke(commandName, commandArgs);
    },
    name,
    args,
  )) as CommandResult<T>;
  if (!result.ok) throw new Error(result.error.message);
  return result.data;
}
async function bootstrap() {
  return validateBootstrap(await command<Bootstrap>('bootstrap'));
}

function reference() {
  const cpu = readFileSync('/proc/stat', 'utf8')
    .split('\n')[0]!
    .trim()
    .split(/\s+/)
    .slice(1, 9)
    .map(Number);
  const interfaces = readdirSync('/sys/class/net').filter((name) => {
    const flags = parseInt(readFileSync(`/sys/class/net/${name}/flags`, 'utf8').trim(), 16);
    return (flags & 1) !== 0 && (flags & 8) === 0;
  });
  const total = (key: string) =>
    interfaces.reduce(
      (n, name) =>
        n + BigInt(readFileSync(`/sys/class/net/${name}/statistics/${key}`, 'utf8').trim()),
      0n,
    );
  return {
    atMs: performance.now(),
    cpuTotal: cpu.reduce((a, b) => a + b, 0),
    idle: cpu[3]! + cpu[4]!,
    interfaces,
    received: total('rx_bytes'),
    sent: total('tx_bytes'),
  };
}

describe('US1 actual desktop collector', () => {
  it('advances coherent real samples, keeps collecting while delivery is hidden, and restores latest', async () => {
    let boot = await bootstrap();
    await browser.waitUntil(
      async () => {
        boot = await bootstrap();
        return (boot.view.sample?.sequence ?? 0) >= 3;
      },
      { timeout: 15000 },
    );
    const first = boot.view;
    const sample = first.sample!;
    expect(sample.memory.totalBytes.status).toBe('valid');
    expect(sample.cpu.totalPct.status).toBe('valid');
    // Opt-in hardware assertion for validation hosts with a verified NVIDIA adapter.
    // Other native hosts retain the capability-dependent baseline contract.
    if (process.env.MEOWNDESK_EXPECT_NVIDIA === '1') {
      expect(boot.gpus.some((gpu) => gpu.vendor === 'nvidia' && gpu.available)).toBe(true);
      for (const field of [
        'utilizationPct',
        'vramTotalBytes',
        'vramUsedBytes',
        'temperatureC',
        'powerW',
      ] as const) {
        expect(sample.gpu[field].status).toBe('valid');
        expect(sample.gpu[field].value).not.toBeNull();
      }
      expect(sample.power.primaryKind).toBe('gpu_watts');
      expect(sample.power.watts.status).toBe('valid');
    }

    expect(first.history?.endpointElapsedMs).toBe(sample.elapsedMs);
    expect(first.history?.charts).toHaveLength(6);
    expect(sample.host.environment).toBe(
      release().toLowerCase().includes('microsoft') ? 'wsl2' : 'native',
    );
    if (sample.host.environment === 'wsl2')
      await expect(browser.$('.guest-label')).toHaveText('WSL2 guest · Guest resources only');
    const hidden = (await command<{ view: View }>('set_delivery_visibility', { visible: false }))
      .view;
    expect(hidden.visible).toBe(false);
    await browser.waitUntil(
      async () => ((await bootstrap()).view.sample?.sequence ?? 0) >= sample.sequence + 3,
      { timeout: 10000 },
    );
    const restored = (await command<{ view: View }>('set_delivery_visibility', { visible: true }))
      .view;
    expect(restored.visible).toBe(true);
    expect(restored.sample!.sequence).toBeGreaterThan(sample.sequence);
    await browser.waitUntil(
      async () => (await browser.$('main').getText()).includes('Interface traffic'),
      { timeout: 10000 },
    );
    await browser.waitUntil(
      async () =>
        !(await browser.$('.status-notices').getText()).includes('Live updates interrupted'),
      { timeout: 5000, timeoutMsg: 'Visible delivery did not recover after restore' },
    );
    const comparisons: Record<string, unknown> = {};
    if (platform() === 'linux') {
      const mem = readFileSync('/proc/meminfo', 'utf8');
      const total = BigInt(/^MemTotal:\s+(\d+)/m.exec(mem)?.[1] ?? '0') * 1024n;
      const available = BigInt(/^MemAvailable:\s+(\d+)/m.exec(mem)?.[1] ?? '0') * 1024n;
      const fs = statfsSync('/', { bigint: true });
      comparisons.ram = {
        scope: 'same guest /proc/meminfo',
        observedTotalBytes: sample.memory.totalBytes.value,
        referenceTotalBytes: String(total),
        observedUsedBytes: restored.sample!.memory.usedBytes.value,
        nearbyReferenceUsedBytes: String(total - available),
        note: 'Live used values vary between asynchronous reads.',
      };
      expect(sample.memory.totalBytes.value).toBe(String(total));
      comparisons.volume = {
        scope: 'same guest /',
        status: sample.disk.totalBytes.status,
        observedTotalBytes: sample.disk.totalBytes.value,
        referenceTotalBytes: String(fs.bsize * fs.blocks),
      };
      if (sample.disk.totalBytes.status === 'valid')
        expect(sample.disk.totalBytes.value).toBe(String(fs.bsize * fs.blocks));
    }
    if (platform() === 'linux') {
      const observations = [];
      const comparisonStart = (await bootstrap()).view.sample!;
      let previous = reference();
      const referenceStart = previous;
      let sequence = comparisonStart.sequence;
      let observedElapsed = comparisonStart.elapsedMs;
      let comparisonEnd = comparisonStart;
      for (let i = 0; i < 10; i++) {
        let current = restored;
        await browser.waitUntil(
          async () => {
            current = (await bootstrap()).view;
            return current.sample!.sequence > sequence;
          },
          { timeout: 3000, interval: 100 },
        );
        sequence = current.sample!.sequence;
        const next = reference();
        const seconds = (next.atMs - previous.atMs) / 1000;
        observations.push({
          referenceWindowMs: next.atMs - previous.atMs,
          observedWindowMs: current.sample!.elapsedMs - observedElapsed,
          sampleSequence: sequence,
          elapsedMs: current.sample!.elapsedMs,
          cpuObservedPct: current.sample!.cpu.totalPct.value,
          cpuReferencePct:
            100 * (1 - (next.idle - previous.idle) / (next.cpuTotal - previous.cpuTotal)),
          receiveObservedBytesPerSecond: current.sample!.network.receiveBytesPerSecond.value,
          receiveReferenceBytesPerSecond: Number(next.received - previous.received) / seconds,
          sendObservedBytesPerSecond: current.sample!.network.sendBytesPerSecond.value,
          sendReferenceBytesPerSecond: Number(next.sent - previous.sent) / seconds,
          interfaces: next.interfaces,
        });
        observedElapsed = current.sample!.elapsedMs;
        comparisonEnd = current.sample!;
        previous = next;
      }
      comparisons.trends = {
        scope: 'same guest whole CPU and OS-up non-loopback interfaces',
        observations,
        cumulative: {
          observedReceivedBytes: String(
            BigInt(comparisonEnd.network.receivedSessionBytes.value!) -
              BigInt(comparisonStart.network.receivedSessionBytes.value!),
          ),
          referenceReceivedBytes: String(previous.received - referenceStart.received),
          observedSentBytes: String(
            BigInt(comparisonEnd.network.sentSessionBytes.value!) -
              BigInt(comparisonStart.network.sentSessionBytes.value!),
          ),
          referenceSentBytes: String(previous.sent - referenceStart.sent),
        },
        note: 'Ten nearby 1Hz samples; asynchronous read boundaries, debug guest evidence only.',
      };
    }
    mkdirSync('artifacts/validation', { recursive: true });
    writeFileSync(
      `artifacts/validation/${evidenceName}-native-observation.json`,
      JSON.stringify(
        {
          at: new Date().toISOString(),
          platform: platform(),
          release: release(),
          classification:
            sample.host.environment === 'wsl2'
              ? 'WSL2 guest best effort; not native Windows/Linux acceptance'
              : 'native host observation',
          first,
          restored,
          comparisons,
        },
        null,
        2,
      ),
    );
    mkdirSync('artifacts/validation/screenshots', { recursive: true });
    for (const [width, height] of [
      [1440, 1000],
      [1024, 768],
    ] as const) {
      await browser.setWindowSize(width, height);
      await browser.saveScreenshot(
        `artifacts/validation/screenshots/${evidenceName}-guest-${width}x${height}.png`,
      );
      expect(
        await browser.execute(() => document.documentElement.scrollWidth <= window.innerWidth),
      ).toBe(true);
    }
  });
});
