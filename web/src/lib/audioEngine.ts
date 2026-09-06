/**
 * Web Audio orchestration.
 *
 * SPEC section 8 requires an explicit user action before audio starts, so
 * `start()` must only ever be called from inside a real gesture handler — that
 * is the whole reason `AudioContext` construction lives here rather than at
 * module load.
 *
 * The audio path is: worker produces samples -> main thread forwards them ->
 * `AudioWorklet` buffers and resamples -> gain -> destination. Samples cross as
 * transferable `Float32Array`s, so nothing is copied and no `SharedArrayBuffer`
 * is needed (which would demand cross-origin isolation the deployment target
 * cannot rely on).
 */

// `?url` emits the worklet verbatim as a hashed local asset and hands back a
// path Vite rewrites for whatever base the site is served from. That keeps it
// working at a domain root and under a subpath, with nothing fetched remotely.
import processorUrl from '../audio/exhaust-processor.js?url';

export interface AudioStatus {
  /** Whether the graph is built and the context is running. */
  running: boolean;
  /** Samples currently buffered in the worklet. */
  buffered: number;
  /** Render quanta that ran dry. Non-zero means the simulation stalled. */
  underruns: number;
  /** Samples delivered to the worklet since it started. */
  received: number;
  /** Device sample rate, which is rarely the solver's own. */
  deviceRateHz: number;
}

export type AudioStatusListener = (status: AudioStatus) => void;

const SILENT: AudioStatus = {
  running: false,
  buffered: 0,
  underruns: 0,
  received: 0,
  deviceRateHz: 0,
};

/**
 * FFT size for the spectrum view.
 *
 * Large on purpose. A six-cylinder diesel idles with a firing fundamental near
 * 28 Hz, and the default 2048-point transform puts fewer than two bins below
 * 50 Hz — far too coarse to see whether the low end is where it should be.
 */
const FFT_SIZE = 8192;

export class AudioEngine {
  private context: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private gainNode: GainNode | null = null;
  private analyser: AnalyserNode | null = null;
  private spectrum: Uint8Array | null = null;
  private status: AudioStatus = { ...SILENT };
  private listener: AudioStatusListener | null = null;
  private volume = 0.6;

  constructor(listener?: AudioStatusListener) {
    this.listener = listener ?? null;
  }

  /** Whether audio is currently playing. */
  get isRunning(): boolean {
    return this.status.running;
  }

  current(): AudioStatus {
    return this.status;
  }

  /**
   * Build the audio graph. Must be called from a user gesture handler.
   *
   * `sourceRateHz` is the rate the simulation produces at; the worklet
   * resamples from it to whatever the device runs at.
   */
  async start(sourceRateHz: number): Promise<void> {
    if (this.context) {
      await this.context.resume();
      this.update({ running: this.context.state === 'running' });
      return;
    }

    const context = new AudioContext();
    await context.audioWorklet.addModule(processorUrl);

    const node = new AudioWorkletNode(context, 'exhaust-processor', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [1],
      processorOptions: { sourceRateHz },
    });

    node.port.onmessage = (event: MessageEvent) => {
      const data = event.data as
        | { type: 'status'; buffered: number; underruns: number; received: number }
        | undefined;
      if (!data || data.type !== 'status') return;
      this.update({
        buffered: data.buffered,
        underruns: data.underruns,
        received: data.received,
      });
    };

    const gainNode = context.createGain();
    gainNode.gain.value = this.volume;

    // The analyser sits on the output path, so what it shows is what is
    // actually being played rather than what the simulation believes it sent.
    const analyser = context.createAnalyser();
    analyser.fftSize = FFT_SIZE;
    analyser.smoothingTimeConstant = 0.6;

    node.connect(gainNode);
    gainNode.connect(analyser);
    analyser.connect(context.destination);

    this.context = context;
    this.node = node;
    this.gainNode = gainNode;
    this.analyser = analyser;
    this.spectrum = new Uint8Array(analyser.frequencyBinCount);

    // Some browsers hand back a suspended context even from a gesture.
    await context.resume();

    this.update({
      running: context.state === 'running',
      deviceRateHz: context.sampleRate,
    });
  }

  /** Hand a block of samples to the worklet, transferring ownership. */
  push(samples: Float32Array): void {
    if (!this.node || samples.length === 0) return;
    this.node.port.postMessage({ type: 'samples', samples }, [samples.buffer]);
  }

  /**
   * Current output spectrum, or null when audio is not running.
   *
   * Returns magnitudes in `0..255` per bin, and the width of a bin in hertz so
   * the caller can put the bins on a frequency axis.
   */
  readSpectrum(): { magnitudes: Uint8Array; binHz: number } | null {
    if (!this.analyser || !this.spectrum || !this.context) return null;
    // Reuse the same array every frame; this runs on an animation frame.
    this.analyser.getByteFrequencyData(this.spectrum);
    return {
      magnitudes: this.spectrum,
      binHz: this.context.sampleRate / this.analyser.fftSize,
    };
  }

  setVolume(volume: number): void {
    this.volume = Math.max(0, Math.min(1, volume));
    if (this.gainNode && this.context) {
      // Ramp rather than jump: a step change in gain is an audible click.
      this.gainNode.gain.setTargetAtTime(this.volume, this.context.currentTime, 0.01);
    }
  }

  getVolume(): number {
    return this.volume;
  }

  /** Discard anything buffered, for a reset or an engine change. */
  flush(): void {
    this.node?.port.postMessage({ type: 'flush' });
  }

  async stop(): Promise<void> {
    if (!this.context) return;
    const context = this.context;
    this.node?.disconnect();
    this.gainNode?.disconnect();
    this.analyser?.disconnect();
    this.node = null;
    this.gainNode = null;
    this.analyser = null;
    this.spectrum = null;
    this.context = null;
    this.update({ ...SILENT });
    await context.close();
  }

  private update(patch: Partial<AudioStatus>): void {
    this.status = { ...this.status, ...patch };
    this.listener?.(this.status);
  }
}
