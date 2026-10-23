import type { Reading, View } from './types';
export function missing<T>(reason = 'Awaiting the first observation'): Reading<T> {
  return {
    status: 'unavailable',
    value: null,
    sampledAtMs: null,
    elapsedMs: null,
    scopeId: null,
    reason,
  };
}
export function combinedRate(a?: Reading<number>, b?: Reading<number>): Reading<number> {
  if (
    a?.status === 'valid' &&
    b?.status === 'valid' &&
    a.value !== null &&
    b.value !== null &&
    a.scopeId === b.scopeId
  ) {
    return {
      status: 'valid',
      value: a.value + b.value,
      sampledAtMs: Math.min(a.sampledAtMs!, b.sampledAtMs!),
      elapsedMs: Math.min(a.elapsedMs!, b.elapsedMs!),
      scopeId: a.scopeId,
      reason: null,
    };
  }
  const failed = [a, b].find((r) => r && r.status !== 'valid');
  return {
    ...missing<number>('Both component rates must be valid'),
    ...(failed
      ? {
          status: failed.status === 'stale' ? 'unavailable' : failed.status,
          scopeId: failed.scopeId,
          reason: failed.reason,
        }
      : {}),
  };
}
export function scopeLabel(
  view: View | null,
  id: string | null | undefined,
  fallback: string,
): string {
  return view?.sample?.scopes.find((scope) => scope.id === id)?.label ?? fallback;
}
