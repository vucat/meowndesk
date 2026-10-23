/** Pure display formatting. Raw values stay untouched for sorting and thresholds. */

const DECIMAL_BYTES = /^(0|[1-9][0-9]*)$/;
const U64_MAX = 18_446_744_073_709_551_615n;

export function parseByteString(text: string): bigint {
  if (!DECIMAL_BYTES.test(text)) {
    throw new RangeError('byte amounts must be canonical unsigned decimal strings');
  }
  const value = BigInt(text);
  if (value > U64_MAX) {
    throw new RangeError('byte amount exceeds the u64 domain');
  }
  return value;
}

function trimZeros(value: number, decimals: number): string {
  return value
    .toFixed(decimals)
    .replace(/\.0+$/, '')
    .replace(/(\.\d*?)0+$/, '$1');
}

/** At most one decimal for ordinary values; small nonzero rates keep a meaningful digit. */
export function formatNumber(value: number): string {
  if (!Number.isFinite(value)) return '—';
  if (value === 0) return '0';
  const magnitude = Math.abs(value);
  if (magnitude >= 100) return Math.round(value).toString();
  if (magnitude >= 0.1) return trimZeros(value, 1);
  return trimZeros(value, 3);
}

function scaled(value: bigint, scale: bigint): number {
  return Number(value / scale) + Number(value % scale) / Number(scale);
}

/**
 * 1024-based units (KB/MB/GB/TB) so RAM and VRAM match Windows Task Manager.
 * Uses BigInt so byte amounts are never rounded early.
 */
export function formatBytes(bytes: string | bigint): string {
  const value = typeof bytes === 'string' ? parseByteString(bytes) : bytes;
  if (value < 0n) throw new RangeError('byte amounts must not be negative');
  if (value < 1_024n) return `${value} B`;
  if (value < 1_048_576n) return `${formatNumber(scaled(value, 1_024n))} KB`;
  if (value < 1_073_741_824n) return `${formatNumber(scaled(value, 1_048_576n))} MB`;
  if (value < 1_099_511_627_776n) return `${formatNumber(scaled(value, 1_073_741_824n))} GB`;
  return `${formatNumber(scaled(value, 1_099_511_627_776n))} TB`;
}

/** B/s, KB/s, or MB/s with unit scaling for whole-interface rates. */
export function formatRate(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 0) return '—';
  if (bytesPerSecond < 1_000) return `${formatNumber(bytesPerSecond)} B/s`;
  if (bytesPerSecond < 1_000_000) return `${formatNumber(bytesPerSecond / 1_000)} KB/s`;
  return `${formatNumber(bytesPerSecond / 1_000_000)} MB/s`;
}

export function formatPercent(pct: number): string {
  return Number.isFinite(pct) ? `${formatNumber(pct)}%` : '—';
}

export function formatWatts(watts: number): string {
  return Number.isFinite(watts) ? `${formatNumber(watts)} W` : '—';
}

export function formatTemperature(celsius: number): string {
  return Number.isFinite(celsius) ? `${formatNumber(celsius)} °C` : '—';
}

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—';
  const total = Math.floor(seconds);
  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  const remaining = total % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${remaining}s`;
  return `${remaining}s`;
}
