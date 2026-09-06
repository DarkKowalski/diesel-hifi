import { describe, expect, it } from 'vitest';

import { applyStageToGraph, buildStageGraph } from '../src/lib/audioEngine';
import { CABIN_SPEC } from '../src/lib/cabin';

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
  disconnected = 0;

  constructor(readonly kind: string) {}

  connect<T extends FakeNode>(destination: T): T {
    this.outgoing.push(destination);
    return destination;
  }

  disconnect(): void {
    this.disconnected += 1;
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
  it('sends the source down both paths and mixes them back together', () => {
    const { graph } = build();
    const input = graph.input as unknown as FakeNode;
    const output = graph.output as unknown as FakeNode;
    const dry = graph.dryGain as unknown as FakeNode;
    const wet = graph.wetGain as unknown as FakeNode;

    expect(input.outgoing).toContain(dry);
    expect(dry.outgoing).toContain(output);

    // The wet path is long, so check it reaches rather than naming every hop.
    expect(reaches(input, wet)).toBe(true);
    expect(wet.outgoing).toContain(output);

    // And the dry path really is dry: nothing between the split and the mix.
    expect(dry.outgoing).toEqual([output]);
  });

  it('creates nothing that can produce sound on its own', () => {
    const { context } = build();
    const kinds = new Set(context.created.map((node) => node.kind));
    expect([...kinds].sort()).toEqual(
      ['biquad', 'compressor', 'convolver', 'delay', 'gain', 'panner'].filter((kind) =>
        kinds.has(kind),
      ),
    );
    // Every one of those is a filter of its input. Silence in, silence out.
    expect(kinds.has('oscillator')).toBe(false);
    expect(kinds.has('bufferSource')).toBe(false);
  });

  it('builds the filter chain in the order the specification gives', () => {
    const { context } = build();
    const filters = context.created.filter((node): node is FakeBiquad => node.kind === 'biquad');
    expect(filters).toHaveLength(CABIN_SPEC.filters.length);

    filters.forEach((filter, i) => {
      const spec = CABIN_SPEC.filters[i]!;
      expect(filter.type).toBe(spec.type);
      expect(filter.frequency.value).toBe(spec.frequencyHz);
      expect(filter.Q.value).toBe(spec.q);
      expect(filter.gain.value).toBe(spec.gainDb);
    });

    // In series, not in parallel: each filter feeds the next.
    for (let i = 0; i + 1 < filters.length; i += 1) {
      expect(filters[i]!.outgoing).toContain(filters[i + 1]!);
    }
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

  it('allows each reflection tap enough delay line to hold its delay', () => {
    const { context } = build();
    const delays = context.created.filter((node): node is FakeDelay => node.kind === 'delay');
    expect(delays).toHaveLength(CABIN_SPEC.taps.length);
    delays.forEach((delay, i) => {
      const tap = CABIN_SPEC.taps[i]!;
      expect(delay.delayTime.value).toBe(tap.delayS);
      expect(delay.maxDelayTime).toBeGreaterThanOrEqual(tap.delayS);
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
