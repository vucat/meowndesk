import type { ProcessObservation, RankingMetric, View } from '../telemetry/types';
import { formatPercent, formatWatts } from '../telemetry/format';
import { scopeLabel } from '../telemetry/selectors';

const METRICS: readonly { id: RankingMetric; label: string }[] = [
  { id: 'cpu', label: 'CPU' },
  { id: 'gpu', label: 'GPU' },
  { id: 'power', label: 'Power' },
];

function listFor(summaries: View['processSummaries'], metric: RankingMetric) {
  if (!summaries) return [];
  if (metric === 'cpu') return summaries.cpuTop;
  if (metric === 'gpu') return summaries.gpuTop;
  return summaries.powerTop;
}

function valueFor(process: ProcessObservation, metric: RankingMetric): number | null {
  if (metric === 'cpu') return process.cpuPct.value;
  if (metric === 'gpu') return process.gpuPct.value;
  return process.powerW.value;
}

export function ResourceLoadCard({
  view,
  onRankingMetric,
}: {
  view: View | null;
  onRankingMetric: (metric: RankingMetric) => void;
}) {
  const summaries = view?.processSummaries;
  const supported = summaries?.supportedRankingMetrics ?? ['cpu'];
  const selected = view?.selection.rankingMetric ?? 'cpu';
  const entries = listFor(summaries ?? null, selected);
  const unit = selected === 'power' ? 'W' : '%';
  const format = selected === 'power' ? formatWatts : formatPercent;
  const values = entries
    .map((process) => valueFor(process, selected))
    .filter((value): value is number => value !== null && Number.isFinite(value));
  const max = values.length > 0 ? Math.max(...values) : 0;
  // Idle percentages must not fill the track; watts keep their own measured maximum.
  const scale = unit === '%' ? Math.max(max, 20) : max;
  const reasons = METRICS.filter((metric) => !supported.includes(metric.id)).map(
    (metric) =>
      `${metric.label}: ${metric.id === 'gpu' ? 'per-process GPU attribution is unavailable for the selected adapter' : 'measured per-process power is unavailable'}`,
  );
  return (
    <>
      <div className="breakdown-selector" role="radiogroup" aria-label="Ranking metric">
        {METRICS.map((metric) => {
          const enabled = supported.includes(metric.id);
          return (
            <label key={metric.id} className="breakdown-option">
              <input
                type="radio"
                name="ranking-metric"
                value={metric.id}
                checked={selected === metric.id}
                disabled={!view || !enabled}
                aria-describedby={!enabled ? 'ranking-unavailable' : undefined}
                onChange={() => onRankingMetric(metric.id)}
              />
              <span>{metric.label}</span>
            </label>
          );
        })}
      </div>
      {entries.length > 0 ? (
        <>
          <ul className="rank-list">
            {entries.map((process) => {
              const value = valueFor(process, selected);
              return (
                <li key={process.key}>
                  <span className="rank-badge" aria-hidden="true">
                    {process.name.slice(0, 1).toLocaleUpperCase()}
                  </span>
                  <span className="rank-label">{process.name}</span>
                  <span className="rank-value">{value === null ? '—' : format(value)}</span>
                  <span className="rank-track" aria-hidden="true">
                    <span
                      className="rank-bar"
                      style={{
                        width: scale > 0 && value !== null ? `${(value / scale) * 100}%` : '0%',
                      }}
                    />
                  </span>
                </li>
              );
            })}
          </ul>
          <p className="card-note rank-note">
            Raw{' '}
            {selected === 'cpu'
              ? 'whole-machine CPU'
              : selected === 'gpu'
                ? 'selected-adapter'
                : 'measured'}{' '}
            {unit}
            {selected === 'gpu'
              ? ` · ${scopeLabel(view, entries[0]?.gpuPct.scopeId, 'Selected GPU')}`
              : ''}
            {selected === 'power' ? ' · measured process power' : ''}
          </p>
        </>
      ) : (
        <p className="summary-empty">No readable {selected.toUpperCase()} process observations</p>
      )}
      {reasons.length > 0 && (
        <p className="card-note" id="ranking-unavailable">
          {reasons.join('; ')}
        </p>
      )}
    </>
  );
}
