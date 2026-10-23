import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { nativePrerequisites } from './support/driver';

try {
  nativePrerequisites();
  const child = spawn(
    process.execPath,
    [resolve('node_modules/@wdio/cli/bin/wdio.js'), 'run', 'wdio.conf.ts'],
    { stdio: 'inherit' },
  );
  child.on('error', (error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
  child.on('exit', (code) => {
    process.exitCode = code ?? 1;
  });
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
