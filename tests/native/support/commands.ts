import { browser } from '@wdio/globals';
import { mkdirSync, writeFileSync } from 'node:fs';
import { platform, release } from 'node:os';
import { validateBootstrap } from '../../../src/telemetry/validate';
import type { CommandResult } from '../../../src/telemetry/types';

export async function command<T>(name: string, args: object = {}): Promise<T> {
  const raw: unknown = await browser.execute(
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
  );
  const result = raw as CommandResult<T>;
  if (!result.ok) throw new Error(result.error.message);
  return result.data;
}
export async function bootstrap() {
  await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('h1'))), {
    timeout: 10000,
    interval: 25,
  });
  return validateBootstrap(await command<unknown>('bootstrap'));
}
export function evidence(name: string, data: unknown) {
  mkdirSync('artifacts/validation', { recursive: true });
  writeFileSync(
    `artifacts/validation/phase7-${name}.json`,
    JSON.stringify(
      {
        at: new Date().toISOString(),
        platform: platform(),
        kernel: release(),
        classification: release().toLowerCase().includes('microsoft')
          ? 'WSL2 guest, provisional'
          : 'native OS, reference hardware not established',
        data,
      },
      null,
      2,
    ),
  );
}
export async function advancingSample(sequence: number, timeout: number) {
  let boot = await bootstrap();
  await browser.waitUntil(
    async () => {
      boot = await bootstrap();
      return (boot.view.sample?.sequence ?? 0) > sequence;
    },
    {
      timeout,
      interval: 50,
      timeoutMsg: 'SC-002: no advancing sample within two configured intervals',
    },
  );
  return boot;
}
