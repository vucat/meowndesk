// Acceptance expectations from the wire readings. Sidebar Disk/Network show combined rates;
// sidebar Power always uses measured watts, even when the card primary is battery percent.
import type { Reading, View } from '../telemetry/types';
import { formatBytes, formatPercent, formatRate, formatWatts } from '../telemetry/format';
export function expectedSummaries(view: View): Record<string, string> {
  const sample = view.sample;
  if (!sample) return {};
  const value = <T>(reading: Reading<T>): T | null =>
    reading.status === 'valid' || reading.status === 'stale' ? reading.value : null;
  const combined = (a: Reading<number>, b: Reading<number>): string =>
    a.status === 'valid' && b.status === 'valid' && a.value !== null && b.value !== null
      ? formatRate(a.value + b.value)
      : '—';
  const cpu = value(sample.cpu.totalPct);
  const memory = value(sample.memory.usedBytes);
  const gpu = value(sample.gpu.utilizationPct);
  const power = value(sample.power.watts);
  return {
    cpu: cpu === null ? '—' : formatPercent(cpu),
    memory: memory === null ? '—' : formatBytes(memory),
    gpu: gpu === null ? '—' : formatPercent(gpu),
    disk: combined(sample.disk.readBytesPerSecond, sample.disk.writeBytesPerSecond),
    network: combined(sample.network.receiveBytesPerSecond, sample.network.sendBytesPerSecond),
    power: power === null ? '—' : formatWatts(power),
  };
}
