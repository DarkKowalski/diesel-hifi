import type { Snapshot } from '../worker/protocol';

const AVERAGED = [
  'rpm', 'vehicleSpeedMPerS', 'brakeTorqueCycleNm', 'brakePowerCycleW',
  'indicatedTorqueCycleNm', 'fuelPerCycleMg', 'bsfcGPerKwh', 'boostPressurePa',
  'turboShaftRadPerS', 'intakePressurePa', 'exhaustPressurePa', 'wastegatePosition',
  'egrRate', 'starterCurrentA', 'starterTerminalVoltageV', 'starterEngagement',
  'torqueStarterNm', 'brakeAbsorbedPowerW', 'torqueDrivelineNm',
] as const;

/** Presentation only: ten readings per second, averaged over incoming snapshots.
 * Safety/state transitions bypass the window; pressure peaks remain actual peaks.
 */
export class DisplaySampler {
  #last: Snapshot | null = null;
  #startedAt = 0;
  #count = 0;
  #sums = new Float64Array(AVERAGED.length);

  push(snapshot: Snapshot, nowMs: number): Snapshot | null {
    const immediate = !this.#last || snapshot.state !== this.#last.state
      || snapshot.configId !== this.#last.configId || snapshot.simTimeS < this.#last.simTimeS
      || (snapshot.rpm === 0 && this.#last.rpm !== 0) || snapshot.state === 'fault';
    this.#last = snapshot;
    if (immediate) {
      this.#clear(nowMs);
      return snapshot;
    }
    AVERAGED.forEach((key, i) => { this.#sums[i] = this.#sums[i]! + snapshot[key]; });
    this.#count++;
    if (nowMs - this.#startedAt < 100) return null;
    const display = { ...snapshot };
    AVERAGED.forEach((key, i) => { display[key] = this.#sums[i]! / this.#count; });
    this.#clear(nowMs);
    return display;
  }

  #clear(nowMs: number): void {
    this.#startedAt = nowMs;
    this.#count = 0;
    this.#sums.fill(0);
  }
}
