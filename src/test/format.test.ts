import { describe, expect, it } from 'vitest';
import { memoryBadge, presentReading } from '../telemetry/availability';
import {
  formatBytes,
  formatDuration,
  formatPercent,
  formatRate,
  formatTemperature,
  formatWatts,
  parseByteString,
} from '../telemetry/format';
import type { Reading } from '../telemetry/types';

const valid = <T>(value: T): Reading<T> => ({
  status: 'valid',
  value,
  sampledAtMs: 1761091199000,
  elapsedMs: 63000,
  scopeId: 'host:test',
  reason: null,
});

describe('1024-based byte and rate formatting', () => {
  it('uses binary units without collapsing a nonzero amount', () => {
    expect(formatBytes(0n)).toBe('0 B');
    expect(formatBytes(999n)).toBe('999 B');
    expect(formatBytes(1024n)).toBe('1 KB');
    expect(formatBytes('1500000')).toBe('1.4 MB');
    expect(formatBytes('734003200')).toBe('700 MB');
    expect(formatBytes('12025908429')).toBe('11.2 GB');
    expect(formatBytes('34359738368')).toBe('32 GB');
  });

  it('matches Windows Task Manager RAM and VRAM labels', () => {
    expect(formatBytes('68719476736')).toBe('64 GB');
    expect(formatBytes('12884901888')).toBe('12 GB');
  });

  it('formats rates and percentages with meaningful precision', () => {
    expect(formatRate(0)).toBe('0 B/s');
    expect(formatRate(0.05)).toBe('0.05 B/s');
    expect(formatRate(999)).toBe('999 B/s');
    expect(formatRate(12500)).toBe('12.5 KB/s');
    expect(formatRate(1250000.5)).toBe('1.3 MB/s');
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(42.5)).toBe('42.5%');
    expect(formatPercent(100)).toBe('100%');
  });

  it('formats units and durations for labels', () => {
    expect(formatWatts(88.25)).toBe('88.3 W');
    expect(formatTemperature(-5)).toBe('-5 °C');
    expect(formatDuration(9)).toBe('9s');
    expect(formatDuration(65)).toBe('1m 5s');
    expect(formatDuration(3660)).toBe('1h 1m');
    expect(formatDuration(90000)).toBe('1d 1h');
    expect(formatDuration(Number.NaN)).toBe('—');
  });

  it('rejects noncanonical byte strings before formatting', () => {
    for (const invalid of ['', '01', '-1', '1.5', '1 ', '18446744073709551616']) {
      expect(() => formatBytes(invalid), invalid).toThrow();
    }
    expect(parseByteString('18446744073709551615')).toBe(18446744073709551615n);
  });
});

describe('availability presentation', () => {
  it('distinguishes a measured zero, unavailable, and stale last-valid values', () => {
    const zero = presentReading(valid(0), formatPercent);
    expect(zero).toMatchObject({ text: '0%', tone: 'value', isZero: true });

    const missing = presentReading<number>(
      {
        status: 'unavailable',
        value: null,
        sampledAtMs: null,
        elapsedMs: null,
        scopeId: 'host:test',
        reason: 'Warm-up',
      },
      formatPercent,
    );
    expect(missing).toMatchObject({ text: '—', tone: 'missing', reason: 'Warm-up', isZero: false });

    const stale = presentReading(
      {
        status: 'stale',
        value: 42.5,
        sampledAtMs: 1761091198000,
        elapsedMs: 62000,
        scopeId: 'host:test',
        reason: 'No newer observation',
      },
      formatPercent,
    );
    expect(stale).toMatchObject({ text: '42.5%', tone: 'stale' });
  });

  it('applies badge thresholds to raw percentages', () => {
    expect(memoryBadge(79.99).level).toBe('normal');
    expect(memoryBadge(80).level).toBe('elevated');
    expect(memoryBadge(89.99).level).toBe('elevated');
    expect(memoryBadge(90).level).toBe('critical');
    expect(memoryBadge(79.99).label).toBe('Normal');
  });
});
