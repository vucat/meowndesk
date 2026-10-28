import type { ProcessObservation, ProcessSummaries, Reading, View } from '../telemetry/types';
import { parseByteAmount, sharePercent } from '../telemetry/display';
import { formatBytes, formatPercent } from '../telemetry/format';
import { missing } from '../telemetry/selectors';
import { BentoCard } from './BentoCard';
import { MetricValue } from './MetricValue';

const MAX_COLUMNS = 5;

function wholeMachineCpu(summaries: ProcessSummaries, entry: ProcessObservation): Reading<number> {
  const companion =
    summaries.cpuTop.find((process) => process.key === entry.key) ??
    summaries.cpuTop.find((process) => process.name === entry.name);
  return companion?.cpuPct ?? entry.cpuPct;
}

/**
 * Compact replacement for Memory by Type, Memory by App and Resource Load: up to five process
 * columns combining resident RAM and whole-machine CPU without inventing a composite score.
 */
export function TopProcessesCard({
  view,
  interrupted = false,
}: {
  view: View | null;
  interrupted?: boolean;
}) {
  const summaries = view?.processSummaries ?? null;
  const memoryTop = summaries?.memoryTop ?? [];
  const entries = (memoryTop.length > 0 ? memoryTop : (summaries?.cpuTop ?? [])).slice(
    0,
    MAX_COLUMNS,
  );
  const usingCpuFallback = memoryTop.length === 0;
  // The wire total covers the complete memory list (up to eight apps), even though
  // this strip draws at most five. An empty memory list has no measured total.
  const displayedTotal = usingCpuFallback
    ? null
    : parseByteAmount(summaries?.displayedMemoryTotalBytes);
  // A derived total is only as recent as its oldest contributing observation.
  const totalSource = memoryTop.reduce<Reading<string> | null>((oldest, entry) => {
    const reading = entry.residentBytes;
    return oldest === null || (reading.elapsedMs ?? Infinity) < (oldest.elapsedMs ?? Infinity)
      ? reading
      : oldest;
  }, null);
  const totalReading =
    totalSource && displayedTotal !== null
      ? { ...totalSource, value: displayedTotal.toString() }
      : missing<string>('The memory list has no readable resident observations');
  return (
    <BentoCard
      title="Top Processes"
      resource="cpu"
      className="top-processes-card"
      badge={
        <span className="top-processes-sort">
          {usingCpuFallback ? 'Sorted by CPU' : 'Sorted by memory'}
        </span>
      }
    >
      {entries.length === 0 ? (
        <p className="summary-empty">No readable process observations</p>
      ) : (
        <>
          <p className="top-processes-meta">
            Combined Memory &amp; CPU Load · Memory list total{' '}
            <span className="top-processes-total">
              <MetricValue reading={totalReading} format={formatBytes} interrupted={interrupted} />
            </span>
            {interrupted && (
              <span className="top-processes-stale"> · Last valid process readings</span>
            )}
          </p>
          <ul
            className="top-processes-strip"
            style={{ gridTemplateColumns: `repeat(${entries.length}, minmax(0, 1fr))` }}
          >
            {entries.map((entry) => {
              const cpu = summaries ? wholeMachineCpu(summaries, entry) : entry.cpuPct;
              const bytes =
                entry.residentBytes.status === 'valid'
                  ? parseByteAmount(entry.residentBytes.value)
                  : null;
              const share = sharePercent(bytes, displayedTotal);
              const shareReading: Reading<number> =
                share === null
                  ? missing<number>(
                      'A share requires a valid resident reading and a positive memory-list total',
                    )
                  : { ...entry.residentBytes, value: share };
              return (
                <li key={entry.key} className="top-process-item">
                  <span className="top-process-top">
                    <span className="top-process-name" title={entry.name}>
                      {entry.name}
                    </span>
                    <span className="top-process-cpu">
                      <MetricValue reading={cpu} format={formatPercent} interrupted={interrupted} />
                      <span className="sr-only"> whole-machine CPU</span>
                    </span>
                  </span>
                  <span className="top-process-mid">
                    <span className="top-process-ram">
                      <MetricValue
                        reading={entry.residentBytes}
                        format={formatBytes}
                        interrupted={interrupted}
                      />
                      <span className="sr-only"> resident memory</span>
                    </span>
                    <span className="top-process-share">
                      <MetricValue
                        reading={shareReading}
                        format={formatPercent}
                        interrupted={interrupted}
                      />
                      <span className="sr-only"> of memory list total</span>
                    </span>
                  </span>
                  <span className="top-process-track" aria-hidden="true">
                    <span
                      className="top-process-bar"
                      style={{ width: interrupted || share === null ? '0%' : `${share}%` }}
                    />
                  </span>
                </li>
              );
            })}
          </ul>
          <p className="card-note">
            Total and shares cover the memory list (up to eight apps); up to five are shown here.
            Shared pages may be counted more than once.
          </p>
        </>
      )}
    </BentoCard>
  );
}
