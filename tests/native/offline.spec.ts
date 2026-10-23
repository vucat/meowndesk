/// <reference types="mocha" />
import { browser, expect } from '@wdio/globals';
import { advancingSample, bootstrap, evidence } from './support/commands';

describe('production local-only webview', () => {
  it('loads local assets, rejects remote fetch through production CSP and keeps updating', async () => {
    const boot = await bootstrap();
    const audit = await browser.execute(async () => {
      const resources = performance.getEntriesByType('resource').map((entry) => entry.name);
      let blocked = false;
      const violations: { directive: string; blockedUri: string }[] = [];
      const onViolation = (event: SecurityPolicyViolationEvent) =>
        violations.push({ directive: event.effectiveDirective, blockedUri: event.blockedURI });
      document.addEventListener('securitypolicyviolation', onViolation);
      try {
        await fetch('https://example.invalid/meowndesk-offline-probe');
      } catch {
        blocked = true;
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
      document.removeEventListener('securitypolicyviolation', onViolation);
      return {
        violations,
        resources,
        blocked,
        csp: document
          .querySelector('meta[http-equiv="Content-Security-Policy"]')
          ?.getAttribute('content'),
        online: navigator.onLine,
      };
    });
    expect(audit.blocked).toBe(true);
    expect(
      audit.violations.some(
        (event) =>
          event.directive === 'connect-src' &&
          event.blockedUri.startsWith('https://example.invalid'),
      ),
    ).toBe(true);
    for (const resource of audit.resources) {
      const url = new URL(resource);
      expect(
        ['tauri:', 'asset:', 'ipc:'].includes(url.protocol) ||
          ['tauri.localhost', 'asset.localhost', 'ipc.localhost'].includes(url.hostname),
      ).toBe(true);
    }
    await advancingSample(boot.view.sample?.sequence ?? 0, 2 * boot.view.preferences.intervalMs);
    const forbidden: Record<string, boolean> = {};
    for (const name of ['set_fixture', 'kill_process', 'read_file', 'plugin:shell|execute']) {
      forbidden[name] = await browser.execute(async (commandName) => {
        try {
          const api = (
            window as unknown as { __TAURI_INTERNALS__: { invoke(name: string): Promise<unknown> } }
          ).__TAURI_INTERNALS__;
          await api.invoke(commandName);
          return false;
        } catch {
          return true;
        }
      }, name);
      expect(forbidden[name]).toBe(true);
    }
    evidence('offline', {
      audit,
      forbidden,
      networkIsolation: 'CSP enforced; OS-level network disconnect/firewall capture pending',
      fixtures: 'production handler rejects fixture commands',
    });
  });
});
