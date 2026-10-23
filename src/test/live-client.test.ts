import { expect, it, vi } from 'vitest';
import { TelemetryClient, type TelemetryTransport } from '../telemetry/client';
import { ViewStore } from '../app/view-store';
import { MockTransport } from './mock-transport';
import type { Bootstrap, CommandResult, Delivery, View } from '../telemetry/types';
function driver() {
  const transport = new MockTransport('full');
  let receive: ((delivery: Delivery) => void) | undefined;
  const subscribe = transport.subscribe.bind(transport);
  transport.subscribe = (callback) => {
    receive = callback;
    return subscribe(callback);
  };
  let now = 0;
  const store = new ViewStore(() => now);
  const client = new TelemetryClient(transport, store);
  const ack = vi.spyOn(transport, 'ack');
  return {
    client,
    store,
    transport,
    ack,
    time: (value: number) => {
      now = value;
    },
    send: (delivery: Delivery) => receive!(delivery),
  };
}
function delivery(view: View, sequence: number): Delivery {
  return {
    contractVersion: 1,
    sessionId: view.sessionId,
    subscriptionId: '6b5c4d3e-2f1a-4b0c-9d8e-7f6a5b4c3d2e',
    deliverySequence: sequence,
    view: structuredClone(view),
  };
}
it('bootstraps before subscribing and ACKs even a rejected revision or sample rollback', async () => {
  const d = driver();
  await d.client.start();
  try {
    expect(d.ack).toHaveBeenCalledWith(0, expect.any(String));
    const accepted = d.store.getSnapshot().view!;
    expect(Object.isFrozen(accepted.sample?.cpu.totalPct)).toBe(true);
    const old = delivery(accepted, 2);
    old.view.revision -= 1;
    d.send(old);
    expect(d.store.getSnapshot().view).toBe(accepted);
    expect(d.ack).toHaveBeenCalledWith(2, expect.any(String));
    const rollback = delivery(accepted, 3);
    rollback.view.revision += 1;
    rollback.view.sample!.sequence -= 1;
    d.send(rollback);
    expect(d.store.getSnapshot().view).toBe(accepted);
    expect(d.ack).toHaveBeenCalledWith(3, expect.any(String));
  } finally {
    d.client.stop();
  }
});
it('same-sample control revisions cannot refresh stale telemetry, advancing recovery can', async () => {
  const d = driver();
  await d.client.start();
  try {
    d.time(2001);
    d.store.setDocumentVisibility(true);
    expect(d.store.getSnapshot().interrupted).toBe(true);
    const same = delivery(d.store.getSnapshot().view!, 2);
    same.view.revision++;
    d.send(same);
    expect(d.store.getSnapshot().interrupted).toBe(true);
    const next = delivery(d.store.getSnapshot().view!, 3);
    next.view.sample!.sequence++;
    d.send(next);
    expect(d.store.getSnapshot().interrupted).toBe(false);
  } finally {
    d.client.stop();
  }
});
it('version mismatch stops subscription without coercing the payload', async () => {
  const transport = new MockTransport('full');
  const bootstrap = transport.bootstrap();
  if (!bootstrap.ok) throw new Error('fixture must bootstrap');
  bootstrap.data.view.contractVersion = 2;
  transport.bootstrap = () => bootstrap;
  const subscribe = vi.spyOn(transport, 'subscribe');
  const client = new TelemetryClient(transport);
  await client.start();
  try {
    expect(subscribe).not.toHaveBeenCalled();
    expect(client.store.getSnapshot().error).toMatch(/Incompatible app version/);
  } finally {
    client.stop();
  }
});
it('malformed deliveries and retired subscription identities cannot overwrite accepted data', async () => {
  const d = driver();
  await d.client.start();
  try {
    const accepted = d.store.getSnapshot().view!;
    const wrong = delivery(accepted, 2);
    wrong.subscriptionId = '00000000-0000-4000-8000-000000000000';
    wrong.view.sample!.sequence++;
    d.send(wrong);
    expect(d.store.getSnapshot().view).toBe(accepted);
    d.send(null as unknown as Delivery);
    expect(d.store.getSnapshot().view).toBe(accepted);
    expect(d.store.getSnapshot().error).toMatch(/could not be validated/);
  } finally {
    d.client.stop();
  }
});

it('curates unexpected transport failures', async () => {
  const transport = new MockTransport('full');
  transport.bootstrap = () => {
    throw new Error('/private/provider/path raw failure');
  };
  const client = new TelemetryClient(transport);
  await client.start();
  expect(client.store.getSnapshot().error).toBe('Monitoring could not start');
  client.stop();
});
it('reconnects a failed startup credit only on a later visibility action', async () => {
  const transport = new MockTransport('full');
  const subscribe = vi.spyOn(transport, 'subscribe');
  vi.spyOn(transport, 'ack').mockReturnValueOnce({ ok: true, data: { accepted: false } });
  const client = new TelemetryClient(transport);
  try {
    await client.start();
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(client.store.getSnapshot().error).toMatch(/subscription could not start/);
    await client.setVisible(true);
    expect(subscribe).toHaveBeenCalledTimes(2);
    expect(client.store.getSnapshot().error).toBeNull();
  } finally {
    client.stop();
  }
});

it.each(['range', 'memory', 'ranking', 'pause', 'preferences'] as const)(
  'recovers failed startup credit on a later %s action and receives advancing data',
  async (action) => {
    const d = driver();
    const subscribe = vi.spyOn(d.transport, 'subscribe');
    d.ack.mockReturnValueOnce({ ok: true, data: { accepted: false } });
    try {
      await d.client.start();
      expect(subscribe).toHaveBeenCalledTimes(1);
      if (action === 'pause') await d.client.setPaused(true);
      else if (action === 'preferences')
        await d.client.setPreferences(2000, d.transport.currentView.preferences.selectedGpuId);
      else {
        const selection = d.transport.currentView.selection;
        await d.client.setView({
          ...selection,
          ...(action === 'range' ? { range: '5m' } : {}),
          ...(action === 'memory' ? { memoryView: 'vram' } : {}),
          ...(action === 'ranking' ? { rankingMetric: 'gpu' } : {}),
        });
      }
      expect(subscribe).toHaveBeenCalledTimes(2);
      if (action === 'pause') {
        const frozen = d.store.getSnapshot().view!.sample!.sequence;
        d.transport.tick();
        expect(d.store.getSnapshot().view!.sample!.sequence).toBe(frozen);
        await d.client.setPaused(false);
      }
      const before = d.store.getSnapshot().view!.sample!.sequence;
      d.transport.tick();
      expect(d.store.getSnapshot().view!.sample!.sequence).toBeGreaterThan(before);
      expect(d.store.getSnapshot().error).toBeNull();
    } finally {
      d.client.stop();
    }
  },
);

it('recovers a later ACK failure on Resume without retrying in the background', async () => {
  const d = driver();
  const subscribe = vi.spyOn(d.transport, 'subscribe');
  try {
    await d.client.start();
    d.ack.mockReturnValueOnce({
      ok: false,
      error: { code: 'invalid_subscription', message: 'The channel closed', retryable: true },
    });
    d.transport.tick();
    await Promise.resolve();
    expect(subscribe).toHaveBeenCalledTimes(1);
    await d.client.setPaused(false);
    expect(subscribe).toHaveBeenCalledTimes(2);
    const before = d.store.getSnapshot().view!.sample!.sequence;
    d.transport.tick();
    expect(d.store.getSnapshot().view!.sample!.sequence).toBeGreaterThan(before);
  } finally {
    d.client.stop();
  }
});

it.each(['overlapping-controls', 'retired-generation'] as const)(
  'keeps subscription recovery safe for %s',
  async (scenario) => {
    const d = driver();
    const subscribe = vi.spyOn(d.transport, 'subscribe');
    const transport: TelemetryTransport = d.transport;
    d.ack.mockReturnValueOnce({ ok: true, data: { accepted: false } });
    try {
      await d.client.start();
      let finish: (result: CommandResult<{ removed: boolean }>) => void = () => {};
      vi.spyOn(transport, 'unsubscribe').mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      const range = d.client.setView({ ...d.transport.currentView.selection, range: '5m' });
      await Promise.resolve();
      const other = scenario === 'overlapping-controls' ? d.client.setPaused(false) : undefined;
      if (scenario === 'retired-generation') {
        d.client.stop();
        await d.client.start();
      }
      finish({ ok: true, data: { removed: true } });
      await Promise.all([range, other]);
      expect(subscribe).toHaveBeenCalledTimes(2);
      const before = d.store.getSnapshot().view!.sample!.sequence;
      d.transport.tick();
      expect(d.store.getSnapshot().view!.sample!.sequence).toBeGreaterThan(before);
    } finally {
      d.client.stop();
    }
  },
);

it('accepts a validated GPU refresh in the current session', async () => {
  const d = driver();
  try {
    await d.client.start();
    const response = d.transport.bootstrap();
    if (!response.ok) throw new Error('fixture must bootstrap');
    response.data.gpus[0]!.name = 'Updated adapter label';
    d.transport.bootstrap = () => response;
    await d.client.refreshGpuChoices();
    expect(d.client.gpuChoices()[0]?.name).toBe('Updated adapter label');
  } finally {
    d.client.stop();
  }
});

it('keeps the last validated GPU catalog when refresh contains malformed descriptors', async () => {
  const d = driver();
  try {
    await d.client.start();
    const previous = d.client.gpuChoices();
    const response = d.transport.bootstrap();
    if (!response.ok) throw new Error('fixture must bootstrap');
    Reflect.deleteProperty(response.data.gpus[0]!, 'vendor');
    d.transport.bootstrap = () => response;
    await d.client.refreshGpuChoices();
    expect(d.client.gpuChoices()).toEqual(previous);
  } finally {
    d.client.stop();
  }
});

it('stops incompatible refreshes without replacing the validated GPU catalog', async () => {
  const d = driver();
  const unsubscribe = vi.spyOn(d.transport, 'unsubscribe');
  try {
    await d.client.start();
    const previous = d.client.gpuChoices();
    const response = d.transport.bootstrap();
    if (!response.ok) throw new Error('fixture must bootstrap');
    response.data.view.contractVersion = 2;
    response.data.gpus[0]!.name = 'Retired catalog';
    d.transport.bootstrap = () => response;
    await d.client.refreshGpuChoices();
    expect(d.client.gpuChoices()).toEqual(previous);
    expect(d.store.getSnapshot().error).toMatch(/Incompatible app version/);
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  } finally {
    d.client.stop();
  }
});

it('does not let a delayed control response clear an incompatible-version failure', async () => {
  const d = driver();
  const transport: TelemetryTransport = d.transport;
  try {
    await d.client.start();
    const response = d.transport.setView({ ...d.transport.currentView.selection, range: '5m' });
    let finish: (result: CommandResult<{ view: View }>) => void = () => {};
    vi.spyOn(transport, 'setView').mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = d.client.setView({ ...d.transport.currentView.selection, range: '5m' });
    const boot = d.transport.bootstrap();
    if (!boot.ok) throw new Error('fixture must bootstrap');
    boot.data.view.contractVersion = 2;
    d.transport.bootstrap = () => boot;
    await d.client.refreshGpuChoices();
    finish(response);
    await pending;
    expect(d.store.getSnapshot().error).toMatch(/Incompatible app version/);
  } finally {
    d.client.stop();
  }
});

it.each(['stopped', 'restarted', 'older-refresh', 'newer-view', 'different-session'] as const)(
  'ignores a %s GPU refresh response',
  async (retired) => {
    const d = driver();
    try {
      await d.client.start();
      const previous = structuredClone(d.client.gpuChoices());
      const response = d.transport.bootstrap();
      if (!response.ok) throw new Error('fixture must bootstrap');
      response.data.gpus[0]!.name = 'Retired catalog';
      let finish: (result: CommandResult<Bootstrap>) => void = () => {};
      const transport: TelemetryTransport = d.transport;
      vi.spyOn(transport, 'bootstrap').mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      const pending = d.client.refreshGpuChoices();
      if (retired === 'stopped') d.client.stop();
      if (retired === 'restarted') {
        d.client.stop();
        await d.client.start();
      }
      if (retired === 'older-refresh') await d.client.refreshGpuChoices();
      if (retired === 'newer-view')
        await d.client.setView({ ...d.transport.currentView.selection, range: '5m' });
      if (retired === 'different-session') {
        response.data.view.sessionId = '00000000-0000-4000-8000-000000000000';
        response.data.view.sample!.host.sessionId = response.data.view.sessionId;
      }
      finish(response);
      await pending;
      expect(d.client.gpuChoices()).toEqual(previous);
    } finally {
      d.client.stop();
    }
  },
);

it('retires a pending subscription when refresh detects an incompatible version', async () => {
  const d = driver();
  const transport: TelemetryTransport = d.transport;
  const response = d.transport.subscribe(() => {});
  let finish: (result: typeof response) => void = () => {};
  vi.spyOn(transport, 'subscribe').mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const unsubscribe = vi.spyOn(d.transport, 'unsubscribe');
  try {
    const pending = d.client.start();
    await vi.waitFor(() => expect(transport.subscribe).toHaveBeenCalledTimes(1));
    const boot = d.transport.bootstrap();
    if (!boot.ok) throw new Error('fixture must bootstrap');
    boot.data.view.contractVersion = 2;
    d.transport.bootstrap = () => boot;
    await d.client.refreshGpuChoices();
    finish(response);
    await pending;
    expect(d.ack).not.toHaveBeenCalled();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
    expect(d.store.getSnapshot().error).toMatch(/Incompatible app version/);
  } finally {
    d.client.stop();
  }
});
