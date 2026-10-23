import { useCallback, useRef, useState } from 'react';
import type { ViewSnapshot } from './view-store';
import type { GpuDescriptor, HistoryRange, MemoryView, RankingMetric } from '../telemetry/types';
import { BentoCard } from '../components/BentoCard';
import { CoreTelemetryCards } from '../components/CoreTelemetryCards';
import { HardwareSidebar } from '../components/HardwareSidebar';
import { HistoryControls } from '../components/HistoryControls';
import { MemoryAppCard } from '../components/MemoryAppCard';
import { MemoryTypeCard } from '../components/MemoryTypeCard';
import { ResourceLoadCard } from '../components/ResourceLoadCard';
import { StatusNotice } from '../components/StatusNotice';
import { Settings } from './Settings';
export function Overview({
  snapshot,
  gpus,
  onRangeChange,
  onPauseChange,
  onMemoryView,
  onRankingMetric,
  onSavePreferences,
  onRefreshChoices,
}: {
  snapshot: ViewSnapshot;
  gpus: readonly GpuDescriptor[];
  onRangeChange: (range: HistoryRange) => void;
  onPauseChange: (paused: boolean) => void;
  onMemoryView: (memoryView: MemoryView) => void;
  onRankingMetric: (metric: RankingMetric) => void;
  onSavePreferences: (
    intervalMs: number,
    selectedGpuId: string | null,
  ) => Promise<{ ok: boolean; persisted: boolean }>;
  onRefreshChoices: () => Promise<void>;
}) {
  const { view, interrupted, error } = snapshot;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsTrigger = useRef<HTMLButtonElement>(null);
  const sample = view?.sample;
  const gpu = sample?.scopes.find((scope) => scope.id === sample.gpu.utilizationPct.scopeId);
  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
    settingsTrigger.current?.focus();
  }, []);
  return (
    <div className={`overview-shell ${interrupted ? 'updates-interrupted' : ''}`}>
      <a className="skip-link" href="#overview-main">
        Skip to Overview
      </a>
      <HardwareSidebar view={view} interrupted={interrupted} />
      <main className="overview-main" id="overview-main" tabIndex={-1}>
        <header className="overview-header">
          <div>
            <p className="eyebrow">Your workstation, in view</p>
            <h1>Overview</h1>
            <p className="header-subtitle">
              {sample?.host.hostname ?? 'Workstation'}
              {gpu ? ` / ${gpu.label}` : ''}
            </p>
          </div>
          <div className="header-controls">
            <div className="live-state">
              <span className={interrupted ? 'live-dot dot-warning' : 'live-dot'} />
              {view?.mode === 'paused' ? 'Paused' : sample ? 'Live' : 'Waiting'}
            </div>
            <HistoryControls
              view={view}
              onRangeChange={onRangeChange}
              onPauseChange={onPauseChange}
            />
            <button
              ref={settingsTrigger}
              id="settings-trigger"
              type="button"
              className="settings-trigger"
              onClick={() => {
                void Promise.resolve(onRefreshChoices()).finally(() => setSettingsOpen(true));
              }}
              disabled={!view}
            >
              Settings
            </button>
          </div>
        </header>
        <StatusNotice
          notices={view?.notices ?? []}
          error={error}
          initializing={!sample}
          interrupted={interrupted}
        />
        <div className="overview-grid">
          <CoreTelemetryCards view={view} interrupted={interrupted} />
          <BentoCard title="Memory by Type" resource="memory" className="summary-card">
            <MemoryTypeCard view={view} onMemoryView={onMemoryView} />
          </BentoCard>
          <MemoryAppCard view={view} />
          <BentoCard title="Resource Load" resource="cpu" className="summary-card">
            <ResourceLoadCard view={view} onRankingMetric={onRankingMetric} />
          </BentoCard>
        </div>
        <footer className="overview-footer">
          <span>Local telemetry · Interface traffic and written bytes are session totals</span>
          <span>
            {sample
              ? `Observed ${new Date(sample.capturedAtMs).toLocaleTimeString()}`
              : 'Awaiting observations'}
          </span>
        </footer>
      </main>
      {settingsOpen && view && (
        <Settings view={view} gpus={gpus} onSave={onSavePreferences} onClose={closeSettings} />
      )}
    </div>
  );
}
