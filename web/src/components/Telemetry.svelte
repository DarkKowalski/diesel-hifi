<script lang="ts">
  import { sim } from '../lib/state.svelte';
  import { bar, degrees, kelvin, mg, mpa, nm, rpm, seconds } from '../lib/format';

  const snapshot = $derived(sim.snapshot);
</script>

<section class="panel" aria-labelledby="telemetry-heading">
  <h2 id="telemetry-heading">Telemetry</h2>

  {#if !snapshot}
    <p class="muted">No snapshot yet.</p>
  {:else}
    <div class="rpm" data-testid="rpm-block">
      <span class="value" data-testid="rpm">{rpm(snapshot.rpm)}</span>
      <span class="unit">rpm</span>
      <span class="state" data-testid="run-state">{snapshot.state}</span>
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
      <div><dt>Starter torque</dt><dd>{nm(snapshot.torqueStarterNm)} N m</dd></div>
      <div><dt>Load torque</dt><dd>{nm(snapshot.torqueLoadNm)} N m</dd></div>
      <div><dt>Net torque</dt><dd data-testid="torque-net">{nm(snapshot.torqueNetNm)} N m</dd></div>
      <div><dt>Fuel per cycle</dt><dd data-testid="fuel">{mg(snapshot.fuelPerCycleMg)} mg</dd></div>
      <div><dt>Fuel demand</dt><dd>{mg(snapshot.fuelDemandMg)} mg</dd></div>
      <div><dt>Intake pressure</dt><dd>{bar(snapshot.intakePressurePa)} bar</dd></div>
      <div><dt>Exhaust pressure</dt><dd>{bar(snapshot.exhaustPressurePa)} bar</dd></div>
    </dl>

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
      limit rather than a target. Milestone 1 runs naturally aspirated, so pressures stay far below
      it.
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
