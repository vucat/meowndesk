import { describe, expect, it } from 'vitest';
import { validateBootstrap, validateView } from '../telemetry/validate';
import type { Delivery } from '../telemetry/types';
import { MockTransport, SCENARIOS } from './mock-transport';

function collect(transport: MockTransport): Delivery[] {
  const deliveries: Delivery[] = [];
  const subscribed = transport.subscribe((delivery) => {
    validateView(delivery.view);
    deliveries.push(delivery);
  });
  expect(subscribed.ok).toBe(true);
  return deliveries;
}

describe('deterministic mock transport', () => {
  it('starts every scenario from a valid view and keeps ticking valid views', () => {
    for (const scenario of SCENARIOS) {
      const transport = new MockTransport(scenario.id);
      const bootstrap = transport.bootstrap();
      expect(bootstrap.ok, scenario.id).toBe(true);
      if (!bootstrap.ok) continue;
      validateBootstrap(bootstrap.data);
      transport.tick();
      transport.tick();
      expect(() => validateView(transport.currentView), scenario.id).not.toThrow();
    }
  });

  it('sends one in-flight delivery and replaces a pending view', () => {
    const transport = new MockTransport('full');
    const deliveries = collect(transport);

    expect(transport.ack(0)).toEqual({ ok: true, data: { accepted: true } });
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0]?.deliverySequence).toBe(1);

    transport.tick();
    transport.tick();
    expect(deliveries).toHaveLength(1);

    expect(transport.ack(1)).toEqual({ ok: true, data: { accepted: true } });
    expect(deliveries).toHaveLength(2);
    expect(deliveries[1]?.view.sample?.sequence).toBe(65);

    transport.tick();
    transport.tick();
    expect(transport.ack(2).ok).toBe(true);
    expect(deliveries).toHaveLength(3);
    expect(deliveries[2]?.view.sample?.sequence).toBe(67);

    expect(transport.ack(2)).toEqual({ ok: true, data: { accepted: false } });
    expect(transport.ack(0)).toEqual({ ok: true, data: { accepted: false } });
  });

  it('suppresses delivery while paused and resumes without replaying missed views', () => {
    const transport = new MockTransport('full');
    const deliveries = collect(transport);
    transport.ack(0);
    transport.tick();
    transport.ack(1);

    expect(transport.setPaused(true).ok).toBe(true);
    transport.tick();
    transport.tick();
    expect(deliveries).toHaveLength(2);

    expect(transport.setPaused(false).ok).toBe(true);
    expect(transport.ack(2).ok).toBe(true);
    expect(deliveries).toHaveLength(2);
    transport.tick();
    expect(deliveries).toHaveLength(3);
    expect(deliveries[2]?.view.mode).toBe('live');
  });

  it('represents missing GPU, denied CPU, stale memory and an empty interface set explicitly', () => {
    const noGpu = new MockTransport('no_gpu');
    const noGpuView = noGpu.currentView;
    expect(noGpuView.preferences.selectedGpuId).toBeNull();
    expect(noGpuView.sample?.gpu.utilizationPct.status).toBe('unsupported');
    expect(noGpuView.memoryBreakdowns.vram).toBeNull();

    const denied = new MockTransport('denied').currentView;
    expect(denied.sample?.cpu.totalPct.status).toBe('permission_denied');
    expect(denied.sample?.memory.usedBytes.status).toBe('valid');

    const stale = new MockTransport('stale').currentView;
    expect(stale.sample?.memory.usedPct).toMatchObject({ status: 'stale', value: 35 });

    const empty = new MockTransport('no_active_interface').currentView;
    expect(empty.sample?.network.receiveBytesPerSecond).toMatchObject({
      status: 'valid',
      value: 0,
    });
    expect(empty.sample?.network.receivedSessionBytes.value).toBe('0');
  });

  it('marks rates unavailable after a counter reset', () => {
    const transport = new MockTransport('reset');
    transport.tick();
    expect(transport.currentView.sample?.network.receiveBytesPerSecond.status).toBe('valid');
    transport.tick();
    const reset = transport.currentView;
    expect(reset.sample?.network.receiveBytesPerSecond.status).toBe('unavailable');
    expect(reset.sample?.network.sendBytesPerSecond.reason).toMatch(/reset/i);
    validateView(reset);
  });

  it('switches GPU scope atomically and keeps old-derived values unavailable', () => {
    const transport = new MockTransport('gpu_change');
    transport.tick();
    transport.tick();
    const view = transport.currentView;
    const gpuScope = view.sample?.scopes.find((scope) => scope.kind === 'gpu');
    expect(gpuScope?.id).toBe('gpu:GPU-fixture-2');
    expect(gpuScope?.epoch).toBe(5);
    expect(view.preferences.selectedGpuId).toBe('GPU-fixture-2');
    expect(view.sample?.gpu.utilizationPct).toMatchObject({
      status: 'unavailable',
      scopeId: 'gpu:GPU-fixture-2',
    });
    expect(view.memoryBreakdowns.vram).toBeNull();
    expect(view.processSummaries?.gpuTop).toEqual([]);
    validateView(view);

    const rejected = transport.setView({
      range: '1m',
      memoryView: 'ram',
      rankingMetric: 'gpu',
    });
    expect(rejected.ok).toBe(false);
    if (!rejected.ok) expect(rejected.error.code).toBe('unsupported_ranking');
  });

  it('keeps churned process lists bounded and coherent', () => {
    const transport = new MockTransport('process_churn');
    transport.tick();
    transport.tick();
    const summaries = transport.currentView.processSummaries;
    expect(summaries?.memoryTop.length).toBeLessThanOrEqual(8);
    expect(summaries?.memoryTop[0]?.name).toBe('new-workload');
    expect(summaries?.memoryTop[0]?.key).toMatch(/^7777:\d+$/);
    validateView(transport.currentView);
  });

  it('requires Resume before changing the GPU while paused', () => {
    const transport = new MockTransport('full');
    transport.setPaused(true);
    const rejected = transport.setPreferences(1000, '0000:0e:00.0');
    expect(rejected.ok).toBe(false);
    if (!rejected.ok) expect(rejected.error.code).toBe('resume_required');
    expect(transport.setPaused(false).ok).toBe(true);
    expect(transport.setPreferences(2000, '0000:0e:00.0').ok).toBe(true);
  });
});
