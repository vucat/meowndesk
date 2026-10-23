import { expect, it } from 'vitest';
import type { Reading } from '../telemetry/types';
import { createFreshness as create } from '../telemetry/freshness';
const frame = (
  sampleSequence: number | null = 1,
  intervalMs = 1000,
  visible = true,
  mode: 'live' | 'paused' = 'live',
) => ({ sampleSequence, intervalMs, visible, mode });
it('detects total delivery silence strictly beyond two intervals', () => {
  const w = create();
  w.observe(frame(), 0);
  expect(w.interrupted(2000)).toBe(false);
  expect(w.interrupted(2001)).toBe(true);
});
it('same-sample controls and interval changes do not manufacture sample progress', () => {
  const w = create();
  w.observe(frame(), 0);
  w.observe(frame(), 1900);
  expect(w.interrupted(2001)).toBe(true);
  w.observe(frame(1, 5000), 2100);
  // The contract requires an advancing accepted sample to clear an existing warning.
  expect(w.interrupted(10000)).toBe(true);
  expect(w.interrupted(10001)).toBe(true);
  w.observe(frame(2, 5000), 10002);
  expect(w.interrupted(10002)).toBe(false);
});
it('starts initial, Resume and show observation windows and excludes pause/hide', () => {
  const w = create();
  w.observe(frame(null), 0);
  expect(w.interrupted(2001)).toBe(true);
  w.observe(frame(1), 2002);
  expect(w.interrupted(2002)).toBe(false);
  w.observe(frame(1, 1000, false), 2100);
  expect(w.interrupted(99999)).toBe(false);
  w.observe(frame(1), 100000);
  expect(w.interrupted(102000)).toBe(false);
  expect(w.interrupted(102001)).toBe(true);
  w.observe(frame(1, 1000, true, 'paused'), 102002);
  expect(w.interrupted(199999)).toBe(false);
  w.observe(frame(1), 200000);
  expect(w.interrupted(200001)).toBe(true); // Existing warning requires an advancing accepted sample.
  w.observe(frame(2), 200002);
  expect(w.interrupted(200003)).toBe(false);
});
it('adds presentation-only staleness and preserves explicit unsupported and denied readings', () => {
  const w = create();
  w.observe(frame(), 0);
  for (const status of ['unsupported', 'permission_denied', 'unavailable'] as const) {
    const r: Reading<unknown> = {
      status,
      value: null,
      sampledAtMs: null,
      elapsedMs: null,
      scopeId: null,
      reason: 'Unavailable',
    };
    expect(w.presentationStatus(r, 2001)).toBe(status);
    expect(r.status).toBe(status);
  }
  const valid: Reading<unknown> = {
    status: 'valid',
    value: 0,
    sampledAtMs: 1,
    elapsedMs: 0,
    scopeId: 'cpu',
    reason: null,
  };
  expect(w.presentationStatus(valid, 2001)).toBe('stale');
  expect(valid.status).toBe('valid');
  expect(valid.value).toBe(0);
});

it('uses changed intervals immediately before a warning, without resetting the observation clock', () => {
  const w = create();
  w.observe(frame(), 0);
  w.observe(frame(1, 5000), 1900);
  expect(w.interrupted(10000)).toBe(false);
  expect(w.interrupted(10001)).toBe(true);
  const short = create();
  short.observe(frame(1, 5000), 0);
  short.observe(frame(1, 1000), 2100);
  expect(short.interrupted(2100)).toBe(true);
});
