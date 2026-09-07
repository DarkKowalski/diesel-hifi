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
  /**
   * Exhaust gas recirculation enable.
   *
   * Defaults on: the manual states EGR is active across the whole speed range.
   * Turning it off shows the trade the real calibration makes, since
   * recirculation costs power and fuel to buy lower NOx.
   */
  egrEnabled: boolean;
  /**
   * Decompression brake stage: 0 off, through 3.
   *
   * This is the driver's switch, not what the brake does. Whether a stage
   * actually engages is decided against the published operating conditions —
   * pedal released, above 1000 rpm — and the snapshot reports the difference.
   */
  brakeStage: number;
  /** Selected gear, 0 for neutral. */
  gear: number;
  /** Road grade in percent. Negative is a descent. */
  roadGradePercent: number;
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
  exhaustTemperatureK: number;
  residualFraction: number;

  /**
   * Turbocharger and EGR. All computed rather than prescribed: Milestone 2 wrote
   * manifold pressure from a schedule, Milestone 3 makes it an outcome.
   */
  boostPressurePa: number;
  turboShaftRadPerS: number;
  wastegatePosition: number;
  egrRate: number;
  egrValvePosition: number;
  intakeBurnedFraction: number;
  compressorFlowKgPerS: number;
  turbineFlowKgPerS: number;
  egrFlowKgPerS: number;

  /**
   * Engine brake and driveline.
   *
   * `brakeStageActive` is what the brake is *doing*, which is not always what
   * the switch asks for: the published conditions can hold it off while the
   * driver has stage III selected, and telemetry has to be able to say so.
   */
  brakeStageActive: number;
  brakeActive: boolean;
  /** Power the engine is absorbing, watts, positive while braking. */
  brakeAbsorbedPowerW: number;
  /** Road speed, m/s. Zero in neutral. */
  vehicleSpeedMPerS: number;
  /** Road load at the crank. Negative on a descent. */
  torqueDrivelineNm: number;
  reflectedInertiaKgM2: number;
  gearEngaged: boolean;

  /** Exhaust level against the configured reference. Samples arrive separately. */
  audioLevelDb: number;

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
  exhaustPressurePa: number;
  turboShaftRadPerS: number;
  wastegatePosition: number;
  egrRate: number;
  residualFraction: number;
  brakeStage: number;
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
  /** Whether EGR runs during the sweep. Off measures what recirculation costs. */
  egrEnabled: boolean;
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
  | { t: 'sweep'; rid: number; options: SweepOptions }
  /**
   * Start or stop producing exhaust audio.
   *
   * Draining costs a copy per batch, so it is only done when someone is
   * listening. The UI enables this from inside the user gesture that starts the
   * AudioContext.
   */
  | { t: 'setAudio'; rid: number; enabled: boolean };

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
  /**
   * A block of engine audio frames, unsolicited like snapshots.
   *
   * `samples` is transferred rather than copied, so the worker must not touch it
   * afterwards. `sampleRateHz` is the solver's own rate; the consumer resamples.
   *
   * **Interleaved by radiating path**, `paths` floats to a frame: exhaust,
   * block, body. They cross separately because they do not reach a driver by
   * the same route — the exhaust from a stack metres behind and below, the block
   * through the bulkhead, the body through the mounts and the seat — so the
   * listening stage gives each its own transfer. `paths` travels with the block
   * rather than being assumed, so a fourth path could not silently misalign
   * every consumer that de-interleaves.
   */
  | {
      t: 'audio';
      rid: null;
      samples: Float32Array;
      paths: number;
      sampleRateHz: number;
      dropped: number;
    }
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
  'setAudio',
]);

const FROM_WORKER_KINDS = new Set<string>([
  'ready',
  'configs',
  'provenance',
  'ok',
  'snapshot',
  'sweepProgress',
  'sweepResult',
  'audio',
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
  // Snapshots, audio blocks and worker-initiated errors are streams rather than
  // responses, so they may arrive without a request id.
  if (value.t === 'snapshot' || value.t === 'audio' || value.t === 'error') {
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
  egrEnabled: true,
  // Brake off, out of gear, on the flat: Milestone 4 contributes nothing until
  // it is asked to.
  brakeStage: 0,
  gear: 0,
  roadGradePercent: 0,
};

/** A full-load sweep across the usable speed range. */
export const DEFAULT_SWEEP: SweepOptions = {
  startRpm: 600,
  endRpm: 2000,
  stepRpm: 100,
  pedal: 1,
  settleCycles: 100,
  measureCycles: 12,
  egrEnabled: true,
};
