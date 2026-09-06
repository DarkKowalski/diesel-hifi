import { describe, expect, it } from 'vitest';

import {
  DEFAULT_CONTROLS,
  DEFAULT_RESET,
  DEFAULT_SWEEP,
  isFromWorker,
  isToWorker,
  PROTOCOL_VERSION,
} from '../src/worker/protocol';

describe('protocol version', () => {
  it('is pinned', () => {
    expect(PROTOCOL_VERSION).toBe(1);
  });
});

describe('isToWorker', () => {
  it('accepts every request kind', () => {
    const messages = [
      { t: 'init', rid: 1 },
      { t: 'listConfigs', rid: 2 },
      { t: 'provenance', rid: 3 },
      { t: 'selectConfig', rid: 4, id: 'mercedes-benz-om471-9-m3d-375kw' },
      { t: 'reset', rid: 5, options: DEFAULT_RESET },
      { t: 'setControls', rid: 6, controls: DEFAULT_CONTROLS },
      { t: 'run', rid: 7, running: true },
      { t: 'stepOnce', rid: 8, steps: 100 },
      { t: 'sweep', rid: 9, options: DEFAULT_SWEEP },
    ];
    for (const message of messages) {
      expect(isToWorker(message), JSON.stringify(message)).toBe(true);
    }
  });

  it('rejects malformed messages', () => {
    for (const bad of [
      null,
      undefined,
      42,
      'init',
      {},
      { t: 'init' },
      { rid: 1 },
      { t: 'nope', rid: 1 },
      { t: 'init', rid: '1' },
      [],
    ]) {
      expect(isToWorker(bad), JSON.stringify(bad)).toBe(false);
    }
  });
});

describe('isFromWorker', () => {
  it('accepts responses that carry a request id', () => {
    expect(isFromWorker({ t: 'ok', rid: 3 })).toBe(true);
    expect(isFromWorker({ t: 'configs', rid: 3, configs: [] })).toBe(true);
    expect(isFromWorker({ t: 'ready', rid: 1 })).toBe(true);
  });

  it('accepts sweep progress and results', () => {
    expect(isFromWorker({ t: 'sweepProgress', rid: 4, done: 2, total: 15, rpm: 800 })).toBe(true);
    expect(isFromWorker({ t: 'sweepResult', rid: 4, points: [], peaks: null })).toBe(true);
  });

  it('accepts unsolicited snapshots and errors on a null request id', () => {
    expect(isFromWorker({ t: 'snapshot', rid: null, snapshot: {} })).toBe(true);
    expect(isFromWorker({ t: 'error', rid: null, code: 'X', message: 'y' })).toBe(true);
  });

  it('requires a request id on everything else', () => {
    expect(isFromWorker({ t: 'ok', rid: null })).toBe(false);
    expect(isFromWorker({ t: 'ok' })).toBe(false);
  });

  it('rejects unknown kinds and non-objects', () => {
    for (const bad of [null, 7, 'ok', { t: 'mystery', rid: 1 }, { rid: 1 }]) {
      expect(isFromWorker(bad), JSON.stringify(bad)).toBe(false);
    }
  });
});

describe('defaults', () => {
  it('start the engine stopped, unfuelled and unloaded', () => {
    expect(DEFAULT_CONTROLS).toEqual({
      pedal: 0,
      loadTorqueNm: 0,
      starter: false,
      ignition: false,
    });
    expect(DEFAULT_RESET.seed).toBe(0);
    expect(DEFAULT_RESET.initialRpm).toBe(0);
  });
});

describe('DEFAULT_SWEEP', () => {
  it('covers the usable speed range at full load', () => {
    expect(DEFAULT_SWEEP.pedal).toBe(1);
    expect(DEFAULT_SWEEP.startRpm).toBeLessThan(DEFAULT_SWEEP.endRpm);
    expect(DEFAULT_SWEEP.stepRpm).toBeGreaterThan(0);
    expect(DEFAULT_SWEEP.settleCycles).toBeGreaterThan(0);
    expect(DEFAULT_SWEEP.measureCycles).toBeGreaterThan(0);
  });
});
