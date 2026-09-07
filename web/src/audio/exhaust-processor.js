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
 * The samples arrive **interleaved by radiating path** — exhaust, block, body —
 * and leave on one output with that many channels, which the graph splits into
 * three chains so each can be given its own cab transfer. They are three mono
 * signals rather than a spatial layout; the stereo image is made downstream by
 * the panned reflection taps and the stereo convolver.
 *
 * One ring buffer and one fractional read position serve all of them, and that
 * is the point rather than an economy. The three paths are the same instant of
 * the same engine, so they have to stay sample-aligned: three buffers with three
 * sets of cursors could be filled or drained unevenly and slide apart, and a
 * fixed delay between the exhaust and the block is not something a listener
 * would hear as anything but wrong. Interleaving makes misalignment
 * unrepresentable.
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

/** Ring capacity in frames: about 1.5 s at 40 kHz, well past scheduler jitter. */
const CAPACITY = 65536;

/**
 * Radiating paths per frame, and so output channels.
 *
 * Fixed here rather than taken from the message, because the ring is allocated
 * once in the constructor and the node's channel count is fixed when the graph
 * is built. The worker sends `paths` with every block and `push` checks it, so a
 * mismatch is reported rather than silently de-interleaved wrongly.
 */
const PATHS = 3;

/**
 * Frames to accumulate before playback starts, and after any dry spell.
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

    this.buffer = new Float32Array(CAPACITY * PATHS);
    /** Write and read cursors, in frames. */
    this.write = 0;
    this.read = 0;
    this.available = 0;
    /** Blocks rejected for arriving with the wrong number of paths. */
    this.malformed = 0;

    /**
     * Read position between `prevSample` and `nextSample`, in `[0, 1)`.
     *
     * The two straddling samples are kept so output can be interpolated between
     * them. Holding the nearest one instead is a zero-order hold, and at a
     * non-integer rate ratio that is a staircase whose steps vary in width —
     * broadband imaging noise well above anything the simulation produced.
     */
    this.fraction = 0;
    /**
     * The two straddling frames, and the most recent interpolated one.
     *
     * One `fraction` for all three paths, so they cannot drift: whatever the
     * read position is, it is the same read position for every path.
     */
    this.prevFrame = new Float32Array(PATHS);
    this.nextFrame = new Float32Array(PATHS);
    /** Most recent interpolated frame, held through fades. */
    this.lastFrame = new Float32Array(PATHS);
    /** Landing place for a frame taken from the ring; avoids allocating here. */
    this.scratchFrame = new Float32Array(PATHS);

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
      this.push(message.samples, message.paths);
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
      this.prevFrame.fill(0);
      this.nextFrame.fill(0);
      this.lastFrame.fill(0);
      this.priming = true;
    }
  }

  /**
   * Append interleaved frames.
   *
   * A block whose path count disagrees with this processor's is refused rather
   * than de-interleaved on a guess: reading three-path frames as two, or the
   * reverse, produces a signal that is neither silent nor recognisably wrong,
   * and shuffles which cab transfer each path receives. Counted so it can be
   * seen rather than swallowed.
   */
  push(samples, paths) {
    if (paths !== undefined && paths !== PATHS) {
      this.malformed += 1;
      return;
    }
    const frames = Math.floor(samples.length / PATHS);
    this.received += frames;
    for (let f = 0; f < frames; f += 1) {
      const base = this.write * PATHS;
      for (let p = 0; p < PATHS; p += 1) {
        this.buffer[base + p] = samples[f * PATHS + p];
      }
      this.write = (this.write + 1) % CAPACITY;
      if (this.available === CAPACITY) {
        // Full: the oldest frame goes, all paths together. Dropping the stale
        // end keeps latency bounded, which matters more than keeping every
        // frame — and dropping a whole frame is what keeps the paths aligned.
        this.read = (this.read + 1) % CAPACITY;
        this.overflowed += 1;
      } else {
        this.available += 1;
      }
    }
  }

  /** Copy the next frame into `into`, or return false when the ring is dry. */
  take(into) {
    if (this.available === 0) return false;
    const base = this.read * PATHS;
    for (let p = 0; p < PATHS; p += 1) {
      into[p] = this.buffer[base + p];
    }
    this.read = (this.read + 1) % CAPACITY;
    this.available -= 1;
    return true;
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) return true;

    const frameLength = output[0].length;
    // However many channels the graph actually gave us. Normally PATHS; guarded
    // so a mismatch writes what it can rather than reading past an array.
    const channels = Math.min(output.length, PATHS);
    const fadeStep = 1 / FADE_SAMPLES;

    // Hold output at silence until the cushion is filled, but fade rather than
    // cut: the envelope still has to run so a fade-out completes.
    if (this.priming) {
      if (this.available < PRIME_SAMPLES) {
        for (let i = 0; i < frameLength; i += 1) {
          this.envelope = Math.max(0, this.envelope - fadeStep);
          const level = this.gain * this.envelope;
          for (let c = 0; c < channels; c += 1) {
            output[c][i] = this.lastFrame[c] * level;
          }
        }
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

    for (let i = 0; i < frameLength; i += 1) {
      if (!starved) {
        // Interpolate between the frames either side of the read position. One
        // position and one fraction for every path, which is what keeps them
        // aligned through the resampling as well as through the buffering.
        for (let p = 0; p < PATHS; p += 1) {
          this.lastFrame[p] =
            this.prevFrame[p] + (this.nextFrame[p] - this.prevFrame[p]) * this.fraction;
        }

        this.fraction += step;
        while (this.fraction >= 1) {
          // Take into scratch *before* advancing, so a dry ring leaves the two
          // straddling frames exactly as they were. Advancing first and undoing
          // it on failure would overwrite `nextFrame` with `prevFrame` and lose
          // a real sample, which is a click at the moment of an underrun —
          // precisely where the fade envelope is trying to avoid one.
          if (!this.take(this.scratchFrame)) {
            starved = true;
            this.fraction = 0;
            break;
          }
          this.prevFrame.set(this.nextFrame);
          this.nextFrame.set(this.scratchFrame);
          this.fraction -= 1;
        }
      }
      // Fade towards silence once dry, and back up once playing again. Holding
      // the last frame under a falling envelope decays to zero instead of
      // stepping to it.
      const target = starved ? 0 : 1;
      this.envelope =
        target > this.envelope
          ? Math.min(1, this.envelope + fadeStep)
          : Math.max(0, this.envelope - fadeStep);
      const level = this.gain * this.envelope;
      for (let c = 0; c < channels; c += 1) {
        output[c][i] = this.lastFrame[c] * level;
      }
    }

    if (starved) {
      this.underruns += 1;
      // Rebuild the cushion rather than limping along one block at a time,
      // which would otherwise underrun again immediately and keep crackling.
      this.priming = true;
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
      malformed: this.malformed,
    });
  }
}

registerProcessor('exhaust-processor', ExhaustProcessor);
