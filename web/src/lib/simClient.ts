/**
 * Typed client for the simulation worker.
 *
 * Wraps `postMessage` in promises correlated by request id, exposes the snapshot
 * stream as a callback, and turns worker-level failures into the same
 * `{ code, message }` shape the Rust core produces.
 *
 * The client never touches WASM directly: everything goes through the worker, so
 * the simulation always runs off the UI thread.
 */

import {
  isFromWorker,
  type ConfigSummary,
  type Controls,
  type FromWorker,
  type ProvenanceReport,
  type OperatingPoint,
  type ResetOptions,
  type Snapshot,
  type SweepOptions,
  type SweepPeaks,
  type ToWorker,
} from '../worker/protocol';

export class SimClientError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'SimClientError';
    this.code = code;
  }
}

export interface ReadyInfo {
  apiVersion: number;
  snapshotVersion: number;
  protocolVersion: number;
  activeId: string;
  fixedStepS: number;
  maxStepsPerBatch: number;
}

export interface SimClientHandlers {
  onSnapshot?: (snapshot: Snapshot) => void;
  onError?: (error: SimClientError) => void;
  onSweepProgress?: (done: number, total: number, rpm: number) => void;
}

export interface SweepResult {
  points: OperatingPoint[];
  peaks: SweepPeaks | null;
}

interface Pending {
  resolve: (message: FromWorker) => void;
  reject: (error: SimClientError) => void;
}

/** Builds the module worker. Overridable so unit tests can inject a fake. */
export type WorkerFactory = () => Worker;

const defaultWorkerFactory: WorkerFactory = () =>
  new Worker(new URL('../worker/sim.worker.ts', import.meta.url), {
    type: 'module',
    name: 'diesel-sim',
  });

export class SimClient {
  #worker: Worker;
  #pending = new Map<number, Pending>();
  #nextRid = 1;
  #handlers: SimClientHandlers;
  #closed = false;

  constructor(handlers: SimClientHandlers = {}, factory: WorkerFactory = defaultWorkerFactory) {
    this.#handlers = handlers;
    this.#worker = factory();
    this.#worker.onmessage = (event: MessageEvent<unknown>) => this.#receive(event.data);
    this.#worker.onerror = (event) => {
      this.#emitError(
        new SimClientError('WORKER_CRASHED', event.message || 'the simulation worker crashed'),
      );
    };
    this.#worker.onmessageerror = () => {
      this.#emitError(
        new SimClientError('WORKER_MESSAGE_ERROR', 'a worker message could not be deserialized'),
      );
    };
  }

  #emitError(error: SimClientError): void {
    this.#handlers.onError?.(error);
  }

  #receive(data: unknown): void {
    if (!isFromWorker(data)) {
      this.#emitError(
        new SimClientError('MALFORMED_MESSAGE', 'the worker sent an unrecognised message'),
      );
      return;
    }

    if (data.t === 'snapshot' && data.rid === null) {
      this.#handlers.onSnapshot?.(data.snapshot);
      return;
    }

    // Progress is a stream on an outstanding request id, not its resolution.
    if (data.t === 'sweepProgress') {
      this.#handlers.onSweepProgress?.(data.done, data.total, data.rpm);
      return;
    }

    if (data.rid === null) {
      // A worker-initiated error, e.g. the stepping loop faulting.
      if (data.t === 'error') {
        this.#emitError(new SimClientError(data.code, data.message));
      }
      return;
    }

    const pending = this.#pending.get(data.rid);
    if (pending === undefined) {
      if (data.t === 'error') {
        this.#emitError(new SimClientError(data.code, data.message));
      }
      return;
    }
    this.#pending.delete(data.rid);

    if (data.t === 'error') {
      const error = new SimClientError(data.code, data.message);
      this.#emitError(error);
      pending.reject(error);
    } else {
      pending.resolve(data);
    }
  }

  #request(build: (rid: number) => ToWorker): Promise<FromWorker> {
    if (this.#closed) {
      return Promise.reject(new SimClientError('CLIENT_CLOSED', 'the simulation client is closed'));
    }
    const rid = this.#nextRid++;
    return new Promise<FromWorker>((resolve, reject) => {
      this.#pending.set(rid, { resolve, reject });
      this.#worker.postMessage(build(rid));
    });
  }

  async init(): Promise<ReadyInfo> {
    const message = await this.#request((rid) => ({ t: 'init', rid }));
    if (message.t !== 'ready') throw new SimClientError('PROTOCOL', 'expected a ready message');
    const { apiVersion, snapshotVersion, protocolVersion, activeId, fixedStepS, maxStepsPerBatch } =
      message;
    return { apiVersion, snapshotVersion, protocolVersion, activeId, fixedStepS, maxStepsPerBatch };
  }

  async listConfigs(): Promise<ConfigSummary[]> {
    const message = await this.#request((rid) => ({ t: 'listConfigs', rid }));
    if (message.t !== 'configs') throw new SimClientError('PROTOCOL', 'expected a configs message');
    return message.configs;
  }

  async provenance(): Promise<ProvenanceReport> {
    const message = await this.#request((rid) => ({ t: 'provenance', rid }));
    if (message.t !== 'provenance') {
      throw new SimClientError('PROTOCOL', 'expected a provenance message');
    }
    return message.report;
  }

  async selectConfig(id: string): Promise<void> {
    await this.#request((rid) => ({ t: 'selectConfig', rid, id }));
  }

  async reset(options: ResetOptions): Promise<void> {
    await this.#request((rid) => ({ t: 'reset', rid, options }));
  }

  async setControls(controls: Controls): Promise<void> {
    await this.#request((rid) => ({ t: 'setControls', rid, controls }));
  }

  async run(running: boolean): Promise<void> {
    await this.#request((rid) => ({ t: 'run', rid, running }));
  }

  /**
   * Run a dynamometer sweep.
   *
   * Progress arrives through `onSweepProgress`; the promise resolves with the
   * measured points and their peaks.
   */
  async sweep(options: SweepOptions): Promise<SweepResult> {
    const message = await this.#request((rid) => ({ t: 'sweep', rid, options }));
    if (message.t !== 'sweepResult') {
      throw new SimClientError('PROTOCOL', 'expected a sweep result');
    }
    return { points: message.points, peaks: message.peaks };
  }

  /** Advance an exact number of fixed steps. Deterministic; used by tests. */
  async stepOnce(steps: number): Promise<Snapshot> {
    const message = await this.#request((rid) => ({ t: 'stepOnce', rid, steps }));
    if (message.t !== 'snapshot') {
      throw new SimClientError('PROTOCOL', 'expected a snapshot message');
    }
    return message.snapshot;
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    for (const pending of this.#pending.values()) {
      pending.reject(new SimClientError('CLIENT_CLOSED', 'the simulation client was closed'));
    }
    this.#pending.clear();
    this.#worker.terminate();
  }
}
