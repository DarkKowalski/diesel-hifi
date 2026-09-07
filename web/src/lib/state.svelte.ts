/**
 * UI state, backed by Svelte 5 runes.
 *
 * All engine data — the selector entries, telemetry, and provenance — arrives
 * from the real configuration API through the worker. Nothing about the engine
 * is hard-coded here.
 */

import { AudioEngine, type AudioPath, type AudioStage, type AudioStatus } from './audioEngine';
import { AUDIO_PATHS } from './cabin';
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

  // Exhaust audio. Starts only from an explicit user action (README "WASM and worker
  // API").
  audioRunning = $state(false);
  audioBuffered = $state(0);
  audioUnderruns = $state(0);
  audioReceived = $state(0);
  audioDeviceRateHz = $state(0);
  audioSampleRateHz = $state(0);
  audioVolume = $state(0.6);
  audioStarting = $state(false);
  /**
   * Listening position. `raw` is the tailpipe signal the solver produces;
   * `cockpit` filters it for the cab. Presentation only — it changes nothing
   * about the simulation, and the snapshot's own level reading is unaffected.
   */
  audioStage = $state<AudioStage>('cockpit');
  /**
   * Which radiating paths are audible.
   *
   * A listening control, like the stage: the solver produces all three whatever
   * this says, and muting one changes no state. It is here because the balance
   * between the paths is what decides whether the engine sounds like a truck,
   * and before they crossed the boundary separately the only way to hear one was
   * to edit the engine configuration and rebuild. The balance itself is still
   * calibrated against `audio_probe` rather than by ear.
   */
  audioPaths = $state<Record<AudioPath, boolean>>({
    exhaust: true,
    block: true,
    body: true,
  });

  #client: SimClient | null = null;
  #audio: AudioEngine | null = null;

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
        onAudio: (samples, paths, sampleRateHz) => {
          this.audioSampleRateHz = sampleRateHz;
          // Hand the block straight on; the engine transfers it to the worklet.
          this.#audio?.push(samples, paths);
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
    // Merge and apply locally *before* the round trip, not after.
    //
    // The controls are a whole struct, so every patch has to send the other
    // fields too. Waiting for the worker to answer before recording the change
    // means two inputs moved in quick succession — a pedal and a load, which is
    // exactly how a truck is driven away — both build their message from the
    // same stale copy, and the second silently undoes the first. The window is
    // one worker round trip, and it widens whenever the worker is busy.
    //
    // A failed call surfaces as an error rather than as a silently reverted
    // control, which is the better of the two failures.
    const next = { ...this.controls, ...patch };
    this.controls = next;
    await this.#withClient(async (client) => {
      await client.setControls(next);
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

  /**
   * Start exhaust audio.
   *
   * Must be called from a user gesture handler: README "WASM and worker API" requires
   * an
   * explicit user action before Web Audio starts, and browsers enforce it.
   */
  async enableAudio(): Promise<void> {
    if (this.audioRunning || this.audioStarting) return;
    this.audioStarting = true;
    this.clearError();
    try {
      const engine = new AudioEngine((status: AudioStatus) => {
        this.audioRunning = status.running;
        this.audioBuffered = status.buffered;
        this.audioUnderruns = status.underruns;
        this.audioReceived = status.received;
        this.audioDeviceRateHz = status.deviceRateHz;
      });
      engine.setVolume(this.audioVolume);
      // Choose the stage before the graph exists, so the chosen one is in place
      // the moment sound starts rather than crossfading in after it.
      engine.setStage(this.audioStage);
      for (const path of AUDIO_PATHS) {
        engine.setPathEnabled(path, this.audioPaths[path]);
      }
      // The solver's own rate; the worklet resamples from it to the device.
      const sourceRateHz = this.audioSampleRateHz || 1 / (this.ready?.fixedStepS ?? 0.000025);
      await engine.start(sourceRateHz);
      this.#audio = engine;
      await this.#withClient(async (client) => {
        await client.setAudio(true);
      });
    } catch (error) {
      this.#fail(error);
    } finally {
      this.audioStarting = false;
    }
  }

  async disableAudio(): Promise<void> {
    await this.#withClient(async (client) => {
      await client.setAudio(false);
    });
    await this.#audio?.stop();
    this.#audio = null;
    this.audioRunning = false;
    this.audioBuffered = 0;
  }

  /** Current output spectrum, for the verification view. Null when silent. */
  readAudioSpectrum(): { magnitudes: Uint8Array; binHz: number } | null {
    return this.#audio?.readSpectrum() ?? null;
  }

  /** Level actually leaving the graph, in dBFS. Null when silent. */
  readAudioOutputLevelDb(): number | null {
    return this.#audio?.readOutputLevelDb() ?? null;
  }

  setAudioVolume(volume: number): void {
    this.audioVolume = volume;
    this.#audio?.setVolume(volume);
  }

  /** Switch listening position. Takes effect immediately, crossfaded. */
  setAudioStage(stage: AudioStage): void {
    this.audioStage = stage;
    this.#audio?.setStage(stage);
  }

  /** Silence or restore one radiating path. Ramped, so it does not click. */
  setAudioPath(path: AudioPath, enabled: boolean): void {
    this.audioPaths = { ...this.audioPaths, [path]: enabled };
    this.#audio?.setPathEnabled(path, enabled);
  }

  async resetSimulation(): Promise<void> {
    await this.#withClient(async (client) => {
      await client.run(false);
      await client.reset({ ...DEFAULT_RESET });
      this.controls = { ...DEFAULT_CONTROLS };
      this.running = false;
      // Buffered samples belong to the run that produced them.
      this.#audio?.flush();
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
    void this.#audio?.stop();
    this.#audio = null;
  }
}

export const sim = new SimStore();
