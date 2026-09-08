/** Presentation state. Physics and validation stay behind the worker boundary. */
import { AudioEngine, type AudioStage } from './audioEngine';
import { SimClient, SimClientError, type ReadyInfo } from './simClient';
import { DisplaySampler } from './displaySampler';
import { DEFAULT_CONTROLS, DEFAULT_RESET, type ConfigSummary, type Controls, type Snapshot } from '../worker/protocol';

export type Lifecycle = 'idle' | 'loading' | 'ready' | 'error';

class SimStore {
  lifecycle = $state<Lifecycle>('idle');
  ready = $state<ReadyInfo | null>(null);
  configs = $state<ConfigSummary[]>([]);
  activeId = $state('');
  snapshot = $state<Snapshot | null>(null);
  readout = $state<Snapshot | null>(null);
  controls = $state<Controls>({ ...DEFAULT_CONTROLS });
  errorCode = $state('');
  running = $state(false);
  busy = $state(false);
  speedLimitPaused = $state(false);
  audioRunning = $state(false);
  audioStarting = $state(false);
  audioError = $state(false);
  audioRequested = $state(false);
  audioVolume = $state(1);
  audioStage = $state<AudioStage>('cockpit');

  #client: SimClient | null = null;
  #audio: AudioEngine | null = null;
  #display = new DisplaySampler();

  get activeConfig(): ConfigSummary | null {
    return this.configs.find((config) => config.id === this.activeId) ?? null;
  }

  #fail = (error: unknown): void => {
    this.errorCode = error instanceof SimClientError ? error.code : 'UI_ERROR';
    if (this.errorCode === 'SPEED_LIMIT_EXCEEDED') this.speedLimitPaused = true;
    if (this.lifecycle !== 'ready' || this.errorCode === 'WORKER_CRASHED') this.lifecycle = 'error';
    this.running = false;
  };

  async start(): Promise<void> {
    if (this.#client) return;
    this.lifecycle = 'loading';
    try {
      this.#client = new SimClient({
        onSnapshot: (snapshot) => {
          this.snapshot = snapshot;
          const display = this.#display.push(snapshot, performance.now());
          if (display) this.readout = display;
          if (snapshot.state === 'fault') this.running = false;
        },
        onError: this.#fail,
        onAudio: (samples, paths) => this.#audio?.push(samples, paths),
      });
      this.ready = await this.#client.init();
      this.configs = await this.#client.listConfigs();
      this.activeId = this.ready.activeId;
      this.lifecycle = 'ready';
    } catch (error) { this.#fail(error); }
  }

  async #withClient(action: (client: SimClient) => Promise<void>): Promise<boolean> {
    if (!this.#client) return false;
    try { await action(this.#client); return true; }
    catch (error) { this.#fail(error); return false; }
  }

  async selectConfig(id: string): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    await this.#withClient(async (client) => {
      await client.selectConfig(id);
      this.activeId = id;
      this.#clearSession();
    });
    this.busy = false;
  }

  async setControls(patch: Partial<Controls>): Promise<boolean> {
    // Record the merge before awaiting so rapid controls never overwrite each other.
    const next = { ...this.controls, ...patch };
    this.controls = next;
    return this.#withClient(async (client) => { await client.setControls(next); });
  }

  async startEngine(): Promise<void> {
    if (this.lifecycle !== 'ready' || this.busy || this.speedLimitPaused || this.snapshot?.state === 'fault') return;
    this.busy = true;
    await this.enableAudio();
    if (await this.setControls({ ignition: true, starter: true })) {
      await this.#withClient(async (client) => { await client.run(true); this.running = true; this.errorCode = ''; });
    }
    this.busy = false;
  }

  async stopEngine(): Promise<void> {
    await this.setControls({ ignition: false, starter: false, pedal: 0 });
  }

  async resumeSimulation(): Promise<void> {
    if (!this.speedLimitPaused || this.busy) return;
    this.busy = true;
    // The recovery button explicitly confirms this input change, not a session reset.
    this.controls = { ...this.controls, roadGradePercent: 0 };
    await this.#withClient(async (client) => {
      await client.setControls({ ...this.controls });
      await client.setAudio(this.audioRunning);
      this.#audio?.flush();
      await client.run(true);
      this.running = true;
      this.speedLimitPaused = false;
      this.errorCode = '';
    });
    this.busy = false;
  }

  /** Called by Start engine or Resume sound while a browser gesture is active. */
  async enableAudio(): Promise<void> {
    if (this.audioRunning || this.audioStarting || !this.ready) return;
    this.audioStarting = true;
    this.audioRequested = true;
    this.audioError = false;
    // Dispose a context suspended by the device before starting a new one.
    const previous = this.#audio;
    const engine = new AudioEngine((status) => { if (this.#audio === engine) this.audioRunning = status.running; });
    this.#audio = engine;
    try {
      if (previous) void previous.stop();
      engine.setVolume(this.audioVolume);
      engine.setStage(this.audioStage);
      await engine.start(1 / this.ready.fixedStepS);
      if (!await this.#withClient(async (client) => { await client.setAudio(true); })) throw new Error('Audio stream unavailable');
    } catch {
      await engine.stop();
      this.#audio = null;
      this.audioRunning = false;
      this.audioError = true;
    } finally { this.audioStarting = false; }
  }

  setAudioVolume(volume: number): void { this.audioVolume = volume; this.#audio?.setVolume(volume); }
  setAudioStage(stage: AudioStage): void { this.audioStage = stage; this.#audio?.setStage(stage); }

  #clearSession(): void {
    this.controls = { ...DEFAULT_CONTROLS };
    this.running = false;
    this.speedLimitPaused = false;
    this.errorCode = '';
    this.#audio?.flush();
  }

  async resetSimulation(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    await this.#withClient(async (client) => {
      await client.run(false);
      await client.reset({ ...DEFAULT_RESET });
      this.#clearSession();
    });
    this.busy = false;
  }

  async releaseStarterIfRunning(): Promise<void> {
    if (this.controls.starter && this.snapshot?.state === 'running') await this.setControls({ starter: false });
  }

  destroy(): void {
    this.#client?.close();
    this.#client = null;
    void this.#audio?.stop();
    this.#audio = null;
  }
}

export const sim = new SimStore();
