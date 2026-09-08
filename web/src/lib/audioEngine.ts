/**
 * Web Audio orchestration.
 *
 * README "WASM and worker API" requires an explicit user action before audio starts, so
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
 *                        ┌──────────── dry ────────────────────────────┐
 *   worklet ─ splitter ─ path gains                                    mix ─ volume ─ analyser ─ out
 *              (3 ch)    └─ per-path EQ + delay ─┬─ arrivals ─ direct ─┐
 *                                                └─ room send ─ taps ──┤
 *                                                      └─ convolver ───┤
 *                                                   room ─ shared EQ ─ compressor ─ wet
 * ```
 *
 * The worklet hands over three channels, not one: the exhaust, the block and
 * the body reach a driver by different routes, so each gets its own transfer
 * before anything is summed. `dry` is the three added straight back up, which is
 * exactly the single sample the solver used to emit.
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
import {
  AUDIO_PATHS,
  CABIN_SPEC,
  generateImpulseResponse,
  type AudioPath,
  type AudioStage,
  type CabinSpec,
  type FilterStage,
} from './cabin';
import {
  canSelect,
  initialCompareState,
  measureClip,
  rematch,
  sourceGains,
  type ClipInfo,
  type ClipSlot,
  type CompareSource,
  type CompareState,
} from './compare';

export type { AudioPath, AudioStage } from './cabin';

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
  /** Where the source connects. Feeds both stages. */
  input: AudioNode;
  /** Where both stages land. Connects onward to volume and the analyser. */
  output: AudioNode;
  dryGain: GainNode;
  wetGain: GainNode;
  /**
   * One gain per radiating path, for soloing and muting.
   *
   * Downstream of the split and upstream of everything else, so it removes a
   * path from both stages at once — which is what makes it a listening control
   * rather than a second balance. The engine keeps producing all three; nothing
   * about the simulation changes.
   */
  pathGains: Record<AudioPath, GainNode>;
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

  /** Build a biquad chain onto `from`, returning its far end. */
  const chain = (from: AudioNode, stages: readonly FilterStage[]): AudioNode => {
    let tail = from;
    for (const stage of stages) {
      const filter = keep(context.createBiquadFilter());
      filter.type = stage.type;
      filter.frequency.value = stage.frequencyHz;
      filter.Q.value = stage.q;
      filter.gain.value = stage.gainDb;
      tail.connect(filter);
      tail = filter;
    }
    return tail;
  };

  // Split the worklet's three channels apart. The split happens immediately at
  // the node output because `ChannelSplitterNode` is specified as explicit and
  // discrete: anything else in between could apply Web Audio's up- and
  // down-mixing rules and treat three mono paths as some surround layout.
  const splitter = keep(context.createChannelSplitter(AUDIO_PATHS.length));
  input.connect(splitter);

  // Per-path gains, for soloing. Ahead of both stages so a muted path is muted
  // in the cab and in the raw sum alike.
  const pathGains = {} as Record<AudioPath, GainNode>;
  AUDIO_PATHS.forEach((path, index) => {
    const gain = keep(context.createGain());
    splitter.connect(gain, index);
    pathGains[path] = gain;
  });

  // Dry: the three paths added back up, untouched. That sum is exactly the
  // single sample the solver used to emit — the saturation is decided on the mix
  // and applied to the paths as a common gain — so this is the same signal the
  // spectrum view was built to verify.
  const dryGain = keep(context.createGain());
  for (const path of AUDIO_PATHS) {
    pathGains[path].connect(dryGain);
  }
  dryGain.connect(mix);

  // Wet: each path by its own route, then everything shared.
  //
  // This is what the split is for. The exhaust leaves a stack metres behind and
  // below and arrives muffled and late; the block is two feet away through the
  // bulkhead and keeps its top end; the body comes through the mounts and the
  // seat with no air path, so it gets no delay and no room at all. One chain
  // could not say any of that, and the values it settled on instead are
  // recorded in `cabin.ts`.
  // `arrivals` is every path after its own route: the sound as it gets to the
  // driver. `roomSource` is the part of it that arrives *through the air* and so
  // has reflections and a tail. They are different sums because the body path
  // belongs in the first and not the second.
  const arrivals = keep(context.createGain());
  const roomSource = keep(context.createGain());

  for (const path of AUDIO_PATHS) {
    const stage = spec.paths[path];

    // How loud this path is at the driver, before its transfer colours it.
    //
    // Inside the wet chain on purpose: `pathGains` is the solo control and feeds
    // the dry sum as well, so trimming there would move the raw stage — and the
    // raw stage is the solver's own output, which is what every figure in the
    // README is measured from. This says where the listener is sitting; it does
    // not say what the engine did.
    let source: AudioNode = pathGains[path];
    if (stage.levelDb !== 0) {
      const trim = keep(context.createGain());
      trim.gain.value = 10 ** (stage.levelDb / 20);
      source.connect(trim);
      source = trim;
    }

    let tail: AudioNode = chain(source, stage.filters);

    if (stage.delayS > 0) {
      const delay = keep(context.createDelay(Math.max(stage.delayS, 0.001) * 2));
      delay.delayTime.value = stage.delayS;
      tail.connect(delay);
      tail = delay;
    }

    tail.connect(arrivals);

    // A zero send is left unbuilt rather than built at zero gain. The body path
    // does not reach the driver through the air, so it has no reflections and no
    // tail; a silent node claiming otherwise would be a worse description.
    if (stage.roomSend > 0) {
      const send = keep(context.createGain());
      send.gain.value = stage.roomSend;
      tail.connect(send);
      send.connect(roomSource);
    }
  }

  // Fan out to the direct sound, the discrete early reflections, and the
  // diffuse tail, then sum them again.
  const room = keep(context.createGain());

  const direct = keep(context.createGain());
  direct.gain.value = spec.directGain;
  arrivals.connect(direct);
  direct.connect(room);

  // What the early reflections bounce off is upholstery, and upholstery does not
  // return the top end. One damping filter shared by both taps rather than one
  // each: they are hitting the same kind of surface, and two identical filters
  // would be two nodes making one statement.
  const reflectionDamping = keep(context.createBiquadFilter());
  reflectionDamping.type = 'lowpass';
  reflectionDamping.frequency.value = spec.reflectionDampingHz;
  reflectionDamping.Q.value = 0.7;
  roomSource.connect(reflectionDamping);

  for (const tap of spec.taps) {
    const delay = keep(context.createDelay(Math.max(tap.delayS, 0.001) * 2));
    delay.delayTime.value = tap.delayS;
    const level = keep(context.createGain());
    level.gain.value = tap.gain;
    const panner = keep(context.createStereoPanner());
    panner.pan.value = tap.pan;
    reflectionDamping.connect(delay);
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
  roomSource.connect(convolver);
  convolver.connect(reverb);
  reverb.connect(room);

  // The shared stages last, so they act once on everything that reaches the
  // driver rather than once per path and again through the reflections. They are
  // the cab as a box and the listener's speaker; the routing above was the three
  // sources and where they are.
  const shared = chain(room, spec.filters);

  // Trim before the compressor, make it up after. Splitting the level match
  // across the compressor is what lets idle and full load both come out level
  // with the dry path; a single output gain can only match one of them.
  const trim = keep(context.createGain());
  trim.gain.value = spec.dynamics.inputGain;
  shared.connect(trim);

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

  return { input, output: mix, dryGain, wetGain, pathGains, nodes };
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

/**
 * One loaded comparison clip: its decoded samples, its measurements, and the
 * nodes playing it.
 */
interface LoadedClip {
  info: ClipInfo;
  buffer: AudioBuffer;
  gain: GainNode;
  source: AudioBufferSourceNode | null;
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
  private pathEnabled: Record<AudioPath, boolean> = {
    exhaust: true,
    block: true,
    body: true,
  };

  /**
   * Gain the whole engine chain passes through, for the A/B switch.
   *
   * Downstream of both stages and upstream of the volume control, so switching
   * to a reference recording silences the engine without disturbing either
   * stage's crossfade or the volume the listener set.
   */
  private engineBus: GainNode | null = null;
  private compare: CompareState = initialCompareState();
  private clips: Partial<Record<ClipSlot, LoadedClip>> = {};

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

    // One output carrying the three radiating paths as discrete channels. The
    // splitter downstream separates them; they are three mono signals rather
    // than a spatial layout, and the stereo image is made after the split by
    // the panned reflection taps and the stereo convolver.
    const node = new AudioWorkletNode(context, 'exhaust-processor', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [AUDIO_PATHS.length],
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

    // The A/B switch. The engine's whole chain passes through one gain, and each
    // comparison clip has its own; only one is ever open. The clips join *after*
    // this point and before the volume control, because a reference recording is
    // already a recording of somebody's cab — running it through ours would
    // filter a cab through a cab, and the thing being compared is the finished
    // sound at the driver's ear.
    const engineBus = context.createGain();

    node.connect(graph.input);
    graph.output.connect(engineBus);
    engineBus.connect(gainNode);
    gainNode.connect(analyser);
    analyser.connect(context.destination);

    this.context = context;
    this.node = node;
    this.graph = graph;
    this.engineBus = engineBus;
    this.gainNode = gainNode;
    this.analyser = analyser;
    this.spectrum = new Uint8Array(analyser.frequencyBinCount);
    this.waveform = new Float32Array(analyser.fftSize);

    // No ramp here: the graph has produced nothing yet, so there is no step
    // discontinuity to smooth and starting mid-crossfade would be audible.
    this.applyStage(0);
    for (const path of AUDIO_PATHS) {
      graph.pathGains[path].gain.value = this.pathEnabled[path] ? 1 : 0;
    }

    // Some browsers hand back a suspended context even from a gesture.
    await context.resume();

    this.update({
      running: context.state === 'running',
      deviceRateHz: context.sampleRate,
    });
  }

  /**
   * Hand a block of interleaved frames to the worklet, transferring ownership.
   *
   * `paths` travels with the block. The worklet refuses a block whose count
   * disagrees with its own rather than de-interleaving on a guess: reading
   * three-path frames as two produces a signal that is neither silent nor
   * recognisably wrong, and shuffles which cab transfer each path receives.
   */
  push(samples: Float32Array, paths: number): void {
    if (!this.node || samples.length === 0) return;
    this.node.port.postMessage({ type: 'samples', samples, paths }, [samples.buffer]);
  }

  /**
   * Silence or restore one radiating path.
   *
   * A listening control, downstream of everything and upstream of both stages,
   * so it changes no state and the solver keeps producing all three. The
   * balance itself is calibrated against `audio_probe`, not by ear; this is for
   * hearing what each path contributes.
   */
  setPathEnabled(path: AudioPath, enabled: boolean): void {
    this.pathEnabled[path] = enabled;
    const gain = this.graph?.pathGains[path];
    if (gain && this.context) {
      // Ramp rather than jump: a step in gain is a step in the output.
      gain.gain.setTargetAtTime(enabled ? 1 : 0, this.context.currentTime, 0.01);
    }
  }

  isPathEnabled(path: AudioPath): boolean {
    return this.pathEnabled[path];
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

  // --- level-matched comparison ------------------------------------------
  //
  // See `compare.ts` for why a comparison has to be matched and switched rather
  // than mixed. Everything below is the graph side of that: the arithmetic is
  // there, the nodes are here.

  /** The comparison's current state, for the UI to render. */
  comparison(): CompareState {
    return this.compare;
  }

  /**
   * Decode a local file into a comparison slot and measure it.
   *
   * The bytes come from a file input and are decoded in the browser. Nothing is
   * uploaded: the deployment has no backend, and these are other people's
   * recordings.
   *
   * Replacing a slot stops whatever it was playing first, so a re-load cannot
   * leave two clips running into the same gain.
   */
  async loadClip(slot: ClipSlot, name: string, bytes: ArrayBuffer): Promise<ClipInfo> {
    if (!this.context) throw new Error('audio is not running');
    const buffer = await this.context.decodeAudioData(bytes);

    const channels: Float32Array[] = [];
    for (let c = 0; c < buffer.numberOfChannels; c += 1) {
      channels.push(buffer.getChannelData(c));
    }
    const { levelDb, peakDb } = measureClip(channels);
    const info: ClipInfo = {
      name,
      durationS: buffer.duration,
      sampleRateHz: buffer.sampleRate,
      channels: buffer.numberOfChannels,
      levelDb,
      peakDb,
    };

    this.stopClip(slot);
    const existing = this.clips[slot];
    const gain = existing?.gain ?? this.context.createGain();
    if (!existing) {
      gain.gain.value = 0;
      gain.connect(this.gainNode ?? this.context.destination);
    }
    this.clips[slot] = { info, buffer, gain, source: null };

    this.compare = rematch(
      { ...this.compare, clips: { ...this.compare.clips, [slot]: info } },
      this.compare.targetDb,
    );
    this.applyComparison();
    return info;
  }

  /**
   * Choose which source is audible.
   *
   * Selecting a slot with nothing loaded is refused rather than silently
   * producing silence, which would look exactly like a broken comparison.
   */
  setComparisonSource(source: CompareSource): void {
    if (!canSelect(this.compare, source)) return;
    this.compare = { ...this.compare, source };
    this.applyComparison();
  }

  /**
   * Match the loaded clips to a level, in dBFS.
   *
   * Normally the engine's own measured output level, so the switch is between
   * two things at the same loudness. Re-matching is explicit rather than
   * continuous: an automatic match would be a compressor keyed to the engine,
   * and would flatten exactly the loudness differences between operating points
   * that the comparison is trying to judge.
   */
  matchComparisonTo(targetDb: number): CompareState {
    this.compare = rematch(this.compare, targetDb);
    this.applyComparison();
    return this.compare;
  }

  /** Whether the clips loop. */
  setComparisonLoop(loop: boolean): void {
    this.compare = { ...this.compare, loop };
    for (const clip of Object.values(this.clips)) {
      if (clip?.source) clip.source.loop = loop;
    }
  }

  /** Apply the switch, ramping so nothing steps. */
  private applyComparison(): void {
    if (!this.context) return;
    const gains = sourceGains(this.compare);
    const now = this.context.currentTime;
    this.engineBus?.gain.setTargetAtTime(gains.engine, now, 0.01);

    for (const slot of ['reference', 'candidate'] as ClipSlot[]) {
      const clip = this.clips[slot];
      if (!clip) continue;
      const target = gains[slot];
      clip.gain.gain.setTargetAtTime(target, now, 0.01);
      // Only the selected clip runs. A buffer source that has been stopped
      // cannot be restarted, so a fresh one is created each time — which is what
      // the Web Audio API expects of them.
      if (target > 0 && !clip.source) {
        const source = this.context.createBufferSource();
        source.buffer = clip.buffer;
        source.loop = this.compare.loop;
        source.connect(clip.gain);
        source.start();
        clip.source = source;
      } else if (target === 0 && clip.source) {
        this.stopClip(slot);
      }
    }
  }

  private stopClip(slot: ClipSlot): void {
    const clip = this.clips[slot];
    if (!clip?.source) return;
    try {
      clip.source.stop();
    } catch {
      // Already stopped, or never started. Either way there is nothing to do.
    }
    clip.source.disconnect();
    clip.source = null;
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
    for (const slot of ['reference', 'candidate'] as ClipSlot[]) {
      this.stopClip(slot);
      this.clips[slot]?.gain.disconnect();
    }
    this.clips = {};
    this.compare = initialCompareState();
    this.engineBus?.disconnect();
    this.gainNode?.disconnect();
    this.analyser?.disconnect();
    this.node = null;
    this.graph = null;
    this.engineBus = null;
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
