import { accessSync, constants } from 'node:fs';
import { delimiter, isAbsolute, join, resolve } from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';
import { createConnection, createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';

function executable(command: string): string {
  const suffixes = process.platform === 'win32' ? ['', '.exe', '.cmd'] : [''];
  const directories = isAbsolute(command) ? [''] : (process.env.PATH ?? '').split(delimiter);
  for (const directory of directories) {
    for (const suffix of suffixes) {
      const candidate = directory ? join(directory, `${command}${suffix}`) : `${command}${suffix}`;
      try {
        accessSync(candidate, constants.X_OK);
        return candidate;
      } catch {
        // Try the next PATH entry.
      }
    }
  }
  throw new Error(`Native test prerequisite missing: ${command}. See README.md.`);
}

export function nativePrerequisites() {
  if (process.platform !== 'linux' && process.platform !== 'win32') {
    throw new Error('Native tests require a real Windows or Linux target.');
  }
  if (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    throw new Error('Native Linux tests require a graphical session (DISPLAY or WAYLAND_DISPLAY).');
  }
  const binary = resolve(
    process.env.MEOWNDESK_BINARY ??
      `src-tauri/target/debug/meowndesk${process.platform === 'win32' ? '.exe' : ''}`,
  );
  try {
    accessSync(binary, constants.X_OK);
  } catch {
    throw new Error(
      `Native executable missing: ${binary}. Run npm run tauri build -- --debug --no-bundle.`,
    );
  }
  return {
    binary,
    driver: executable(process.env.TAURI_DRIVER ?? 'tauri-driver'),
    platformDriver: executable(
      process.env.NATIVE_WEBDRIVER ??
        (process.platform === 'win32' ? 'msedgedriver' : 'WebKitWebDriver'),
    ),
  };
}

export async function startDriver(): Promise<ChildProcess> {
  const { driver, platformDriver } = nativePrerequisites();
  // Refuse to attach to a server owned by another test run.
  await new Promise<void>((resolveFree, reject) => {
    const probe = createServer();
    probe.once('error', () => reject(new Error('Native test port 4444 is already in use.')));
    probe.listen(4444, '127.0.0.1', () => probe.close(() => resolveFree()));
  });
  const child = spawn(driver, ['--port', '4444', '--native-driver', platformDriver], {
    stdio: 'inherit',
  });
  let failure: Error | undefined;
  child.once('error', (error) => {
    failure = error;
  });
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    if (failure || child.exitCode !== null || child.signalCode !== null) {
      throw failure ?? new Error(`tauri-driver exited with code ${child.exitCode}.`);
    }
    const ready = await new Promise<boolean>((resolveReady) => {
      const socket = createConnection({ host: '127.0.0.1', port: 4444 });
      const finish = (result: boolean) => {
        socket.destroy();
        resolveReady(result);
      };
      socket.setTimeout(200);
      socket.once('connect', () => finish(true));
      socket.once('error', () => finish(false));
      socket.once('timeout', () => finish(false));
    });
    if (ready) return child;
    await delay(100);
  }
  await stopDriver(child);
  throw new Error('tauri-driver did not become ready within 10 seconds.');
}

export async function stopDriver(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise<void>((resolveExit) => {
    const timeout = setTimeout(() => child.kill('SIGKILL'), 3_000);
    child.once('exit', () => {
      clearTimeout(timeout);
      resolveExit();
    });
    child.kill('SIGTERM');
  });
}
