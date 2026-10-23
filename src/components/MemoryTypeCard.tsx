import type { MemoryBreakdown, MemorySegment, MemoryView, View } from '../telemetry/types';
import { formatBytes, formatPercent } from '../telemetry/format';

const COLORS: Record<MemorySegment['role'], string> = {
  used: 'var(--memory)',
  available: '#45454f',
  free: '#2e9f63',
  reserved: '#8b6cf0',
  unclassified: '#767682',
};

function Donut({ breakdown, label }: { breakdown: MemoryBreakdown; label: string }) {
  const total = BigInt(breakdown.totalBytes);
  const shares = breakdown.segments.map((segment) =>
    total > 0n ? Number((BigInt(segment.bytes) * 1_000_000n) / total) / 10_000 : 0,
  );
  const text = breakdown.segments
    .map((segment) => `${segment.label} ${formatBytes(segment.bytes)}`)
    .join(', ');
  return (
    <div
      className="memory-donut"
      role="img"
      aria-label={`${label}: ${formatPercent(breakdown.usedPct)} in use. ${text}`}
    >
      <svg viewBox="0 0 120 120" aria-hidden="true">
        <circle className="donut-track" cx="60" cy="60" r="48" pathLength={100} />
        {breakdown.segments.map((segment, index) => {
          const share = shares[index] ?? 0;
          const offset = shares.slice(0, index).reduce((sum, value) => sum + value, 0);
          return (
            <circle
              key={segment.id}
              cx="60"
              cy="60"
              r="48"
              pathLength={100}
              stroke={COLORS[segment.role]}
              strokeDasharray={`${share} ${100 - share}`}
              strokeDashoffset={-offset}
              transform="rotate(-90 60 60)"
            />
          );
        })}
      </svg>
      <div className="donut-center">
        <strong>{formatPercent(breakdown.usedPct)}</strong>
        <span>in use</span>
      </div>
    </div>
  );
}

export function MemoryTypeCard({
  view,
  onMemoryView,
}: {
  view: View | null;
  onMemoryView: (memoryView: MemoryView) => void;
}) {
  const memoryView = view?.selection.memoryView ?? 'ram';
  const breakdown = view?.memoryBreakdowns[memoryView] ?? null;
  return (
    <>
      <div className="breakdown-selector" role="radiogroup" aria-label="Memory type">
        {(['ram', 'vram'] as const).map((kind) => (
          <label key={kind} className="breakdown-option">
            <input
              type="radio"
              name="memory-view"
              value={kind}
              checked={memoryView === kind}
              disabled={!view}
              onChange={() => onMemoryView(kind)}
            />
            <span>{kind === 'ram' ? 'System RAM' : 'Dedicated VRAM'}</span>
          </label>
        ))}
      </div>
      {breakdown ? (
        <>
          <div className="memory-breakdown">
            <Donut
              breakdown={breakdown}
              label={memoryView === 'ram' ? 'System RAM' : 'Dedicated VRAM'}
            />
            <dl className="summary-pairs memory-legend">
              {breakdown.segments.map((segment) => (
                <div key={segment.id}>
                  <dt>
                    <span
                      className={`segment-dot segment-${segment.role}`}
                      style={{ background: COLORS[segment.role] }}
                    />
                    {segment.label}
                  </dt>
                  <dd>{formatBytes(segment.bytes)}</dd>
                </div>
              ))}
              {memoryView === 'ram' && view?.sample?.memory.cachedBytes.value != null && (
                <div title="Cached (overlapping, not a slice)">
                  <dt>
                    <span className="segment-dot segment-cached" />
                    Cached
                  </dt>
                  <dd>{formatBytes(view.sample.memory.cachedBytes.value)}</dd>
                </div>
              )}
            </dl>
          </div>
          <p className="card-note">
            {breakdown.coverage === 'partial' ? 'Partial coverage · ' : ''}
            {breakdown.note ?? `Total ${formatBytes(breakdown.totalBytes)}`}
          </p>
          {memoryView === 'ram' && view?.sample?.memory.cachedBytes.value != null && (
            <p className="sr-only">
              Cached (overlapping, not a slice):{' '}
              {formatBytes(view?.sample?.memory.cachedBytes.value ?? '0')}
            </p>
          )}
        </>
      ) : (
        <p className="summary-empty">
          {memoryView === 'vram'
            ? 'Dedicated VRAM is unavailable for the selected GPU'
            : 'RAM breakdown is unavailable'}
        </p>
      )}
      <p className="card-note">RAM and dedicated VRAM are separate memory domains.</p>
    </>
  );
}
