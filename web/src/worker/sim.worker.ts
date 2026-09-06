/**
 * The simulation worker.
 *
 * Owns the WASM module lifecycle and the fixed-step schedule. Wall-clock time is
 * converted into a whole number of fixed steps, and the whole batch is advanced
 * with a single call into WebAssembly — never one call per step.
 *
 * Determinism note: the *core* is deterministic for a given step count. Live
 * playback derives its step count from wall-clock time, so a real-time browser
 * session is not bit-reproducible across machines. `stepOnce` bypasses the clock
 * for reproducible runs.
 *
 * No SharedArrayBuffer, no cross-origin isolation, no network access.
 */

import init, {
  api_version,
  snapshot_version,
  SimHandle,
} from '../wasm/sim_wasm.js';
import wasmUrl from '../wasm/sim_wasm_bg.wasm?url';

import {
  isToWorker,
  PROTOCOL_VERSION,
  type ConfigSummary,
  type FromWorker,
  type OperatingPoint,
  type ProvenanceReport,
  type Snapshot,
  type SweepOptions,
  type SweepPeaks,
  type ToWorker,
} from './protocol';

/** Never let a long tab-suspension turn into a huge catch-up batch. */
const MAX_CATCHUP_S = 0.25;
/** Upper bound on snapshot postMessage rate. */
const SNAPSHOT_INTERVAL_MS = 16;
/** How often the scheduler wakes up while running. */
const TICK_INTERVAL_MS = 8;

let handle: SimHandle | null = null;
let fixedStepS = 0;
let maxStepsPerBatch = 1;

let running = false;
/**
 * Whether anyone is listening. Draining costs a copy per batch, so it is only
 * done once the UI has actually started audio from a user gesture.
 */
let audioEnabled = false;
let audioSampleRateHz = 0;
let timer: ReturnType<typeof setTimeout> | null = null;
let lastTickMs = 0;
let accumulatorS = 0;
let lastPostMs = 0;

function post(message: FromWorker): void {
  self.postMessage(message);
}

/** Normalise anything thrown across the WASM boundary into `{ code, message }`. */
function describe(error: unknown): { code: string; message: string } {
  if (typeof error === 'object' && error !== null) {
    const record = error as Record<string, unknown>;
    if (typeof record.code === 'string' && typeof record.message === 'string') {
      return { code: record.code, message: record.message };
    }
  }
  if (error instanceof Error) {
    return { code: 'WORKER_ERROR', message: error.message };
  }
  return { code: 'WORKER_ERROR', message: String(error) };
}

function fail(rid: number | null, error: unknown): void {
  const { code, message } = describe(error);
  post({ t: 'error', rid, code, message });
}

function requireHandle(): SimHandle {
  if (handle === null) {
    throw { code: 'NOT_INITIALIZED', message: 'the simulation is not initialised yet' };
  }
  return handle;
}

function stopLoop(): void {
  running = false;
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

function startLoop(): void {
  if (running) return;
  running = true;
  lastTickMs = performance.now();
  accumulatorS = 0;
  scheduleTick();
}

function scheduleTick(): void {
  timer = setTimeout(tick, TICK_INTERVAL_MS);
}

function tick(): void {
  if (!running || handle === null) return;

  const now = performance.now();
  const elapsedS = Math.min((now - lastTickMs) / 1000, MAX_CATCHUP_S);
  lastTickMs = now;
  accumulatorS += elapsedS;

  let steps = Math.floor(accumulatorS / fixedStepS);
  if (steps > maxStepsPerBatch) {
    // Drop the surplus rather than spiralling: real time wins over sim time.
    steps = maxStepsPerBatch;
    accumulatorS = 0;
  } else {
    accumulatorS -= steps * fixedStepS;
  }

  if (steps > 0) {
    try {
      const snapshot = handle.advance(steps) as Snapshot;

      // Drain every batch, not on the snapshot's slower cadence: the buffer
      // holds exactly one batch, so skipping a drain would drop samples.
      if (audioEnabled) {
        const samples = handle.drainAudio();
        if (samples.length > 0) {
          self.postMessage(
            {
              t: 'audio',
              rid: null,
              samples,
              sampleRateHz: audioSampleRateHz,
              dropped: handle.audioDropped(),
            } satisfies FromWorker,
            [samples.buffer],
          );
        }
      }

      if (now - lastPostMs >= SNAPSHOT_INTERVAL_MS) {
        lastPostMs = now;
        post({ t: 'snapshot', rid: null, snapshot });
      }
    } catch (error) {
      stopLoop();
      fail(null, error);
      // Surface the latched fault state to the UI as well.
      try {
        post({ t: 'snapshot', rid: null, snapshot: handle.snapshot() as Snapshot });
      } catch {
        // The handle is unusable; the error message above is what matters.
      }
      return;
    }
  }

  scheduleTick();
}

/**
 * Measure a dynamometer sweep one point at a time.
 *
 * Each point is a separate call into WebAssembly, with a progress message in
 * between, so a long sweep never blocks the worker for its whole duration and
 * the UI can show how far along it is.
 */
function runSweep(rid: number, options: SweepOptions): void {
  const sim = requireHandle();
  // Pausing the live loop keeps the sweep off the same time slice, so a sweep
  // measured while the engine is running still takes a bounded wall-clock time.
  const wasRunning = running;
  stopLoop();

  const speeds = sim.sweepSpeeds(options) as number[];
  const points: OperatingPoint[] = [];
  let index = 0;

  const measureNext = (): void => {
    if (handle === null) return;
    if (index >= speeds.length) {
      const peaks = (SimHandle.sweepPeaks(points) ?? null) as SweepPeaks | null;
      post({ t: 'sweepResult', rid, points, peaks });
      if (wasRunning) startLoop();
      return;
    }
    const rpm = speeds[index]!;
    try {
      points.push(
        handle.operatingPoint(rpm, {
          pedal: options.pedal,
          settleCycles: options.settleCycles,
          measureCycles: options.measureCycles,
          egrEnabled: options.egrEnabled,
          // A dynamometer sweep is a fuelled sweep with the brake off.
          ignition: true,
          brakeStage: 0,
        }) as OperatingPoint,
      );
    } catch (error) {
      fail(rid, error);
      if (wasRunning) startLoop();
      return;
    }
    index += 1;
    post({ t: 'sweepProgress', rid, done: index, total: speeds.length, rpm });
    setTimeout(measureNext, 0);
  };

  measureNext();
}

async function handleInit(rid: number): Promise<void> {
  if (handle === null) {
    await init({ module_or_path: wasmUrl });
    handle = new SimHandle();
    fixedStepS = handle.fixedStepSeconds();
    maxStepsPerBatch = handle.maxStepsPerBatch();
  }
  post({
    t: 'ready',
    rid,
    apiVersion: api_version(),
    snapshotVersion: snapshot_version(),
    protocolVersion: PROTOCOL_VERSION,
    activeId: handle.activeConfigId(),
    fixedStepS,
    maxStepsPerBatch,
  });
  post({ t: 'snapshot', rid: null, snapshot: handle.snapshot() as Snapshot });
}

function dispatch(message: ToWorker): void {
  switch (message.t) {
    case 'init':
      void handleInit(message.rid).catch((error) => fail(message.rid, error));
      return;

    case 'listConfigs': {
      const configs = requireHandle().listConfigs() as ConfigSummary[];
      post({ t: 'configs', rid: message.rid, configs });
      return;
    }

    case 'provenance': {
      const report = requireHandle().configProvenance() as ProvenanceReport;
      post({ t: 'provenance', rid: message.rid, report });
      return;
    }

    case 'selectConfig': {
      const sim = requireHandle();
      stopLoop();
      sim.selectConfig(message.id);
      post({ t: 'ok', rid: message.rid });
      post({ t: 'snapshot', rid: null, snapshot: sim.snapshot() as Snapshot });
      return;
    }

    case 'reset': {
      const sim = requireHandle();
      stopLoop();
      sim.reset(message.options);
      post({ t: 'ok', rid: message.rid });
      post({ t: 'snapshot', rid: null, snapshot: sim.snapshot() as Snapshot });
      return;
    }

    case 'setControls': {
      requireHandle().setControls(message.controls);
      post({ t: 'ok', rid: message.rid });
      return;
    }

    case 'run': {
      requireHandle();
      if (message.running) {
        startLoop();
      } else {
        stopLoop();
      }
      post({ t: 'ok', rid: message.rid });
      return;
    }

    case 'stepOnce': {
      const sim = requireHandle();
      const snapshot = sim.advance(message.steps) as Snapshot;
      post({ t: 'snapshot', rid: message.rid, snapshot });
      return;
    }

    case 'sweep': {
      runSweep(message.rid, message.options);
      return;
    }

    case 'setAudio': {
      const sim = requireHandle();
      audioEnabled = message.enabled;
      audioSampleRateHz = sim.audioSampleRate();
      // Drop whatever accumulated while nobody was listening, so enabling
      // audio does not begin by playing a stale batch.
      sim.drainAudio();
      post({ t: 'ok', rid: message.rid });
      return;
    }
  }
}

self.onmessage = (event: MessageEvent<unknown>) => {
  const message = event.data;
  if (!isToWorker(message)) {
    fail(null, {
      code: 'MALFORMED_MESSAGE',
      message: `worker received an unrecognised message: ${JSON.stringify(message)}`,
    });
    return;
  }
  try {
    dispatch(message);
  } catch (error) {
    fail(message.rid, error);
  }
};
