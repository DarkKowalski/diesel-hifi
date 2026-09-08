import { describe, expect, it } from 'vitest';
import { DisplaySampler } from '../src/lib/displaySampler';
import type { Snapshot } from '../src/worker/protocol';

const frame = (patch: Partial<Snapshot>) => ({ state: 'running', configId: 'a', simTimeS: 1, rpm: 560, ...patch }) as Snapshot;

describe('readable display samples', () => {
  it('averages crank fluctuations at ten readings per second without changing the source', () => {
    const sampler = new DisplaySampler();
    sampler.push(frame({ rpm: 560 }), 0);
    expect(sampler.push(frame({ rpm: 540, simTimeS: 1.05 }), 50)).toBeNull();
    const source = frame({ rpm: 580, simTimeS: 1.1, peakPressurePaSession: 19e6 });
    const output = sampler.push(source, 100)!;
    expect(output.rpm).toBe(560);
    expect(source.rpm).toBe(580);
    expect(output.peakPressurePaSession).toBe(19e6);
  });
  it('shows faults and stops immediately instead of hiding them in an average', () => {
    const sampler = new DisplaySampler();
    sampler.push(frame({ rpm: 2300 }), 0);
    const fault = frame({ state: 'fault', rpm: 2400, fault: { code: 'SPEED_LIMIT_EXCEEDED', message: 'limit' } });
    expect(sampler.push(fault, 10)).toBe(fault);
    const stopped = frame({ state: 'stopped', rpm: 0 });
    expect(sampler.push(stopped, 20)).toBe(stopped);
  });
  it('discards the previous average on reset and engine selection', () => {
    const sampler = new DisplaySampler();
    sampler.push(frame({ simTimeS: 5 }), 0);
    sampler.push(frame({ rpm: 2000, simTimeS: 6 }), 50);
    const reset = frame({ simTimeS: 0, rpm: 0, state: 'stopped' });
    expect(sampler.push(reset, 110)).toBe(reset);
    const selected = frame({ configId: 'b', rpm: 600 });
    expect(sampler.push(selected, 120)).toBe(selected);
    expect(sampler.push(frame({ configId: 'b', rpm: 620, simTimeS: 2 }), 220)?.rpm).toBe(620);
  });
});
