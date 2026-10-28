import type { DisplayMode } from '../telemetry/display';
import type { HistoryRange, View } from '../telemetry/types';

const RANGES: readonly { value: HistoryRange; label: string }[] = [
  { value: '1m', label: '1m' },
  { value: '5m', label: '5m' },
  { value: '15m', label: '15m' },
  { value: '1h', label: '1h' },
];

const DISPLAY_MODES: readonly { value: DisplayMode; label: string }[] = [
  { value: 'charts', label: 'Charts' },
  { value: 'compact', label: 'Compact' },
];

export function HistoryControls({
  view,
  displayMode,
  onDisplayMode,
  onRangeChange,
  onPauseChange,
}: {
  view: View | null;
  displayMode: DisplayMode;
  onDisplayMode: (mode: DisplayMode) => void;
  onRangeChange: (range: HistoryRange) => void;
  onPauseChange: (paused: boolean) => void;
}) {
  const range = view?.selection.range ?? '1m';
  const paused = view?.mode === 'paused';
  const frozenAt = paused && view ? new Date(view.endpointAtMs).toLocaleTimeString() : null;
  return (
    <div className="history-controls" id="history-controls" data-mode={paused ? 'paused' : 'live'}>
      {displayMode === 'charts' && (
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
      )}
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
      <div className="display-mode-control" role="radiogroup" aria-label="Display mode">
        {DISPLAY_MODES.map((option) => (
          <label key={option.value} className="display-mode-option">
            <input
              type="radio"
              name="display-mode"
              id={`display-mode-${option.value}`}
              value={option.value}
              checked={displayMode === option.value}
              onChange={() => onDisplayMode(option.value)}
            />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
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
