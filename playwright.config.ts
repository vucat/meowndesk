import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  reporter: [
    ['list'],
    ['html', { outputFolder: 'artifacts/validation/browser-report', open: 'never' }],
  ],
  outputDir: 'artifacts/validation/browser-results',
  use: {
    baseURL: 'http://127.0.0.1:1421',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium-reference',
      use: { browserName: 'chromium', viewport: { width: 1440, height: 1000 } },
    },
    {
      name: 'chromium-small',
      use: { browserName: 'chromium', viewport: { width: 1024, height: 768 } },
    },
  ],
  webServer: {
    command: 'node --import tsx tests/browser/support/fixture-server.ts',
    url: 'http://127.0.0.1:1421',
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
