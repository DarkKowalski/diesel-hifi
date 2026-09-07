import { describe, expect, it } from 'vitest';

import {
  CLIP_SLOTS,
  MATCH_LIMIT_DB,
  SILENCE_DB,
  canSelect,
  fromDb,
  initialCompareState,
  matchGain,
  matchShortfallDb,
  measureClip,
  rematch,
  sourceGains,
  toDb,
  type ClipInfo,
} from '../src/lib/compare';

/**
 * The comparison's arithmetic.
 *
 * What these assert is the one property the whole feature rests on: a listening
 * comparison that is not level-matched is a comparison of loudness, and louder
 * wins by margins far too small to notice as level. So the matching has to be
 * right, has to be visible when it cannot reach its target, and must never push
 * a clip into clipping to get there.
 */

function clip(overrides: Partial<ClipInfo> = {}): ClipInfo {
  return {
    name: 'reference.wav',
    durationS: 76,
    sampleRateHz: 48_000,
    channels: 2,
    levelDb: -20,
    peakDb: -6,
    ...overrides,
  };
}

/** A sine of a given amplitude, as one channel. */
function tone(amplitude: number, length = 4_096): Float32Array {
  const out = new Float32Array(length);
  for (let i = 0; i < length; i += 1) {
    out[i] = amplitude * Math.sin((2 * Math.PI * 220 * i) / 48_000);
  }
  return out;
}

describe('measuring a clip', () => {
  it('reports RMS and peak in dBFS', () => {
    const { levelDb, peakDb } = measureClip([tone(0.5)]);
    // A sine's RMS is its amplitude over root two.
    expect(levelDb).toBeCloseTo(toDb(0.5 / Math.SQRT2), 1);
    expect(peakDb).toBeCloseTo(toDb(0.5), 1);
  });

  it('averages across channels rather than taking the loudest', () => {
    // A stereo recording of an engine is not two independent signals, and
    // loudness is what is being matched.
    const both = measureClip([tone(0.5), tone(0.25)]);
    const loud = measureClip([tone(0.5)]);
    expect(both.levelDb).toBeLessThan(loud.levelDb);
  });

  it('floors silence rather than returning negative infinity', () => {
    expect(measureClip([new Float32Array(512)]).levelDb).toBe(SILENCE_DB);
    expect(measureClip([]).levelDb).toBe(SILENCE_DB);
    expect(toDb(0)).toBe(SILENCE_DB);
  });
});

describe('matching levels', () => {
  it('brings a clip to the target', () => {
    const gain = matchGain(clip({ levelDb: -20, peakDb: -30 }), -14);
    expect(toDb(gain)).toBeCloseTo(6, 6);
    expect(matchShortfallDb(clip({ levelDb: -20, peakDb: -30 }), -14)).toBeCloseTo(0, 6);
  });

  it('turns a clip down as readily as up', () => {
    expect(toDb(matchGain(clip({ levelDb: -10 }), -20))).toBeCloseTo(-10, 6);
  });

  it('never lifts a clip into clipping', () => {
    // A clip peaking at −3 dBFS has 3 dB of headroom, whatever the match asks
    // for. Distorting one side of a comparison is worse than not matching it:
    // the distortion is audible and the listener is not expecting it.
    const loud = clip({ levelDb: -20, peakDb: -3 });
    const gain = matchGain(loud, 0);
    expect(toDb(gain)).toBeCloseTo(3, 6);
    expect(toDb(gain) + loud.peakDb).toBeLessThanOrEqual(1e-9);
    // And the shortfall says so, so the UI can report it instead of claiming a
    // match it did not make.
    expect(matchShortfallDb(loud, 0)).toBeCloseTo(17, 6);
  });

  it('refuses to travel more than the limit', () => {
    // A clip needing 40 dB is near silence or clipped. Applying it quietly would
    // hide that and produce a comparison that means nothing.
    const quiet = clip({ levelDb: -80, peakDb: -60 });
    expect(toDb(matchGain(quiet, 0))).toBeCloseTo(MATCH_LIMIT_DB, 6);
    expect(matchShortfallDb(quiet, 0)).toBeGreaterThan(0);
  });

  it('re-derives every loaded slot against a new target', () => {
    // The engine moves twenty-odd decibels between idle and full load and the
    // recording does not move at all, so a match made at one operating point is
    // not a match at another.
    let state = initialCompareState();
    state = {
      ...state,
      clips: {
        // Both with plenty of headroom, so the only thing acting here is the
        // target; the headroom ceiling has its own test above.
        reference: clip({ levelDb: -30, peakDb: -16 }),
        candidate: clip({ levelDb: -10, peakDb: -2 }),
      },
    };

    state = rematch(state, -20);
    expect(toDb(state.gains.reference)).toBeCloseTo(10, 6);
    expect(toDb(state.gains.candidate)).toBeCloseTo(-10, 6);

    state = rematch(state, -25);
    expect(toDb(state.gains.reference)).toBeCloseTo(5, 6);
    expect(state.targetDb).toBe(-25);
  });

  it('leaves an empty slot alone', () => {
    const state = rematch(initialCompareState(), -18);
    for (const slot of CLIP_SLOTS) {
      expect(state.gains[slot]).toBe(1);
    }
  });

  it('round-trips decibels and amplitudes', () => {
    expect(fromDb(toDb(0.25))).toBeCloseTo(0.25, 9);
    expect(toDb(fromDb(-13.5))).toBeCloseTo(-13.5, 9);
  });
});

describe('switching source', () => {
  it('plays exactly one source at a time', () => {
    // Switched, not crossfaded. They are not the same sound, so a crossfade
    // would spend its duration playing a mixture, and a mixture is not a
    // comparison.
    let state = initialCompareState();
    state = { ...state, clips: { reference: clip() }, gains: { reference: 0.5, candidate: 1 } };

    expect(sourceGains(state)).toEqual({ engine: 1, reference: 0, candidate: 0 });

    state = { ...state, source: 'reference' };
    const gains = sourceGains(state);
    expect(gains.engine).toBe(0);
    expect(gains.reference).toBe(0.5);
    expect(gains.candidate).toBe(0);
  });

  it('offers only slots that have something in them', () => {
    const state = { ...initialCompareState(), clips: { reference: clip() } };
    expect(canSelect(state, 'engine')).toBe(true);
    expect(canSelect(state, 'reference')).toBe(true);
    // Selecting an empty slot would be silence, which looks exactly like a
    // broken comparison.
    expect(canSelect(state, 'candidate')).toBe(false);
  });

  it('starts on the engine with nothing loaded', () => {
    const state = initialCompareState();
    expect(state.source).toBe('engine');
    expect(state.clips).toEqual({});
    expect(state.loop).toBe(true);
  });
});
