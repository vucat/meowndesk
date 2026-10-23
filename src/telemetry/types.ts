/** Version-1 telemetry wire types. Names mirror the Rust DTOs exactly (camelCase). */

export const CONTRACT_VERSION = 1;

export type ReadingStatus = 'valid' | 'unsupported' | 'permission_denied' | 'unavailable' | 'stale';

export interface Reading<T> {
  status: ReadingStatus;
  value: T | null;
  sampledAtMs: number | null;
  elapsedMs: number | null;
  scopeId: string | null;
  reason: string | null;
}

/** Unsigned u64 byte amount encoded as a canonical decimal string; format with BigInt. */
export type ByteString = string;

export type Environment = 'native' | 'wsl2';
export type ScopeKind = 'host' | 'cpu_package' | 'gpu' | 'volume' | 'network_aggregate' | 'battery';
export type MetricUnit =
  'pct' | 'bytes' | 'bytes_per_second' | 'watts' | 'celsius' | 'seconds' | 'state';
export type BatteryState = 'charging' | 'discharging' | 'full' | 'on_ac' | 'unknown';
export type PowerPrimaryKind = 'battery_pct' | 'gpu_watts' | 'cpu_package_watts' | 'unavailable';
export type HistoryRange = '1m' | '5m' | '15m' | '1h';
export type MemoryView = 'ram' | 'vram';
export type RankingMetric = 'cpu' | 'gpu' | 'power';
export type Mode = 'live' | 'paused';
export type SegmentRole = 'used' | 'available' | 'free' | 'reserved' | 'unclassified';
export type BreakdownCoverage = 'complete' | 'partial' | 'unavailable';
export type SeriesId =
  'cpuPct' | 'ramUsedPct' | 'gpuPct' | 'diskBytesPerSecond' | 'networkBytesPerSecond' | 'powerW';
export type GpuVendor = 'nvidia' | 'amd' | 'intel' | 'other';
export type GpuKind = 'discrete' | 'integrated' | 'other';

export interface HostIdentity {
  sessionId: string;
  hostname: string;
  osName: string;
  osVersion: string;
  cpuModel: string;
  logicalCpuCount: number;
  environment: Environment;
  uptimeSeconds: number | null;
  scopeLabel: string;
}

export interface ResourceScope {
  id: string;
  kind: ScopeKind;
  label: string;
  provider: string;
  epoch: number;
}

export interface Capability {
  metricId: string;
  scopeId: string;
  unit: MetricUnit;
  provider: string;
  status: ReadingStatus;
  reason: string | null;
}

export interface CpuMetrics {
  totalPct: Reading<number>;
  userPct: Reading<number>;
  systemPct: Reading<number>;
  temperatureC: Reading<number>;
}

export interface MemoryMetrics {
  totalBytes: Reading<ByteString>;
  usedBytes: Reading<ByteString>;
  availableBytes: Reading<ByteString>;
  cachedBytes: Reading<ByteString>;
  usedPct: Reading<number>;
}

export interface GpuMetrics {
  utilizationPct: Reading<number>;
  vramTotalBytes: Reading<ByteString>;
  vramUsedBytes: Reading<ByteString>;
  vramFreeBytes: Reading<ByteString>;
  vramReservedBytes: Reading<ByteString>;
  temperatureC: Reading<number>;
  powerW: Reading<number>;
  fanPct: Reading<number>;
  powerLimitPct: Reading<number>;
}

export interface DiskMetrics {
  totalBytes: Reading<ByteString>;
  freeBytes: Reading<ByteString>;
  readBytesPerSecond: Reading<number>;
  writeBytesPerSecond: Reading<number>;
  writtenSessionBytes: Reading<ByteString>;
}

export interface NetworkMetrics {
  receiveBytesPerSecond: Reading<number>;
  sendBytesPerSecond: Reading<number>;
  receivedSessionBytes: Reading<ByteString>;
  sentSessionBytes: Reading<ByteString>;
}

export interface PowerMetrics {
  primaryKind: PowerPrimaryKind;
  primaryValue: Reading<number>;
  watts: Reading<number>;
  cpuPackageW: Reading<number>;
  gpuBoardW: Reading<number>;
  gpuTemperatureC: Reading<number>;
  fanPct: Reading<number>;
  powerLimitPct: Reading<number>;
  batteryChargePct: Reading<number>;
  batteryHealthPct: Reading<number>;
  batteryRemainingSeconds: Reading<number>;
  batteryState: Reading<BatteryState>;
}

export interface CurrentSample {
  sequence: number;
  capturedAtMs: number;
  elapsedMs: number;
  intervalMs: number;
  host: HostIdentity;
  scopes: ResourceScope[];
  capabilities: Capability[];
  cpu: CpuMetrics;
  memory: MemoryMetrics;
  gpu: GpuMetrics;
  disk: DiskMetrics;
  network: NetworkMetrics;
  power: PowerMetrics;
}

export interface ProcessObservation {
  key: string;
  pid: number;
  name: string;
  residentBytes: Reading<ByteString>;
  cpuPct: Reading<number>;
  gpuPct: Reading<number>;
  powerW: Reading<number>;
}

export interface ProcessSummaries {
  memoryTop: ProcessObservation[];
  cpuTop: ProcessObservation[];
  gpuTop: ProcessObservation[];
  powerTop: ProcessObservation[];
  supportedRankingMetrics: RankingMetric[];
  coverageNotes: string[];
  displayedMemoryTotalBytes: ByteString;
}

export interface MemorySegment {
  id: string;
  label: string;
  bytes: ByteString;
  role: SegmentRole;
}

export interface MemoryBreakdown {
  kind: MemoryView;
  scopeId: string;
  totalBytes: ByteString;
  usedPct: number;
  segments: MemorySegment[];
  coverage: BreakdownCoverage;
  note: string | null;
}

export interface MemoryBreakdowns {
  ram: MemoryBreakdown | null;
  vram: MemoryBreakdown | null;
}

export interface ChartColumn {
  value: number | null;
  sampleCount: number;
  hasGap: boolean;
}

export interface Chart {
  seriesId: SeriesId;
  unit: MetricUnit;
  scopeId: string | null;
  scopeLabel: string | null;
  scaleMax: number;
  columns: ChartColumn[];
  partialHistory: boolean;
  status: ReadingStatus;
}

export interface HistoryProjection {
  range: HistoryRange;
  endpointElapsedMs: number;
  retainedFromElapsedMs: number | null;
  charts: Chart[];
  gpuSampleAveragePct: Reading<number>;
  gpuSamplePeakPct: Reading<number>;
}

export interface ViewSelection {
  range: HistoryRange;
  memoryView: MemoryView;
  rankingMetric: RankingMetric;
}

export interface CollectionPreferences {
  intervalMs: number;
  selectedGpuId: string | null;
}

export interface Notice {
  code: string;
  message: string;
}

export interface View {
  contractVersion: number;
  sessionId: string;
  revision: number;
  mode: Mode;
  visible: boolean;
  inspectionId: string | null;
  endpointElapsedMs: number;
  endpointAtMs: number;
  sample: CurrentSample | null;
  processSummaries: ProcessSummaries | null;
  memoryBreakdowns: MemoryBreakdowns;
  history: HistoryProjection | null;
  selection: ViewSelection;
  preferences: CollectionPreferences;
  notices: Notice[];
}

export interface GpuDescriptor {
  id: string;
  name: string;
  vendor: GpuVendor;
  kind: GpuKind;
  available: boolean;
}

export interface Bootstrap {
  view: View;
  gpus: GpuDescriptor[];
}

export interface Delivery {
  contractVersion: number;
  sessionId: string;
  subscriptionId: string;
  deliverySequence: number;
  view: View;
}

export type CommandErrorCode =
  | 'invalid_argument'
  | 'unknown_gpu'
  | 'resume_required'
  | 'unsupported_ranking'
  | 'invalid_subscription'
  | 'busy'
  | 'not_ready'
  | 'payload_too_large'
  | 'internal';

export interface CommandError {
  code: CommandErrorCode;
  message: string;
  retryable: boolean;
}

export type CommandResult<T> = { ok: true; data: T } | { ok: false; error: CommandError };

export const CHART_SERIES_IDS: readonly SeriesId[] = [
  'cpuPct',
  'ramUsedPct',
  'gpuPct',
  'diskBytesPerSecond',
  'networkBytesPerSecond',
  'powerW',
];

export const METRIC_CATALOG: readonly string[] = [
  'cpu.totalPct',
  'cpu.userPct',
  'cpu.systemPct',
  'cpu.temperatureC',
  'memory.totalBytes',
  'memory.usedBytes',
  'memory.availableBytes',
  'memory.cachedBytes',
  'memory.usedPct',
  'gpu.utilizationPct',
  'gpu.vramTotalBytes',
  'gpu.vramUsedBytes',
  'gpu.vramFreeBytes',
  'gpu.vramReservedBytes',
  'gpu.temperatureC',
  'gpu.powerW',
  'gpu.fanPct',
  'gpu.powerLimitPct',
  'disk.totalBytes',
  'disk.freeBytes',
  'disk.readBytesPerSecond',
  'disk.writeBytesPerSecond',
  'disk.writtenSessionBytes',
  'network.receiveBytesPerSecond',
  'network.sendBytesPerSecond',
  'network.receivedSessionBytes',
  'network.sentSessionBytes',
  'power.primaryValue',
  'power.watts',
  'power.cpuPackageW',
  'power.gpuBoardW',
  'power.gpuTemperatureC',
  'power.fanPct',
  'power.powerLimitPct',
  'power.batteryChargePct',
  'power.batteryHealthPct',
  'power.batteryRemainingSeconds',
  'power.batteryState',
  'battery.chargePct',
  'battery.healthPct',
  'battery.remainingSeconds',
  'battery.state',
];
