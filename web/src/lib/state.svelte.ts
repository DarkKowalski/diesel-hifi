/**
 * UI state, backed by Svelte 5 runes.
 *
 * All engine data — the selector entries, telemetry, and provenance — arrives
 * from the real configuration API through the worker. Nothing about the engine
 * is hard-coded here.
 */

import { SimClient, SimClientError, type ReadyInfo } from './simClient';
import {
  DEFAULT_CONTROLS,
  DEFAULT_RESET,
  DEFAULT_SWEEP,
  type ConfigSummary,
  type Controls,
  type OperatingPoint,
  type ProvenanceReport,
  type Snapshot,
  type SweepPeaks,
} from '../worker/protocol';

export type Lifecycle = 'idle' | 'loading' | 'ready' | 'error';

class SimStore {
  lifecycle = $state<Lifecycle>('idle');
  ready = $state<ReadyInfo | null>(null);
  configs = $state<ConfigSummary[]>([]);
  activeId = $state<string>('');
  snapshot = $state<Snapshot | null>(null);
  provenance = $state<ProvenanceReport | null>(null);
  controls = $state<Controls>({ ...DEFAULT_CONTROLS });
  errorMessage = $state<string>('');
  errorCode = $state<string>('');
  running = $state(false);

  // Dynamometer sweep.
  sweepPoints = $state<OperatingPoint[]>([]);
  sweepPeaks = $state<SweepPeaks | null>(null);
  sweepRunning = $state(false);
  sweepDone = $state(0);
  sweepTotal = $state(0);

  #client: SimClient | null = null;

  get activeConfig(): ConfigSummary | null {
    return this.configs.find((c) => c.id === this.activeId) ?? null;
  }

  #fail = (error: unknown): void => {
    if (error instanceof SimClientError) {
      this.errorCode = error.code;
      this.errorMessage = error.message;
    } else {
      this.errorCode = 'UI_ERROR';
      this.errorMessage = error instanceof Error ? error.message : String(error);
    }
    if (this.lifecycle !== 'ready') {
      this.lifecycle = 'error';
    }
    this.running = false;
  };

  clearError(): void {
    this.errorCode = '';
    this.errorMessage = '';
  }

  async start(): Promise<void> {
    if (this.#client !== null) return;
    this.lifecycle = 'loading';
    try {
      this.#client = new SimClient({
        onSnapshot: (snapshot) => {
          this.snapshot = snapshot;
          if (snapshot.state === 'fault') {
            this.running = false;
          }
        },
        onError: this.#fail,
        onSweepProgress: (done, total) => {
          this.sweepDone = done;
          this.sweepTotal = total;
        },
      });
      this.ready = await this.#client.init();
      this.configs = await this.#client.listConfigs();
      this.activeId = this.ready.activeId;
      this.provenance = await this.#client.provenance();
      this.lifecycle = 'ready';
    } catch (error) {
      this.#fail(error);
    }
  }

  async #withClient(action: (client: SimClient) => Promise<void>): Promise<void> {
    if (this.#client === null) return;
    try {
      await action(this.#client);
    } catch (error) {
      this.#fail(error);
    }
  }

  /** Populate the selector from the real catalog API and select by stable ID. */
  async selectConfig(id: string): Promise<void> {
    await this.#withClient(async (client) => {
      await client.selectConfig(id);
      this.activeId = id;
      this.provenance = await client.provenance();
      // A sweep belongs to the configuration it was measured on.
      this.sweepPoints = [];
      this.sweepPeaks = null;
      this.controls = { ...DEFAULT_CONTROLS };
      this.running = false;
      this.clearError();
    });
  }

  async setControls(patch: Partial<Controls>): Promise<void> {
    const next = { ...this.controls, ...patch };
    await this.#withClient(async (client) => {
      await client.setControls(next);
      this.controls = next;
    });
  }

  /** Engage the starter and enable fuelling. */
  async startEngine(): Promise<void> {
    this.clearError();
    await this.setControls({ ignition: true, starter: true });
    await this.#withClient(async (client) => {
      await client.run(true);
      this.running = true;
    });
  }

  /** Cut fuelling. The engine coasts down under friction. */
  async stopEngine(): Promise<void> {
    await this.setControls({ ignition: false, starter: false, pedal: 0 });
  }

  /**
   * Measure a full-load dynamometer sweep.
   *
   * The peak speeds this returns are an outcome of the project's calibration,
   * not published OEM data, and the UI labels them accordingly.
   */
  async runSweep(): Promise<void> {
    if (this.sweepRunning) return;
    this.sweepRunning = true;
    this.sweepDone = 0;
    this.sweepTotal = 0;
    this.clearError();
    await this.#withClient(async (client) => {
      const result = await client.sweep({ ...DEFAULT_SWEEP });
      this.sweepPoints = result.points;
      this.sweepPeaks = result.peaks;
    });
    this.sweepRunning = false;
  }

  async resetSimulation(): Promise<void> {
    await this.#withClient(async (client) => {
      await client.run(false);
      await client.reset({ ...DEFAULT_RESET });
      this.controls = { ...DEFAULT_CONTROLS };
      this.running = false;
      this.clearError();
    });
  }

  /** Disengage the starter once the engine is running on its own. */
  async releaseStarterIfRunning(): Promise<void> {
    if (this.controls.starter && (this.snapshot?.state ?? 'stopped') === 'running') {
      await this.setControls({ starter: false });
    }
  }

  destroy(): void {
    this.#client?.close();
    this.#client = null;
  }
}

export const sim = new SimStore();
