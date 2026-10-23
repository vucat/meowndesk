import type { ReactNode } from 'react';
import type { Reading, View } from '../telemetry/types';
import { memoryBadge } from '../telemetry/availability';
import {
  formatBytes,
  formatPercent,
  formatRate,
  formatTemperature,
  formatWatts,
  formatDuration,
} from '../telemetry/format';
import { scopeLabel } from '../telemetry/selectors';
import { BentoCard } from './BentoCard';
import { MetricValue } from './MetricValue';
import { DotMatrixChart } from './DotMatrixChart';
function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="secondary-stat">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}
export function CoreTelemetryCards({
  view,
  interrupted,
}: {
  view: View | null;
  interrupted: boolean;
}) {
  const s = view?.sample;
  const chart = (id: string) => view?.history?.charts.find((c) => c.seriesId === id) ?? null;
  const metric = <T,>(reading: Reading<T> | undefined, format: (value: T) => string) => (
    <MetricValue {...(reading ? { reading } : {})} format={format} interrupted={interrupted} />
  );
  const badge =
    s?.memory.usedPct.value !== null && s?.memory.usedPct.value !== undefined
      ? memoryBadge(s.memory.usedPct.value)
      : null;
  const laptop = s?.power.primaryKind === 'battery_pct';
  const networkEmpty = view?.notices.some((n) => n.code === 'network_empty') ?? false;
  const range = view?.selection.range ?? '1m';
  return (
    <>
      <BentoCard title="CPU" resource="cpu">
        <p className="primary-label">Total usage</p>
        <MetricValue
          reading={s?.cpu.totalPct}
          format={formatPercent}
          primary
          interrupted={interrupted}
          testId="cpu-primary"
        />
        <dl className="secondary-stats">
          <Stat label="User">{metric(s?.cpu.userPct, formatPercent)}</Stat>
          <Stat label="System">{metric(s?.cpu.systemPct, formatPercent)}</Stat>
          <Stat label="Package temperature">{metric(s?.cpu.temperatureC, formatTemperature)}</Stat>
        </dl>
        <DotMatrixChart chart={chart('cpuPct')} resource="cpu" label="Total CPU" range={range} />
      </BentoCard>
      <BentoCard
        title="Memory"
        resource="memory"
        badge={
          badge ? (
            <span className={`memory-badge badge-${badge.level}`}>● {badge.label}</span>
          ) : undefined
        }
      >
        <p className="primary-label">RAM in use</p>
        <div className="capacity-primary">
          <MetricValue
            reading={s?.memory.usedBytes}
            format={formatBytes}
            primary
            interrupted={interrupted}
            testId="memory-primary"
          />
          <span className="capacity-total">
            of {metric(s?.memory.totalBytes, formatBytes)} usable
          </span>
        </div>
        <dl className="secondary-stats">
          <Stat label="Used">{metric(s?.memory.usedPct, formatPercent)}</Stat>
          <Stat label="Cached (overlapping)">{metric(s?.memory.cachedBytes, formatBytes)}</Stat>
          <Stat label="Available">{metric(s?.memory.availableBytes, formatBytes)}</Stat>
        </dl>
        <DotMatrixChart
          chart={chart('ramUsedPct')}
          resource="memory"
          label="RAM used"
          range={range}
        />
      </BentoCard>
      <BentoCard title="GPU & VRAM" resource="gpu">
        <p className="primary-label truncate-wrap">
          {scopeLabel(view, s?.gpu.utilizationPct.scopeId, 'No GPU selected')}
        </p>
        <MetricValue
          reading={s?.gpu.utilizationPct}
          format={formatPercent}
          primary
          interrupted={interrupted}
          testId="gpu-primary"
        />
        <dl className="secondary-stats">
          <Stat label="Dedicated VRAM">
            {metric(s?.gpu.vramUsedBytes, formatBytes)}
            <span className="stat-divider"> of </span>
            {metric(s?.gpu.vramTotalBytes, formatBytes)}
          </Stat>
          <Stat label="Sample avg">
            {metric(view?.history?.gpuSampleAveragePct, formatPercent)}
          </Stat>
          <Stat label="Peak">{metric(view?.history?.gpuSamplePeakPct, formatPercent)}</Stat>
        </dl>
        <DotMatrixChart chart={chart('gpuPct')} resource="gpu" label="GPU core" range={range} />
      </BentoCard>
      <BentoCard title="Disk" resource="disk">
        <p className="primary-label">
          {scopeLabel(view, s?.disk.freeBytes.scopeId, 'System volume')} · Free
        </p>
        <div className="capacity-primary">
          <MetricValue
            reading={s?.disk.freeBytes}
            format={formatBytes}
            primary
            interrupted={interrupted}
            testId="disk-primary"
          />
          <span className="capacity-total">of {metric(s?.disk.totalBytes, formatBytes)}</span>
        </div>
        <dl className="secondary-stats">
          <Stat label="Read">{metric(s?.disk.readBytesPerSecond, formatRate)}</Stat>
          <Stat label="Write">{metric(s?.disk.writeBytesPerSecond, formatRate)}</Stat>
          <Stat label="Written since launch">
            {metric(s?.disk.writtenSessionBytes, formatBytes)}
          </Stat>
        </dl>
        <DotMatrixChart
          chart={chart('diskBytesPerSecond')}
          resource="disk"
          label="Read + write"
          range={range}
        />
      </BentoCard>
      <BentoCard title="Network" resource="network">
        <p className="primary-label">Interface traffic · Download</p>
        <MetricValue
          reading={s?.network.receiveBytesPerSecond}
          format={formatRate}
          primary
          interrupted={interrupted}
          testId="network-primary"
        />
        {networkEmpty && <p className="empty-network">No active interfaces</p>}
        <dl className="secondary-stats">
          <Stat label="Upload">{metric(s?.network.sendBytesPerSecond, formatRate)}</Stat>
          <Stat label="Received since launch">
            {metric(s?.network.receivedSessionBytes, formatBytes)}
          </Stat>
          <Stat label="Sent since launch">{metric(s?.network.sentSessionBytes, formatBytes)}</Stat>
        </dl>
        <DotMatrixChart
          chart={chart('networkBytesPerSecond')}
          resource="network"
          label="Download + upload"
          range={range}
        />
      </BentoCard>
      <BentoCard title="Power/Thermals" resource="power">
        <p className="primary-label">
          {laptop
            ? 'Battery charge'
            : s?.power.primaryKind === 'gpu_watts'
              ? 'GPU board power'
              : s?.power.primaryKind === 'cpu_package_watts'
                ? 'CPU package power'
                : 'Measured power'}
        </p>
        <MetricValue
          reading={s?.power.primaryValue}
          format={laptop ? formatPercent : formatWatts}
          primary
          interrupted={interrupted}
          testId="power-primary"
        />
        <dl className="secondary-stats">
          {laptop ? (
            <>
              <Stat label="Battery state">
                {metric(s?.power.batteryState, (value) => value.replaceAll('_', ' '))}
              </Stat>
              <Stat label="Discharge power">{metric(s?.power.watts, formatWatts)}</Stat>
              <Stat
                label={
                  s?.power.batteryRemainingSeconds.status === 'valid'
                    ? 'Remaining (estimate)'
                    : 'Battery health'
                }
              >
                {s?.power.batteryRemainingSeconds.status === 'valid'
                  ? metric(s.power.batteryRemainingSeconds, formatDuration)
                  : metric(s?.power.batteryHealthPct, formatPercent)}
              </Stat>
            </>
          ) : (
            <>
              <Stat label="GPU temperature">
                {metric(s?.power.gpuTemperatureC, formatTemperature)}
              </Stat>
              <Stat label={s?.power.fanPct.status === 'valid' ? 'Fan' : 'Limit (% max)'}>
                {s?.power.fanPct.status === 'valid'
                  ? metric(s.power.fanPct, formatPercent)
                  : metric(s?.power.powerLimitPct, formatPercent)}
              </Stat>
              <Stat label="CPU package">{metric(s?.power.cpuPackageW, formatWatts)}</Stat>
            </>
          )}
        </dl>
        <DotMatrixChart
          chart={chart('powerW')}
          resource="power"
          label={
            laptop
              ? 'Battery discharge'
              : s?.power.primaryKind === 'gpu_watts'
                ? 'GPU board power'
                : 'CPU package power'
          }
          range={range}
        />
      </BentoCard>
    </>
  );
}
