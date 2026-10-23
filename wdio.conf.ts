import type {} from 'webdriverio';
import type { ChildProcess } from 'node:child_process';
import { nativePrerequisites, startDriver, stopDriver } from './tests/native/support/driver';

let driver: ChildProcess | undefined;

export const config: WebdriverIO.Config = {
  runner: 'local',
  hostname: '127.0.0.1',
  port: 4444,
  path: '/',
  specs: ['./tests/native/**/*.spec.ts'],
  maxInstances: 1,
  capabilities: [
    {
      'tauri:options': { application: nativePrerequisites().binary },
      'wdio:enforceWebDriverClassic': true,
    },
  ],
  afterTest: async (_test, _context, result) => {
    if (!result.passed) {
      const { browser } = await import('@wdio/globals');
      console.error(
        'Native page diagnostic',
        await browser.execute(() => ({
          url: location.href,
          title: document.title,
          body: document.documentElement.outerHTML.slice(0, 12000),
        })),
      );
    }
  },
  beforeSession: () => {
    process.env.MEOWNDESK_SESSION_START_MS = String(Date.now());
  },
  before: async () => {
    const { browser } = await import('@wdio/globals');
    await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('h1'))), {
      timeout: 10000,
      interval: 25,
    });
  },
  framework: 'mocha',
  reporters: ['spec'],
  logLevel: 'warn',
  connectionRetryCount: 0,
  waitforTimeout: 10_000,
  mochaOpts: { timeout: 60_000 },
  outputDir: './artifacts/validation/native-results',
  onPrepare: async () => {
    driver = await startDriver();
  },
  onComplete: async () => {
    if (driver) await stopDriver(driver);
  },
};
