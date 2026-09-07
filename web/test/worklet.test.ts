import { describe, expect, it } from 'vitest';

// `?raw` rather than a filesystem read: it is the same mechanism the app uses to
// hand the worklet to `addModule`, it needs no Node type declarations, and it
// keeps this test tied to the file that ships instead of to a path string.
import workletSource from '../src/audio/exhaust-processor.js?raw';

/**
 * Playback audit: the real worklet, driven at real device rates.
 *
 * `web/src/audio/exhaust-processor.js` is the last thing between the solver and
 * the speaker, and until now nothing tested it. Everything it does is a place
 * the sound can be damaged in a way that looks like a physics problem: the
 * resample can be at the wrong rate, the three paths can slide apart, the ring
 * can drop or duplicate frames, the drift trim can run away, and interpolation
 * can put imaging products where the engine is not.
 *
 * The file is a dependency-free `AudioWorklet` module: no imports, and a
 * `registerProcessor` call at the end. That is exactly what makes it loadable
 * here — evaluate it with the three globals a worklet realm provides and it
 * hands back its own class. Nothing is duplicated, so this tests the file that
 * ships rather than a copy of it that will drift.
 *
 * Both device rates are exercised. 48 kHz is the common one, 44.1 kHz is the
 * other common one, and neither divides the solver's 40 kHz — which is the whole
 * reason there is a resampler to audit.
 */

/** Paths per frame, as the worklet fixes them: exhaust, block, body. */
const PATHS = 3;
/** The solver's rate. */
const SOURCE_RATE = 40_000;
/** The worklet's cushion, in frames. Must match `PRIME_SAMPLES`. */
const PRIME = 2400;
/** Web Audio's render quantum. */
const QUANTUM = 128;
/**
 * Quanta between status messages. Must match `REPORT_INTERVAL`.
 *
 * Any test that reads a counter has to render at least this many blocks, or it
 * is asserting against a message the worklet never sent.
 */
const REPORT_INTERVAL = 32;

/** Source frames a run of `quanta` output blocks will consume, with headroom. */
function supply(quanta: number, deviceRateHz: number): number {
  return Math.ceil((quanta * QUANTUM * SOURCE_RATE) / deviceRateHz) + PRIME * 2;
}

interface StatusMessage {
  type: 'status';
  buffered: number;
  underruns: number;
  received: number;
  overflowed: number;
  malformed: number;
}

interface Processor {
  port: { onmessage: ((event: { data: unknown }) => void) | null };
  process(inputs: unknown[], outputs: Float32Array[][]): boolean;
}

/**
 * Load the shipped worklet and instantiate it for a device rate.
 *
 * `sampleRate` is a worklet-realm global, so it is supplied as one rather than
 * patched onto `globalThis`, which would leak between tests running in the same
 * worker.
 */
function load(deviceRateHz: number) {
  const source = workletSource;

  const statuses: StatusMessage[] = [];
  class FakeWorkletProcessor {
    port = {
      onmessage: null as ((event: { data: unknown }) => void) | null,
      postMessage(message: StatusMessage) {
        statuses.push(message);
      },
    };
  }

  let registered: (new (options: unknown) => Processor) | null = null;
  const factory = new Function(
    'AudioWorkletProcessor',
    'registerProcessor',
    'sampleRate',
    `${source}\nreturn null;`,
  );
  factory(
    FakeWorkletProcessor,
    (_name: string, cls: new (options: unknown) => Processor) => {
      registered = cls;
    },
    deviceRateHz,
  );
  if (!registered) throw new Error('the worklet did not register a processor');

  const processor = new (registered as unknown as new (options: unknown) => Processor)({
    processorOptions: { sourceRateHz: SOURCE_RATE },
  });

  const send = (message: unknown) => processor.port.onmessage?.({ data: message });
  const push = (samples: Float32Array, paths: number = PATHS) =>
    send({ type: 'samples', samples, paths });

  /** Render `quanta` blocks, returning one array per channel. */
  const render = (quanta: number): Float32Array[] => {
    const channels = Array.from({ length: PATHS }, () => new Float32Array(quanta * QUANTUM));
    for (let q = 0; q < quanta; q += 1) {
      const block = Array.from({ length: PATHS }, () => new Float32Array(QUANTUM));
      processor.process([], [block]);
      for (let c = 0; c < PATHS; c += 1) {
        channels[c]!.set(block[c]!, q * QUANTUM);
      }
    }
    return channels;
  };

  return { processor, push, send, render, statuses };
}

/** Interleave three generators into frames the worklet accepts. */
function frames(count: number, make: (index: number) => [number, number, number]): Float32Array {
  const out = new Float32Array(count * PATHS);
  for (let i = 0; i < count; i += 1) {
    const [a, b, c] = make(i);
    out[i * PATHS] = a;
    out[i * PATHS + 1] = b;
    out[i * PATHS + 2] = c;
  }
  return out;
}

/** A tone on path 0, and the same tone doubled and tripled on paths 1 and 2. */
function scaledTone(count: number, hz: number): Float32Array {
  return frames(count, (i) => {
    const v = Math.sin((2 * Math.PI * hz * i) / SOURCE_RATE);
    return [v, 2 * v, 3 * v];
  });
}

/** The same tone, but continuing its phase across successive blocks. */
function toneFeeder(hz: number): (count: number) => Float32Array {
  let start = 0;
  return (count) => {
    const block = frames(count, (i) => {
      const v = Math.sin((2 * Math.PI * hz * (start + i)) / SOURCE_RATE);
      return [v, 2 * v, 3 * v];
    });
    start += count;
    return block;
  };
}

/**
 * Drive the worklet the way the worker does: a nominal block of frames for
 * every block rendered.
 *
 * Handing it one enormous buffer and then rendering against it is not the same
 * measurement. The buffer would drain steadily through the run, the drift trim
 * would follow it down, and the read step would sweep — which is frequency
 * modulation, and would put sidebands into the spectrum that only exist because
 * of how the test fed it. Anything measuring pitch or spectral purity has to
 * hold the buffer still first.
 */
function steady(deviceRateHz: number, hz: number, quanta: number) {
  const worklet = load(deviceRateHz);
  const feed = toneFeeder(hz);
  const perQuantum = Math.round((QUANTUM * SOURCE_RATE) / deviceRateHz);

  worklet.push(feed(PRIME));
  const channels = Array.from({ length: PATHS }, () => new Float32Array(quanta * QUANTUM));
  for (let q = 0; q < quanta; q += 1) {
    const block = worklet.render(1);
    for (let c = 0; c < PATHS; c += 1) {
      channels[c]!.set(block[c]!, q * QUANTUM);
    }
    worklet.push(feed(perQuantum));
  }
  return { ...worklet, channels };
}

/** Frequency of the strongest component, interpolated across the peak bin. */
function dominantHz(samples: Float32Array, deviceRateHz: number): number {
  const n = 1 << Math.floor(Math.log2(samples.length));
  const re = new Float64Array(n);
  const im = new Float64Array(n);
  for (let i = 0; i < n; i += 1) {
    re[i] = samples[i]! * 0.5 * (1 - Math.cos((2 * Math.PI * i) / n));
  }
  fft(re, im);

  const magnitude = (k: number) => Math.hypot(re[k]!, im[k]!);
  let peak = 1;
  for (let k = 2; k < n / 2; k += 1) {
    if (magnitude(k) > magnitude(peak)) peak = k;
  }
  // Quadratic interpolation on the log magnitudes, so the answer is not
  // quantised to the bin grid.
  const [left, centre, right] = [
    Math.log(magnitude(peak - 1) + 1e-30),
    Math.log(magnitude(peak) + 1e-30),
    Math.log(magnitude(peak + 1) + 1e-30),
  ];
  const offset = (0.5 * (left - right)) / (left - 2 * centre + right);
  return ((peak + offset) * deviceRateHz) / n;
}

/** In-place radix-2 FFT, for the imaging measurement. */
function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i += 1) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j |= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j]!, re[i]!];
      [im[i], im[j]] = [im[j]!, im[i]!];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    for (let base = 0; base < n; base += len) {
      for (let k = 0; k < len / 2; k += 1) {
        const angle = (-2 * Math.PI * k) / len;
        const wr = Math.cos(angle);
        const wi = Math.sin(angle);
        const lo = base + k;
        const hi = lo + len / 2;
        const vr = re[hi]! * wr - im[hi]! * wi;
        const vi = re[hi]! * wi + im[hi]! * wr;
        re[hi] = re[lo]! - vr;
        im[hi] = im[lo]! - vi;
        re[lo] = re[lo]! + vr;
        im[lo] = im[lo]! + vi;
      }
    }
  }
}

/**
 * Worst non-signal component, in dB relative to the signal.
 *
 * Everything outside the main lobe of the input tone is something the chain
 * added: at this point in the graph the only candidate is the resampler.
 */
function worstSpurDbc(samples: Float32Array, signalHz: number, deviceRateHz: number): number {
  const n = 1 << Math.floor(Math.log2(samples.length));
  const re = new Float64Array(n);
  const im = new Float64Array(n);
  for (let i = 0; i < n; i += 1) {
    re[i] = samples[i]! * 0.5 * (1 - Math.cos((2 * Math.PI * i) / n));
  }
  fft(re, im);

  const binHz = deviceRateHz / n;
  const signalBin = Math.round(signalHz / binHz);
  let signal = 0;
  let spur = 0;
  for (let k = 1; k < n / 2; k += 1) {
    const power = re[k]! * re[k]! + im[k]! * im[k]!;
    if (Math.abs(k - signalBin) <= 3) signal = Math.max(signal, power);
    // Ignore DC and the first few bins: the fade envelope at the start of
    // playback is a real low-frequency term and is not an imaging product.
    else if (k > 8) spur = Math.max(spur, power);
  }
  return 10 * Math.log10(spur / Math.max(signal, 1e-30));
}

describe.each([48_000, 44_100])('the worklet at %i Hz', (deviceRateHz) => {
  it('stays silent until its cushion is filled', () => {
    // Playing the instant the first block lands leaves the buffer hovering near
    // empty and running dry several times a second, which is heard as a steady
    // crackle.
    const { push, render } = load(deviceRateHz);
    push(scaledTone(PRIME - QUANTUM, 300));
    const [channel] = render(4);
    expect(Math.max(...channel!)).toBe(0);
    expect(Math.min(...channel!)).toBe(0);
  });

  it('resamples at the ratio between the two clocks', () => {
    // The rate check that matters: a tone that goes in at 300 Hz must come out
    // at 300 Hz, whatever the device rate. Getting the ratio inverted, or
    // reading `sampleRate` as the source rate, produces a signal that is
    // perfectly clean and at the wrong pitch — which sounds like an engine
    // running at the wrong speed rather than like a bug.
    const { channels } = steady(deviceRateHz, 300, 128);
    // Past the fade-in, where the envelope is at one.
    const measured = dominantHz(channels[0]!.subarray(QUANTUM * 8), deviceRateHz);
    // Within the drift trim's own one percent ceiling, which is a real and
    // deliberate departure from the nominal ratio.
    expect(Math.abs(measured - 300) / 300).toBeLessThan(0.01);
  });

  it('keeps the three paths sample-aligned through the resampler', () => {
    // The property the interleaved ring exists to guarantee. Linear
    // interpolation is linear, so if every path is read at the same fractional
    // position, an input that is exactly doubled comes out exactly doubled. Any
    // drift between the paths — a per-path cursor, a partial frame, a dropped
    // sample on one channel — breaks that exactly.
    const { channels } = steady(deviceRateHz, 700, 60);
    const [first, second, third] = channels;

    for (let i = 0; i < first!.length; i += 1) {
      expect(Math.abs(second![i]! - 2 * first![i]!)).toBeLessThan(1e-6);
      expect(Math.abs(third![i]! - 3 * first![i]!)).toBeLessThan(1e-6);
    }
  });

  it('adds nothing above −38 dB relative to the signal', () => {
    // Linear interpolation images. The question is only whether the images sit
    // under the engine, and the answer has to be re-measured whenever the solver
    // gains top end — an interpolator ample for a signal stopping at 500 Hz need
    // not stay ample at 4 kHz. Measured here rather than trusted from a comment.
    const { channels } = steady(deviceRateHz, 900, 256);
    const spur = worstSpurDbc(channels[0]!.subarray(QUANTUM * 16), 900, deviceRateHz);
    expect(spur).toBeLessThan(-38);
  });

  it('outputs silence when it runs dry rather than repeating itself', () => {
    // Because the samples *are* the simulation, a stall must be audible. The
    // alternative — looping the last block — disguises a stall as an engine
    // holding a note, which is the one failure mode that cannot be debugged by
    // listening.
    const { push, render, statuses } = load(deviceRateHz);
    push(scaledTone(PRIME + QUANTUM * 4, 300));
    render(40);

    // Nothing more arrives. After the fade completes the output must be exactly
    // zero, not a repeat.
    const [channel] = render(40);
    const tail = channel!.subarray(channel!.length - QUANTUM * 4);
    expect(Math.max(...tail.map(Math.abs))).toBe(0);

    const last = statuses.at(-1)!;
    expect(last.underruns).toBeGreaterThan(0);
  });

  it('fades rather than cutting, at both ends of a dry spell', () => {
    // A step to or from silence is broadband, and after the cab filters it rings
    // at the cutoff and is heard as a crack. That happens exactly when the
    // engine is started or stopped.
    const { push, render } = load(deviceRateHz);
    push(scaledTone(PRIME + QUANTUM, 300));
    const [channel] = render(60);

    let worstStep = 0;
    for (let i = 1; i < channel!.length; i += 1) {
      worstStep = Math.max(worstStep, Math.abs(channel![i]! - channel![i - 1]!));
    }
    // A 300 Hz tone at 48 kHz steps by at most about 0.04 between samples; a cut
    // to silence would step by the full amplitude.
    expect(worstStep).toBeLessThan(0.2);
  });

  it('drops whole frames when overrun, and stays aligned', () => {
    // Dropping a single sample rather than a frame would slide the paths apart
    // for the rest of the session, and a fixed offset between the exhaust and
    // the block is not something a listener hears as anything but wrong.
    const { push, render, statuses } = load(deviceRateHz);
    // The ring holds 65536 frames.
    push(scaledTone(70_000, 500));
    render(REPORT_INTERVAL);

    const last = statuses.at(-1)!;
    expect(last.overflowed).toBeGreaterThan(0);
    expect(last.received).toBe(70_000);

    const [first, second] = render(REPORT_INTERVAL);
    for (let i = 0; i < first!.length; i += 1) {
      expect(Math.abs(second![i]! - 2 * first![i]!)).toBeLessThan(1e-6);
    }
  });

  it('refuses a block with the wrong number of paths instead of guessing', () => {
    // Reading three-path frames as two produces a signal that is neither silent
    // nor recognisably wrong, and shuffles which cab transfer each path gets.
    const { push, render, statuses } = load(deviceRateHz);
    push(new Float32Array(600), 2);
    render(REPORT_INTERVAL);
    const last = statuses.at(-1)!;
    expect(last.malformed).toBe(1);
    expect(last.received).toBe(0);
  });

  it('trims the read rate towards the buffer target rather than away from it', () => {
    // The simulation is paced by the worker's wall clock and playback by the
    // device's; nothing keeps them in step, so without a trim the buffer either
    // empties into underruns or fills into latency. The trim must push the right
    // way: a deep buffer has to be read faster than a shallow one.
    //
    // Driven as the worker drives it: a nominal block of frames arrives for
    // every block rendered, so the buffer stays near wherever it started and the
    // trim is being asked to act on a standing offset rather than on a
    // transient.
    const quanta = REPORT_INTERVAL * 2;
    const consumption = (depth: number) => {
      const { push, render, statuses } = load(deviceRateHz);
      push(scaledTone(depth, 300));
      let delivered = depth;
      const perQuantum = Math.round((QUANTUM * SOURCE_RATE) / deviceRateHz);
      for (let q = 0; q < quanta; q += 1) {
        render(1);
        push(scaledTone(perQuantum, 300));
        delivered += perQuantum;
      }
      const last = statuses.at(-1)!;
      expect(last.underruns).toBe(0);
      return delivered - last.buffered;
    };

    // At the target depth the trim is zero by construction; well above it, the
    // trim is against its one percent ceiling.
    const atTarget = consumption(PRIME);
    const deep = consumption(PRIME * 8);
    expect(deep).toBeGreaterThan(atTarget);

    // And the correction stays small: one percent is about a sixth of a
    // semitone at worst, inaudible on a noise-like source and a hundred times
    // more correction than crystal drift needs.
    expect((deep - atTarget) / atTarget).toBeLessThan(0.02);
  });

  it('takes the source rate from the message it is told to', () => {
    // The worker sends the rate the solver actually runs at. If a configuration
    // ever changes the fixed step, the worklet has to follow it rather than
    // holding the 40 kHz default.
    const { push, send, render } = load(deviceRateHz);
    send({ type: 'sourceRate', value: 20_000 });
    push(scaledTone(supply(128, deviceRateHz), 300));
    const [channel] = render(128);

    // Half the source rate is half the playback speed, so the 300 Hz tone comes
    // out at 150 Hz. Two percent, because this run is fed once rather than
    // maintained, so the buffer drains and the trim moves with it.
    const measured = dominantHz(channel!.subarray(QUANTUM * 8), deviceRateHz);
    expect(Math.abs(measured - 150) / 150).toBeLessThan(0.02);
  });

  it('discards everything on a flush, including the interpolator state', () => {
    // A flush follows a reset or an engine change. Leaving the straddling frames
    // behind would interpolate the first sample of the new engine against the
    // last of the old one, which is a click precisely when the graph is trying
    // not to make one.
    const { push, send, render } = load(deviceRateHz);
    push(scaledTone(PRIME * 2, 300));
    render(20);
    send({ type: 'flush' });

    const [channel] = render(4);
    expect(Math.max(...channel!.map(Math.abs))).toBe(0);
  });
});
