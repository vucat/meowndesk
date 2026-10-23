import { useEffect, useState, useSyncExternalStore } from 'react';
import { NativeTransport, TelemetryClient, type TelemetryTransport } from './telemetry/client';
import type { HistoryRange, MemoryView, RankingMetric } from './telemetry/types';
import { Overview } from './app/Overview';
export function App({ transport }: { transport?: TelemetryTransport }) {
  const [client] = useState(() => new TelemetryClient(transport ?? new NativeTransport()));
  const snapshot = useSyncExternalStore(client.store.subscribe, client.store.getSnapshot);
  const range = (next: HistoryRange) => {
    const view = client.store.getSnapshot().view;
    if (view) void client.setView({ ...view.selection, range: next });
  };
  const pause = (paused: boolean) => void client.setPaused(paused);
  const memoryView = (next: MemoryView) => {
    const view = client.store.getSnapshot().view;
    if (view) void client.setView({ ...view.selection, memoryView: next });
  };
  const rankingMetric = (next: RankingMetric) => {
    const view = client.store.getSnapshot().view;
    if (view) void client.setView({ ...view.selection, rankingMetric: next });
  };
  useEffect(() => {
    void client.start().then(() => client.setVisible(document.visibilityState !== 'hidden'));
    const visibility = () => {
      void client.setVisible(document.visibilityState !== 'hidden');
    };
    document.addEventListener('visibilitychange', visibility);
    return () => {
      document.removeEventListener('visibilitychange', visibility);
      client.stop();
    };
  }, [client]);
  return (
    <Overview
      snapshot={snapshot}
      gpus={client.gpuChoices()}
      onRangeChange={range}
      onPauseChange={pause}
      onMemoryView={memoryView}
      onRankingMetric={rankingMetric}
      onSavePreferences={(intervalMs, selectedGpuId) =>
        client.setPreferences(intervalMs, selectedGpuId)
      }
      onRefreshChoices={() => client.refreshGpuChoices()}
    />
  );
}
