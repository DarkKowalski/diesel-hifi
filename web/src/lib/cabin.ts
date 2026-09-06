/**
 * The truck cockpit as a filter.
 *
 * Everything the solver produces is a tailpipe signal: `Acoustics::push` radiates
 * the flow leaving the duct and the cylinder pressure ringing the block. That is
 * roughly what a microphone beside the truck would hear, and it is deliberately
 * flat, because the spectrum view has to be able to prove the energy is where a
 * speaker can reproduce it.
 *
 * A driver is not at the pipe mouth. The tailpipe is metres behind and below,
 * the sound reaches the seat through insulated sheet metal, glass, and the
 * structure itself, and it arrives in a hard box about two metres across.
 *
 * This module is the *description* of that path — filter stages, reflection taps,
 * a room impulse — as plain data and plain arithmetic. It builds no audio nodes
 * and touches no Web Audio API, so it runs and is tested under Node. The graph
 * that consumes it lives in `audioEngine.ts`.
 *
 * **Nothing here is published, and nothing here is physics.** The manual says
 * nothing about how this engine sounds and less about how its cab sounds. These
 * are listening choices, and the UI says so. What the stage may *not* do is add
 * anything: no noise bed, no synthesised rumble, no layered samples. Every value
 * that reaches the speaker is still the solver's cylinder pressure, filtered.
 * Silence in, silence out — which the unit tests assert.
 */

/** Which listening position the output is filtered for. */
export type AudioStage = 'raw' | 'cockpit';

/** One biquad in the cab transfer path. */
export interface FilterStage {
  /** Matches `BiquadFilterType`, kept as a plain string so this file stays DOM-free. */
  type: 'highpass' | 'lowpass' | 'peaking' | 'highshelf' | 'lowshelf';
  frequencyHz: number;
  q: number;
  /** Only meaningful for peaking and shelving stages. */
  gainDb: number;
}

/** A discrete early reflection: one delayed, attenuated, panned copy. */
export interface ReflectionTap {
  delayS: number;
  gain: number;
  /** −1 hard left, +1 hard right. */
  pan: number;
}

/** Parameters of the generated room impulse response. */
export interface ImpulseSpec {
  /** Total length of the response. */
  durationS: number;
  /** Time for the envelope to fall by 60 dB. */
  decayS: number;
  /** Gap before the diffuse tail begins; the discrete taps cover this window. */
  predelayS: number;
  /** Explicit seeds — one per channel, so the tail decorrelates into stereo. */
  seedLeft: number;
  seedRight: number;
}

/** Dynamics of a confined space, and the gains that level-match it to dry. */
export interface CabinDynamics {
  /**
   * Trim ahead of the compressor.
   *
   * This exists because level-matching the two stages at one operating point is
   * not level-matching them. Measured with input and makeup as the only trim,
   * the cab path sat level with the dry one under load and **6.3 dB louder at
   * idle** — the compressor was working at the loud end and doing nothing at
   * the quiet end, so a single output gain could only ever match one of them.
   *
   * Input trim is the knob that separates the two: it moves the quiet end
   * decibel for decibel, and the compressed loud end by rather less. Trimming
   * here and making it back up after the compressor brings both ends together.
   */
  inputGain: number;
  thresholdDb: number;
  kneeDb: number;
  ratio: number;
  attackS: number;
  releaseS: number;
  /**
   * Output trim of the whole wet path.
   *
   * Calibrated so switching stages changes the *character* and not the volume.
   * A post-processing switch that is simply louder always wins a blind
   * comparison for the wrong reason, and there are end-to-end assertions that
   * the two stages land within 3 dB of each other at both idle and full load.
   */
  makeupGain: number;
}

export interface CabinSpec {
  filters: FilterStage[];
  /** Level of the unreflected signal within the wet path. */
  directGain: number;
  taps: ReflectionTap[];
  /** Level of the convolved diffuse tail within the wet path. */
  reverbGain: number;
  impulse: ImpulseSpec;
  dynamics: CabinDynamics;
  /** Seconds to crossfade when the stage is switched. */
  crossfadeS: number;
}

/**
 * The cab.
 *
 * The four biquads are the transfer path in order: what the structure will not
 * pass, what it rings on, what makes a small box sound like a box, and what the
 * insulation takes off the top.
 *
 *   - **30 Hz high pass.** Below this the cab is felt rather than heard. It is
 *     real energy — a six fires at 28 Hz at idle — but no ordinary speaker
 *     reproduces it, so it is headroom spent on nothing, and spending it here is
 *     worse than usual because the compressor downstream would duck the audible
 *     band to make room for it.
 *   - **+5 dB at 85 Hz.** The boom. A cab is a panelled box on air springs with a
 *     low fundamental, and it is the one part of an engine you hear as much
 *     through the seat as through the air.
 *   - **−3 dB at 380 Hz.** Boxiness. Everything with a hard mid resonance sounds
 *     like a cardboard tube until this is pulled down.
 *   - **2.6 kHz low pass.** Glass, insulation, and several metres of air.
 *
 *     This was 1.7 kHz, and it was chosen when the solver produced nothing above
 *     it — which made it free. It is not free now. The structural radiation path
 *     puts real content at 2.6 and 3.8 kHz, and clatter is emphatically audible
 *     from a truck's driver seat, especially at idle. A cab takes the sharp edge
 *     off the exhaust; it does not silence the engine two feet away through the
 *     bulkhead, and a figure that did was describing the glass rather than what
 *     a driver hears.
 *
 * The two taps are the screen and the door: the first strong reflections in a
 * box this size arrive within about 15 ms, and they are what make the difference
 * between a filtered mono signal and a place. They are panned apart because a
 * cab is not symmetric around the driver, and because two decorrelated copies
 * are what turn a mono source into something with width without any phase trick.
 */
export const CABIN_SPEC: CabinSpec = {
  filters: [
    { type: 'highpass', frequencyHz: 30, q: 0.7, gainDb: 0 },
    { type: 'peaking', frequencyHz: 85, q: 1.1, gainDb: 5 },
    { type: 'peaking', frequencyHz: 380, q: 1.0, gainDb: -3 },
    { type: 'lowpass', frequencyHz: 2600, q: 0.7, gainDb: 0 },
  ],
  directGain: 0.75,
  taps: [
    { delayS: 0.0073, gain: 0.28, pan: -0.6 },
    { delayS: 0.0119, gain: 0.22, pan: 0.55 },
  ],
  reverbGain: 0.3,
  impulse: {
    durationS: 0.18,
    decayS: 0.13,
    predelayS: 0.006,
    seedLeft: 0x5eed_1a7e,
    seedRight: 0x1d5e_a5e7,
  },
  dynamics: {
    inputGain: 0.42,
    thresholdDb: -18,
    kneeDb: 12,
    ratio: 3,
    attackS: 0.006,
    releaseS: 0.18,
    // 1.9. Measured through the output analyser, this puts the cab +1.5 dB
    // against raw at idle and −1.2 dB under load. It is a measurement rather
    // than a derivation — it depends on the source spectrum — so the browser
    // suite measures both ends rather than trusting the pair.
    makeupGain: 1.9,
  },
  crossfadeS: 0.04,
};

/**
 * Deterministic noise source.
 *
 * The project seeds every source of randomness explicitly, and this is no
 * exception even though it is presentation rather than physics: an impulse
 * response built from `Math.random` is a different room on every page load, so
 * a level or spectrum measurement taken from the output would not be repeatable
 * and the end-to-end assertions below would be measuring luck.
 *
 * Xorshift32. Cheap, dependency-free, and far better than adequate for shaping
 * noise into a room tail.
 */
function xorshift32(seed: number): () => number {
  // Zero is a fixed point of xorshift; anything else is fine.
  let state = seed >>> 0 || 0x9e37_79b9;
  return () => {
    state ^= state << 13;
    state >>>= 0;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    // Map to [-1, 1).
    return state / 0x8000_0000 - 1;
  };
}

/** One channel of decaying noise, with the leading predelay left silent. */
function impulseChannel(sampleRate: number, spec: ImpulseSpec, seed: number): Float32Array {
  const length = Math.max(1, Math.round(spec.durationS * sampleRate));
  const predelay = Math.min(length, Math.round(spec.predelayS * sampleRate));
  const random = xorshift32(seed);
  const channel = new Float32Array(length);

  // -60 dB over the decay time, which is what a decay time means.
  const decay = Math.max(spec.decayS, 1 / sampleRate);
  const perSample = Math.exp(-6.907_755 / (decay * sampleRate));

  let envelope = 1;
  for (let i = predelay; i < length; i += 1) {
    channel[i] = random() * envelope;
    envelope *= perSample;
  }

  // Normalise to unit energy so the wet level is a property of the room and not
  // of the device sample rate. A 48 kHz device generates half again as many
  // noise samples as a 32 kHz one; without this the same `reverbGain` would be
  // audibly louder on some machines than others.
  let sumSquares = 0;
  for (let i = 0; i < length; i += 1) {
    sumSquares += channel[i]! * channel[i]!;
  }
  const rms = Math.sqrt(sumSquares / length);
  if (rms > 0) {
    const scale = 1 / (rms * Math.sqrt(length));
    for (let i = 0; i < length; i += 1) {
      channel[i]! *= scale;
    }
  }

  return channel;
}

/**
 * The cab's diffuse tail, as two decorrelated channels.
 *
 * Exponentially decaying noise rather than a measured response, because there is
 * no measured response of this cab to have — and a short, dense, quiet tail is
 * doing a modest job here anyway. The discrete taps carry the early reflections
 * that give the space its size; this fills in behind them.
 */
export function generateImpulseResponse(
  sampleRate: number,
  spec: ImpulseSpec = CABIN_SPEC.impulse,
): { left: Float32Array; right: Float32Array } {
  return {
    left: impulseChannel(sampleRate, spec, spec.seedLeft),
    right: impulseChannel(sampleRate, spec, spec.seedRight),
  };
}
