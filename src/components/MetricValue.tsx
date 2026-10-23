import type { Reading } from '../telemetry/types';
import { presentReading } from '../telemetry/availability';
import { missing } from '../telemetry/selectors';
const statuses = {
  valid: '',
  stale: 'Last valid',
  unsupported: 'Unsupported',
  permission_denied: 'Permission denied',
  unavailable: 'Unavailable',
};
export function MetricValue<T>({
  reading,
  format,
  interrupted = false,
  primary = false,
  testId,
}: {
  reading?: Reading<T> | undefined;
  format: (value: T) => string;
  interrupted?: boolean;
  primary?: boolean;
  testId?: string;
}) {
  const original = reading ?? missing<T>();
  const shown: Reading<T> =
    interrupted && original.status === 'valid'
      ? { ...original, status: 'stale', reason: 'Waiting for an advancing observation' }
      : original;
  const presentation = presentReading(shown, format);
  const explanation = [statuses[presentation.status], presentation.reason]
    .filter(Boolean)
    .join(' · ');
  return (
    <span
      className={`metric-value ${primary ? 'metric-primary' : ''} metric-${presentation.tone}`}
      data-testid={testId}
    >
      <span className="metric-number">{presentation.text}</span>
      {presentation.status !== 'valid' && (
        <span className="availability-note" title={explanation}>
          <span>{statuses[presentation.status]}</span>
          {presentation.status === 'stale' && original.sampledAtMs !== null && (
            <time dateTime={new Date(original.sampledAtMs).toISOString()}>
              {' '}
              · {new Date(original.sampledAtMs).toLocaleTimeString()}
            </time>
          )}
          {presentation.reason && <span className="sr-only"> · {presentation.reason}</span>}
        </span>
      )}
    </span>
  );
}
