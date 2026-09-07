import { describe, expect, it } from 'vitest';

import { applyStageToGraph, buildStageGraph } from '../src/lib/audioEngine';
import { AUDIO_PATHS, CABIN_SPEC } from '../src/lib/cabin';

/**
 * Wiring of the post-processing stage.
 *
 * There is no `AudioContext` under Node, so this drives `buildStageGraph` with a
 * stand-in that records what was created and what was connected to what. That
 * covers the part of an audio graph that is easy to get silently wrong: a path
 * that never reaches the mix produces no error and no sound, and a graph that
 * accidentally *generates* something produces sound that is not the simulation.
 *
 * How it sounds is not testable here. The Playwright suite measures the real
 * output through the analyser.
 */

interface Recorded {
  value: number;
  ramps: Array<{ target: number; startTime: number; timeConstant: number }>;
}

class FakeParam implements Recorded {
  value = 0;
  ramps: Array<{ target: number; startTime: number; timeConstant: number }> = [];

  setTargetAtTime(target: number, startTime: number, timeConstant: number): this {
    this.ramps.push({ target, startTime, timeConstant });
    return this;
  }
}

class FakeNode {
  readonly outgoing: FakeNode[] = [];
  /** Which output index each connection was made from, for the splitter. */
  readonly outputs: number[] = [];
  disconnected = 0;

  constructor(readonly kind: string) {}

  connect<T extends FakeNode>(destination: T, output = 0): T {
    this.outgoing.push(destination);
    this.outputs.push(output);
    return destination;
  }

  disconnect(): void {
    this.disconnected += 1;
  }
}

class FakeSplitter extends FakeNode {
  constructor(readonly numberOfOutputs: number) {
    super('splitter');
  }
}

class FakeGain extends FakeNode {
  gain = new FakeParam();
  constructor() {
    super('gain');
  }
}

class FakeBiquad extends FakeNode {
  type = '';
  frequency = new FakeParam();
  Q = new FakeParam();
  gain = new FakeParam();
  constructor() {
    super('biquad');
  }
}

class FakeDelay extends FakeNode {
  delayTime = new FakeParam();
  constructor(readonly maxDelayTime: number) {
    super('delay');
  }
}

class FakePanner extends FakeNode {
  pan = new FakeParam();
  constructor() {
    super('panner');
  }
}

class FakeConvolver extends FakeNode {
  normalize = true;
  buffer: FakeBuffer | null = null;
  constructor() {
    super('convolver');
  }
}

class FakeCompressor extends FakeNode {
  threshold = new FakeParam();
  knee = new FakeParam();
  ratio = new FakeParam();
  attack = new FakeParam();
  release = new FakeParam();
  constructor() {
    super('compressor');
  }
}

class FakeBuffer {
  readonly channels: Float32Array[];
  constructor(
    readonly numberOfChannels: number,
    readonly length: number,
    readonly sampleRate: number,
  ) {
    this.channels = Array.from({ length: numberOfChannels }, () => new Float32Array(length));
  }
  copyToChannel(source: Float32Array, channel: number): void {
    this.channels[channel]!.set(source);
  }
}

/**
 * Only the factory methods the stage is allowed to use.
 *
 * `createOscillator` and `createBufferSource` are deliberately absent: if the
 * cab chain ever tried to generate anything rather than filter what it is given,
 * this would throw rather than quietly start adding sound to the simulation.
 */
class FakeContext {
  readonly created: FakeNode[] = [];
  sampleRate = 48_000;
  currentTime = 12.5;

  private track<T extends FakeNode>(node: T): T {
    this.created.push(node);
    return node;
  }

  createGain(): FakeGain {
    return this.track(new FakeGain());
  }
  createBiquadFilter(): FakeBiquad {
    return this.track(new FakeBiquad());
  }
  createDelay(maxDelayTime: number): FakeDelay {
    return this.track(new FakeDelay(maxDelayTime));
  }
  createStereoPanner(): FakePanner {
    return this.track(new FakePanner());
  }
  createChannelSplitter(numberOfOutputs: number): FakeSplitter {
    return this.track(new FakeSplitter(numberOfOutputs));
  }
  createConvolver(): FakeConvolver {
    return this.track(new FakeConvolver());
  }
  createDynamicsCompressor(): FakeCompressor {
    return this.track(new FakeCompressor());
  }
  createBuffer(channels: number, length: number, sampleRate: number): FakeBuffer {
    return new FakeBuffer(channels, length, sampleRate);
  }
}

/** Is there any path of connections from `from` to `to`? */
function reaches(from: FakeNode, to: FakeNode): boolean {
  const seen = new Set<FakeNode>();
  const queue = [from];
  while (queue.length > 0) {
    const node = queue.shift()!;
    if (node === to) return true;
    if (seen.has(node)) continue;
    seen.add(node);
    queue.push(...node.outgoing);
  }
  return false;
}

function build() {
  const context = new FakeContext();
  const graph = buildStageGraph(context as unknown as BaseAudioContext);
  return { context, graph };
}

describe('the post-processing graph', () => {
  it('sends every path down both stages and mixes them back together', () => {
    const { graph } = build();
    const input = graph.input as unknown as FakeNode;
    const output = graph.output as unknown as FakeNode;
    const dry = graph.dryGain as unknown as FakeNode;
    const wet = graph.wetGain as unknown as FakeNode;

    for (const path of AUDIO_PATHS) {
      const gain = graph.pathGains[path] as unknown as FakeNode;
      // Every path reaches the dry sum directly and the wet stage eventually.
      expect(gain.outgoing, `${path} should reach the dry sum`).toContain(dry);
      expect(reaches(gain, wet), `${path} should reach the wet stage`).toBe(true);
    }

    expect(dry.outgoing).toContain(output);
    expect(reaches(input, wet)).toBe(true);
    expect(wet.outgoing).toContain(output);

    // And the dry stage really is dry: the three paths summed and nothing else
    // between that sum and the mix. This is the signal the spectrum view was
    // built to verify, and it is exactly the sample the solver used to emit.
    expect(dry.outgoing).toEqual([output]);
  });

  it('takes each path from its own splitter output', () => {
    // The failure this catches is silent and nasty: crossing two outputs gives
    // each path the other's cab transfer, which is neither an error nor
    // recognisably wrong — it just moves the exhaust into the bulkhead.
    const { context, graph } = build();
    const splitter = context.created.find(
      (node): node is FakeSplitter => node.kind === 'splitter',
    )!;
    expect(splitter.numberOfOutputs).toBe(AUDIO_PATHS.length);

    AUDIO_PATHS.forEach((path, index) => {
      const gain = graph.pathGains[path] as unknown as FakeNode;
      const at = splitter.outgoing.indexOf(gain);
      expect(at, `${path} should be connected to the splitter`).toBeGreaterThanOrEqual(0);
      expect(splitter.outputs[at], `${path} should come from output ${index}`).toBe(index);
    });
  });

  it('gives the body path no room send, because it does not arrive through air', () => {
    // Structure-borne sound does not arrive as an early reflection or a diffuse
    // tail. The specification says so with a zero, and this is the assertion
    // that the graph honours it by building nothing rather than by building a
    // silent send that would be a worse description.
    const { graph } = build();
    const panners = (graph.nodes as unknown as FakeNode[]).filter(
      (node) => node.kind === 'panner',
    );
    const body = graph.pathGains.body as unknown as FakeNode;
    for (const panner of panners) {
      expect(reaches(body, panner)).toBe(false);
    }
    const convolver = (graph.nodes as unknown as FakeNode[]).find(
      (node) => node.kind === 'convolver',
    )!;
    expect(reaches(body, convolver)).toBe(false);

    // The other two do reach the room, or there would be no room.
    expect(reaches(graph.pathGains.exhaust as unknown as FakeNode, convolver)).toBe(true);
  });

  it('creates nothing that can produce sound on its own', () => {
    const { context } = build();
    const kinds = new Set(context.created.map((node) => node.kind));
    expect([...kinds].sort()).toEqual(
      ['biquad', 'compressor', 'convolver', 'delay', 'gain', 'panner', 'splitter'].filter((kind) =>
        kinds.has(kind),
      ),
    );
    // Every one of those is a filter of its input. Silence in, silence out.
    expect(kinds.has('oscillator')).toBe(false);
    expect(kinds.has('bufferSource')).toBe(false);
  });

  it('builds every filter chain in the order the specification gives', () => {
    const { context } = build();
    const filters = context.created.filter((node): node is FakeBiquad => node.kind === 'biquad');

    // Three per-path chains and then the shared one, in that order.
    const expected = [
      ...AUDIO_PATHS.flatMap((path) => CABIN_SPEC.paths[path].filters),
      ...CABIN_SPEC.filters,
    ];
    expect(filters).toHaveLength(expected.length);

    filters.forEach((filter, i) => {
      const spec = expected[i]!;
      expect(filter.type).toBe(spec.type);
      expect(filter.frequency.value).toBe(spec.frequencyHz);
      expect(filter.Q.value).toBe(spec.q);
      expect(filter.gain.value).toBe(spec.gainDb);
    });
  });

  it('runs each path chain in series and keeps the chains apart', () => {
    const { graph } = build();
    for (const path of AUDIO_PATHS) {
      const stages = CABIN_SPEC.paths[path].filters;
      let tail = graph.pathGains[path] as unknown as FakeNode;
      for (const stage of stages) {
        const next = tail.outgoing.find(
          (node): node is FakeBiquad =>
            node.kind === 'biquad' && (node as FakeBiquad).frequency.value === stage.frequencyHz,
        );
        expect(next, `${path} should continue into its ${stage.frequencyHz} Hz stage`).toBeDefined();
        tail = next!;
      }
    }

    // And no path's chain leaks into another's: the whole point is that the
    // exhaust is not filtered as if it were bolted to the bulkhead.
    const exhaustFirst = CABIN_SPEC.paths.exhaust.filters[0]!.frequencyHz;
    const block = graph.pathGains.block as unknown as FakeNode;
    expect(
      block.outgoing.some(
        (node) => node.kind === 'biquad' && (node as FakeBiquad).frequency.value === exhaustFirst,
      ),
    ).toBe(false);
  });

  it('gives the convolver a stereo response generated for the device rate', () => {
    const { context } = build();
    const convolver = context.created.find(
      (node): node is FakeConvolver => node.kind === 'convolver',
    )!;
    expect(convolver.buffer).not.toBeNull();
    expect(convolver.buffer!.numberOfChannels).toBe(2);
    expect(convolver.buffer!.sampleRate).toBe(context.sampleRate);
    expect(convolver.buffer!.length).toBe(
      Math.round(CABIN_SPEC.impulse.durationS * context.sampleRate),
    );
    // Ours is energy-normalised already; the node's own normalisation would
    // undo exactly the property that keeps the wet level rate-independent.
    expect(convolver.normalize).toBe(false);
  });

  it('allows every delay line enough room to hold its delay', () => {
    const { context } = build();
    const delays = context.created.filter((node): node is FakeDelay => node.kind === 'delay');

    // One per path that is not co-located with the driver, plus one per tap. A
    // zero-delay path builds nothing rather than a delay of zero.
    const propagation = AUDIO_PATHS.map((path) => CABIN_SPEC.paths[path].delayS).filter(
      (delayS) => delayS > 0,
    );
    const expected = [...propagation, ...CABIN_SPEC.taps.map((tap) => tap.delayS)];
    expect(delays).toHaveLength(expected.length);

    delays.forEach((delay, i) => {
      expect(delay.delayTime.value).toBe(expected[i]!);
      expect(delay.maxDelayTime).toBeGreaterThanOrEqual(expected[i]!);
    });
  });

  it('hands back every node it made, so teardown can disconnect them', () => {
    const { context, graph } = build();
    expect(graph.nodes).toHaveLength(context.created.length);
  });
});

describe('switching stage', () => {
  it('sets the gains outright only when asked for no fade', () => {
    const { graph } = build();
    applyStageToGraph(graph, 'cockpit', 0, 0);
    expect(graph.dryGain.gain.value).toBe(0);
    expect(graph.wetGain.gain.value).toBe(1);

    applyStageToGraph(graph, 'raw', 0, 0);
    expect(graph.dryGain.gain.value).toBe(1);
    expect(graph.wetGain.gain.value).toBe(0);
  });

  it('ramps complementarily rather than stepping', () => {
    const { graph } = build();
    const dry = graph.dryGain.gain as unknown as FakeParam;
    const wet = graph.wetGain.gain as unknown as FakeParam;

    applyStageToGraph(graph, 'cockpit', CABIN_SPEC.crossfadeS, 12.5);

    // A step in gain is a step in the output, and a step is broadband: it is
    // heard as a click. Nothing may be assigned directly.
    expect(dry.ramps).toHaveLength(1);
    expect(wet.ramps).toHaveLength(1);
    expect(dry.ramps[0]!.target).toBe(0);
    expect(wet.ramps[0]!.target).toBe(1);
    expect(dry.ramps[0]!.target + wet.ramps[0]!.target).toBe(1);
    expect(dry.ramps[0]!.startTime).toBe(12.5);
    expect(dry.ramps[0]!.timeConstant).toBeCloseTo(CABIN_SPEC.crossfadeS / 3, 6);

    applyStageToGraph(graph, 'raw', CABIN_SPEC.crossfadeS, 13);
    expect(dry.ramps[1]!.target).toBe(1);
    expect(wet.ramps[1]!.target).toBe(0);
  });
});
