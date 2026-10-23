import type { Notice } from '../telemetry/types';
export function StatusNotice({
  notices,
  error,
  initializing,
  interrupted,
}: {
  notices: Notice[];
  error: string | null;
  initializing: boolean;
  interrupted: boolean;
}) {
  return (
    <div className="status-notices" aria-live="polite" aria-atomic="true">
      {initializing && !error && (
        <p className="status-notice" role="status">
          Initializing monitoring…
        </p>
      )}
      {error && (
        <p className="status-notice notice-warning" role="status">
          {error}
        </p>
      )}
      {interrupted && (
        <p className="status-notice notice-warning" role="status">
          Live updates interrupted
        </p>
      )}
      {notices
        .filter((n) => n.code !== 'network_empty')
        .map((notice) => (
          <p
            className={`status-notice ${notice.code === 'baseline_degraded' ? 'notice-warning' : ''}`}
            role="status"
            key={notice.code}
          >
            {notice.message}
          </p>
        ))}
    </div>
  );
}
