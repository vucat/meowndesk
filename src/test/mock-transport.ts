/**
 * Browser-only mock transport for deterministic UI tests. It reuses the shared version-1 fixture and
 * is never imported by a production entry point.
 */

import contractFixture from '../../tests/fixtures/contract-v1.json' with { type: 'json' };
import scenarioFixture from '../../tests/fixtures/scenarios.json' with { type: 'json' };
import type {
  Bootstrap,
  Chart,
  CommandError,
  CommandResult,
  CurrentSample,
  Delivery,
  GpuDescriptor,
  ProcessObservation,
  Reading,
  View,
  ViewSelection,
} from '../telemetry/types';
import { validateBootstrap } from '../telemetry/validate';

export interface Scenario {
  id: string;
  description: string;
  intervalMs: number;
  gpu: 'available' | 'none';
  network: 'active' | 'empty';
  cpu: 'valid' | 'denied';
  memory: 'valid' | 'stale';
  silentAfterTicks: number | null;
  counterResetAfterTicks: number | null;
  gpuChangeAfterTicks: number | null;
  processChurnAfterTicks: number | null;
}

export const SCENARIOS: readonly Scenario[] = (
  scenarioFixture as unknown as { scenarios: Scenario[] }
).scenarios;

const SESSION_ID = '0f7a1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b';
const SUBSCRIPTION_ID = '6b5c4d3e-2f1a-4b0c-9d8e-7f6a5b4c3d2e';
const INSPECTION_ID = '9d8c7b6a-1234-4fed-8abc-0123456789ab';
const NEW_GPU_SCOPE = 'gpu:GPU-fixture-2';
const NEW_GPU_NAME = 'Fixture Second GPU';

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

export class FakeClock {
  elapsedMs: number;
  wallMs: number;

  constructor(elapsedMs = 63000, wallMs = 1761091199000) {
    this.elapsedMs = elapsedMs;
    this.wallMs = wallMs;
  }

  advance(milliseconds: number): void {
    this.elapsedMs += milliseconds;
    this.wallMs += milliseconds;
  }
}

function failure(code: CommandError['code'], message: string): CommandResult<never> {
  return { ok: false, error: { code, message, retryable: false } };
}

function unavailable<T>(scopeId: string | null, reason: string): Reading<T> {
  return {
    status: 'unavailable',
    value: null,
    sampledAtMs: null,
    elapsedMs: null,
    scopeId,
    reason,
  };
}

function unsupported<T>(scopeId: string | null, reason: string): Reading<T> {
  return {
    status: 'unsupported',
    value: null,
    sampledAtMs: null,
    elapsedMs: null,
    scopeId,
    reason,
  };
}

function emptyChart(chart: Chart, scopeId: string | null, scopeLabel: string | null): Chart {
  return {
    ...chart,
    scopeId,
    scopeLabel,
    status: 'unavailable',
    partialHistory: true,
    columns: Array.from({ length: 60 }, () => ({ value: null, sampleCount: 0, hasGap: true })),
    ...(chart.unit === 'pct' ? { scaleMax: 100 } : { scaleMax: 1 }),
  };
}

function sampleReadings(sample: CurrentSample): Reading<unknown>[] {
  const groups = [sample.cpu, sample.memory, sample.gpu, sample.disk, sample.network, sample.power];
  return groups.flatMap((group) =>
    Object.values(group).filter(
      (entry): entry is Reading<unknown> =>
        typeof entry === 'object' && entry !== null && 'status' in entry && 'value' in entry,
    ),
  );
}

function refreshValidTimestamps(sample: CurrentSample): void {
  for (const reading of sampleReadings(sample)) {
    if (reading.status === 'valid') {
      reading.sampledAtMs = sample.capturedAtMs;
      reading.elapsedMs = sample.elapsedMs;
    }
  }
}

function chartValue(chart: Chart, sample: CurrentSample): number | null {
  switch (chart.seriesId) {
    case 'cpuPct':
      return sample.cpu.totalPct.status === 'valid' ? sample.cpu.totalPct.value : null;
    case 'ramUsedPct':
      return sample.memory.usedPct.status === 'valid' ? sample.memory.usedPct.value : null;
    case 'gpuPct':
      return sample.gpu.utilizationPct.status === 'valid' ? sample.gpu.utilizationPct.value : null;
    case 'diskBytesPerSecond': {
      const read = sample.disk.readBytesPerSecond;
      const write = sample.disk.writeBytesPerSecond;
      return read.status === 'valid' &&
        write.status === 'valid' &&
        read.value !== null &&
        write.value !== null
        ? read.value + write.value
        : null;
    }
    case 'networkBytesPerSecond': {
      const receive = sample.network.receiveBytesPerSecond;
      const send = sample.network.sendBytesPerSecond;
      return receive.status === 'valid' &&
        send.status === 'valid' &&
        receive.value !== null &&
        send.value !== null
        ? receive.value + send.value
        : null;
    }
    case 'powerW':
      return sample.power.watts.status === 'valid' ? sample.power.watts.value : null;
  }
}

export class MockTransport {
  readonly clock: FakeClock;
  readonly scenario: Scenario;

  private view: View;
  private frozen: View | null = null;
  private gpus: GpuDescriptor[];
  private subscriber: ((delivery: Delivery) => void) | null = null;
  private credits = false;
  private inFlight: number | null = null;
  private pending: View | null = null;
  private nextSequence = 1;
  private ticks = 0;

  constructor(scenarioId: string, clock = new FakeClock()) {
    const scenario = SCENARIOS.find((entry) => entry.id === scenarioId);
    if (!scenario) throw new Error(`unknown mock scenario: ${scenarioId}`);
    this.scenario = scenario;
    this.clock = clock;

    const source = clone((contractFixture as unknown as { bootstrap: Bootstrap }).bootstrap);
    this.view = source.view;
    this.gpus = source.gpus;
    this.view.sessionId = SESSION_ID;
    this.view.sample!.host.sessionId = SESSION_ID;
    this.view.preferences.intervalMs = scenario.intervalMs;
    this.applyInitialScenario();
    validateBootstrap({ view: this.view, gpus: this.gpus });
  }

  get currentView(): View {
    return clone(this.view);
  }

  private active(): View {
    return this.frozen ?? this.view;
  }

  get tickCount(): number {
    return this.ticks;
  }

  bootstrap(): CommandResult<Bootstrap> {
    return { ok: true, data: { view: clone(this.view), gpus: clone(this.gpus) } };
  }

  subscribe(onView: (delivery: Delivery) => void): CommandResult<{
    subscriptionId: string;
    sessionId: string;
  }> {
    this.subscriber = onView;
    this.credits = false;
    this.inFlight = null;
    this.pending = null;
    return { ok: true, data: { subscriptionId: SUBSCRIPTION_ID, sessionId: SESSION_ID } };
  }

  unsubscribe(): CommandResult<{ removed: boolean }> {
    const removed = this.subscriber !== null;
    this.subscriber = null;
    this.credits = false;
    this.inFlight = null;
    this.pending = null;
    return { ok: true, data: { removed } };
  }

  /** ACK 0 starts the channel; later ACKs must match the current in-flight sequence. */
  ack(deliverySequence: number): CommandResult<{ accepted: boolean }> {
    if (!this.subscriber) return { ok: true, data: { accepted: false } };
    if (deliverySequence === 0 && this.inFlight === null && !this.credits) {
      this.credits = true;
      this.pending = clone(this.view);
      this.flush();
      return { ok: true, data: { accepted: true } };
    }
    if (deliverySequence !== this.inFlight) return { ok: true, data: { accepted: false } };
    this.inFlight = null;
    this.credits = true;
    this.flush();
    return { ok: true, data: { accepted: true } };
  }

  setView(selection: ViewSelection): CommandResult<{ view: View }> {
    const supported = this.active().processSummaries?.supportedRankingMetrics ?? ['cpu'];
    if (!supported.includes(selection.rankingMetric)) {
      return failure(
        'unsupported_ranking',
        `The ${selection.rankingMetric} ranking is unavailable for the active source`,
      );
    }
    const changed =
      this.view.selection.range !== selection.range ||
      this.view.selection.memoryView !== selection.memoryView ||
      this.view.selection.rankingMetric !== selection.rankingMetric;
    this.view.selection = clone(selection);
    if (this.view.history) this.view.history.range = selection.range;
    if (this.frozen) {
      this.frozen.selection = clone(selection);
      if (this.frozen.history) this.frozen.history.range = selection.range;
    }
    if (changed) this.bumpRevision();
    return { ok: true, data: { view: clone(this.active()) } };
  }

  setPaused(paused: boolean): CommandResult<{ view: View }> {
    const target = paused ? 'paused' : 'live';
    if (this.view.mode !== target) {
      this.view.mode = target;
      this.view.inspectionId = paused ? INSPECTION_ID : null;
      this.frozen = paused ? clone(this.view) : null;
      if (paused) this.pending = null;
      this.bumpRevision();
    }
    return { ok: true, data: { view: clone(this.active()) } };
  }

  setVisible(visible: boolean): CommandResult<{ view: View }> {
    if (this.view.visible !== visible) {
      this.view.visible = visible;
      if (this.frozen) this.frozen.visible = visible;
      if (!visible) this.pending = null;
      else if (this.view.mode === 'live' && this.subscriber && this.inFlight === null) {
        this.credits = true;
      }
      this.bumpRevision();
    }
    return { ok: true, data: { view: clone(this.active()) } };
  }

  setPreferences(
    intervalMs: number,
    selectedGpuId: string | null,
  ): CommandResult<{ view: View; persisted: boolean }> {
    if (![1000, 2000, 5000].includes(intervalMs)) {
      return failure('invalid_argument', 'intervalMs must be 1000, 2000 or 5000');
    }
    if (selectedGpuId !== null && !this.gpus.some((gpu) => gpu.id === selectedGpuId)) {
      return failure('unknown_gpu', 'The selected GPU is no longer discovered');
    }
    if (this.view.mode === 'paused' && selectedGpuId !== this.view.preferences.selectedGpuId) {
      return failure('resume_required', 'Resume before changing the monitored GPU');
    }
    this.view.preferences.intervalMs = intervalMs;
    this.view.preferences.selectedGpuId = selectedGpuId;
    this.bumpRevision();
    return { ok: true, data: { view: clone(this.view), persisted: true } };
  }

  /** Advances one collection interval; exact silence is simulated after `silentAfterTicks`. */
  tick(): void {
    this.ticks += 1;
    this.clock.advance(this.scenario.intervalMs);

    if (this.scenario.silentAfterTicks !== null && this.ticks > this.scenario.silentAfterTicks) {
      return;
    }

    const sample = this.view.sample;
    if (!sample) return;
    sample.sequence += 1;
    sample.elapsedMs += this.scenario.intervalMs;
    sample.capturedAtMs += this.scenario.intervalMs;
    sample.intervalMs = this.scenario.intervalMs;
    if (sample.cpu.totalPct.status === 'valid' && sample.cpu.totalPct.value !== null) {
      sample.cpu.totalPct.value = 40 + ((this.ticks * 3) % 30);
      sample.cpu.totalPct.sampledAtMs = sample.capturedAtMs;
      sample.cpu.totalPct.elapsedMs = sample.elapsedMs;
    }
    refreshValidTimestamps(sample);

    if (this.scenario.counterResetAfterTicks === this.ticks) {
      this.view.sample!.network.receiveBytesPerSecond = unavailable(
        this.view.sample!.network.receiveBytesPerSecond.scopeId,
        'Interface counters reset; re-establishing the baseline',
      );
      this.view.sample!.network.sendBytesPerSecond = unavailable(
        this.view.sample!.network.sendBytesPerSecond.scopeId,
        'Interface counters reset; re-establishing the baseline',
      );
    }
    if (this.scenario.processChurnAfterTicks === this.ticks) this.applyProcessChurn();
    if (this.scenario.gpuChangeAfterTicks === this.ticks) this.applyGpuChange();

    this.rotateHistory();
    this.view.endpointElapsedMs = sample.elapsedMs;
    this.view.endpointAtMs = sample.capturedAtMs;
    this.view.history!.endpointElapsedMs = sample.elapsedMs;

    if (this.view.mode === 'live' && this.view.visible) {
      if (this.credits) this.send(clone(this.view));
      else this.pending = clone(this.view);
    }
  }

  private bumpRevision(): void {
    this.view.revision += 1;
    if (this.frozen) this.frozen.revision = this.view.revision;
    this.view.endpointElapsedMs = this.view.sample?.elapsedMs ?? this.view.endpointElapsedMs;
    this.view.endpointAtMs = this.view.sample?.capturedAtMs ?? this.view.endpointAtMs;
  }

  private flush(): void {
    if (!this.credits || !this.pending || this.view.mode !== 'live' || !this.view.visible) return;
    const next = this.pending;
    this.pending = null;
    this.send(next);
  }

  private send(view: View): void {
    if (!this.subscriber) return;
    const delivery: Delivery = {
      contractVersion: 1,
      sessionId: SESSION_ID,
      subscriptionId: SUBSCRIPTION_ID,
      deliverySequence: this.nextSequence,
      view,
    };
    this.nextSequence += 1;
    this.inFlight = delivery.deliverySequence;
    this.credits = false;
    this.subscriber(delivery);
  }

  private applyInitialScenario(): void {
    const sample = this.view.sample!;
    if (this.scenario.id === 'wsl2') {
      sample.host.environment = 'wsl2';
      sample.host.scopeLabel = 'WSL2 guest';
    }
    if (this.scenario.id === 'memory_bento') {
      const summaries = this.view.processSummaries!;
      const source = summaries.memoryTop[0]!;
      const apps = [
        ['WSL2', '9900000000'],
        ['VS Code', '1250000000'],
        ['Chrome', '900000000'],
        ['Ollama', '50000000'],
        ['Docker', '40000000'],
        ['Explorer', '30000000'],
        ['OpenCode', '20000000'],
        ['tiny-app', '1000'],
      ] as const;
      summaries.memoryTop = apps.map(([name, bytes], index) => ({
        ...source,
        key: `bento-${index}`,
        pid: index + 1,
        name,
        residentBytes: { ...source.residentBytes, value: bytes },
      }));
      summaries.displayedMemoryTotalBytes = apps
        .reduce((sum, [, bytes]) => sum + BigInt(bytes), 0n)
        .toString();
    }
    if (this.scenario.id === 'initializing') {
      this.view.sample = null;
      this.view.history = null;
      this.view.processSummaries = null;
      this.view.memoryBreakdowns = { ram: null, vram: null };
      return;
    }
    if (this.scenario.gpu === 'none') {
      const gpuScopeId = sample.scopes.find((scope) => scope.kind === 'gpu')?.id ?? null;
      this.gpus = [];
      this.view.preferences.selectedGpuId = null;
      sample.scopes = sample.scopes.filter((scope) => scope.kind !== 'gpu');
      sample.capabilities = sample.capabilities.filter((entry) => entry.scopeId !== gpuScopeId);
      sample.gpu = {
        utilizationPct: unsupported(null, 'No GPU was discovered'),
        vramTotalBytes: unsupported(null, 'No GPU was discovered'),
        vramUsedBytes: unsupported(null, 'No GPU was discovered'),
        vramFreeBytes: unsupported(null, 'No GPU was discovered'),
        vramReservedBytes: unsupported(null, 'No GPU was discovered'),
        temperatureC: unsupported(null, 'No GPU was discovered'),
        powerW: unsupported(null, 'No GPU was discovered'),
        fanPct: unsupported(null, 'No GPU was discovered'),
        powerLimitPct: unsupported(null, 'No GPU was discovered'),
      };
      sample.power.gpuBoardW = unsupported(null, 'No GPU was discovered');
      sample.power.gpuTemperatureC = unsupported(null, 'No GPU was discovered');
      sample.power.fanPct = unsupported(null, 'No GPU was discovered');
      sample.power.powerLimitPct = unsupported(null, 'No GPU was discovered');
      this.view.memoryBreakdowns.vram = null;
      this.view.processSummaries!.gpuTop = [];
      this.view.processSummaries!.supportedRankingMetrics =
        this.view.processSummaries!.supportedRankingMetrics.filter((metric) => metric !== 'gpu');
      for (const observation of this.allProcessObservations()) {
        observation.gpuPct = unsupported(null, 'No GPU was discovered');
        observation.powerW = unsupported(null, 'Per-process power is unavailable');
      }
      this.view.history!.charts = this.view.history!.charts.map((chart) =>
        chart.seriesId === 'gpuPct' ? emptyChart(chart, null, null) : chart,
      );
      this.view.history!.gpuSampleAveragePct = unsupported(null, 'No GPU was discovered');
      this.view.history!.gpuSamplePeakPct = unsupported(null, 'No GPU was discovered');
    }

    if (this.scenario.network === 'empty') {
      this.view.notices.push({ code: 'network_empty', message: 'No active interfaces' });
      sample.network.receiveBytesPerSecond = {
        ...sample.network.receiveBytesPerSecond,
        status: 'valid',
        value: 0,
        reason: null,
      };
      sample.network.sendBytesPerSecond = {
        ...sample.network.sendBytesPerSecond,
        status: 'valid',
        value: 0,
        reason: null,
      };
      sample.network.receivedSessionBytes.value = '0';
      sample.network.sentSessionBytes.value = '0';
    }

    if (this.scenario.cpu === 'denied') {
      this.view.notices.push({
        code: 'baseline_degraded',
        message: 'Degraded monitoring: cpu.totalPct',
      });
      sample.cpu.totalPct = {
        status: 'permission_denied',
        value: null,
        sampledAtMs: null,
        elapsedMs: null,
        scopeId: sample.cpu.totalPct.scopeId,
        reason: 'CPU counters are not readable',
      };
      sample.cpu.userPct = { ...sample.cpu.userPct, ...clone(sample.cpu.totalPct) };
      sample.cpu.systemPct = { ...sample.cpu.systemPct, ...clone(sample.cpu.totalPct) };
    }

    if (this.scenario.memory === 'stale') {
      const usedPct = sample.memory.usedPct;
      sample.memory.usedPct = {
        status: 'stale',
        value: usedPct.value,
        sampledAtMs: usedPct.sampledAtMs,
        elapsedMs: usedPct.elapsedMs,
        scopeId: usedPct.scopeId,
        reason: 'No newer memory observation',
      };
    }
  }

  private allProcessObservations(): ProcessObservation[] {
    const summaries = this.view.processSummaries!;
    return [
      ...summaries.memoryTop,
      ...summaries.cpuTop,
      ...summaries.gpuTop,
      ...summaries.powerTop,
    ];
  }

  private applyGpuChange(): void {
    const sample = this.view.sample!;
    const previous = sample.scopes.find((scope) => scope.kind === 'gpu');
    if (!previous) return;
    const previousId = previous.id;
    const nextEpoch = previous.epoch + 1;
    for (const scope of sample.scopes) {
      if (scope.kind === 'gpu') {
        scope.id = NEW_GPU_SCOPE;
        scope.label = NEW_GPU_NAME;
        scope.epoch = nextEpoch;
      }
    }
    this.gpus = [
      {
        id: 'GPU-fixture-2',
        name: NEW_GPU_NAME,
        vendor: 'amd',
        kind: 'discrete',
        available: true,
      },
    ];
    this.view.preferences.selectedGpuId = 'GPU-fixture-2';
    sample.capabilities = sample.capabilities.filter((entry) => entry.scopeId !== previousId);
    const awaiting = 'Awaiting a matching poll for the selected GPU';
    sample.gpu.utilizationPct = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.vramTotalBytes = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.vramUsedBytes = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.vramFreeBytes = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.vramReservedBytes = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.temperatureC = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.powerW = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.fanPct = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.gpu.powerLimitPct = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.power.gpuBoardW = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.power.gpuTemperatureC = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.power.fanPct = unavailable(NEW_GPU_SCOPE, awaiting);
    sample.power.powerLimitPct = unavailable(NEW_GPU_SCOPE, awaiting);

    const summaries = this.view.processSummaries!;
    summaries.gpuTop = [];
    summaries.supportedRankingMetrics = summaries.supportedRankingMetrics.filter(
      (metric) => metric !== 'gpu',
    );
    if (this.view.selection.rankingMetric === 'gpu') {
      this.view.selection.rankingMetric = 'cpu';
    }
    for (const observation of this.allProcessObservations()) {
      for (const reading of [
        observation.residentBytes,
        observation.cpuPct,
        observation.gpuPct,
        observation.powerW,
      ]) {
        if (reading.scopeId === previousId) reading.scopeId = NEW_GPU_SCOPE;
      }
      observation.gpuPct = unavailable(NEW_GPU_SCOPE, awaiting);
    }
    this.view.memoryBreakdowns.vram = null;
    this.view.history!.charts = this.view.history!.charts.map((chart) =>
      chart.seriesId === 'gpuPct' ? emptyChart(chart, NEW_GPU_SCOPE, NEW_GPU_NAME) : chart,
    );
    this.view.history!.gpuSampleAveragePct = unavailable(NEW_GPU_SCOPE, awaiting);
    this.view.history!.gpuSamplePeakPct = unavailable(NEW_GPU_SCOPE, awaiting);
    this.bumpRevision();
  }

  private applyProcessChurn(): void {
    const summaries = this.view.processSummaries!;
    const source = summaries.memoryTop[0];
    if (!source) return;
    const newest = clone(source);
    newest.key = `7777:${this.view.sample!.capturedAtMs}`;
    newest.pid = 7777;
    newest.name = 'new-workload';
    newest.residentBytes.value = '1572864000';
    summaries.memoryTop = [newest, ...summaries.memoryTop].slice(0, 8);
    summaries.displayedMemoryTotalBytes = summaries.memoryTop
      .reduce((total, entry) => total + BigInt(entry.residentBytes.value ?? '0'), 0n)
      .toString();
  }

  private rotateHistory(): void {
    const sample = this.view.sample!;
    for (const chart of this.view.history!.charts) {
      if (chart.status !== 'valid') continue;
      const value = chartValue(chart, sample);
      chart.columns.shift();
      chart.columns.push(
        value === null
          ? { value: null, sampleCount: 0, hasGap: true }
          : { value, sampleCount: 1, hasGap: false },
      );
    }
  }
}
