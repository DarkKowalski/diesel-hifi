/**
 * Level-matched A/B between the running engine and local audio files.
 *
 * The reason this exists is that **an untrimmed comparison is not a
 * comparison.** Louder wins. Every listening judgement made against a reference
 * recording at whatever level it happened to be mastered at is a judgement about
 * which one is louder, and the difference does not need to be large: a decibel
 * is enough to swing a preference and is far too small to notice as a level
 * difference.
 *
 * So the comparison here is switched, not mixed, and matched before it is
 * switched. One source is audible at a time, both at the same measured loudness,
 * and the match is a number on screen rather than a slider someone set by ear.
 *
 * Three sources:
 *
 * | Source | What it is |
 * |---|---|
 * | `engine` | the simulation, live, through whichever stage is selected |
 * | `reference` | a local recording of a real engine |
 * | `candidate` | a second local file — usually an exported capture of another version |
 *
 * The two file slots are the same mechanism with different names. `reference`
 * is what the model is being compared *against*; `candidate` is what it is being
 * compared *with*, which is how a change is judged: load the previous version's
 * `mix.wav` from `audio_capture`, and switch between it and the live engine.
 *
 * **Files are read locally and never leave the browser.** No upload, no fetch,
 * no network of any kind — the deployment has no backend to send anything to,
 * and the references are somebody else's recordings.
 *
 * This module is pure arithmetic and holds no Web Audio objects, so it can be
 * tested under Node. The graph that acts on it is in `audioEngine.ts`.
 */

/** Which source is audible. */
export type CompareSource = 'engine' | 'reference' | 'candidate';

/** The two slots a local file can be loaded into. */
export type ClipSlot = Exclude<CompareSource, 'engine'>;

export const CLIP_SLOTS: readonly ClipSlot[] = ['reference', 'candidate'] as const;

/** What is known about a loaded clip, all measured from its own samples. */
export interface ClipInfo {
  /** File name, for display. Never sent anywhere. */
  name: string;
  durationS: number;
  sampleRateHz: number;
  channels: number;
  /** RMS over the whole clip, in dBFS. */
  levelDb: number;
  /** Largest absolute sample, in dBFS. */
  peakDb: number;
}

/**
 * Level floor, in dBFS.
 *
 * Digital silence has no logarithm. Everything here floors rather than returning
 * negative infinity, so a silent clip is a number that can be displayed and
 * compared instead of a hole in the arithmetic.
 */
export const SILENCE_DB = -120;

/**
 * How far the match gain is allowed to travel, in decibels.
 *
 * A match is a trim, not a rescue. If a clip needs 30 dB of correction it is
 * either near silence or clipped, and quietly applying 30 dB would hide that
 * while making a comparison that means nothing. The gain is clamped and the
 * caller can see it hit the stop.
 */
export const MATCH_LIMIT_DB = 24;

/** Decibels from an amplitude, floored at [`SILENCE_DB`]. */
export function toDb(amplitude: number): number {
  if (!Number.isFinite(amplitude) || amplitude <= 0) return SILENCE_DB;
  return Math.max(SILENCE_DB, 20 * Math.log10(amplitude));
}

/** Amplitude from decibels. */
export function fromDb(db: number): number {
  return 10 ** (db / 20);
}

/**
 * RMS and peak of a clip, in dBFS, averaged across its channels.
 *
 * Averaged rather than taken per channel because the comparison is about
 * loudness and a stereo recording of an engine is not two independent signals.
 *
 * This is an unweighted RMS, not a loudness measurement in the broadcast sense.
 * Both sides of the comparison are engine noise in the same rough spectral
 * region, which is the case where the simple measure and the weighted one agree;
 * where they would not — comparing an engine against speech, say — this is not
 * the tool.
 */
export function measureClip(channels: readonly Float32Array[]): { levelDb: number; peakDb: number } {
  let sumSquares = 0;
  let count = 0;
  let peak = 0;
  for (const channel of channels) {
    for (let i = 0; i < channel.length; i += 1) {
      const sample = channel[i]!;
      sumSquares += sample * sample;
      peak = Math.max(peak, Math.abs(sample));
    }
    count += channel.length;
  }
  if (count === 0) return { levelDb: SILENCE_DB, peakDb: SILENCE_DB };
  return { levelDb: toDb(Math.sqrt(sumSquares / count)), peakDb: toDb(peak) };
}

/**
 * Gain that brings a clip to a target level, as a linear multiplier.
 *
 * Clamped to [`MATCH_LIMIT_DB`] either way, and to a ceiling of 1 relative to
 * the clip's own peak so matching cannot drive a clip into clipping: a
 * comparison in which one side is distorted by the comparison is worse than an
 * unmatched one, because the distortion is audible and the operator is not
 * expecting it.
 */
export function matchGain(clip: ClipInfo, targetDb: number): number {
  const wanted = clamp(targetDb - clip.levelDb, -MATCH_LIMIT_DB, MATCH_LIMIT_DB);
  // Headroom left above the clip's own peak, in decibels. A clip peaking at
  // −3 dBFS may be lifted by 3 dB and no more.
  const headroom = Math.max(0, -clip.peakDb);
  return fromDb(Math.min(wanted, headroom));
}

/**
 * Whether a match was able to reach its target, and by how much it missed.
 *
 * Returned separately from the gain so the UI can say "matched" or "as close as
 * the headroom allows" rather than presenting a clamped gain as a match.
 */
export function matchShortfallDb(clip: ClipInfo, targetDb: number): number {
  const wanted = targetDb - clip.levelDb;
  const applied = toDb(matchGain(clip, targetDb));
  return wanted - applied;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

/**
 * The comparison's state, with the file data left out.
 *
 * Held as plain values so the UI can render it and a test can assert on it
 * without either touching an `AudioContext`.
 */
export interface CompareState {
  source: CompareSource;
  clips: Partial<Record<ClipSlot, ClipInfo>>;
  /** Applied gain per slot, linear. */
  gains: Record<ClipSlot, number>;
  /** Level the clips are being matched to, in dBFS. */
  targetDb: number;
  /** Whether the clips loop rather than playing once. */
  loop: boolean;
}

export function initialCompareState(): CompareState {
  return {
    source: 'engine',
    clips: {},
    gains: { reference: 1, candidate: 1 },
    targetDb: SILENCE_DB,
    loop: true,
  };
}

/**
 * Re-derive every slot's gain against a new target level.
 *
 * Called whenever the engine's own output level is re-measured, so a match made
 * at idle does not persist unnoticed into a comparison at full load — the engine
 * moves 20 dB across its range and the reference does not move at all.
 */
export function rematch(state: CompareState, targetDb: number): CompareState {
  const gains = { ...state.gains };
  for (const slot of CLIP_SLOTS) {
    const clip = state.clips[slot];
    if (clip) gains[slot] = matchGain(clip, targetDb);
  }
  return { ...state, targetDb, gains };
}

/**
 * Gain each source should be playing at, given the selected one.
 *
 * Switched rather than crossfaded between different signals: they are not the
 * same sound, so a crossfade would spend its duration playing both at once,
 * which is a mixture and not a comparison. Short ramps at the ends are the
 * graph's business, not this function's.
 */
export function sourceGains(state: CompareState): Record<CompareSource, number> {
  return {
    engine: state.source === 'engine' ? 1 : 0,
    reference: state.source === 'reference' ? state.gains.reference : 0,
    candidate: state.source === 'candidate' ? state.gains.candidate : 0,
  };
}

/** Whether a slot can be selected: it needs a clip in it. */
export function canSelect(state: CompareState, source: CompareSource): boolean {
  return source === 'engine' || state.clips[source] !== undefined;
}
