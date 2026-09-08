/**
 * The truck cockpit as a filter.
 *
 * The solver produces four aligned mechanical sound paths: `Acoustics::push` radiates
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
  /**
   * Time for the low-frequency envelope to fall by 60 dB.
   *
   * Low frequency specifically, because this cab does not decay at one rate.
   * See [`decayHighS`].
   */
  decayS: number;
  /**
   * Time for the envelope **above [`dampingHz`]** to fall by 60 dB.
   *
   * **A truck cab is a small box lined with soft things.** Seats, headliner,
   * carpet, door trim and a bunk are porous absorbers, and porous absorbers are
   * a function of frequency: absorption coefficients for upholstery and carpet
   * run around 0.1 at 125 Hz and 0.6 to 0.9 by 2 kHz. Reverberation time falls
   * roughly as absorption rises, so the top end of a cab dies several times
   * faster than the bottom — which is the single most characteristic thing
   * about how a lined cabin sounds, and why sitting in one is muffled rather
   * than merely quieter.
   *
   * This was one decay constant for every frequency until milestone 14, which
   * is a hard box with equally reflective walls: a tiled bathroom, not a truck.
   * The tail it generated had a flat spectrum, so the cab was returning treble
   * it should have swallowed.
   */
  decayHighS: number;
  /** Corner between the two decay rates. */
  dampingHz: number;
  /**
   * Corner of the one-pole roll-off applied to the noise before anything else.
   *
   * A diffuse tail in a box lined with foam has very little energy at 10 kHz,
   * so this is physically reasonable on its own — but the reason it is
   * *required* is arithmetic. White noise spans to Nyquist, so the share of its
   * energy sitting above [`dampingHz`] depends on the device rate: at 96 kHz
   * more of the tail lands in the fast-decaying band than at 44.1 kHz, and the
   * tail comes out 11% quieter for no reason a listener would accept. Giving the
   * noise a corner of its own fixes its shape in hertz, so the split between the
   * two decay rates stops depending on the machine.
   */
  bandwidthHz: number;
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

/**
 * Which radiating path a per-path stage describes. Order matches the solver.
 *
 * The fourth is the odd one out and is meant to be: it is a separate machine
 * bolted to the outside of the engine, in mesh for a second or two at a time and
 * with a short motor coast-down after release. It gets a stage of its own for the same reason the
 * other three do — it reaches the driver by its own route — and not because the
 * engine has a fourth thing to say.
 */
export const AUDIO_PATHS = ['exhaust', 'block', 'body', 'starter'] as const;
export type AudioPath = (typeof AUDIO_PATHS)[number];

/**
 * Every path audible, which is the default everywhere a solo control exists.
 *
 * Derived from [`AUDIO_PATHS`] rather than written out. It used to be an object
 * literal in two separate files, and adding the starter path broke both — which
 * the type checker caught, but only because `Record<AudioPath, boolean>` demands
 * every key. A literal that has to be updated in step with a list is a literal
 * that will one day not be.
 */
export function allPathsEnabled(): Record<AudioPath, boolean> {
  return Object.fromEntries(AUDIO_PATHS.map((path) => [path, true])) as Record<
    AudioPath,
    boolean
  >;
}

/**
 * How one radiating path reaches the driver.
 *
 * The three do not arrive by the same route, and until they crossed the WASM
 * boundary separately there was no way to say so: one filter chain acted on a
 * signal that had already been added up. The compromises that forced are still
 * legible in the shared stage — its low pass sat at 2.6 kHz because bulkhead
 * clatter had to survive it, which is far too high for a tailpipe several metres
 * away.
 *
 * None of this is published, and none of it is physics. It is a statement about
 * where the driver is sitting relative to three sources.
 */
export interface PathStage {
  /**
   * How loud this path is *at the driver*, relative to how loud it left.
   *
   * The three sources are at three different distances behind three different
   * amounts of steel, glass and rubber, so where a listener sits changes their
   * balance and not only their tone. This is that, and it is deliberately
   * separate from the filters: a shelf that lowers a path also colours it, and
   * the statement being made here is about level.
   *
   * **Cab-only, by construction.** It is applied inside the wet chain, after the
   * per-path solo gains and downstream of the split, so the raw stage stays
   * exactly the sum the solver emitted. A trim that moved the raw stage would be
   * a second balance control masquerading as a listening position, and the raw
   * stage is what every measurement in `README.md` is taken from.
   */
  levelDb: number;
  /** This path's own transfer, before anything shared. */
  filters: FilterStage[];
  /** Propagation delay from the source to the driver's ear. */
  delayS: number;
  /**
   * How much of this path feeds the reflections and the diffuse tail.
   *
   * Zero is meaningful rather than an economy: structure-borne sound does not
   * arrive as a room reflection.
   */
  roomSend: number;
}

export interface CabinSpec {
  /** Per-path transfers, keyed by the solver's channel order. */
  paths: Record<AudioPath, PathStage>;
  /** Shared stages, downstream of the per-path mix. */
  filters: FilterStage[];
  /** Level of the unreflected signal within the wet path. */
  directGain: number;
  taps: ReflectionTap[];
  /**
   * Corner of the one-pole low pass on the early reflections.
   *
   * The same reason as [`ImpulseSpec.decayHighS`], applied a stage earlier. The
   * first thing an exhaust pulse hits on its way round the cab is a seat back, a
   * headliner or a bunk curtain, and what comes off those is not a full-bandwidth
   * copy of what arrived. The taps were exactly that — delayed, attenuated and
   * panned, but spectrally identical to the direct sound — which made the cab
   * *brighter* than the dry signal in the first 12 ms, since two extra copies of
   * the top end arrived on top of it.
   */
  reflectionDampingHz: number;
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
 * The four shared biquads come after the per-path stages and describe the cab
 * as a box and the listener's speaker, not any one source: what the structure
 * will not pass, what it rings on, what a small speaker can actually carry, and
 * what makes a box sound like a box.
 *
 *   - **30 Hz high pass.** Below this the cab is felt rather than heard. It is
 *     real energy — a six fires at 28 Hz at idle — but no ordinary speaker
 *     reproduces it, so it is headroom spent on nothing, and spending it here is
 *     worse than usual because the compressor downstream would duck the audible
 *     band to make room for it.
 *   - **+5 dB low shelf at 150 Hz.** The boom. A cab is a panelled box on air
 *     springs with a low fundamental, and it is the one part of an engine you
 *     hear as much through the seat as through the air.
 *
 *     This was a narrow +5 dB peak at 85 Hz, and it was tuned against a source
 *     that had almost nothing down there — a bump that narrow is a resonance,
 *     and picking one frequency out of a firing comb that sweeps from 28 Hz at
 *     idle to over 100 Hz governed means the boost lands on the fundamental at
 *     one engine speed and between orders at every other. A shelf lifts the
 *     whole region the orders move through, so the weight tracks the engine
 *     instead of appearing at one point in the rev range.
 *   - **+2.5 dB at 200 Hz.** The small-speaker provision, and the one stage here
 *     that is about the listener's hardware rather than about the cab.
 *
 *     A MacBook speaker reproduces essentially nothing below about 150 Hz, and
 *     in-ear monitors are not much better below 50. What carries the pitch on
 *     hardware like that is not the fundamental but its harmonics: the ear
 *     reconstructs a missing fundamental from them, which is why a truck heard
 *     through a laptop still sounds like a truck. Measured on the solver's
 *     output, half to nine tenths of the exhaust path's energy already sits at
 *     the second through tenth orders — 90 to 270 Hz at idle, 140/210/280 Hz at
 *     full load — so this emphasises content that is there rather than
 *     manufacturing content that is not. That distinction is the whole of the
 *     "nothing is added" rule: a filter is allowed, a harmonic generator is not.
 *   - **−2 dB at 600 Hz.** Boxiness. Everything with a hard mid resonance sounds
 *     like a cardboard tube until this is pulled down.
 *
 *     It used to sit at 380 Hz. The block's modal bank now has a mode there —
 *     the bank starts at 210 Hz rather than 480, to give the engine the size a
 *     12.8 litre iron structure ought to have — so a cut at 380 Hz had stopped
 *     removing boxiness and started removing the engine.
 *
 * There is no shared low pass any more, and its absence is the change. It was
 * 1.7 kHz when the solver produced nothing above that, then 2.6 kHz once the
 * structural path put real content at 2.4 and 3.6 kHz — because a cab takes the
 * sharp edge off an exhaust several metres away but does not silence an engine
 * two feet away through the bulkhead. Both figures were one number trying to
 * describe two different routes, and neither could. The per-path stages above
 * describe them separately: 1.6 kHz for the exhaust, 3.2 kHz for the block. A
 * shared low pass on top would only undo that.
 *
 * The two taps are the screen and the door: the first strong reflections in a
 * box this size arrive within about 15 ms, and they are what make the difference
 * between a filtered mono signal and a place. They are panned apart because a
 * cab is not symmetric around the driver, and because two decorrelated copies
 * are what turn a mono source into something with width without any phase trick.
 */
export const CABIN_SPEC: CabinSpec = {
  /**
   * Where each source is, relative to the driver.
   *
   *   - **The exhaust** leaves a stack several metres behind and below the cab
   *     and reaches the driver through the rear wall and a length of outside
   *     air. It is the most muffled of the three and the only one with a
   *     propagation delay worth having: 12 ms is about four metres, and it is
   *     what stops the tailpipe and the block sounding like one source in the
   *     same place. Full room send — it is the path that arrives as a sound in a
   *     space rather than as a sound in the structure.
   *   - **The block** is two feet away through the bulkhead. It is the one thing
   *     genuinely in the cab with the driver, so it keeps its top end: 3.2 kHz
   *     rather than the 2.6 kHz the shared stage had to settle on, plus a little
   *     presence at 1 kHz. A partial room send, because some of it does come
   *     round through the air.
   *   - **The body** arrives through the mounts, the frame and the seat, with no
   *     air path at all. So: no delay, and **no room send whatever**. A
   *     structure-borne path does not arrive as an early reflection or as a
   *     diffuse tail, and reverberating it would be inventing an acoustic route
   *     it does not take. Its low pass is well below anything it produces, and
   *     is there to say that rather than to shape it.
   *
   * ## Why the levels are not all zero
   *
   * They were, until a listener sat in the seat and said the body path wanted
   * bringing up and the other two down. That is the second listening report this
   * project has, and it agrees with the first — *not enough low frequency* — and
   * with the measured reference, which reads 89 to 92% below 80 Hz at idle where
   * the model's raw mix reads 39%.
   *
   * It also agrees with the physics of sitting there rather than beside the
   * truck. A structure-borne path into the seat and floor does not attenuate the
   * way an airborne one does: the exhaust leaves a stack several metres back and
   * has to get in through the cab shell, the block is behind a bulkhead built to
   * keep it out, and the frame is *bolted to the thing the driver is sitting on*.
   * Outside the cab that ordering reverses, which is why this belongs to the
   * listening position and not to the source gains.
   *
   * The split across the three is not arbitrary. The block comes down furthest
   * because it is the clatter path — 68% of it sits in 300 Hz–2 kHz — and the
   * first listening report was that there was too much high frequency. The
   * exhaust is not cut at all, because it carries the firing orders and an
   * exhaust behind the block is how an engine stops sounding like a truck; it is
   * reduced *relative* to the body, which is what was asked for.
   *
   * ## Why these numbers are small, which is the more useful half of the note
   *
   * They are small because they were measured rather than chosen, and the
   * measurement said no. The browser suite holds the cab within 3 dB of the raw
   * stage at **both** idle and load, so that switching stages cannot win a
   * comparison by being louder. A body trim of +6 dB — the first attempt, and
   * the one that sounds like the request — put the cab **+6.0 dB** over raw at
   * idle against −1.9 dB under load, and every intermediate value traded one end
   * for the other. The window is 6 dB wide and any real rebalance opens the
   * spread to about 6 dB on its own.
   *
   * The reason is a property of the *source*, not of this file. The body path
   * sits 1.1 dB behind the exhaust at governed idle and **12.8 dB** behind it at
   * full load, so the same trim is a large change to the mix at one end and
   * almost nothing at the other. A listening-stage gain cannot correct a
   * source-side scaling, which is the lesson already written down in `README.md`
   * as *a path driven by the wrong quantity cannot be fixed with gain* — here in
   * the milder form that a path with the wrong load dependence cannot be fixed
   * with a constant.
   *
   * So what is here is what the cab can honestly contribute: about 2 dB less
   * content above 2 kHz reaching the driver, and the body path 1.5 dB up against
   * an unchanged exhaust and a block 3 dB down. Going further is a source
   * change, and `README.md` records it under **Known deficits**.
   */
  paths: {
    exhaust: {
      levelDb: 0,
      filters: [
        { type: 'lowpass', frequencyHz: 1600, q: 0.7, gainDb: 0 },
        { type: 'peaking', frequencyHz: 400, q: 0.9, gainDb: -3 },
      ],
      delayS: 0.012,
      roomSend: 1.0,
    },
    block: {
      levelDb: -3,
      filters: [
        { type: 'lowpass', frequencyHz: 3200, q: 0.7, gainDb: 0 },
        { type: 'peaking', frequencyHz: 1000, q: 0.9, gainDb: 2 },
      ],
      delayS: 0.002,
      roomSend: 0.4,
    },
    body: {
      levelDb: 1.5,
      filters: [
        { type: 'lowpass', frequencyHz: 450, q: 0.7, gainDb: 0 },
        { type: 'peaking', frequencyHz: 120, q: 0.9, gainDb: 1 },
      ],
      delayS: 0,
      roomSend: 0,
    },
    // Bolted low on the block, a metre or so from the driver's feet, through
    // the bulkhead. Closer than the tailpipe and brighter than either the
    // block or the body: a starter whine is a mechanical mesh in an aluminium
    // housing, so it keeps far more of its top end than a combustion event
    // arriving through iron.
    //
    // A shallow low pass rather than a steep one, because the mesh comb runs
    // from a few hundred hertz to several kilohertz and the whole comb is the
    // sound. The 1.2 kHz lift is the bulkhead panel it radiates through, and
    // the room send is modest: it is a small source close to the listener, so
    // most of what arrives is direct.
    // 3.8 kHz rather than 4: the cab test requires every path to roll its top
    // end off below 4 kHz, and this is the brightest path in the model, so it is
    // the one that would sit on the bound. Meeting the criterion was cheaper
    // than widening it, and it costs nothing — the starter modal bank's highest
    // mode is at 3.1 kHz, so the whole bank still gets through.
    starter: {
      levelDb: -1,
      filters: [
        { type: 'lowpass', frequencyHz: 3800, q: 0.7, gainDb: 0 },
        { type: 'peaking', frequencyHz: 1200, q: 0.9, gainDb: 2 },
      ],
      delayS: 0.003,
      roomSend: 0.3,
    },
  },
  filters: [
    { type: 'highpass', frequencyHz: 35, q: 0.7, gainDb: 0 },
    { type: 'lowshelf', frequencyHz: 150, q: 0.7, gainDb: 5 },
    { type: 'peaking', frequencyHz: 200, q: 0.9, gainDb: 2.5 },
    { type: 'peaking', frequencyHz: 600, q: 1.0, gainDb: -2 },
  ],
  directGain: 0.75,
  taps: [
    { delayS: 0.0073, gain: 0.28, pan: -0.6 },
    { delayS: 0.0119, gain: 0.22, pan: 0.55 },
  ],
  reflectionDampingHz: 1800,
  // 0.3 until milestone 14. A cab that absorbs its top end returns less energy
  // as well as a darker spectrum, and the tail's own normalisation deliberately
  // states only the spectrum — see `impulseChannel`. This is the other half.
  reverbGain: 0.2,
  impulse: {
    durationS: 0.18,
    decayS: 0.13,
    // A quarter of the low-frequency decay. Upholstery, carpet and a headliner
    // absorb several times more at 2 kHz than at 125 Hz, so the treble in a cab
    // is gone while the boom is still going.
    decayHighS: 0.035,
    dampingHz: 900,
    bandwidthHz: 5000,
    predelayS: 0.006,
    seedLeft: 0x5eed_1a7e,
    seedRight: 0x1d5e_a5e7,
  },
  dynamics: {
    inputGain: 0.30,
    thresholdDb: -18,
    kneeDb: 12,
    // Ratio 2 and a 25 ms attack, where these were 3 and 6 ms.
    //
    // A diesel is a series of distinct events, and the edge of each one is most
    // of what makes it sound like an engine rather than like a tone. At 1200 rpm
    // a six fires every 16.7 ms, so a 6 ms attack was biting *inside* every
    // pulse: it caught the leading edge of each firing event and pulled it down,
    // which is exactly the part worth keeping. An attack longer than the firing
    // period lets the transient through and acts on the average instead, which
    // is what a compressor in a listening chain is for.
    ratio: 2,
    attackS: 0.025,
    releaseS: 0.18,
    // Calibrated with inputGain at both browser test points; README records
    // the measured stage spread and the limits of that comparison.
    makeupGain: 2.15,
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

/**
 * One channel of decaying noise, with the leading predelay left silent.
 *
 * **Two decay rates, split at `dampingHz`.** The noise is separated into a low
 * band and a high band by a one-pole filter and its complement, each band is
 * given its own exponential envelope, and the two are added back. The result is
 * a tail whose spectrum grows darker as it decays — which is what a lined box
 * does, and what a single envelope cannot represent however it is tuned.
 *
 * The split is complementary by construction: `low + high` reconstructs the
 * noise exactly, so with equal decay times this reduces to the flat-spectrum
 * tail it replaces. A test asserts that.
 */
function impulseChannel(sampleRate: number, spec: ImpulseSpec, seed: number): Float32Array {
  const length = Math.max(1, Math.round(spec.durationS * sampleRate));
  const predelay = Math.min(length, Math.round(spec.predelayS * sampleRate));
  const random = xorshift32(seed);
  const channel = new Float32Array(length);

  // -60 dB over the decay time, which is what a decay time means.
  const decayLow = Math.max(spec.decayS, 1 / sampleRate);
  const decayHigh = Math.max(spec.decayHighS, 1 / sampleRate);
  const perSampleLow = Math.exp(-6.907_755 / (decayLow * sampleRate));
  const perSampleHigh = Math.exp(-6.907_755 / (decayHigh * sampleRate));

  // One-pole coefficients: one bounding the noise, one splitting the two bands.
  const onePole = (hz: number) => Math.exp((-2 * Math.PI * Math.max(hz, 1)) / sampleRate);
  const limit = onePole(spec.bandwidthHz);
  const pole = onePole(spec.dampingHz);

  // Energy this tail would have carried at one decay rate, accumulated
  // alongside. Dividing by *that* rather than by the tail actually produced is
  // what lets absorption absorb: renormalising the damped tail to unit energy
  // takes the treble out and hands the same energy to the bass, which is a tone
  // control rather than a soft furnishing — and it showed up immediately as a
  // cab that got *louder* at idle the more of its top end it swallowed. The
  // noise is band-limited first so this ratio does not depend on where Nyquist
  // is. With equal decay rates the two energies are the same number and this
  // reduces to the flat tail it replaced.
  let referenceEnergy = 0;

  let limitState = 0;
  let lowState = 0;
  let envelopeLow = 1;
  let envelopeHigh = 1;
  for (let i = predelay; i < length; i += 1) {
    limitState = limitState * limit + random() * (1 - limit);
    const noise = limitState;
    lowState = lowState * pole + noise * (1 - pole);
    const high = noise - lowState;
    channel[i] = lowState * envelopeLow + high * envelopeHigh;
    const reference = noise * envelopeLow;
    referenceEnergy += reference * reference;
    envelopeLow *= perSampleLow;
    envelopeHigh *= perSampleHigh;
  }

  // Normalise to unit energy so the wet level is a property of the room and not
  // of the device sample rate. A 48 kHz device generates half again as many
  // noise samples as a 32 kHz one; without this the same `reverbGain` would be
  // audibly louder on some machines than others.
  //
  if (referenceEnergy > 0) {
    const scale = 1 / Math.sqrt(referenceEnergy);
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
