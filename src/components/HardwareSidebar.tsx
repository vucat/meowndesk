import mark from '../assets/icons/meowndesk.svg';
import type { View } from '../telemetry/types';
import {
  formatBytes,
  formatDuration,
  formatPercent,
  formatRate,
  formatWatts,
} from '../telemetry/format';
import { combinedRate } from '../telemetry/selectors';
import { MetricValue } from './MetricValue';
export function HardwareSidebar({
  view,
  interrupted,
}: {
  view: View | null;
  interrupted: boolean;
}) {
  const s = view?.sample,
    host = s?.host;
  return (
    <aside className="hardware-sidebar" aria-label="Workstation summary">
      <div className="brand">
        <img src={mark} width="30" height="30" className="app-mark" alt="" />
        <strong>Meowndesk</strong>
      </div>
      <nav aria-label="Main">
        <a className="overview-link" href="#overview-main" aria-current="page">
          <span aria-hidden="true">▦</span> Overview
        </a>
      </nav>
      <div className="sidebar-resources" aria-label="Live resource summaries">
        <div className="sidebar-resource resource-cpu">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            CPU
          </span>
          <MetricValue
            reading={s?.cpu.totalPct}
            format={formatPercent}
            interrupted={interrupted}
            testId="cpu-summary"
          />
        </div>
        <div className="sidebar-resource resource-memory">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            Memory
          </span>
          <MetricValue
            reading={s?.memory.usedBytes}
            format={formatBytes}
            interrupted={interrupted}
            testId="memory-summary"
          />
        </div>
        <div className="sidebar-resource resource-gpu">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            GPU
          </span>
          <MetricValue
            reading={s?.gpu.utilizationPct}
            format={formatPercent}
            interrupted={interrupted}
            testId="gpu-summary"
          />
        </div>
        <div className="sidebar-resource resource-disk">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            Disk
          </span>
          <MetricValue
            reading={combinedRate(s?.disk.readBytesPerSecond, s?.disk.writeBytesPerSecond)}
            format={formatRate}
            interrupted={interrupted}
            testId="disk-summary"
          />
        </div>
        <div className="sidebar-resource resource-network">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            Network
          </span>
          <MetricValue
            reading={combinedRate(s?.network.receiveBytesPerSecond, s?.network.sendBytesPerSecond)}
            format={formatRate}
            interrupted={interrupted}
            testId="network-summary"
          />
        </div>
        <div className="sidebar-resource resource-power">
          <span className="sidebar-resource-label">
            <span className="summary-dot" />
            Power
          </span>
          <MetricValue
            reading={s?.power.watts}
            format={formatWatts}
            interrupted={interrupted}
            testId="power-summary"
          />
        </div>
      </div>
      <footer className="host-identity">
        <span className="host-symbol" aria-hidden="true">
          ▤
        </span>
        <strong>{host?.hostname ?? 'Workstation'}</strong>
        <span>{host ? `${host.osName} ${host.osVersion}` : 'Waiting for host information'}</span>
        <span>{host?.cpuModel ?? 'CPU model unavailable'}</span>
        {host?.environment === 'wsl2' && (
          <span className="guest-label">WSL2 guest · Guest resources only</span>
        )}
        <span className="uptime-label">
          Uptime{' '}
          {host?.uptimeSeconds === null || host?.uptimeSeconds === undefined
            ? 'unavailable'
            : formatDuration(host.uptimeSeconds)}
        </span>
      </footer>
    </aside>
  );
}
