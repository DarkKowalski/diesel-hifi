import { describe, expect, it } from 'vitest';

import { CABIN_SPEC, generateImpulseResponse } from '../src/lib/cabin';

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
    const energyAt = (rate: number) =>
      generateImpulseResponse(rate).left.reduce((sum, s) => sum + s * s, 0);
    expect(energyAt(44_100)).toBeCloseTo(1, 6);
    expect(energyAt(96_000)).toBeCloseTo(1, 6);
  });

  it('decorrelates the two channels without changing their energy', () => {
    const { left, right } = generateImpulseResponse(RATE);
    expect(Array.from(left)).not.toEqual(Array.from(right));

    const energy = (channel: Float32Array) => channel.reduce((sum, s) => sum + s * s, 0);
    expect(energy(right)).toBeCloseTo(energy(left), 6);

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
    const lowpass = CABIN_SPEC.filters.find((f) => f.type === 'lowpass');
    expect(lowpass).toBeDefined();
    expect(lowpass!.frequencyHz).toBeLessThan(4_000);
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
