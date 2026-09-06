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
  /** Motored (no-combustion) pressure per cylinder, for comparison. */
  cylinderMotoredPressurePa: number[];
  peakPressurePaCycle: number;
  peakPressurePaSession: number;
  peakGasTemperatureK: number;

  // Instantaneous torque terms.
  torqueGasNm: number;
  torquePumpingNm: number;
  torqueFrictionNm: number;
  torqueAccessoryNm: number;
  torqueStarterNm: number;
  torqueLoadNm: number;
  torqueNetNm: number;

  /**
   * Whole-cycle averages. Instantaneous torque swings by more than a thousand
   * newton-metres inside a cycle, so these are the values worth reading for
   * anything quantitative. Only meaningful once `cycleValid` is set.
   */
  cycleValid: boolean;
  cyclesCompleted: number;
  indicatedTorqueCycleNm: number;
  brakeTorqueCycleNm: number;
  brakePowerCycleW: number;
  imepPa: number;
  bmepPa: number;
  bsfcGPerKwh: number;

  fuelPerCycleMg: number;
  fuelDemandMg: number;
  /** Published APCRS variant: "standard" or "amplified". */
  injectionVariant: string;
  injectionPressurePa: number;
  ignitionDelayRad: number;
  premixedFraction: number;

  intakePressurePa: number;
  intakeTemperatureK: number;
  exhaustPressurePa: number;
  residualFraction: number;

  fault?: SimFault | null;
}

/** Mirrors `sim_core::dyno::OperatingPoint`. */
export interface OperatingPoint {
  rpm: number;
  pedal: number;
  brakeTorqueNm: number;
  indicatedTorqueNm: number;
  frictionTorqueNm: number;
  pumpingTorqueNm: number;
  brakePowerW: number;
  imepPa: number;
  bmepPa: number;
  fuelMgPerCycle: number;
  bsfcGPerKwh: number;
  peakPressurePa: number;
  peakGasTemperatureK: number;
  airFuelRatio: number;
  intakePressurePa: number;
  residualFraction: number;
  converged: boolean;
}

/** Mirrors `sim_core::dyno::SweepOptions`. */
export interface SweepOptions {
  startRpm: number;
  endRpm: number;
  stepRpm: number;
  pedal: number;
  settleCycles: number;
  measureCycles: number;
}

/**
 * Mirrors `sim_core::dyno::SweepPeaks`.
 *
 * The engine speeds here are an outcome of this project's calibration. The
 * source manual does not publish the speeds at which the real engine reaches
 * its rated figures, and the UI must label them as calibrated.
 */
export interface SweepPeaks {
  peakPowerW: number;
  peakPowerRpm: number;
  peakTorqueNm: number;
  peakTorqueRpm: number;
  maxPeakPressurePa: number;
  bestBsfcGPerKwh: number;
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
  | { t: 'stepOnce'; rid: number; steps: number }
  /** Run a dynamometer sweep, reporting progress between points. */
  | { t: 'sweep'; rid: number; options: SweepOptions };

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
  | { t: 'sweepProgress'; rid: number; done: number; total: number; rpm: number }
  | { t: 'sweepResult'; rid: number; points: OperatingPoint[]; peaks: SweepPeaks | null }
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
  'sweep',
]);

const FROM_WORKER_KINDS = new Set<string>([
  'ready',
  'configs',
  'provenance',
  'ok',
  'snapshot',
  'sweepProgress',
  'sweepResult',
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

/** A full-load sweep across the usable speed range. */
export const DEFAULT_SWEEP: SweepOptions = {
  startRpm: 600,
  endRpm: 2000,
  stepRpm: 100,
  pedal: 1,
  settleCycles: 60,
  measureCycles: 12,
};
