/** Production command/Channel lifecycle. Tests supply a browser-only transport through App props. */
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type {
  Bootstrap,
  CommandResult,
  Delivery,
  GpuDescriptor,
  View,
  ViewSelection,
} from './types';
import { validateBootstrap, validateDelivery } from './validate';
import { ViewStore } from '../app/view-store';
type Result<T> = CommandResult<T> | Promise<CommandResult<T>>;
export interface TelemetryTransport {
  bootstrap(): Result<Bootstrap>;
  subscribe(
    onView: (delivery: Delivery) => void,
  ): Result<{ subscriptionId: string; sessionId: string }>;
  ack(sequence: number, subscriptionId?: string): Result<{ accepted: boolean }>;
  unsubscribe(subscriptionId?: string): Result<{ removed: boolean }>;
  setVisible(visible: boolean): Result<{ view: View }>;
  setView(selection: ViewSelection): Result<{ view: View }>;
  setPaused(paused: boolean): Result<{ view: View }>;
  setPreferences(
    intervalMs: number,
    selectedGpuId: string | null,
  ): Result<{ view: View; persisted: boolean }>;
}
export class NativeTransport implements TelemetryTransport {
  private subscriptionId: string | undefined;
  private channel: Channel<Delivery> | null = null;
  async bootstrap(): Promise<CommandResult<Bootstrap>> {
    if (!isTauri())
      return {
        ok: false,
        error: {
          code: 'internal',
          message: 'Hardware telemetry is unavailable in this browser',
          retryable: false,
        },
      };
    return invoke('bootstrap');
  }
  async subscribe(
    onView: (delivery: Delivery) => void,
  ): Promise<CommandResult<{ subscriptionId: string; sessionId: string }>> {
    const channel = new Channel<Delivery>();
    channel.onmessage = onView;
    const result = await invoke<CommandResult<{ subscriptionId: string; sessionId: string }>>(
      'subscribe_telemetry',
      { onView: channel },
    );
    if (result.ok) {
      this.subscriptionId = result.data.subscriptionId;
      this.channel = channel;
    }
    return result;
  }
  ack(
    sequence: number,
    subscriptionId = this.subscriptionId,
  ): Promise<CommandResult<{ accepted: boolean }>> {
    return invoke('ack_telemetry', { subscriptionId, deliverySequence: sequence });
  }
  async unsubscribe(
    subscriptionId = this.subscriptionId,
  ): Promise<CommandResult<{ removed: boolean }>> {
    if (!subscriptionId) return { ok: true, data: { removed: false } };
    try {
      return await invoke<CommandResult<{ removed: boolean }>>('unsubscribe_telemetry', {
        subscriptionId,
      });
    } finally {
      if (subscriptionId === this.subscriptionId) {
        this.subscriptionId = undefined;
        if (this.channel) this.channel.onmessage = () => {};
        this.channel = null;
      }
    }
  }
  setVisible(visible: boolean): Promise<CommandResult<{ view: View }>> {
    return invoke('set_delivery_visibility', { visible });
  }
  setView(selection: ViewSelection): Promise<CommandResult<{ view: View }>> {
    return invoke('set_view', { selection });
  }
  setPaused(paused: boolean): Promise<CommandResult<{ view: View }>> {
    return invoke('set_paused', { paused });
  }
  setPreferences(
    intervalMs: number,
    selectedGpuId: string | null,
  ): Promise<CommandResult<{ view: View; persisted: boolean }>> {
    return invoke('set_preferences', { intervalMs, selectedGpuId });
  }
}
class MonitoringError extends Error {}
function data<T>(result: CommandResult<T>): T {
  if (!result.ok) throw new MonitoringError(result.error.message);
  return result.data;
}
function incompatible(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    'contractVersion' in value &&
    value.contractVersion !== 1
  );
}
export class TelemetryClient {
  readonly store: ViewStore;
  private gpus: GpuDescriptor[] = [];
  private generation = 0;
  private subscription: { subscriptionId: string; sessionId: string } | null = null;
  private lastDelivery = 0;
  private needsReconnect = false;
  private incompatible = false;
  private active = false;
  private recovery: { generation: number; promise: Promise<void> } | null = null;
  private refreshSequence = 0;
  constructor(
    private readonly transport: TelemetryTransport,
    store = new ViewStore(),
  ) {
    this.store = store;
  }
  async start(): Promise<void> {
    const generation = ++this.generation;
    this.active = true;
    this.incompatible = false;
    try {
      const bootstrap = data(await this.transport.bootstrap());
      if (!this.active || generation !== this.generation) return;
      if (incompatible(bootstrap.view)) {
        this.incompatible = true;
        throw new MonitoringError('Incompatible app version. Restart Meowndesk.');
      }
      validateBootstrap(bootstrap);
      this.gpus = bootstrap.gpus;
      this.store.accept(bootstrap.view, true);
      await this.connect(generation);
    } catch (error) {
      if (this.active && generation === this.generation) {
        this.needsReconnect = !this.incompatible;
        this.store.error(
          error instanceof MonitoringError ? error.message : 'Monitoring could not start',
        );
      }
    }
  }
  private async connect(generation: number): Promise<void> {
    if (this.incompatible || !this.current(generation)) return;
    const subscription = data(
      await this.transport.subscribe((delivery) => {
        void this.receive(delivery, generation);
      }),
    );
    if (!this.current(generation)) {
      await Promise.resolve(this.transport.unsubscribe(subscription.subscriptionId)).catch(
        () => {},
      );
      return;
    }
    if (subscription.sessionId !== this.store.getSnapshot().view?.sessionId) {
      await this.transport.unsubscribe(subscription.subscriptionId);
      throw new MonitoringError('The telemetry session changed. Reopen the window.');
    }
    this.subscription = subscription;
    this.lastDelivery = 0;
    this.needsReconnect = false;
    if (!data(await this.transport.ack(0, subscription.subscriptionId)).accepted) {
      throw new MonitoringError('The telemetry subscription could not start');
    }
  }
  private current(generation: number): boolean {
    return this.active && !this.incompatible && generation === this.generation;
  }
  /** One retry per user action, shared by overlapping controls; never a background retry loop. */
  private async recover(generation: number): Promise<void> {
    if (
      !this.current(generation) ||
      this.incompatible ||
      !this.needsReconnect ||
      !this.store.getSnapshot().view?.visible
    )
      return;
    if (this.recovery?.generation === generation) return this.recovery.promise;
    const promise = (async () => {
      const old = this.subscription;
      this.subscription = null;
      if (old) {
        // A closed backend subscription must not prevent its replacement.
        try {
          await this.transport.unsubscribe(old.subscriptionId);
        } catch {
          // subscribe replaces the old backend owner even if unsubscribe could not reach it.
        }
      }
      if (!this.current(generation) || this.incompatible) return;
      try {
        await this.connect(generation);
      } catch (error) {
        if (this.current(generation)) this.needsReconnect = true;
        throw error;
      }
    })();
    this.recovery = { generation, promise };
    try {
      await promise;
    } finally {
      if (this.recovery?.promise === promise) this.recovery = null;
    }
  }
  private async rejectVersion(): Promise<void> {
    this.incompatible = true;
    this.needsReconnect = false;
    this.store.error('Incompatible app version. Restart Meowndesk.');
    const subscription = this.subscription;
    this.subscription = null;
    if (subscription)
      await Promise.resolve(this.transport.unsubscribe(subscription.subscriptionId)).catch(
        () => {},
      );
  }
  private async receive(raw: Delivery, generation: number): Promise<void> {
    const subscription = this.subscription;
    if (!this.active || generation !== this.generation || !subscription) return;
    if (raw === null || typeof raw !== 'object') {
      this.store.error('A telemetry update could not be validated');
      return;
    }
    if (incompatible(raw) || incompatible(raw.view)) {
      await this.rejectVersion();
      return;
    }
    try {
      const delivery = validateDelivery(raw);
      if (new TextEncoder().encode(JSON.stringify(delivery)).byteLength > 256 * 1024)
        throw new Error('The telemetry view exceeded its delivery limit');
      if (
        delivery.subscriptionId === subscription.subscriptionId &&
        delivery.sessionId === subscription.sessionId &&
        delivery.deliverySequence > this.lastDelivery
      ) {
        this.store.accept(delivery.view);
        this.lastDelivery = delivery.deliverySequence;
      }
    } catch {
      this.store.error('A telemetry update could not be validated');
    } finally {
      // Rejected old views still release their own credit; they never grant another owner's credit.
      if (
        Number.isSafeInteger(raw.deliverySequence) &&
        raw.deliverySequence > 0 &&
        typeof raw.subscriptionId === 'string'
      ) {
        try {
          const result = await this.transport.ack(raw.deliverySequence, raw.subscriptionId);
          if (
            (!result.ok || !result.data.accepted) &&
            this.current(generation) &&
            raw.subscriptionId === this.subscription?.subscriptionId
          )
            this.needsReconnect = true;
        } catch {
          if (this.current(generation) && raw.subscriptionId === this.subscription?.subscriptionId)
            this.needsReconnect = true;
        }
      }
    }
  }
  async setVisible(visible: boolean): Promise<void> {
    this.store.setDocumentVisibility(visible);
    if (!this.active || this.incompatible || !this.store.getSnapshot().view) return;
    const generation = this.generation;
    try {
      const response = data(await this.transport.setVisible(visible));
      if (!this.current(generation)) return;
      this.store.accept(response.view);
      if (visible) await this.recover(generation);
    } catch {
      if (!this.current(generation)) return;
      this.needsReconnect = true;
      this.store.error('Monitoring visibility could not be updated');
    }
  }
  async setView(selection: ViewSelection): Promise<void> {
    if (!this.active || this.incompatible || !this.store.getSnapshot().view) return;
    const generation = this.generation;
    try {
      const response = data(await this.transport.setView(selection));
      if (!this.current(generation)) return;
      this.store.accept(response.view);
      await this.recover(generation);
    } catch (error) {
      if (!this.current(generation)) return;
      this.store.error(
        error instanceof MonitoringError ? error.message : 'The time range could not be changed',
      );
    }
  }
  async setPaused(paused: boolean): Promise<void> {
    if (!this.active || this.incompatible || !this.store.getSnapshot().view) return;
    const generation = this.generation;
    try {
      const response = data(await this.transport.setPaused(paused));
      if (!this.current(generation)) return;
      this.store.accept(response.view);
      await this.recover(generation);
    } catch (error) {
      if (!this.current(generation)) return;
      this.store.error(
        error instanceof MonitoringError ? error.message : 'The pause state could not be changed',
      );
    }
  }
  gpuChoices(): readonly GpuDescriptor[] {
    return this.gpus;
  }
  /** Refresh adapter choices on demand; discovery may finish after the first bootstrap. */
  async refreshGpuChoices(): Promise<void> {
    if (!this.active || this.incompatible) return;
    const generation = this.generation;
    const refresh = ++this.refreshSequence;
    try {
      const bootstrap = data(await this.transport.bootstrap());
      if (!this.current(generation) || refresh !== this.refreshSequence || this.incompatible)
        return;
      if (incompatible(bootstrap.view)) {
        await this.rejectVersion();
        return;
      }
      validateBootstrap(bootstrap);
      if (bootstrap.view.sessionId !== this.store.getSnapshot().view?.sessionId) return;
      if (this.store.accept(bootstrap.view)) this.gpus = bootstrap.gpus;
    } catch {
      // Keep the last known choices; the Settings dialog still reports their availability.
    }
  }
  async setPreferences(
    intervalMs: number,
    selectedGpuId: string | null,
  ): Promise<{ ok: boolean; persisted: boolean }> {
    if (!this.active || this.incompatible || !this.store.getSnapshot().view)
      return { ok: false, persisted: false };
    const generation = this.generation;
    try {
      const response = data(await this.transport.setPreferences(intervalMs, selectedGpuId));
      if (!this.current(generation)) return { ok: false, persisted: response.persisted };
      this.store.accept(response.view);
      await this.recover(generation);
      return { ok: true, persisted: response.persisted };
    } catch (error) {
      if (!this.current(generation)) return { ok: false, persisted: false };
      this.store.error(
        error instanceof MonitoringError ? error.message : 'Collection settings could not be saved',
      );
      return { ok: false, persisted: false };
    }
  }
  stop(): void {
    this.active = false;
    ++this.generation;
    this.store.stop();
    const old = this.subscription;
    this.subscription = null;
    if (old) void Promise.resolve(this.transport.unsubscribe(old.subscriptionId)).catch(() => {});
  }
}
