import { useEffect, useRef, useState } from 'react';
import type { GpuDescriptor, View } from '../telemetry/types';

const INTERVALS = [
  { value: 1000, label: '1 second' },
  { value: 2000, label: '2 seconds' },
  { value: 5000, label: '5 seconds' },
] as const;

export function Settings({
  view,
  gpus,
  onSave,
  onClose,
}: {
  view: View;
  gpus: readonly GpuDescriptor[];
  onSave: (
    intervalMs: number,
    selectedGpuId: string | null,
  ) => Promise<{
    ok: boolean;
    persisted: boolean;
  }>;
  onClose: () => void;
}) {
  const [intervalMs, setIntervalMs] = useState(view.preferences.intervalMs);
  const [selectedGpuId, setSelectedGpuId] = useState(view.preferences.selectedGpuId);
  const [saving, setSaving] = useState(false);
  const first = useRef<HTMLInputElement>(null);
  const modal = useRef<HTMLDivElement>(null);
  const paused = view.mode === 'paused';

  useEffect(() => {
    first.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
      if (event.key !== 'Tab' || !modal.current) return;
      const focusable = [
        ...modal.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), [tabindex="0"]',
        ),
      ].filter(
        (element) =>
          !(element instanceof HTMLInputElement && element.type === 'radio') || element.checked,
      );
      const firstElement = focusable[0];
      const lastElement = focusable[focusable.length - 1];
      if (!firstElement || !lastElement) return;
      if (event.shiftKey && document.activeElement === firstElement) {
        event.preventDefault();
        lastElement.focus();
      } else if (!event.shiftKey && document.activeElement === lastElement) {
        event.preventDefault();
        firstElement.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const save = async () => {
    setSaving(true);
    const hasAvailableGpu = gpus.some((gpu) => gpu.available);
    const result = await onSave(intervalMs, hasAvailableGpu ? selectedGpuId : null);
    setSaving(false);
    if (result.ok) onClose();
  };

  return (
    <div className="settings-backdrop">
      <div
        ref={modal}
        className="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
      >
        <div className="settings-heading">
          <h2>Settings</h2>
          <p>Local collection preferences only.</p>
        </div>
        <fieldset className="settings-group">
          <legend>Sampling interval</legend>
          {INTERVALS.map((option) => (
            <label key={option.value} className="settings-option">
              <input
                ref={option.value === 1000 ? first : undefined}
                type="radio"
                name="sampling-interval"
                value={option.value}
                checked={intervalMs === option.value}
                onChange={() => setIntervalMs(option.value)}
              />
              <span>{option.label}</span>
            </label>
          ))}
        </fieldset>
        <fieldset className="settings-group">
          <legend>Monitored GPU</legend>
          {gpus.length === 0 ? (
            <p className="settings-note">No GPU was discovered; CPU/RAM monitoring continues.</p>
          ) : (
            gpus
              .filter((gpu) => gpu.available)
              .map((gpu) => (
                <label key={gpu.id} className="settings-option">
                  <input
                    type="radio"
                    name="monitored-gpu"
                    value={gpu.id}
                    checked={selectedGpuId === gpu.id}
                    disabled={paused}
                    onChange={() => setSelectedGpuId(gpu.id)}
                  />
                  <span>{gpu.name}</span>
                </label>
              ))
          )}
          {paused && <p className="settings-note">Resume monitoring to change the selected GPU.</p>}
        </fieldset>
        <div className="settings-actions">
          <button type="button" className="settings-cancel" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="settings-save"
            disabled={saving}
            onClick={() => void save()}
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}
