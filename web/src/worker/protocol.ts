/**
 * Message protocol between the UI thread and the simulation worker.
 *
 * Both sides import these types, so the protocol cannot drift. Every request
 * carries a request id (`rid`) that the worker echoes, which is what lets the
 * client wrap postMessage in promises.
 *
 * Snapshots arrive unsolicited on a `rid` of `null`: they are a stream, not a
 * response.
 */

export const PROTOCOL_VERSION = 1;

// --- Types mirroring the `sim-core` serde shapes -----------------------------

/** Mirrors `sim_core::Controls` (serialized camelCase). */
export interface Controls {
  /** Fuel request, 0..=1. Not a throttle plate. */
  pedal: number;
  /** External resisting torque, N m. */
  loadTorqueNm: number;
  starter: boolean;
  ignition: boolean;
}

/** Mirrors `sim_core::ResetOptions` (serialized camelCase). */
export interface ResetOptions {
  seed: number;
  initialRpm: number;
  initialCrankRad: number;
  coolantTempK: number;
}

/** Mirrors `sim_core::ConfigSummary` (serialized camelCase). */
export interface ConfigSummary {
  id: string;
  displayName: string;
  manufacturerReference: string;
  powerCode: string;
  cylinders: number;
  displacementL: number;
  ratedPowerKw: number;
  ratedTorqueNm: number;
  idleRpm: number;
  disclaimer: string;
}

export type RunState = 'stopped' | 'cranking' | 'running' | 'fault';

export interface SimFault {
  code: string;
  message: string;
}

/** Mirrors `sim_core::Snapshot` (serialized camelCase). */
export interface Snapshot {
  schemaVersion: number;
  configId: string;
  state: RunState;
  simTimeS: number;
  crankAngleRad: number;
  rpm: number;
  stepsAdvanced: number;
  cylinderPressurePa: number[];
  peakPressurePaCycle: number;
  peakPressurePaSession: number;
  torqueGasNm: number;
  torquePumpingNm: number;
  torqueFrictionNm: number;
  torqueAccessoryNm: number;
  torqueStarterNm: number;
  torqueLoadNm: number;
  torqueNetNm: number;
  fuelPerCycleMg: number;
  fuelDemandMg: number;
  intakePressurePa: number;
  exhaustPressurePa: number;
  peakGasTemperatureK: number;
  fault?: SimFault | null;
}

/**
 * Provenance types keep the snake_case field names of the configuration
 * document itself, because the same Rust types both parse the document and
 * cross the boundary.
 */
export interface SourceRecord {
  id: string;
  title: string;
  publisher: string;
  technical_status: string;
  order_number: string;
  scope: string;
  local_path?: string | null;
}

export type ProvenanceStatus = 'published' | 'derived' | 'calibrated';

export interface ProvenanceEntry {
  path: string;
  status: ProvenanceStatus;
  value_note: string;
  source_id?: string | null;
  locator?: string | null;
  formula?: string | null;
  inputs: string[];
  purpose?: string | null;
  safe_range?: [number, number] | null;
}

export interface ProvenanceReport {
  config_id: string;
  display_name: string;
  disclaimer: string;
  sources: SourceRecord[];
  entries: ProvenanceEntry[];
  published_count: number;
  derived_count: number;
  calibrated_count: number;
}

// --- Messages ---------------------------------------------------------------

export type ToWorker =
  | { t: 'init'; rid: number }
  | { t: 'listConfigs'; rid: number }
  | { t: 'provenance'; rid: number }
  | { t: 'selectConfig'; rid: number; id: string }
  | { t: 'reset'; rid: number; options: ResetOptions }
  | { t: 'setControls'; rid: number; controls: Controls }
  | { t: 'run'; rid: number; running: boolean }
  /** Advance an exact step count with no wall-clock involvement. Deterministic. */
  | { t: 'stepOnce'; rid: number; steps: number };

export type FromWorker =
  | {
      t: 'ready';
      rid: number;
      apiVersion: number;
      snapshotVersion: number;
      protocolVersion: number;
      activeId: string;
      fixedStepS: number;
      maxStepsPerBatch: number;
    }
  | { t: 'configs'; rid: number; configs: ConfigSummary[] }
  | { t: 'provenance'; rid: number; report: ProvenanceReport }
  | { t: 'ok'; rid: number }
  | { t: 'snapshot'; rid: number | null; snapshot: Snapshot }
  | { t: 'error'; rid: number | null; code: string; message: string };

// --- Guards -----------------------------------------------------------------

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

const TO_WORKER_KINDS = new Set<string>([
  'init',
  'listConfigs',
  'provenance',
  'selectConfig',
  'reset',
  'setControls',
  'run',
  'stepOnce',
]);

const FROM_WORKER_KINDS = new Set<string>([
  'ready',
  'configs',
  'provenance',
  'ok',
  'snapshot',
  'error',
]);

export function isToWorker(value: unknown): value is ToWorker {
  return (
    isRecord(value) &&
    typeof value.t === 'string' &&
    TO_WORKER_KINDS.has(value.t) &&
    typeof value.rid === 'number'
  );
}

export function isFromWorker(value: unknown): value is FromWorker {
  if (!isRecord(value) || typeof value.t !== 'string' || !FROM_WORKER_KINDS.has(value.t)) {
    return false;
  }
  // Snapshots and worker-initiated errors may arrive without a request id.
  if (value.t === 'snapshot' || value.t === 'error') {
    return value.rid === null || typeof value.rid === 'number';
  }
  return typeof value.rid === 'number';
}

export const DEFAULT_RESET: ResetOptions = {
  seed: 0,
  initialRpm: 0,
  initialCrankRad: 0,
  coolantTempK: 293.15,
};

export const DEFAULT_CONTROLS: Controls = {
  pedal: 0,
  loadTorqueNm: 0,
  starter: false,
  ignition: false,
};
