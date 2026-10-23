import type { HistoryRange, View } from '../telemetry/types';

const RANGES: readonly { value: HistoryRange; label: string }[] = [
  { value: '1m', label: '1m' },
  { value: '5m', label: '5m' },
  { value: '15m', label: '15m' },
  { value: '1h', label: '1h' },
];

export function HistoryControls({
  view,
  onRangeChange,
  onPauseChange,
}: {
  view: View | null;
  onRangeChange: (range: HistoryRange) => void;
  onPauseChange: (paused: boolean) => void;
}) {
  const range = view?.selection.range ?? '1m';
  const paused = view?.mode === 'paused';
  const frozenAt = paused && view ? new Date(view.endpointAtMs).toLocaleTimeString() : null;
  return (
    <div className="history-controls" id="history-controls" data-mode={paused ? 'paused' : 'live'}>
      <div className="range-control" role="radiogroup" aria-label="Time range">
        {RANGES.map((option) => (
          <label key={option.value} className="range-option">
            <input
              type="radio"
              name="history-range"
              id={`range-${option.value}`}
              value={option.value}
              checked={range === option.value}
              disabled={!view}
              onChange={() => onRangeChange(option.value)}
            />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
      <button
        type="button"
        id="history-pause"
        className="pause-control"
        aria-pressed={paused}
        disabled={!view}
        onClick={() => onPauseChange(!paused)}
      >
        {paused ? 'Resume' : 'Pause'}
      </button>
      <p
        className={paused ? 'frozen-endpoint is-frozen' : 'sr-only'}
        role="status"
        data-testid="history-status"
      >
        {frozenAt ? `Frozen at ${frozenAt}` : 'Live updates'}
      </p>
    </div>
  );
}
