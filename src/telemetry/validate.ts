/** Runtime validation of untrusted version-1 wire payloads. Mirrors the Rust model exactly. */

import { utf8Length } from './text';
import {
  CONTRACT_VERSION,
  CHART_SERIES_IDS,
  METRIC_CATALOG,
  type Bootstrap,
  type Capability,
  type Chart,
  type CurrentSample,
  type Delivery,
  type HistoryProjection,
  type HostIdentity,
  type MemoryBreakdown,
  type Notice,
  type ProcessObservation,
  type ProcessSummaries,
  type Reading,
  type ReadingStatus,
  type ResourceScope,
  type SeriesId,
  type View,
  type ViewSelection,
} from './types';

const MAX_JSON_SAFE_INT = 9_007_199_254_740_991;
const MESSAGE_MAX_BYTES = 160;
const LABEL_MAX_BYTES = 128;
const PROVIDER_MAX_BYTES = 32;
const METRIC_ID_MAX_BYTES = 48;
const MAX_SCOPES = 16;
const MAX_CAPABILITIES = 64;
const MAX_NOTICES = 8;
const MAX_GPUS = 16;
const CHART_COLUMNS = 60;
const U64_MAX = 18_446_744_073_709_551_615n;
const DECIMAL_BYTES = /^(0|[1-9][0-9]*)$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const STATUSES: readonly ReadingStatus[] = [
  'valid',
  'unsupported',
  'permission_denied',
  'unavailable',
  'stale',
];
const SERIES_UNITS: Record<SeriesId, string> = {
  cpuPct: 'pct',
  ramUsedPct: 'pct',
  gpuPct: 'pct',
  diskBytesPerSecond: 'bytes_per_second',
  networkBytesPerSecond: 'bytes_per_second',
  powerW: 'watts',
};

export class ContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ContractError';
  }
}

type Json = Record<string, unknown>;
type Domain = (value: unknown, path: string) => void;

function fail(path: string, message: string): never {
  throw new ContractError(`${path}: ${message}`);
}

function object(value: unknown, path: string): Json {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    fail(path, 'expected an object');
  }
  return value as Json;
}

function array(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) fail(path, 'expected an array');
  return value;
}

function exactKeys(value: Json, path: string, keys: readonly string[]): void {
  for (const key of Object.keys(value)) {
    if (!keys.includes(key)) fail(path, `unexpected field ${key}`);
  }
  for (const key of keys) {
    if (!(key in value)) fail(path, `missing field ${key}`);
  }
}

function stringValue(value: unknown, path: string): string {
  if (typeof value !== 'string') fail(path, 'expected a string');
  return value;
}

function booleanValue(value: unknown, path: string): boolean {
  if (typeof value !== 'boolean') fail(path, 'expected a boolean');
  return value;
}

function numberValue(value: unknown, path: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    fail(path, 'expected a finite number');
  }
  return value;
}

function integerValue(value: unknown, path: string): number {
  const number = numberValue(value, path);
  if (!Number.isInteger(number) || number < 0 || number > MAX_JSON_SAFE_INT) {
    fail(path, 'expected a JSON-safe nonnegative integer');
  }
  return number;
}

function enumValue<T extends string>(value: unknown, path: string, allowed: readonly T[]): T {
  const text = stringValue(value, path);
  if (!(allowed as readonly string[]).includes(text)) fail(path, `unexpected value ${text}`);
  return text as T;
}

function textValue(value: unknown, path: string, maxBytes: number): string {
  const text = stringValue(value, path);
  if (utf8Length(text) > maxBytes) fail(path, `exceeds ${maxBytes} UTF-8 bytes`);
  if (/\p{Cc}/u.test(text)) fail(path, 'contains control characters');
  return text;
}

function opaqueId(value: unknown, path: string): string {
  const text = textValue(value, path, LABEL_MAX_BYTES);
  if (text.length === 0) fail(path, 'identifier must not be empty');
  return text;
}

function uuidValue(value: unknown, path: string): string {
  const text = stringValue(value, path);
  if (!UUID.test(text)) fail(path, 'expected a UUID');
  return text;
}

function byteValue(value: unknown, path: string): bigint {
  const text = stringValue(value, path);
  if (!DECIMAL_BYTES.test(text)) fail(path, 'expected a canonical unsigned decimal string');
  const amount = BigInt(text);
  if (amount > U64_MAX) fail(path, 'byte amount exceeds the u64 domain');
  return amount;
}

const pct: Domain = (value, path) => {
  const number = numberValue(value, path);
  if (number < 0 || number > 100) fail(path, 'must be within [0,100]');
};
const nonNegative: Domain = (value, path) => {
  if (numberValue(value, path) < 0) fail(path, 'must not be negative');
};
const temperature: Domain = (value, path) => {
  const number = numberValue(value, path);
  if (number < -273.15 || number > 1000) fail(path, 'must be a supported Celsius value');
};
const bytes: Domain = (value, path) => {
  byteValue(value, path);
};
const noDomain: Domain = () => {};
const batteryState: Domain = (value, path) =>
  enumValue(value, path, ['charging', 'discharging', 'full', 'on_ac', 'unknown']);

interface ReadingContext {
  scopes: Set<string>;
}

function reading(value: unknown, path: string, domain: Domain, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, ['status', 'value', 'sampledAtMs', 'elapsedMs', 'scopeId', 'reason']);
  const status = enumValue(data.status, `${path}.status`, STATUSES);
  const hasValue = data.value !== null;

  if (status === 'valid') {
    if (!hasValue) fail(path, 'a valid reading requires a value');
    integerValue(data.sampledAtMs, `${path}.sampledAtMs`);
    integerValue(data.elapsedMs, `${path}.elapsedMs`);
    if (data.reason !== null) fail(path, 'a valid reading must not carry a reason');
    if (data.scopeId === null) fail(path, 'a valid reading requires a scopeId');
  } else if (status === 'stale') {
    if (data.reason === null) fail(path, 'a stale reading requires a reason');
    textValue(data.reason, `${path}.reason`, MESSAGE_MAX_BYTES);
    if (hasValue) {
      integerValue(data.sampledAtMs, `${path}.sampledAtMs`);
      integerValue(data.elapsedMs, `${path}.elapsedMs`);
    } else if (data.sampledAtMs !== null || data.elapsedMs !== null) {
      fail(path, 'timestamps are only valid when a stale value exists');
    }
  } else {
    if (hasValue) fail(path, `a ${status} reading must not carry a value`);
    if (data.reason === null) fail(path, `a ${status} reading requires a reason`);
    textValue(data.reason, `${path}.reason`, MESSAGE_MAX_BYTES);
    if (data.sampledAtMs !== null || data.elapsedMs !== null) {
      fail(path, `a ${status} reading must not carry observation timestamps`);
    }
  }

  if (data.reason !== null) textValue(data.reason, `${path}.reason`, MESSAGE_MAX_BYTES);
  if (data.scopeId !== null) {
    const scopeId = textValue(data.scopeId, `${path}.scopeId`, LABEL_MAX_BYTES);
    if (context.scopes.size > 0 && !context.scopes.has(scopeId)) {
      fail(path, 'scopeId does not reference a discovered scope');
    }
  }
  if (hasValue) domain(data.value, `${path}.value`);
}

function host(value: unknown, path: string): HostIdentity {
  const data = object(value, path);
  exactKeys(data, path, [
    'sessionId',
    'hostname',
    'osName',
    'osVersion',
    'cpuModel',
    'logicalCpuCount',
    'environment',
    'uptimeSeconds',
    'scopeLabel',
  ]);
  uuidValue(data.sessionId, `${path}.sessionId`);
  for (const key of ['hostname', 'osName', 'osVersion', 'cpuModel', 'scopeLabel'] as const) {
    textValue(data[key], `${path}.${key}`, LABEL_MAX_BYTES);
  }
  const cpus = integerValue(data.logicalCpuCount, `${path}.logicalCpuCount`);
  if (cpus === 0 || cpus > 4096) fail(`${path}.logicalCpuCount`, 'must be between 1 and 4096');
  enumValue(data.environment, `${path}.environment`, ['native', 'wsl2']);
  if (data.uptimeSeconds !== null) integerValue(data.uptimeSeconds, `${path}.uptimeSeconds`);
  return data as unknown as HostIdentity;
}

function scope(value: unknown, path: string): ResourceScope {
  const data = object(value, path);
  exactKeys(data, path, ['id', 'kind', 'label', 'provider', 'epoch']);
  opaqueId(data.id, `${path}.id`);
  enumValue(data.kind, `${path}.kind`, [
    'host',
    'cpu_package',
    'gpu',
    'volume',
    'network_aggregate',
    'battery',
  ]);
  textValue(data.label, `${path}.label`, LABEL_MAX_BYTES);
  textValue(data.provider, `${path}.provider`, PROVIDER_MAX_BYTES);
  integerValue(data.epoch, `${path}.epoch`);
  return data as unknown as ResourceScope;
}

function capability(value: unknown, path: string, context: ReadingContext): Capability {
  const data = object(value, path);
  exactKeys(data, path, ['metricId', 'scopeId', 'unit', 'provider', 'status', 'reason']);
  const metricId = stringValue(data.metricId, `${path}.metricId`);
  if (!METRIC_CATALOG.includes(metricId)) fail(`${path}.metricId`, 'is outside the metric catalog');
  if (utf8Length(metricId) > METRIC_ID_MAX_BYTES) {
    fail(`${path}.metricId`, `exceeds ${METRIC_ID_MAX_BYTES} UTF-8 bytes`);
  }
  const scopeId = opaqueId(data.scopeId, `${path}.scopeId`);
  if (!context.scopes.has(scopeId)) fail(`${path}.scopeId`, 'references an undiscovered scope');
  enumValue(data.unit, `${path}.unit`, [
    'pct',
    'bytes',
    'bytes_per_second',
    'watts',
    'celsius',
    'seconds',
    'state',
  ]);
  textValue(data.provider, `${path}.provider`, PROVIDER_MAX_BYTES);
  const status = enumValue(data.status, `${path}.status`, STATUSES);
  if (status === 'stale') fail(`${path}.status`, 'must be a current support state');
  if (status === 'valid') {
    if (data.reason !== null) fail(path, 'a valid capability must not carry a reason');
  } else if (data.reason === null) {
    fail(path, 'a non-valid capability requires a reason');
  } else {
    textValue(data.reason, `${path}.reason`, MESSAGE_MAX_BYTES);
  }
  return data as unknown as Capability;
}

function validBytes(data: Json): bigint | null {
  if (data.status !== 'valid' || data.value === null) return null;
  return byteValue(data.value, 'value');
}

function cpuGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, ['totalPct', 'userPct', 'systemPct', 'temperatureC']);
  reading(data.totalPct, `${path}.totalPct`, pct, context);
  reading(data.userPct, `${path}.userPct`, pct, context);
  reading(data.systemPct, `${path}.systemPct`, pct, context);
  reading(data.temperatureC, `${path}.temperatureC`, temperature, context);
}

function memoryGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, ['totalBytes', 'usedBytes', 'availableBytes', 'cachedBytes', 'usedPct']);
  for (const key of ['totalBytes', 'usedBytes', 'availableBytes', 'cachedBytes'] as const) {
    reading(data[key], `${path}.${key}`, bytes, context);
  }
  reading(data.usedPct, `${path}.usedPct`, pct, context);

  const total = validBytes(object(data.totalBytes, `${path}.totalBytes`));
  const used = validBytes(object(data.usedBytes, `${path}.usedBytes`));
  const available = validBytes(object(data.availableBytes, `${path}.availableBytes`));
  if (total !== null) {
    if (used !== null && used > total) fail(path, 'used bytes exceed total capacity');
    if (available !== null && available > total)
      fail(path, 'available bytes exceed total capacity');
    if (used !== null && available !== null && used + available > total) {
      fail(path, 'used plus available exceeds total capacity');
    }
  }
}

function gpuGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, [
    'utilizationPct',
    'vramTotalBytes',
    'vramUsedBytes',
    'vramFreeBytes',
    'vramReservedBytes',
    'temperatureC',
    'powerW',
    'fanPct',
    'powerLimitPct',
  ]);
  reading(data.utilizationPct, `${path}.utilizationPct`, pct, context);
  for (const key of [
    'vramTotalBytes',
    'vramUsedBytes',
    'vramFreeBytes',
    'vramReservedBytes',
  ] as const) {
    reading(data[key], `${path}.${key}`, bytes, context);
  }
  reading(data.temperatureC, `${path}.temperatureC`, temperature, context);
  reading(data.powerW, `${path}.powerW`, nonNegative, context);
  reading(data.fanPct, `${path}.fanPct`, pct, context);
  reading(data.powerLimitPct, `${path}.powerLimitPct`, pct, context);

  const total = validBytes(object(data.vramTotalBytes, `${path}.vramTotalBytes`));
  if (total !== null) {
    let sum = 0n;
    for (const key of ['vramUsedBytes', 'vramFreeBytes', 'vramReservedBytes'] as const) {
      const amount = validBytes(object(data[key], `${path}.${key}`));
      if (amount !== null) {
        if (amount > total) fail(path, 'a VRAM category exceeds total capacity');
        sum += amount;
      }
    }
    if (sum > total) fail(path, 'VRAM categories exceed total capacity');
  }
}

function diskGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, [
    'totalBytes',
    'freeBytes',
    'readBytesPerSecond',
    'writeBytesPerSecond',
    'writtenSessionBytes',
  ]);
  reading(data.totalBytes, `${path}.totalBytes`, bytes, context);
  reading(data.freeBytes, `${path}.freeBytes`, bytes, context);
  reading(data.readBytesPerSecond, `${path}.readBytesPerSecond`, nonNegative, context);
  reading(data.writeBytesPerSecond, `${path}.writeBytesPerSecond`, nonNegative, context);
  reading(data.writtenSessionBytes, `${path}.writtenSessionBytes`, bytes, context);

  const total = validBytes(object(data.totalBytes, `${path}.totalBytes`));
  const free = validBytes(object(data.freeBytes, `${path}.freeBytes`));
  if (total !== null && free !== null && free > total)
    fail(path, 'free bytes exceed total capacity');
}

function networkGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, [
    'receiveBytesPerSecond',
    'sendBytesPerSecond',
    'receivedSessionBytes',
    'sentSessionBytes',
  ]);
  reading(data.receiveBytesPerSecond, `${path}.receiveBytesPerSecond`, nonNegative, context);
  reading(data.sendBytesPerSecond, `${path}.sendBytesPerSecond`, nonNegative, context);
  reading(data.receivedSessionBytes, `${path}.receivedSessionBytes`, bytes, context);
  reading(data.sentSessionBytes, `${path}.sentSessionBytes`, bytes, context);
}

function powerGroup(value: unknown, path: string, context: ReadingContext): void {
  const data = object(value, path);
  exactKeys(data, path, [
    'primaryKind',
    'primaryValue',
    'watts',
    'cpuPackageW',
    'gpuBoardW',
    'gpuTemperatureC',
    'fanPct',
    'powerLimitPct',
    'batteryChargePct',
    'batteryHealthPct',
    'batteryRemainingSeconds',
    'batteryState',
  ]);
  const kind = enumValue(data.primaryKind, `${path}.primaryKind`, [
    'battery_pct',
    'gpu_watts',
    'cpu_package_watts',
    'unavailable',
  ]);
  reading(
    data.primaryValue,
    `${path}.primaryValue`,
    kind === 'battery_pct' ? pct : nonNegative,
    context,
  );
  reading(data.watts, `${path}.watts`, nonNegative, context);
  reading(data.cpuPackageW, `${path}.cpuPackageW`, nonNegative, context);
  reading(data.gpuBoardW, `${path}.gpuBoardW`, nonNegative, context);
  reading(data.gpuTemperatureC, `${path}.gpuTemperatureC`, temperature, context);
  reading(data.fanPct, `${path}.fanPct`, pct, context);
  reading(data.powerLimitPct, `${path}.powerLimitPct`, pct, context);
  reading(data.batteryChargePct, `${path}.batteryChargePct`, pct, context);
  reading(data.batteryHealthPct, `${path}.batteryHealthPct`, pct, context);
  reading(data.batteryRemainingSeconds, `${path}.batteryRemainingSeconds`, noDomain, context);
  reading(data.batteryState, `${path}.batteryState`, batteryState, context);
}

function sample(value: unknown, path: string): CurrentSample {
  const data = object(value, path);
  exactKeys(data, path, [
    'sequence',
    'capturedAtMs',
    'elapsedMs',
    'intervalMs',
    'host',
    'scopes',
    'capabilities',
    'cpu',
    'memory',
    'gpu',
    'disk',
    'network',
    'power',
  ]);
  integerValue(data.sequence, `${path}.sequence`);
  integerValue(data.capturedAtMs, `${path}.capturedAtMs`);
  integerValue(data.elapsedMs, `${path}.elapsedMs`);
  if (![1000, 2000, 5000].includes(numberValue(data.intervalMs, `${path}.intervalMs`))) {
    fail(`${path}.intervalMs`, 'must be 1000, 2000 or 5000');
  }
  host(data.host, `${path}.host`);

  const scopes = array(data.scopes, `${path}.scopes`);
  if (scopes.length === 0 || scopes.length > MAX_SCOPES) {
    fail(`${path}.scopes`, `must contain 1..=${MAX_SCOPES} scopes`);
  }
  const ids = new Set<string>();
  scopes.forEach((scopeValue, index) => {
    const parsed = scope(scopeValue, `${path}.scopes[${index}]`);
    if (ids.has(parsed.id)) fail(`${path}.scopes[${index}].id`, 'is duplicated');
    ids.add(parsed.id);
  });
  const context: ReadingContext = { scopes: ids };

  const capabilities = array(data.capabilities, `${path}.capabilities`);
  if (capabilities.length > MAX_CAPABILITIES) {
    fail(`${path}.capabilities`, `must not exceed ${MAX_CAPABILITIES} entries`);
  }
  capabilities.forEach((entry, index) =>
    capability(entry, `${path}.capabilities[${index}]`, context),
  );

  cpuGroup(data.cpu, `${path}.cpu`, context);
  memoryGroup(data.memory, `${path}.memory`, context);
  gpuGroup(data.gpu, `${path}.gpu`, context);
  diskGroup(data.disk, `${path}.disk`, context);
  networkGroup(data.network, `${path}.network`, context);
  powerGroup(data.power, `${path}.power`, context);
  return data as unknown as CurrentSample;
}

function processObservation(
  value: unknown,
  path: string,
  context: ReadingContext,
): ProcessObservation {
  const data = object(value, path);
  exactKeys(data, path, ['key', 'pid', 'name', 'residentBytes', 'cpuPct', 'gpuPct', 'powerW']);
  opaqueId(data.key, `${path}.key`);
  integerValue(data.pid, `${path}.pid`);
  textValue(data.name, `${path}.name`, LABEL_MAX_BYTES);
  reading(data.residentBytes, `${path}.residentBytes`, bytes, context);
  reading(data.cpuPct, `${path}.cpuPct`, pct, context);
  reading(data.gpuPct, `${path}.gpuPct`, pct, context);
  reading(data.powerW, `${path}.powerW`, nonNegative, context);
  return data as unknown as ProcessObservation;
}

function orderedBy<T extends number | bigint>(
  value: unknown,
  path: string,
  context: ReadingContext,
  metric: (observation: ProcessObservation) => T | null,
): void {
  const list = array(value, path);
  let previous: { value: T; key: string } | null = null;
  list.forEach((entry, index) => {
    const observation = processObservation(entry, `${path}[${index}]`, context);
    const current = metric(observation);
    if (current === null) fail(path, 'an entry lacks its ranking metric');
    if (
      previous !== null &&
      (current > previous.value || (current === previous.value && observation.key <= previous.key))
    ) {
      fail(path, 'is not ordered by raw value then process key');
    }
    previous = { value: current, key: observation.key };
  });
}

function processSummaries(value: unknown, path: string, context: ReadingContext): ProcessSummaries {
  const data = object(value, path);
  exactKeys(data, path, [
    'memoryTop',
    'cpuTop',
    'gpuTop',
    'powerTop',
    'supportedRankingMetrics',
    'coverageNotes',
    'displayedMemoryTotalBytes',
  ]);
  if (array(data.memoryTop, `${path}.memoryTop`).length > 8) {
    fail(`${path}.memoryTop`, 'must not exceed 8 entries');
  }
  for (const key of ['cpuTop', 'gpuTop', 'powerTop'] as const) {
    if (array(data[key], `${path}.${key}`).length > 5) {
      fail(`${path}.${key}`, 'must not exceed 5 entries');
    }
  }
  orderedBy(data.memoryTop, `${path}.memoryTop`, context, (observation) =>
    observation.residentBytes.status === 'valid' && observation.residentBytes.value !== null
      ? byteValue(observation.residentBytes.value, `${path}.memoryTop`)
      : null,
  );
  orderedBy(data.cpuTop, `${path}.cpuTop`, context, (observation) =>
    observation.cpuPct.status === 'valid' ? observation.cpuPct.value : null,
  );
  orderedBy(data.gpuTop, `${path}.gpuTop`, context, (observation) =>
    observation.gpuPct.status === 'valid' ? observation.gpuPct.value : null,
  );
  orderedBy(data.powerTop, `${path}.powerTop`, context, (observation) =>
    observation.powerW.status === 'valid' ? observation.powerW.value : null,
  );

  const metrics = array(data.supportedRankingMetrics, `${path}.supportedRankingMetrics`);
  const seen = new Set<string>();
  metrics.forEach((metric, index) => {
    const parsed = enumValue(metric, `${path}.supportedRankingMetrics[${index}]`, [
      'cpu',
      'gpu',
      'power',
    ]);
    if (seen.has(parsed)) fail(`${path}.supportedRankingMetrics`, 'contains duplicates');
    seen.add(parsed);
  });

  const notes = array(data.coverageNotes, `${path}.coverageNotes`);
  if (notes.length > MAX_NOTICES) fail(`${path}.coverageNotes`, 'must not exceed 8 entries');
  notes.forEach((note, index) =>
    textValue(note, `${path}.coverageNotes[${index}]`, MESSAGE_MAX_BYTES),
  );

  const displayed = byteValue(data.displayedMemoryTotalBytes, `${path}.displayedMemoryTotalBytes`);
  let sum = 0n;
  for (const observation of array(data.memoryTop, `${path}.memoryTop`)) {
    const parsed = object(observation, `${path}.memoryTop`);
    const resident = object(parsed.residentBytes, `${path}.memoryTop.residentBytes`);
    if (resident.status === 'valid' && resident.value !== null) {
      sum += byteValue(resident.value, `${path}.memoryTop.residentBytes.value`);
    }
  }
  if (sum !== displayed) {
    fail(`${path}.displayedMemoryTotalBytes`, 'must equal the displayed resident sum');
  }
  return data as unknown as ProcessSummaries;
}

function breakdown(value: unknown, path: string, kind: 'ram' | 'vram'): MemoryBreakdown {
  const data = object(value, path);
  exactKeys(data, path, [
    'kind',
    'scopeId',
    'totalBytes',
    'usedPct',
    'segments',
    'coverage',
    'note',
  ]);
  if (enumValue(data.kind, `${path}.kind`, ['ram', 'vram']) !== kind) {
    fail(`${path}.kind`, 'does not match its RAM/VRAM slot');
  }
  opaqueId(data.scopeId, `${path}.scopeId`);
  const total = byteValue(data.totalBytes, `${path}.totalBytes`);
  if (total === 0n) fail(`${path}.totalBytes`, 'must be positive');
  pct(data.usedPct, `${path}.usedPct`);
  const segments = array(data.segments, `${path}.segments`);
  if (segments.length > 8) fail(`${path}.segments`, 'must not exceed 8 entries');
  let sum = 0n;
  const ids = new Set<string>();
  segments.forEach((entry, index) => {
    const segment = object(entry, `${path}.segments[${index}]`);
    exactKeys(segment, `${path}.segments[${index}]`, ['id', 'label', 'bytes', 'role']);
    const id = opaqueId(segment.id, `${path}.segments[${index}].id`);
    if (ids.has(id)) fail(`${path}.segments`, 'contains duplicate ids');
    ids.add(id);
    textValue(segment.label, `${path}.segments[${index}].label`, LABEL_MAX_BYTES);
    sum += byteValue(segment.bytes, `${path}.segments[${index}].bytes`);
    enumValue(segment.role, `${path}.segments[${index}].role`, [
      'used',
      'available',
      'free',
      'reserved',
      'unclassified',
    ]);
  });
  if (sum > total) fail(path, 'segments exceed total capacity');
  const coverage = enumValue(data.coverage, `${path}.coverage`, [
    'complete',
    'partial',
    'unavailable',
  ]);
  if (coverage === 'complete' && sum !== total) {
    fail(path, 'a complete breakdown must partition total capacity');
  }
  if (coverage === 'unavailable' && segments.length > 0) {
    fail(path, 'an unavailable breakdown must not carry segments');
  }
  if (data.note !== null) textValue(data.note, `${path}.note`, MESSAGE_MAX_BYTES);
  return data as unknown as MemoryBreakdown;
}

function chart(value: unknown, path: string, context: ReadingContext): Chart {
  const data = object(value, path);
  exactKeys(data, path, [
    'seriesId',
    'unit',
    'scopeId',
    'scopeLabel',
    'scaleMax',
    'columns',
    'partialHistory',
    'status',
  ]);
  const seriesId = enumValue(data.seriesId, `${path}.seriesId`, [
    'cpuPct',
    'ramUsedPct',
    'gpuPct',
    'diskBytesPerSecond',
    'networkBytesPerSecond',
    'powerW',
  ]);
  const unit = enumValue(data.unit, `${path}.unit`, [
    'pct',
    'bytes',
    'bytes_per_second',
    'watts',
    'celsius',
    'seconds',
    'state',
  ]);
  if (unit !== SERIES_UNITS[seriesId]) fail(path, 'unit does not match the series');
  if (data.scopeId !== null) {
    const scopeId = opaqueId(data.scopeId, `${path}.scopeId`);
    if (context.scopes.size > 0 && !context.scopes.has(scopeId)) {
      fail(`${path}.scopeId`, 'does not reference a discovered scope');
    }
  }
  if (data.scopeLabel !== null) textValue(data.scopeLabel, `${path}.scopeLabel`, LABEL_MAX_BYTES);
  const scaleMax = numberValue(data.scaleMax, `${path}.scaleMax`);
  if (scaleMax < 1) fail(`${path}.scaleMax`, 'must be at least 1');
  const percentSeries = unit === 'pct';
  if (percentSeries && scaleMax !== 100) fail(`${path}.scaleMax`, 'percent charts use 0-100');
  booleanValue(data.partialHistory, `${path}.partialHistory`);

  const columns = array(data.columns, `${path}.columns`);
  if (columns.length !== CHART_COLUMNS) {
    fail(`${path}.columns`, `expected exactly ${CHART_COLUMNS} columns`);
  }
  let largest = 0;
  let sampled = 0;
  columns.forEach((entry, index) => {
    const column = object(entry, `${path}.columns[${index}]`);
    exactKeys(column, `${path}.columns[${index}]`, ['value', 'sampleCount', 'hasGap']);
    const sampleCount = integerValue(column.sampleCount, `${path}.columns[${index}].sampleCount`);
    booleanValue(column.hasGap, `${path}.columns[${index}].hasGap`);
    if (column.value === null) {
      if (sampleCount !== 0) fail(path, 'an empty column must have zero sampleCount');
      if (column.hasGap !== true) fail(path, 'an empty column must be marked as a gap');
    } else {
      if (sampleCount === 0) fail(path, 'a valued column requires samples');
      const number = numberValue(column.value, `${path}.columns[${index}].value`);
      if (percentSeries ? number < 0 || number > 100 : number < 0) {
        fail(`${path}.columns[${index}].value`, 'is outside its domain');
      }
      largest = Math.max(largest, number);
      sampled += 1;
    }
  });
  if (!percentSeries && scaleMax < largest) fail(`${path}.scaleMax`, 'is below the largest column');
  const status = enumValue(data.status, `${path}.status`, STATUSES);
  if (status === 'valid' && sampled === 0) fail(path, 'a valid chart requires at least one sample');
  if (status === 'unavailable' && sampled !== 0) {
    fail(path, 'an unavailable chart must not carry samples');
  }
  if (status !== 'valid' && status !== 'unavailable') {
    fail(`${path}.status`, 'chart status must be valid or unavailable');
  }
  return data as unknown as Chart;
}

function history(
  value: unknown,
  path: string,
  context: ReadingContext,
  selection: ViewSelection,
  endpoint: number,
): HistoryProjection {
  const data = object(value, path);
  exactKeys(data, path, [
    'range',
    'endpointElapsedMs',
    'retainedFromElapsedMs',
    'charts',
    'gpuSampleAveragePct',
    'gpuSamplePeakPct',
  ]);
  if (enumValue(data.range, `${path}.range`, ['1m', '5m', '15m', '1h']) !== selection.range) {
    fail(`${path}.range`, 'must match the selected range');
  }
  if (integerValue(data.endpointElapsedMs, `${path}.endpointElapsedMs`) !== endpoint) {
    fail(`${path}.endpointElapsedMs`, 'must equal the view endpoint');
  }
  if (data.retainedFromElapsedMs !== null) {
    const retained = integerValue(data.retainedFromElapsedMs, `${path}.retainedFromElapsedMs`);
    if (retained > endpoint) fail(`${path}.retainedFromElapsedMs`, 'must not exceed the endpoint');
  }
  const charts = array(data.charts, `${path}.charts`);
  if (charts.length !== CHART_SERIES_IDS.length) fail(`${path}.charts`, 'must contain six charts');
  const seen = new Set<string>();
  charts.forEach((entry, index) => {
    const parsed = chart(entry, `${path}.charts[${index}]`, context);
    if (seen.has(parsed.seriesId)) fail(`${path}.charts`, 'contains duplicate series ids');
    seen.add(parsed.seriesId);
  });
  for (const series of CHART_SERIES_IDS) {
    if (!seen.has(series)) fail(`${path}.charts`, `is missing the ${series} series`);
  }
  reading(data.gpuSampleAveragePct, `${path}.gpuSampleAveragePct`, pct, context);
  reading(data.gpuSamplePeakPct, `${path}.gpuSamplePeakPct`, pct, context);
  return data as unknown as HistoryProjection;
}

function selection(value: unknown, path: string): ViewSelection {
  const data = object(value, path);
  exactKeys(data, path, ['range', 'memoryView', 'rankingMetric']);
  enumValue(data.range, `${path}.range`, ['1m', '5m', '15m', '1h']);
  enumValue(data.memoryView, `${path}.memoryView`, ['ram', 'vram']);
  enumValue(data.rankingMetric, `${path}.rankingMetric`, ['cpu', 'gpu', 'power']);
  return data as unknown as ViewSelection;
}

function preferences(value: unknown, path: string): void {
  const data = object(value, path);
  exactKeys(data, path, ['intervalMs', 'selectedGpuId']);
  if (![1000, 2000, 5000].includes(numberValue(data.intervalMs, `${path}.intervalMs`))) {
    fail(`${path}.intervalMs`, 'must be 1000, 2000 or 5000');
  }
  if (data.selectedGpuId !== null) opaqueId(data.selectedGpuId, `${path}.selectedGpuId`);
}

function notices(value: unknown, path: string): Notice[] {
  const list = array(value, path);
  if (list.length > MAX_NOTICES) fail(path, 'must not exceed 8 entries');
  list.forEach((entry, index) => {
    const notice = object(entry, `${path}[${index}]`);
    exactKeys(notice, `${path}[${index}]`, ['code', 'message']);
    textValue(notice.code, `${path}[${index}].code`, PROVIDER_MAX_BYTES);
    const message = textValue(notice.message, `${path}[${index}].message`, MESSAGE_MAX_BYTES);
    if (message.length === 0) fail(`${path}[${index}].message`, 'must not be empty');
  });
  return list as unknown as Notice[];
}

export function validateSample(value: unknown, path = 'sample'): CurrentSample {
  return sample(value, path);
}

export function validateView(value: unknown, path = 'view'): View {
  const data = object(value, path);
  exactKeys(data, path, [
    'contractVersion',
    'sessionId',
    'revision',
    'mode',
    'visible',
    'inspectionId',
    'endpointElapsedMs',
    'endpointAtMs',
    'sample',
    'processSummaries',
    'memoryBreakdowns',
    'history',
    'selection',
    'preferences',
    'notices',
  ]);
  if (numberValue(data.contractVersion, `${path}.contractVersion`) !== CONTRACT_VERSION) {
    fail(`${path}.contractVersion`, 'must be 1');
  }
  const sessionId = uuidValue(data.sessionId, `${path}.sessionId`);
  integerValue(data.revision, `${path}.revision`);
  const mode = enumValue(data.mode, `${path}.mode`, ['live', 'paused']);
  booleanValue(data.visible, `${path}.visible`);
  if (mode === 'live') {
    if (data.inspectionId !== null) fail(`${path}.inspectionId`, 'must be null while live');
  } else if (data.inspectionId === null) {
    fail(`${path}.inspectionId`, 'must be a UUID while paused');
  } else {
    uuidValue(data.inspectionId, `${path}.inspectionId`);
  }
  const endpointElapsedMs = integerValue(data.endpointElapsedMs, `${path}.endpointElapsedMs`);
  integerValue(data.endpointAtMs, `${path}.endpointAtMs`);
  preferences(data.preferences, `${path}.preferences`);
  notices(data.notices, `${path}.notices`);
  const parsedSelection = selection(data.selection, `${path}.selection`);

  const context: ReadingContext = { scopes: new Set() };
  let sampleElapsed: number | null = null;
  if (data.sample !== null) {
    const parsedSample = sample(data.sample, `${path}.sample`);
    if (parsedSample.host.sessionId !== sessionId) {
      fail(`${path}.sample.host.sessionId`, 'must match the view session');
    }
    sampleElapsed = parsedSample.elapsedMs;
    parsedSample.scopes.forEach((entry) => context.scopes.add(entry.id));
  }
  if (sampleElapsed !== null && sampleElapsed > endpointElapsedMs) {
    fail(`${path}.endpointElapsedMs`, 'must not precede the sample');
  }
  if (data.processSummaries !== null) {
    processSummaries(data.processSummaries, `${path}.processSummaries`, context);
  }
  const breakdowns = object(data.memoryBreakdowns, `${path}.memoryBreakdowns`);
  exactKeys(breakdowns, `${path}.memoryBreakdowns`, ['ram', 'vram']);
  if (breakdowns.ram !== null) breakdown(breakdowns.ram, `${path}.memoryBreakdowns.ram`, 'ram');
  if (breakdowns.vram !== null) breakdown(breakdowns.vram, `${path}.memoryBreakdowns.vram`, 'vram');
  if (data.history !== null) {
    history(data.history, `${path}.history`, context, parsedSelection, endpointElapsedMs);
  }
  return data as unknown as View;
}

function gpuDescriptor(value: unknown, path: string): void {
  const data = object(value, path);
  exactKeys(data, path, ['id', 'name', 'vendor', 'kind', 'available']);
  opaqueId(data.id, `${path}.id`);
  textValue(data.name, `${path}.name`, LABEL_MAX_BYTES);
  enumValue(data.vendor, `${path}.vendor`, ['nvidia', 'amd', 'intel', 'other']);
  enumValue(data.kind, `${path}.kind`, ['discrete', 'integrated', 'other']);
  booleanValue(data.available, `${path}.available`);
}

export function validateBootstrap(value: unknown): Bootstrap {
  const data = object(value, 'bootstrap');
  exactKeys(data, 'bootstrap', ['view', 'gpus']);
  const view = validateView(data.view, 'bootstrap.view');
  const gpus = array(data.gpus, 'bootstrap.gpus');
  if (gpus.length > MAX_GPUS) fail('bootstrap.gpus', `must not exceed ${MAX_GPUS} entries`);
  const ids = new Set<string>();
  gpus.forEach((entry, index) => {
    gpuDescriptor(entry, `bootstrap.gpus[${index}]`);
    const id = stringValue(object(entry, 'gpu').id, `bootstrap.gpus[${index}].id`);
    if (ids.has(id)) fail('bootstrap.gpus', 'contains duplicate ids');
    ids.add(id);
  });
  const selected = view.preferences.selectedGpuId;
  if (selected === null) {
    if (gpus.length > 0) fail('bootstrap.view.preferences.selectedGpuId', 'must name a GPU');
  } else if (!ids.has(selected)) {
    fail('bootstrap.view.preferences.selectedGpuId', 'must match a discovered GPU');
  }
  return data as unknown as Bootstrap;
}

export function validateDelivery(value: unknown): Delivery {
  const data = object(value, 'delivery');
  exactKeys(data, 'delivery', [
    'contractVersion',
    'sessionId',
    'subscriptionId',
    'deliverySequence',
    'view',
  ]);
  if (numberValue(data.contractVersion, 'delivery.contractVersion') !== CONTRACT_VERSION) {
    fail('delivery.contractVersion', 'must be 1');
  }
  const sessionId = uuidValue(data.sessionId, 'delivery.sessionId');
  uuidValue(data.subscriptionId, 'delivery.subscriptionId');
  const sequence = integerValue(data.deliverySequence, 'delivery.deliverySequence');
  if (sequence === 0) fail('delivery.deliverySequence', 'must be positive');
  const view = validateView(data.view, 'delivery.view');
  if (view.sessionId !== sessionId) fail('delivery.sessionId', 'must match its view');
  return data as unknown as Delivery;
}

/** Narrow a `Reading<T>` to a valid value without inventing one. */
export function validReadingValue<T>(reading: Reading<T>): T | null {
  return reading.status === 'valid' ? reading.value : null;
}
