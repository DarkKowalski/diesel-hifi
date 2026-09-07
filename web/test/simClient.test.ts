import { describe, expect, it, vi } from 'vitest';

import { SimClient, SimClientError, type SimClientHandlers } from '../src/lib/simClient';
import type { FromWorker, Snapshot, ToWorker } from '../src/worker/protocol';

/**
 * A stand-in for the real module worker. Unit tests exercise the correlation and
 * error handling in `SimClient`; the real WASM-in-a-worker path is covered by
 * the Playwright suite.
 */
class FakeWorker implements Partial<Worker> {
  onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  onmessageerror: ((event: MessageEvent<unknown>) => void) | null = null;

  readonly sent: ToWorker[] = [];
  terminated = false;
  responder: (message: ToWorker) => FromWorker | null = () => null;

  postMessage(message: ToWorker): void {
    this.sent.push(message);
    const reply = this.responder(message);
    if (reply !== null) {
      queueMicrotask(() => this.emit(reply));
    }
  }

  terminate(): void {
    this.terminated = true;
  }

  emit(message: unknown): void {
    this.onmessage?.({ data: message } as MessageEvent<unknown>);
  }
}

function makeClient(
  responder: (message: ToWorker) => FromWorker | null,
  handlers: SimClientHandlers = {},
) {
  const worker = new FakeWorker();
  worker.responder = responder;
  const client = new SimClient(handlers, () => worker as unknown as Worker);
  return { client, worker };
}

const READY: FromWorker = {
  t: 'ready',
  rid: 1,
  apiVersion: 1,
  snapshotVersion: 1,
  protocolVersion: 1,
  activeId: 'mercedes-benz-om471-9-m3d-375kw',
  fixedStepS: 0.000025,
  maxStepsPerBatch: 20000,
};

describe('SimClient', () => {
  it('resolves init with the ready payload', async () => {
    const { client } = makeClient((m) => (m.t === 'init' ? { ...READY, rid: m.rid } : null));
    await expect(client.init()).resolves.toMatchObject({
      apiVersion: 1,
      activeId: 'mercedes-benz-om471-9-m3d-375kw',
      fixedStepS: 0.000025,
    });
  });

  it('correlates concurrent requests by request id', async () => {
    const { client, worker } = makeClient(() => null);

    const configs = client.listConfigs();
    const provenance = client.provenance();
    expect(worker.sent).toHaveLength(2);

    const configsRid = worker.sent[0]!.rid;
    const provenanceRid = worker.sent[1]!.rid;
    expect(configsRid).not.toBe(provenanceRid);

    // Reply out of order; each promise must still get its own answer.
    worker.emit({
      t: 'provenance',
      rid: provenanceRid,
      report: { config_id: 'x', entries: [], sources: [] },
    });
    worker.emit({ t: 'configs', rid: configsRid, configs: [{ id: 'x' }] });

    await expect(provenance).resolves.toMatchObject({ config_id: 'x' });
    await expect(configs).resolves.toEqual([{ id: 'x' }]);
  });

  it('rejects with a structured error and reports it to the handler', async () => {
    const onError = vi.fn();
    const { client } = makeClient(
      (m) =>
        m.t === 'selectConfig'
          ? { t: 'error', rid: m.rid, code: 'UNKNOWN_CONFIG_ID', message: 'no such engine' }
          : null,
      { onError },
    );

    const error = await client.selectConfig('nope').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(SimClientError);
    expect((error as SimClientError).code).toBe('UNKNOWN_CONFIG_ID');
    expect(onError).toHaveBeenCalledOnce();
  });

  it('streams unsolicited snapshots to the handler without resolving a request', async () => {
    const onSnapshot = vi.fn();
    const { worker } = makeClient(() => null, { onSnapshot });

    const snapshot = { rpm: 561.2, state: 'running' } as unknown as Snapshot;
    worker.emit({ t: 'snapshot', rid: null, snapshot });

    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
  });

  it('streams sweep progress without resolving the request, then resolves on the result', async () => {
    const onSweepProgress = vi.fn();
    const { client, worker } = makeClient(() => null, { onSweepProgress });

    const pending = client.sweep({
      startRpm: 800,
      endRpm: 1000,
      stepRpm: 100,
      pedal: 1,
      settleCycles: 10,
      measureCycles: 4,
      egrEnabled: true,
    });
    const rid = worker.sent[0]!.rid;
    expect(worker.sent[0]!.t).toBe('sweep');

    worker.emit({ t: 'sweepProgress', rid, done: 1, total: 3, rpm: 800 });
    worker.emit({ t: 'sweepProgress', rid, done: 2, total: 3, rpm: 900 });
    expect(onSweepProgress).toHaveBeenCalledTimes(2);
    expect(onSweepProgress).toHaveBeenLastCalledWith(2, 3, 900);

    worker.emit({
      t: 'sweepResult',
      rid,
      points: [{ rpm: 800 }, { rpm: 900 }, { rpm: 1000 }],
      peaks: { peakPowerW: 300000, peakPowerRpm: 1000 },
    });

    const result = await pending;
    expect(result.points).toHaveLength(3);
    expect(result.peaks).toMatchObject({ peakPowerRpm: 1000 });
  });

  it('streams audio blocks to the handler without resolving a request', () => {
    const onAudio = vi.fn();
    const { worker } = makeClient(() => null, { onAudio });

    const samples = new Float32Array([0.1, -0.2, 0.3]);
    worker.emit({ t: 'audio', rid: null, samples, paths: 3, sampleRateHz: 40000, dropped: 0 });

    expect(onAudio).toHaveBeenCalledOnce();
    expect(onAudio).toHaveBeenCalledWith(samples, 3, 40000, 0);
  });

  it('enables and disables audio production through the protocol', async () => {
    const { client, worker } = makeClient((m) => ({ t: 'ok', rid: m.rid }));

    await client.setAudio(true);
    await client.setAudio(false);

    expect(worker.sent.map((m) => m.t)).toEqual(['setAudio', 'setAudio']);
    expect(worker.sent[0]).toMatchObject({ t: 'setAudio', enabled: true });
    expect(worker.sent[1]).toMatchObject({ t: 'setAudio', enabled: false });
  });

  it('surfaces worker-initiated errors', () => {
    const onError = vi.fn();
    const { worker } = makeClient(() => null, { onError });

    worker.emit({ t: 'error', rid: null, code: 'NON_FINITE_STATE', message: 'diverged' });

    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0]![0].code).toBe('NON_FINITE_STATE');
  });

  it('reports malformed worker messages instead of throwing', () => {
    const onError = vi.fn();
    const { worker } = makeClient(() => null, { onError });

    worker.emit({ t: 'not-a-real-message' });

    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0]![0].code).toBe('MALFORMED_MESSAGE');
  });

  it('surfaces a worker crash', () => {
    const onError = vi.fn();
    const { worker } = makeClient(() => null, { onError });

    worker.onerror?.({ message: 'boom' } as ErrorEvent);

    expect(onError.mock.calls[0]![0].code).toBe('WORKER_CRASHED');
  });

  it('terminates the worker and rejects everything in flight on close', async () => {
    const { client, worker } = makeClient(() => null);
    const pending = client.listConfigs();

    client.close();

    expect(worker.terminated).toBe(true);
    await expect(pending).rejects.toMatchObject({ code: 'CLIENT_CLOSED' });
    await expect(client.listConfigs()).rejects.toMatchObject({ code: 'CLIENT_CLOSED' });
  });

  it('sends the exact protocol messages the worker expects', async () => {
    const { client, worker } = makeClient((m) => ({ t: 'ok', rid: m.rid }));

    await client.setControls({
      pedal: 0.5,
      loadTorqueNm: 100,
      starter: false,
      ignition: true,
      egrEnabled: true,
      brakeStage: 0,
      gear: 0,
      roadGradePercent: 0,
    });
    await client.run(true);
    await client.reset({ seed: 3, initialRpm: 0, initialCrankRad: 0, coolantTempK: 293.15 });

    expect(worker.sent.map((m) => m.t)).toEqual(['setControls', 'run', 'reset']);
    expect(worker.sent[0]).toMatchObject({
      t: 'setControls',
      controls: { pedal: 0.5, ignition: true },
    });
    expect(worker.sent[2]).toMatchObject({ t: 'reset', options: { seed: 3 } });
  });
});
