/**
 * Frontend-local display-mode derivations for the compact Overview presentation.
 *
 * These helpers never rewrite wire readings: raw values stay untouched for sorting, badges and
 * thresholds. They only derive a 0–100 micro-meter fill, and every byte conversion goes through
 * BigInt before the single float conversion.
 */

import type { Reading, SeriesId, View } from './types';

export type DisplayMode = 'charts' | 'compact';

const DECIMAL_BYTES = /^(0|[1-9][0-9]*)$/;

/** Clamp a derived percentage into the 0–100 meter domain. */
export function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

function validValue<T>(reading: Reading<T> | undefined): T | null {
  return reading?.status === 'valid' ? reading.value : null;
}

/** Percent and capacity metrics fill over their own 0–100 domain. */
export function percentFill(reading: Reading<number> | undefined): number {
  const value = validValue(reading);
  return value === null ? 0 : clampPercent(value);
}

/** Parse an unsigned decimal byte string without throwing on curated/unavailable input. */
export function parseByteAmount(text: string | null | undefined): bigint | null {
  return typeof text === 'string' && DECIMAL_BYTES.test(text) ? BigInt(text) : null;
}

/** Free-capacity share; both readings must be valid and the total must be positive. */
export function capacityFreeFill(
  free: Reading<string> | undefined,
  total: Reading<string> | undefined,
): number {
  const freeBytes = parseByteAmount(validValue(free));
  const totalBytes = parseByteAmount(validValue(total));
  if (freeBytes === null || totalBytes === null || totalBytes <= 0n) return 0;
  return clampPercent(Number((freeBytes * 1000n) / totalBytes) / 10);
}

/** Rate and watt metrics fill against the selected window's projected scale. */
export function scaleFill(
  reading: Reading<number> | undefined,
  scaleMax: number | undefined,
): number {
  const value = validValue(reading);
  if (value === null || scaleMax === undefined || !Number.isFinite(scaleMax) || scaleMax <= 0) {
    return 0;
  }
  return clampPercent((value / scaleMax) * 100);
}

export function chartScale(view: View | null, seriesId: SeriesId): number | undefined {
  return view?.history?.charts.find((chart) => chart.seriesId === seriesId)?.scaleMax;
}

/**
 * Battery charge fills over 0–100; GPU/CPU watts fill against the projected power scale.
 * Whole-system power is never inferred when the primary kind is unavailable.
 */
export function powerFill(view: View | null): number {
  const power = view?.sample?.power;
  if (!power) return 0;
  if (power.primaryKind === 'battery_pct') return percentFill(power.primaryValue);
  if (power.primaryKind === 'gpu_watts' || power.primaryKind === 'cpu_package_watts') {
    return scaleFill(power.primaryValue, chartScale(view, 'powerW'));
  }
  return 0;
}

/** Share of the displayed process total; null when no denominator is available. */
export function sharePercent(bytes: bigint | null, displayedTotal: bigint | null): number | null {
  if (bytes === null || displayedTotal === null || displayedTotal <= 0n) return null;
  return Number((bytes * 1000n) / displayedTotal) / 10;
}
