/// <reference types="mocha" />
import type {} from 'webdriverio';
import type {} from 'expect-webdriverio';
import { browser, expect } from '@wdio/globals';

describe('native desktop shell', () => {
  it('renders the bundled shell inside the real platform webview', async () => {
    await expect(browser.$('h1')).toHaveText('Overview');
    await expect(browser.$('.brand')).toHaveText('Meowndesk');
    expect(
      await browser.$$('[role=region], section[aria-labelledby]').length,
    ).toBeGreaterThanOrEqual(9);
    await browser.waitUntil(
      () =>
        browser.execute(() => {
          const icon = document.querySelector<HTMLImageElement>('.app-mark');
          return Boolean(icon?.complete && icon.naturalWidth > 0);
        }),
      { timeoutMsg: 'The bundled icon did not load in the native webview.' },
    );
    await browser.waitUntil(
      () =>
        browser.execute(() =>
          Array.from(document.fonts).some(
            (font) => font.family === 'Inter' && font.status === 'loaded',
          ),
        ),
      { timeoutMsg: 'The bundled Inter font did not load in the native webview.' },
    );
    const background = await browser.$('body').getCSSProperty('background-color');
    expect(background.parsed.hex).toBe('#0b0b0c');
  });
});
