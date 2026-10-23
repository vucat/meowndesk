/** Monotonic presentation state; received readings and frozen snapshots are never rewritten. */
import type { Mode, Reading, ReadingStatus } from './types';
export interface FreshnessInput {
  sampleSequence: number | null;
  intervalMs: number;
  visible: boolean;
  mode: Mode;
}
export function createFreshness() {
  let sequence: number | null = null;
  let windowAt: number | null = null;
  let active = false;
  let interval = 1000;
  let warning = false;
  function interrupted(now: number): boolean {
    if (!active) return false;
    if (windowAt !== null && now - windowAt > interval * 2) warning = true;
    return warning;
  }
  return {
    observe(input: FreshnessInput, now: number): void {
      const nextActive = input.visible && input.mode === 'live';
      const advanced =
        input.sampleSequence !== null && (sequence === null || input.sampleSequence > sequence);
      if (windowAt === null || (!active && nextActive) || advanced) windowAt = now;
      if (advanced) {
        sequence = input.sampleSequence;
        warning = false;
      }
      interval = input.intervalMs;
      active = nextActive;
    },
    interrupted,
    nextCheckMs(now: number): number | null {
      if (!active || interrupted(now)) return null;
      return Math.max(1, (windowAt ?? now) + interval * 2 + 1 - now);
    },
    presentationStatus(reading: Reading<unknown>, now: number): ReadingStatus {
      return reading.status === 'valid' && interrupted(now) ? 'stale' : reading.status;
    },
  };
}
