/**
 * Web Audio orchestration.
 *
 * SPEC section 8 requires an explicit user action before audio starts, so
 * `start()` must only ever be called from inside a real gesture handler — that
 * is the whole reason `AudioContext` construction lives here rather than at
 * module load.
 *
 * The audio path is: worker produces samples -> main thread forwards them ->
 * `AudioWorklet` buffers and resamples -> post-processing stage -> gain ->
 * destination. Samples cross as transferable `Float32Array`s, so nothing is
 * copied and no `SharedArrayBuffer` is needed (which would demand cross-origin
 * isolation the deployment target cannot rely on).
 *
 * ```text
 *              ┌─ dry ───────────────────────────────────┐
 *   worklet ─ split                                      mix ─ volume ─ analyser ─ out
 *              └─ cab EQ ─ reflections ─ compressor ─ wet ┘
 * ```
 *
 * Both paths are built once and always run; switching stages crossfades between
 * them. Building the cab chain lazily would mean allocating and connecting nodes
 * while audio is playing, and the convolver in particular takes long enough to
 * accept a buffer to be heard as a hitch.
 */

// `?url` emits the worklet verbatim as a hashed local asset and hands back a
// path Vite rewrites for whatever base the site is served from. That keeps it
// working at a domain root and under a subpath, with nothing fetched remotely.
import processorUrl from '../audio/exhaust-processor.js?url';
import { CABIN_SPEC, generateImpulseResponse, type AudioStage, type CabinSpec } from './cabin';

export type { AudioStage } from './cabin';

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

/** Everything the post-processing stage owns, plus the two ends it exposes. */
export interface StageGraph {
  /** Where the source connects. Feeds both paths. */
  input: AudioNode;
  /** Where both paths land. Connects onward to volume and the analyser. */
  output: AudioNode;
  dryGain: GainNode;
  wetGain: GainNode;
  /** Every node, for teardown. */
  nodes: AudioNode[];
}

/**
 * Build the dry and cockpit paths and mix them.
 *
 * Takes the context rather than reaching for one, so the wiring can be driven by
 * a stand-in under Node. What this asserts about is topology — that both paths
 * exist and reach the same mix — which is exactly the part that is easy to get
 * silently wrong and inaudible in a passing test suite.
 */
export function buildStageGraph(
  context: BaseAudioContext,
  spec: CabinSpec = CABIN_SPEC,
): StageGraph {
  const nodes: AudioNode[] = [];
  const keep = <T extends AudioNode>(node: T): T => {
    nodes.push(node);
    return node;
  };

  const input = keep(context.createGain());
  const mix = keep(context.createGain());

  // Dry: the tailpipe signal, untouched. This is the path the spectrum view was
  // built to verify, and it stays exactly as it was.
  const dryGain = keep(context.createGain());
  input.connect(dryGain);
  dryGain.connect(mix);

  // Wet: the same samples, through the cab.
  let tail: AudioNode = input;
  for (const stage of spec.filters) {
    const filter = keep(context.createBiquadFilter());
    filter.type = stage.type;
    filter.frequency.value = stage.frequencyHz;
    filter.Q.value = stage.q;
    filter.gain.value = stage.gainDb;
    tail.connect(filter);
    tail = filter;
  }

  // Fan the filtered signal out to the direct sound, the discrete early
  // reflections, and the diffuse tail, then sum them again.
  const room = keep(context.createGain());

  const direct = keep(context.createGain());
  direct.gain.value = spec.directGain;
  tail.connect(direct);
  direct.connect(room);

  for (const tap of spec.taps) {
    const delay = keep(context.createDelay(Math.max(tap.delayS, 0.001) * 2));
    delay.delayTime.value = tap.delayS;
    const level = keep(context.createGain());
    level.gain.value = tap.gain;
    const panner = keep(context.createStereoPanner());
    panner.pan.value = tap.pan;
    tail.connect(delay);
    delay.connect(level);
    level.connect(panner);
    panner.connect(room);
  }

  const convolver = keep(context.createConvolver());
  // A generated response, so no asset is fetched and the build stays
  // self-contained. Normalisation is ours — the node's own would undo the
  // energy scaling that keeps the wet level independent of the device rate.
  convolver.normalize = false;
  const impulse = generateImpulseResponse(context.sampleRate, spec.impulse);
  const buffer = context.createBuffer(2, impulse.left.length, context.sampleRate);
  buffer.copyToChannel(impulse.left, 0);
  buffer.copyToChannel(impulse.right, 1);
  convolver.buffer = buffer;
  const reverb = keep(context.createGain());
  reverb.gain.value = spec.reverbGain;
  tail.connect(convolver);
  convolver.connect(reverb);
  reverb.connect(room);

  // Trim before the compressor, make it up after. Splitting the level match
  // across the compressor is what lets idle and full load both come out level
  // with the dry path; a single output gain can only match one of them.
  const trim = keep(context.createGain());
  trim.gain.value = spec.dynamics.inputGain;
  room.connect(trim);

  // A hard box compresses what happens inside it, and this also keeps idle
  // audible without the start transient — the loudest thing the engine does —
  // arriving flat against the ceiling.
  const compressor = keep(context.createDynamicsCompressor());
  compressor.threshold.value = spec.dynamics.thresholdDb;
  compressor.knee.value = spec.dynamics.kneeDb;
  compressor.ratio.value = spec.dynamics.ratio;
  compressor.attack.value = spec.dynamics.attackS;
  compressor.release.value = spec.dynamics.releaseS;
  trim.connect(compressor);

  const makeup = keep(context.createGain());
  makeup.gain.value = spec.dynamics.makeupGain;
  compressor.connect(makeup);

  const wetGain = keep(context.createGain());
  makeup.connect(wetGain);
  wetGain.connect(mix);

  return { input, output: mix, dryGain, wetGain, nodes };
}

/**
 * Crossfade the graph to a stage.
 *
 * Split out from the engine so it can be driven without an `AudioContext`. A
 * `fadeS` of zero sets the gains outright, which is only correct before anything
 * has been played.
 *
 * The fade is linear rather than equal-power because the paths are correlated —
 * they are the same signal — so an equal-power curve would push the sum above
 * unity through the middle of the switch and be heard as a lump.
 */
export function applyStageToGraph(
  graph: StageGraph,
  stage: AudioStage,
  fadeS: number,
  now: number,
): void {
  const wet = stage === 'cockpit' ? 1 : 0;
  if (fadeS <= 0) {
    graph.dryGain.gain.value = 1 - wet;
    graph.wetGain.gain.value = wet;
    return;
  }
  // `setTargetAtTime` approaches exponentially; a third of the fade as the time
  // constant lands within a percent or so of the target by the end of it.
  const tau = fadeS / 3;
  graph.dryGain.gain.setTargetAtTime(1 - wet, now, tau);
  graph.wetGain.gain.setTargetAtTime(wet, now, tau);
}

export class AudioEngine {
  private context: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private graph: StageGraph | null = null;
  private gainNode: GainNode | null = null;
  private analyser: AnalyserNode | null = null;
  private spectrum: Uint8Array | null = null;
  private waveform: Float32Array | null = null;
  private status: AudioStatus = { ...SILENT };
  private listener: AudioStatusListener | null = null;
  private volume = 0.6;
  private stage: AudioStage = 'cockpit';

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

    const graph = buildStageGraph(context);

    const gainNode = context.createGain();
    gainNode.gain.value = this.volume;

    // The analyser sits on the output path, so what it shows is what is
    // actually being played rather than what the simulation believes it sent.
    // That still holds with post-processing in the chain: switching to the cab
    // stage visibly changes the spectrum, because it audibly changes the sound.
    const analyser = context.createAnalyser();
    analyser.fftSize = FFT_SIZE;
    analyser.smoothingTimeConstant = 0.6;

    node.connect(graph.input);
    graph.output.connect(gainNode);
    gainNode.connect(analyser);
    analyser.connect(context.destination);

    this.context = context;
    this.node = node;
    this.graph = graph;
    this.gainNode = gainNode;
    this.analyser = analyser;
    this.spectrum = new Uint8Array(analyser.frequencyBinCount);
    this.waveform = new Float32Array(analyser.fftSize);

    // No ramp here: the graph has produced nothing yet, so there is no step
    // discontinuity to smooth and starting mid-crossfade would be audible.
    this.applyStage(0);

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

  /**
   * Level of what is actually leaving the graph, in dBFS.
   *
   * Distinct from the snapshot's `audioLevelDb`, which is the solver's own RMS
   * at the source and knows nothing about the stage, the volume control, or the
   * device. This is measured after everything, which is what makes it the right
   * number to check the two stages against each other with: a post-processing
   * switch that is merely louder would flatter itself in any listening test.
   *
   * Returns null when audio is not running, and floors at −120 dBFS so digital
   * silence reads as a number rather than as negative infinity.
   */
  readOutputLevelDb(): number | null {
    if (!this.analyser || !this.waveform) return null;
    this.analyser.getFloatTimeDomainData(this.waveform);
    let sumSquares = 0;
    for (let i = 0; i < this.waveform.length; i += 1) {
      sumSquares += this.waveform[i]! * this.waveform[i]!;
    }
    const rms = Math.sqrt(sumSquares / this.waveform.length);
    if (!Number.isFinite(rms) || rms <= 1e-6) return -120;
    return 20 * Math.log10(rms);
  }

  /**
   * Choose the listening position.
   *
   * Crossfaded, not switched. The two paths carry the same signal, so a step
   * change in either gain is a step change in the output — and a step is
   * broadband, heard as a click exactly like the ones the worklet's fade
   * envelope and `setVolume`'s ramp exist to avoid.
   */
  setStage(stage: AudioStage): void {
    this.stage = stage;
    this.applyStage(CABIN_SPEC.crossfadeS);
  }

  getStage(): AudioStage {
    return this.stage;
  }

  private applyStage(fadeS: number): void {
    if (!this.graph || !this.context) return;
    applyStageToGraph(this.graph, this.stage, fadeS, this.context.currentTime);
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
    for (const node of this.graph?.nodes ?? []) {
      node.disconnect();
    }
    this.gainNode?.disconnect();
    this.analyser?.disconnect();
    this.node = null;
    this.graph = null;
    this.gainNode = null;
    this.analyser = null;
    this.spectrum = null;
    this.waveform = null;
    this.context = null;
    this.update({ ...SILENT });
    await context.close();
  }

  private update(patch: Partial<AudioStatus>): void {
    this.status = { ...this.status, ...patch };
    this.listener?.(this.status);
  }
}
