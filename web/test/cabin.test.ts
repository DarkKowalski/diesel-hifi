import { describe, expect, it } from 'vitest';

import {
  AUDIO_PATHS,
  CABIN_SPEC,
  generateImpulseResponse,
  type PathStage,
} from '../src/lib/cabin';

/**
 * The cab impulse response and the values that describe the cab.
 *
 * `cabin.ts` is deliberately free of Web Audio, so all of this runs under Node.
 * What cannot be checked here is how it sounds; what can be checked is that it
 * is repeatable, bounded, decays, and does not quietly depend on the machine it
 * is generated on.
 */

const RATE = 48_000;

describe('the cab impulse response', () => {
  it('is identical for the same seed and rate', () => {
    const a = generateImpulseResponse(RATE);
    const b = generateImpulseResponse(RATE);
    // Bit-identical, not close. The project seeds every source of randomness
    // explicitly; an impulse that varied per page load would make the level and
    // spectrum measurements taken from the output unrepeatable.
    expect(Array.from(a.left)).toEqual(Array.from(b.left));
    expect(Array.from(a.right)).toEqual(Array.from(b.right));
  });

  it('is a different room for a different seed', () => {
    const a = generateImpulseResponse(RATE);
    const b = generateImpulseResponse(RATE, { ...CABIN_SPEC.impulse, seedLeft: 12_345 });
    expect(Array.from(a.left)).not.toEqual(Array.from(b.left));
  });

  it('has the requested length at any device rate', () => {
    for (const rate of [32_000, 44_100, 48_000, 96_000]) {
      const { left, right } = generateImpulseResponse(rate);
      const expected = Math.round(CABIN_SPEC.impulse.durationS * rate);
      expect(left.length).toBe(expected);
      expect(right.length).toBe(expected);
    }
  });

  it('is finite and bounded', () => {
    const { left, right } = generateImpulseResponse(RATE);
    for (const channel of [left, right]) {
      for (const sample of channel) {
        expect(Number.isFinite(sample)).toBe(true);
        expect(Math.abs(sample)).toBeLessThanOrEqual(1);
      }
    }
  });

  it('leaves the predelay silent, so the discrete taps are heard first', () => {
    const { left } = generateImpulseResponse(RATE);
    const predelay = Math.round(CABIN_SPEC.impulse.predelayS * RATE);
    expect(predelay).toBeGreaterThan(0);
    for (let i = 0; i < predelay; i += 1) {
      expect(left[i]).toBe(0);
    }
    // And it does start immediately afterwards, rather than being silent
    // everywhere for some other reason.
    const tail = left.slice(predelay, predelay + 64);
    expect(tail.some((sample) => sample !== 0)).toBe(true);
  });

  it('gets darker as it decays, because a cab is lined with soft things', () => {
    // The point of the two-band tail. Porous absorbers — seats, headliner,
    // carpet, a bunk — take several times more energy out at 2 kHz than at
    // 125 Hz, so the treble in a cab is gone while the boom is still going.
    // A single decay constant cannot say that at any setting, and this measures
    // the thing it could not say.
    const { left } = generateImpulseResponse(RATE);
    const predelay = Math.round(CABIN_SPEC.impulse.predelayS * RATE);

    // Crude high/low split: a first difference is a high pass, a running sum of
    // adjacent pairs a low pass. Enough to compare two halves of one signal.
    const brightness = (from: number, to: number) => {
      let high = 0;
      let low = 0;
      for (let i = from + 1; i < to; i += 1) {
        const d = left[i]! - left[i - 1]!;
        const s = left[i]! + left[i - 1]!;
        high += d * d;
        low += s * s;
      }
      return high / Math.max(low, 1e-30);
    };

    const span = Math.floor((left.length - predelay) / 4);
    const early = brightness(predelay, predelay + span);
    const late = brightness(predelay + span * 3, left.length);

    expect(
      late,
      `the tail should darken as it decays (early ${early.toFixed(3)}, late ${late.toFixed(3)})`,
    ).toBeLessThan(early * 0.7);
  });

  it('splits the bands complementarily, so equal decays ignore the corner', () => {
    // The band split is `low` and `noise - low`, which reconstructs the noise
    // exactly. So when the two decay rates agree, the corner between them must
    // make no difference whatever — the tail collapses back to the
    // single-envelope one this replaced.
    //
    // Asserted as bit-equality across two very different corners rather than
    // through a spectral measure, because it is an exact algebraic property and
    // a proxy metric would only test it approximately. It is also the property
    // that breaks first if the split is ever rewritten as two independent
    // filters.
    const flat = { ...CABIN_SPEC.impulse, decayHighS: CABIN_SPEC.impulse.decayS };
    const low = generateImpulseResponse(RATE, { ...flat, dampingHz: 300 });
    const high = generateImpulseResponse(RATE, { ...flat, dampingHz: 6_000 });

    expect(Array.from(low.left)).toEqual(Array.from(high.left));
    expect(Array.from(low.right)).toEqual(Array.from(high.right));

    // And with the shipped decays it does depend on the corner, so the equality
    // above is a property of the split rather than of the corner being ignored.
    const damped = generateImpulseResponse(RATE, { ...CABIN_SPEC.impulse, dampingHz: 300 });
    expect(Array.from(damped.left)).not.toEqual(Array.from(low.left));
  });

  it('decays', () => {
    const { left } = generateImpulseResponse(RATE);
    const energy = (from: number, to: number) =>
      left.slice(from, to).reduce((sum, s) => sum + s * s, 0);

    const quarter = Math.floor(left.length / 4);
    const first = energy(quarter * 0, quarter * 1);
    const second = energy(quarter * 1, quarter * 2);
    const last = energy(quarter * 3, quarter * 4);

    expect(second).toBeLessThan(first);
    expect(last).toBeLessThan(second);
    // A 130 ms decay over a 180 ms response is about −8 dB by the last quarter's
    // start; anything close to the beginning's energy would be a stuck envelope.
    expect(last).toBeLessThan(first * 0.1);
  });

  it('carries the same energy whatever the device rate', () => {
    // Otherwise the same `reverbGain` would be a different amount of room on a
    // 96 kHz machine than on a 44.1 kHz one.
    //
    // Equal to *each other* rather than to one. The normalisation divides by the
    // energy an undamped tail would have carried, so a cab that absorbs its top
    // end genuinely ends up with a quieter tail — currently about a third of the
    // energy — and that is the change, not a bug in the scaling. What has to
    // stay fixed is that the figure does not depend on the device.
    const energyAt = (rate: number) =>
      generateImpulseResponse(rate).left.reduce((sum, s) => sum + s * s, 0);
    const at44 = energyAt(44_100);
    const at96 = energyAt(96_000);
    expect(at44).toBeGreaterThan(0);
    expect(at96 / at44).toBeGreaterThan(0.95);
    expect(at96 / at44).toBeLessThan(1.05);

    // And it is below one, which is the absorption showing up as a number.
    expect(at44).toBeLessThan(0.9);
  });

  it('decorrelates the two channels without changing their energy', () => {
    const { left, right } = generateImpulseResponse(RATE);
    expect(Array.from(left)).not.toEqual(Array.from(right));

    // Within a quarter rather than to six places, and the looseness is a
    // measured property rather than a shrug. Each channel is normalised against
    // what its *own* noise would have carried undamped, so how much the damping
    // then removes is a function of that channel's realisation — and the tail's
    // energy is dominated by its first twenty milliseconds, which at this
    // bandwidth is only a few hundred independent samples. Two seeds differ by
    // about a decibel and there is no setting at which they would not.
    //
    // What this still catches is the thing worth catching: one channel loud and
    // the other quiet, which would pull the stereo image to one side.
    const energy = (channel: Float32Array) => channel.reduce((sum, s) => sum + s * s, 0);
    const ratio = energy(right) / energy(left);
    expect(ratio).toBeGreaterThan(0.8);
    expect(ratio).toBeLessThan(1.25);

    // Decorrelated, not merely different: a strong correlation would collapse to
    // the centre and there would be no width at all.
    let dot = 0;
    for (let i = 0; i < left.length; i += 1) dot += left[i]! * right[i]!;
    expect(Math.abs(dot)).toBeLessThan(0.2);
  });
});

describe('the cab specification', () => {
  it('keeps every filter inside the audible band with a real Q', () => {
    for (const filter of CABIN_SPEC.filters) {
      expect(filter.frequencyHz).toBeGreaterThanOrEqual(20);
      expect(filter.frequencyHz).toBeLessThanOrEqual(20_000);
      expect(filter.q).toBeGreaterThan(0);
      expect(Math.abs(filter.gainDb)).toBeLessThanOrEqual(12);
    }
  });

  it('rolls the top end off, which is the whole point of being inside a cab', () => {
    // Per path now, and not shared. One low pass was one number trying to
    // describe two routes: it sat at 2.6 kHz so bulkhead clatter survived it,
    // which is far too high for a tailpipe several metres away. Each path says
    // it for itself, and every one of them has to say it.
    expect(CABIN_SPEC.filters.some((f) => f.type === 'lowpass')).toBe(false);
    for (const path of AUDIO_PATHS) {
      const lowpass = CABIN_SPEC.paths[path].filters.find((f) => f.type === 'lowpass');
      expect(lowpass, `${path} needs a low pass`).toBeDefined();
      expect(lowpass!.frequencyHz).toBeLessThan(4_000);
    }
  });

  it('puts the exhaust further away than the block', () => {
    // The physical claim the split exists to make. The exhaust leaves a stack
    // metres behind and below; the block is two feet away through the bulkhead.
    // So the exhaust arrives later and duller, and if that ever inverts the two
    // sources have swapped places.
    const { exhaust, block, body } = CABIN_SPEC.paths;
    expect(exhaust.delayS).toBeGreaterThan(block.delayS);
    expect(exhaust.roomSend).toBeGreaterThan(block.roomSend);

    const cutoff = (stage: PathStage) =>
      stage.filters.find((f) => f.type === 'lowpass')!.frequencyHz;
    expect(cutoff(exhaust)).toBeLessThan(cutoff(block));

    // The body path reaches the seat through steel, not through air: no
    // propagation delay and no room at all. Zero here is a statement, so it is
    // asserted rather than left to be inferred from a small number.
    expect(body.delayS).toBe(0);
    expect(body.roomSend).toBe(0);
  });

  it('keeps every per-path filter inside the audible band with a real Q', () => {
    for (const path of AUDIO_PATHS) {
      for (const filter of CABIN_SPEC.paths[path].filters) {
        expect(filter.frequencyHz).toBeGreaterThanOrEqual(20);
        expect(filter.frequencyHz).toBeLessThanOrEqual(20_000);
        expect(filter.q).toBeGreaterThan(0);
        expect(Math.abs(filter.gainDb)).toBeLessThanOrEqual(12);
      }
      // A delay long enough to read as an echo rather than as distance would be
      // describing a different vehicle.
      expect(CABIN_SPEC.paths[path].delayS).toBeLessThan(0.05);
      expect(CABIN_SPEC.paths[path].roomSend).toBeGreaterThanOrEqual(0);
      expect(CABIN_SPEC.paths[path].roomSend).toBeLessThanOrEqual(1);
    }
  });

  it('keeps the early reflections early', () => {
    // Past about 50 ms a reflection stops reading as a room and starts reading
    // as a distinct echo, which a two-metre cab does not have.
    expect(CABIN_SPEC.taps.length).toBeGreaterThan(1);
    for (const tap of CABIN_SPEC.taps) {
      expect(tap.delayS).toBeGreaterThan(0);
      expect(tap.delayS).toBeLessThan(0.05);
      expect(Math.abs(tap.pan)).toBeLessThanOrEqual(1);
      expect(tap.gain).toBeGreaterThan(0);
      expect(tap.gain).toBeLessThan(1);
    }
    // Panned apart, or the two taps would just be one louder tap.
    const [first, second] = CABIN_SPEC.taps;
    expect(Math.sign(first!.pan)).not.toBe(Math.sign(second!.pan));
  });

  it('crossfades over a time that is short but not a step', () => {
    expect(CABIN_SPEC.crossfadeS).toBeGreaterThan(0.005);
    expect(CABIN_SPEC.crossfadeS).toBeLessThan(0.25);
  });
});
