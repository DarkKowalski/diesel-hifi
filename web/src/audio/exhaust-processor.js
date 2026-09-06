/**
 * Exhaust audio worklet.
 *
 * The samples this plays are not synthesised. They come from the solver, which
 * runs at a 25 us fixed step — a 40 kHz sample rate — and emits one sample per
 * step taken from the computed blowdown across the exhaust ports. This file only
 * has to buffer them and put them on the device's clock.
 *
 * Deliberately dependency-free plain JavaScript, with no imports. An
 * `AudioWorklet` module runs in its own realm, module support varies, and the
 * build must stay self-contained; a file with no imports works either way and is
 * emitted verbatim as a hashed local asset.
 *
 * Two jobs beyond buffering:
 *
 *   - **Resample.** The solver's rate is fixed by its physics; the device's rate
 *     is whatever the browser gives us. Linear interpolation between the two
 *     needs no configuration, which is why the graph never asks for a particular
 *     `sampleRate`.
 *
 *     It was worth rechecking once the solver gained real content above 2 kHz,
 *     because an interpolator that was ample for a signal stopping at 500 Hz need
 *     not stay ample. Measured at 40 kHz to 48 kHz, the worst imaging product is
 *     −65 dBc for a 900 Hz input, −46 dBc at 2.6 kHz and −38.5 dBc at 3.8 kHz —
 *     the last landing at 11.8 kHz, where the cab stage's low pass takes another
 *     20-odd dB off it. That is well under the engine it sits beneath, so linear
 *     interpolation stays. The measurement is recorded here because the next
 *     person to add top end should repeat it rather than trust it.
 *   - **Underrun honestly.** If the simulation stalls, output silence and count
 *     it. Repeating the last block would paper over a real stall, and the sound
 *     glitching when the simulation glitches is correct — the audio *is* the
 *     simulation.
 */

/** Ring capacity in samples: about 1.5 s at 40 kHz, well past scheduler jitter. */
const CAPACITY = 65536;

/**
 * Samples to accumulate before playback starts, and after any dry spell.
 *
 * About 60 ms at 40 kHz. The worker delivers in ~8 ms ticks whose arrival is at
 * the mercy of the scheduler, so playing the instant the first block lands
 * leaves the buffer hovering near empty and running dry several times a second —
 * audible as a steady crackle. Waiting for a cushion trades a barely noticeable
 * start-up delay for continuous output.
 */
const PRIME_SAMPLES = 2400;

/**
 * Samples over which output fades between silence and playing.
 *
 * About 5 ms. Cutting straight to zero when the buffer runs dry, or straight to
 * full when it refills, is a step discontinuity — and a step is broadband, so
 * after the muffler filter it rings at the cutoff and is heard as a crack. That
 * happens exactly when the engine is started or stopped, because that is when
 * amplitude changes fastest and the buffer is most likely to run dry.
 */
const FADE_SAMPLES = 256;

/**
 * How hard the read rate is nudged to hold the buffer near its target, and how
 * far it may be nudged.
 *
 * The simulation is paced by the worker's wall clock; playback is paced by the
 * audio device's own clock. Nothing keeps those two in step, so the buffer
 * drifts — slowly emptying until it underruns, or slowly filling until latency
 * is unpleasant. Trimming the read rate by a fraction of a percent locks it in
 * place. A one percent ceiling is a pitch change of about a sixth of a
 * semitone at worst, inaudible on a noise-like source, and a hundred times more
 * correction than crystal drift actually needs.
 */
const TRIM_GAIN = 0.05;
const MAX_TRIM = 0.01;

/** How often to report status back to the main thread, in render quanta. */
const REPORT_INTERVAL = 32;

class ExhaustProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();

    const parameters = (options && options.processorOptions) || {};
    /** Rate the incoming samples were produced at. */
    this.sourceRateHz = parameters.sourceRateHz || 40000;

    this.buffer = new Float32Array(CAPACITY);
    this.write = 0;
    this.read = 0;
    this.available = 0;

    /**
     * Read position between `prevSample` and `nextSample`, in `[0, 1)`.
     *
     * The two straddling samples are kept so output can be interpolated between
     * them. Holding the nearest one instead is a zero-order hold, and at a
     * non-integer rate ratio that is a staircase whose steps vary in width —
     * broadband imaging noise well above anything the simulation produced.
     */
    this.fraction = 0;
    this.prevSample = 0;
    this.nextSample = 0;
    /** Most recent interpolated value, held through fades. */
    this.lastSample = 0;

    this.underruns = 0;
    this.received = 0;
    this.overflowed = 0;
    this.quanta = 0;
    this.gain = 1;
    /** Filling the cushion; output silence until it is full. */
    this.priming = true;
    /** Output envelope, 0 while silent and 1 while playing. */
    this.envelope = 0;

    this.port.onmessage = (event) => this.onMessage(event.data);
  }

  onMessage(message) {
    if (!message) return;

    if (message.type === 'samples' && message.samples) {
      this.push(message.samples);
      return;
    }
    if (message.type === 'gain' && typeof message.value === 'number') {
      this.gain = Math.max(0, Math.min(1, message.value));
      return;
    }
    if (message.type === 'sourceRate' && typeof message.value === 'number') {
      this.sourceRateHz = message.value;
      return;
    }
    if (message.type === 'flush') {
      this.write = 0;
      this.read = 0;
      this.available = 0;
      this.fraction = 0;
      this.prevSample = 0;
      this.nextSample = 0;
      this.lastSample = 0;
      this.priming = true;
    }
  }

  push(samples) {
    this.received += samples.length;
    for (let i = 0; i < samples.length; i += 1) {
      this.buffer[this.write] = samples[i];
      this.write = (this.write + 1) % CAPACITY;
      if (this.available === CAPACITY) {
        // Full: the oldest sample goes. Dropping the stale end keeps latency
        // bounded, which matters more than keeping every sample.
        this.read = (this.read + 1) % CAPACITY;
        this.overflowed += 1;
      } else {
        this.available += 1;
      }
    }
  }

  /** Take one sample, or return null when the buffer has run dry. */
  take() {
    if (this.available === 0) return null;
    const sample = this.buffer[this.read];
    this.read = (this.read + 1) % CAPACITY;
    this.available -= 1;
    return sample;
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) return true;

    const channel = output[0];
    const fadeStep = 1 / FADE_SAMPLES;

    // Hold output at silence until the cushion is filled, but fade rather than
    // cut: the envelope still has to run so a fade-out completes.
    if (this.priming) {
      if (this.available < PRIME_SAMPLES) {
        for (let i = 0; i < channel.length; i += 1) {
          this.envelope = Math.max(0, this.envelope - fadeStep);
          channel[i] = this.lastSample * this.gain * this.envelope;
        }
        for (let c = 1; c < output.length; c += 1) output[c].set(channel);
        this.quanta += 1;
        this.report();
        return true;
      }
      this.priming = false;
    }

    // Hold the buffer at its target depth against clock drift between the
    // simulation's pacing and the device's.
    const drift = (this.available - PRIME_SAMPLES) / PRIME_SAMPLES;
    const trim = Math.max(-MAX_TRIM, Math.min(MAX_TRIM, drift * TRIM_GAIN));
    const step = (this.sourceRateHz / sampleRate) * (1 + trim);
    let starved = false;

    for (let i = 0; i < channel.length; i += 1) {
      if (!starved) {
        // Interpolate between the samples either side of the read position.
        this.lastSample =
          this.prevSample + (this.nextSample - this.prevSample) * this.fraction;

        this.fraction += step;
        while (this.fraction >= 1) {
          const next = this.take();
          if (next === null) {
            starved = true;
            this.fraction = 0;
            break;
          }
          this.prevSample = this.nextSample;
          this.nextSample = next;
          this.fraction -= 1;
        }
      }
      // Fade towards silence once dry, and back up once playing again. Holding
      // the last sample under a falling envelope decays to zero instead of
      // stepping to it.
      const target = starved ? 0 : 1;
      this.envelope =
        target > this.envelope
          ? Math.min(1, this.envelope + fadeStep)
          : Math.max(0, this.envelope - fadeStep);
      channel[i] = this.lastSample * this.gain * this.envelope;
    }

    if (starved) {
      this.underruns += 1;
      // Rebuild the cushion rather than limping along one block at a time,
      // which would otherwise underrun again immediately and keep crackling.
      this.priming = true;
    }

    // Mono source, so mirror into any remaining channels.
    for (let c = 1; c < output.length; c += 1) {
      output[c].set(channel);
    }

    this.quanta += 1;
    this.report();
    return true;
  }

  report() {
    if (this.quanta % REPORT_INTERVAL !== 0) return;
    this.port.postMessage({
      type: 'status',
      buffered: this.available,
      underruns: this.underruns,
      received: this.received,
      overflowed: this.overflowed,
    });
  }
}

registerProcessor('exhaust-processor', ExhaustProcessor);
