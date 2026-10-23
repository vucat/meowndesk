import type {} from '@wdio/types';

declare global {
  namespace WebdriverIO {
    interface Capabilities {
      'tauri:options'?: { application: string };
    }
  }
}
