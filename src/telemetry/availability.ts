/** Presentation of per-field availability without inventing values. */

import type { Reading, ReadingStatus } from './types';

export type PresentationTone = 'value' | 'stale' | 'missing';

export interface ReadingPresentation {
  text: string;
  tone: PresentationTone;
  status: ReadingStatus;
  reason: string | null;
  isZero: boolean;
}

function isZeroValue(value: unknown): boolean {
  return value === 0 || value === '0' || value === 0n;
}

/**
 * A valid zero stays a value; stale keeps its last valid value muted; every other state shows an
 * em dash with the curated reason.
 */
export function presentReading<T>(
  reading: Reading<T>,
  format: (value: T) => string,
): ReadingPresentation {
  const hasValue = reading.value !== null;
  if (hasValue && (reading.status === 'valid' || reading.status === 'stale')) {
    return {
      text: format(reading.value as T),
      tone: reading.status === 'stale' ? 'stale' : 'value',
      status: reading.status,
      reason: reading.reason,
      isZero: isZeroValue(reading.value),
    };
  }
  return {
    text: '—',
    tone: 'missing',
    status: reading.status,
    reason: reading.reason,
    isZero: false,
  };
}

export type MemoryBadgeLevel = 'normal' | 'elevated' | 'critical';

export interface MemoryBadge {
  level: MemoryBadgeLevel;
  label: string;
}

/** Thresholds use the raw percent before display rounding. */
export function memoryBadge(usedPct: number): MemoryBadge {
  if (usedPct >= 90) return { level: 'critical', label: 'Critical' };
  if (usedPct >= 80) return { level: 'elevated', label: 'Elevated' };
  return { level: 'normal', label: 'Normal' };
}

export function isLastValidStale(reading: Reading<unknown>): boolean {
  return reading.status === 'stale' && reading.value !== null;
}
