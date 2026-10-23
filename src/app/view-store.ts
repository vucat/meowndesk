/** One immutable accepted View, shared by all Overview cards. Transport and clock are injected. */
import type { View } from '../telemetry/types';
import { validateView } from '../telemetry/validate';
import { createFreshness } from '../telemetry/freshness';
export interface ViewSnapshot {
  view: View | null;
  error: string | null;
  interrupted: boolean;
}
function freeze<T>(value: T): T {
  if (typeof value === 'object' && value !== null && !Object.isFrozen(value)) {
    Object.freeze(value);
    Object.values(value).forEach(freeze);
  }
  return value;
}
export class ViewStore {
  private snapshot: ViewSnapshot = { view: null, error: null, interrupted: false };
  private listeners = new Set<() => void>();
  private freshness = createFreshness();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private documentVisible = true;
  constructor(private readonly now: () => number = () => performance.now()) {}
  getSnapshot = (): ViewSnapshot => this.snapshot;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private emit(next: ViewSnapshot): void {
    this.snapshot = Object.freeze(next);
    this.listeners.forEach((listener) => listener());
  }
  accept(value: unknown, allowNewSession = false): boolean {
    const view = validateView(value);
    const current = this.snapshot.view;
    if (current && current.sessionId !== view.sessionId) {
      if (!allowNewSession) return false;
      this.freshness = createFreshness();
    } else if (current) {
      if (view.revision < current.revision) return false;
      if ((view.sample?.sequence ?? -1) < (current.sample?.sequence ?? -1)) return false;
      if (current.mode === 'paused' && view.mode === 'live' && view.revision <= current.revision)
        return false;
    }
    this.observe(view);
    this.emit({
      view: freeze(view),
      error: null,
      interrupted: this.freshness.interrupted(this.now()),
    });
    this.schedule();
    return true;
  }
  setDocumentVisibility(visible: boolean): void {
    this.documentVisible = visible;
    if (this.snapshot.view) this.observe(this.snapshot.view);
    this.emit({ ...this.snapshot, interrupted: this.freshness.interrupted(this.now()) });
    this.schedule();
  }
  error(message: string): void {
    this.emit({ ...this.snapshot, error: message });
  }
  stop(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
  private observe(view: View): void {
    this.freshness.observe(
      {
        sampleSequence: view.sample?.sequence ?? null,
        intervalMs: view.preferences.intervalMs,
        visible: view.visible && this.documentVisible,
        mode: view.mode,
      },
      this.now(),
    );
  }
  private schedule(): void {
    this.stop();
    const delay = this.freshness.nextCheckMs(this.now());
    if (delay !== null)
      this.timer = setTimeout(() => {
        this.timer = null;
        const interrupted = this.freshness.interrupted(this.now());
        if (interrupted !== this.snapshot.interrupted) this.emit({ ...this.snapshot, interrupted });
        this.schedule();
      }, delay);
  }
}
