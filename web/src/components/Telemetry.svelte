<script lang="ts">
  import { sim } from '../lib/state.svelte';
  import { amps, bar, degrees, kelvin, mg, mpa, nm, percent, rpm, seconds, volts } from '../lib/format';

  const snapshot = $derived(sim.snapshot);
  const controls = $derived(sim.controls);
</script>

<section class="panel" aria-labelledby="telemetry-heading">
  <h2 id="telemetry-heading">Telemetry</h2>

  {#if !snapshot}
    <p class="muted">No snapshot yet.</p>
  {:else}
    <div class="rpm" data-testid="rpm-block">
      <span class="value" data-testid="rpm">{rpm(snapshot.rpm)}</span>
      <span class="unit">rpm</span>
      <span class="state" data-testid="run-state">{sim.speedLimitPaused ? 'paused — speed limit' : snapshot.state}</span>
    </div>

    <dl class="grid">
      <div><dt>Crank angle</dt><dd data-testid="crank-angle">{degrees(snapshot.crankAngleRad)}°</dd></div>
      <div><dt>Sim time</dt><dd data-testid="sim-time">{seconds(snapshot.simTimeS)} s</dd></div>
      <div><dt>Steps</dt><dd data-testid="steps">{snapshot.stepsAdvanced}</dd></div>
      <div><dt>Peak pressure (cycle)</dt><dd data-testid="peak-cycle">{mpa(snapshot.peakPressurePaCycle)} MPa</dd></div>
      <div><dt>Peak pressure (session)</dt><dd data-testid="peak-session">{mpa(snapshot.peakPressurePaSession)} MPa</dd></div>
      <div><dt>Peak gas temperature</dt><dd>{kelvin(snapshot.peakGasTemperatureK)} K</dd></div>
      <div><dt>Gas torque</dt><dd data-testid="torque-gas">{nm(snapshot.torqueGasNm)} N m</dd></div>
      <div><dt>Pumping torque</dt><dd>{nm(snapshot.torquePumpingNm)} N m</dd></div>
      <div><dt>Friction torque</dt><dd>{nm(snapshot.torqueFrictionNm)} N m</dd></div>
      <div><dt>Accessory torque</dt><dd>{nm(snapshot.torqueAccessoryNm)} N m</dd></div>
      <div><dt>Starter torque</dt><dd data-testid="torque-starter">{nm(snapshot.torqueStarterNm)} N m</dd></div>
      <div><dt>Starter current</dt><dd data-testid="starter-current">{amps(snapshot.starterCurrentA)} A</dd></div>
      <div><dt>Starter terminal</dt><dd data-testid="starter-volts">{volts(snapshot.starterTerminalVoltageV)} V</dd></div>
      <div><dt>Pinion engagement</dt><dd data-testid="starter-engagement">{percent(snapshot.starterEngagement)}%</dd></div>
      <div><dt>Load torque</dt><dd>{nm(snapshot.torqueLoadNm)} N m</dd></div>
      <div><dt>Net torque</dt><dd data-testid="torque-net">{nm(snapshot.torqueNetNm)} N m</dd></div>
      <div><dt>Fuel per cycle</dt><dd data-testid="fuel">{mg(snapshot.fuelPerCycleMg)} mg</dd></div>
      <div><dt>Injection variant</dt><dd data-testid="variant">{snapshot.injectionVariant}</dd></div>
      <div><dt>Injection pressure</dt><dd>{bar(snapshot.injectionPressurePa)} bar</dd></div>
      <div><dt>Ignition delay</dt><dd>{degrees(snapshot.ignitionDelayRad)}°</dd></div>
      <div><dt>Premixed fraction</dt><dd>{(snapshot.premixedFraction * 100).toFixed(0)}%</dd></div>
      <div><dt>Residual gas</dt><dd>{(snapshot.residualFraction * 100).toFixed(1)}%</dd></div>
      <div><dt>Fuel demand</dt><dd>{mg(snapshot.fuelDemandMg)} mg</dd></div>
      <div><dt>Intake pressure</dt><dd data-testid="intake-pressure">{bar(snapshot.intakePressurePa)} bar</dd></div>
      <div><dt>Intake temperature</dt><dd>{kelvin(snapshot.intakeTemperatureK)} K</dd></div>
      <div><dt>Exhaust pressure</dt><dd data-testid="exhaust-pressure">{bar(snapshot.exhaustPressurePa)} bar</dd></div>
      <div><dt>Exhaust temperature</dt><dd>{kelvin(snapshot.exhaustTemperatureK)} K</dd></div>
    </dl>

    <h3>Air path</h3>
    <p class="muted small">
      Boost is computed, not prescribed: the turbine drives a shaft with inertia, and the wastegate
      regulates towards the ECU setpoint.
    </p>
    <dl class="grid" data-testid="air-path">
      <div>
        <dt>Boost</dt>
        <dd data-testid="boost">{bar(snapshot.boostPressurePa)} bar</dd>
      </div>
      <div>
        <dt>Turbo shaft</dt>
        <dd data-testid="turbo-shaft">
          {((snapshot.turboShaftRadPerS * 60) / (2 * Math.PI) / 1000).toFixed(1)} krpm
        </dd>
      </div>
      <div>
        <dt>Wastegate</dt>
        <dd data-testid="wastegate">{(snapshot.wastegatePosition * 100).toFixed(0)}%</dd>
      </div>
      <div>
        <dt>EGR rate</dt>
        <dd data-testid="egr-rate">{(snapshot.egrRate * 100).toFixed(1)}%</dd>
      </div>
      <div>
        <dt>EGR valve</dt>
        <dd data-testid="egr-valve">{(snapshot.egrValvePosition * 100).toFixed(0)}%</dd>
      </div>
      <div>
        <dt>Intake burned gas</dt>
        <dd data-testid="intake-burned">{(snapshot.intakeBurnedFraction * 100).toFixed(1)}%</dd>
      </div>
      <div>
        <dt>Compressor flow</dt>
        <dd>{snapshot.compressorFlowKgPerS.toFixed(3)} kg/s</dd>
      </div>
      <div>
        <dt>EGR flow</dt>
        <dd>{snapshot.egrFlowKgPerS.toFixed(3)} kg/s</dd>
      </div>
    </dl>

    <h3>Engine brake and driveline</h3>
    <p class="muted small">
      Braking torque is not injected anywhere: the brake cam opens an exhaust valve while the
      cylinder is shut, and the resulting pressure drives the crank through the same slider-crank
      geometry as combustion does.
    </p>
    <dl class="grid" data-testid="brake-driveline">
      <div>
        <dt>Brake stage</dt>
        <dd data-testid="brake-stage-active">
          {snapshot.brakeActive ? `${['off', 'I', 'II', 'III'][snapshot.brakeStageActive]}` : 'off'}
        </dd>
      </div>
      <div>
        <dt>Absorbed power</dt>
        <dd data-testid="brake-absorbed">{(snapshot.brakeAbsorbedPowerW / 1000).toFixed(1)} kW</dd>
      </div>
      <div>
        <dt>Gear</dt>
        <dd data-testid="gear-readout">{snapshot.gearEngaged ? controls.gear : 'N'}</dd>
      </div>
      <div>
        <dt>Road speed</dt>
        <dd data-testid="vehicle-speed">{(snapshot.vehicleSpeedMPerS * 3.6).toFixed(1)} km/h</dd>
      </div>
      <div>
        <dt>Road load</dt>
        <dd data-testid="torque-driveline">{nm(snapshot.torqueDrivelineNm)} N m</dd>
      </div>
      <div>
        <dt>Reflected inertia</dt>
        <dd data-testid="reflected-inertia">{snapshot.reflectedInertiaKgM2.toFixed(1)} kg m²</dd>
      </div>
    </dl>

    <h3>Whole-cycle averages</h3>
    {#if !snapshot.cycleValid}
      <p class="muted small">No complete four-stroke cycle yet.</p>
    {:else}
      <dl class="grid" data-testid="cycle-averages">
        <div><dt>Brake torque</dt><dd data-testid="brake-torque">{nm(snapshot.brakeTorqueCycleNm)} N m</dd></div>
        <div><dt>Indicated torque</dt><dd>{nm(snapshot.indicatedTorqueCycleNm)} N m</dd></div>
        <div><dt>Brake power</dt><dd data-testid="brake-power">{(snapshot.brakePowerCycleW / 1000).toFixed(1)} kW</dd></div>
        <div><dt>BMEP</dt><dd>{bar(snapshot.bmepPa)} bar</dd></div>
        <div><dt>IMEP</dt><dd>{bar(snapshot.imepPa)} bar</dd></div>
        <div><dt>Fuel consumption</dt><dd>{snapshot.bsfcGPerKwh.toFixed(0)} g/kW·h</dd></div>
        <div><dt>Cycles completed</dt><dd>{snapshot.cyclesCompleted}</dd></div>
      </dl>
      <p class="muted small">
        Instantaneous torque swings by more than a thousand newton-metres inside a cycle. These
        whole-cycle averages are the values worth reading.
      </p>
    {/if}

    <h3>Cylinder pressure</h3>
    <ol class="cylinders" data-testid="cylinders">
      {#each snapshot.cylinderPressurePa as pressure, index (index)}
        <li>
          <span class="label">#{index + 1}</span>
          <span class="meter" aria-hidden="true">
            <span
              class="fill"
              style={`width: ${Math.min(100, (pressure / 23e6) * 100).toFixed(2)}%`}
            ></span>
          </span>
          <span class="reading">{bar(pressure)} bar</span>
        </li>
      {/each}
    </ol>
    <p class="muted small">
      The bar scale is the published 230 bar combustion-pressure envelope, used here as a validation
      limit rather than a target. Under the calibrated boost schedule a full-load cycle now
      approaches it, which is what makes the envelope a real constraint on injection timing.
    </p>
  {/if}
</section>

<style>
  .rpm {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.9rem;
  }
  .rpm .value {
    font-size: 2.6rem;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .rpm .unit {
    color: var(--muted);
  }
  .rpm .state {
    margin-left: auto;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 0.7rem;
    border: 1px solid var(--rule);
    border-radius: 999px;
    padding: 0.15rem 0.6rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 0.15rem 1.25rem;
    margin: 0;
  }
  .grid > div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding: 0.15rem 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  h3 {
    margin: 1rem 0 0.4rem;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .cylinders {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.25rem;
  }
  .cylinders li {
    display: grid;
    grid-template-columns: 2.2rem 1fr 6rem;
    align-items: center;
    gap: 0.5rem;
  }
  .meter {
    height: 0.55rem;
    background: var(--rule);
    border-radius: 999px;
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .reading {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
