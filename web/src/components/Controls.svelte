<script lang="ts">
  import { sim } from '../lib/state.svelte';

  const controls = $derived(sim.controls);
  const state = $derived(sim.snapshot?.state ?? 'stopped');
  const maxLoadNm = 5000;

  async function onPedal(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    await sim.setControls({ pedal: Number(input.value) / 100 });
  }

  async function onLoad(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    await sim.setControls({ loadTorqueNm: Number(input.value) });
  }

  async function onEgr(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    await sim.setControls({ egrEnabled: input.checked });
  }

  const BRAKE_STAGES = [
    { value: 0, label: 'Off' },
    { value: 1, label: 'I' },
    { value: 2, label: 'II' },
    { value: 3, label: 'III' },
  ];

  async function onBrakeStage(stage: number) {
    await sim.setControls({ brakeStage: stage });
  }

  // What the switch asks for is not always what the brake does: the published
  // conditions hold it off with the pedal down or below 1000 rpm.
  const brakeActive = $derived(sim.snapshot?.brakeActive ?? false);
  const brakeRequested = $derived(controls.brakeStage > 0);
</script>

<section class="panel" aria-labelledby="controls-heading">
  <h2 id="controls-heading">Controls</h2>

  <div class="buttons">
    <button data-testid="start" onclick={() => sim.startEngine()} disabled={state === 'running'}>
      Start
    </button>
    <button data-testid="stop" onclick={() => sim.stopEngine()} disabled={state === 'stopped'}>
      Stop
    </button>
    <button data-testid="reset" onclick={() => sim.resetSimulation()}>Reset</button>
  </div>

  <label class="field">
    <span>Pedal (fuel request) <b>{(controls.pedal * 100).toFixed(0)}%</b></span>
    <input
      data-testid="pedal"
      type="range"
      min="0"
      max="100"
      step="1"
      value={controls.pedal * 100}
      oninput={onPedal}
    />
  </label>

  <label class="field">
    <span>External load <b>{controls.loadTorqueNm.toFixed(0)} N m</b></span>
    <input
      data-testid="load"
      type="range"
      min="0"
      max={maxLoadNm}
      step="50"
      value={controls.loadTorqueNm}
      oninput={onLoad}
    />
  </label>

  <label class="toggle">
    <input
      data-testid="egr-toggle"
      type="checkbox"
      checked={controls.egrEnabled}
      onchange={onEgr}
    />
    <span>Exhaust gas recirculation</span>
  </label>

  <div class="field">
    <span>Engine brake <b>{brakeRequested ? `stage ${BRAKE_STAGES[controls.brakeStage]?.label}` : 'off'}</b></span>
    <div class="stages" role="group" aria-label="Engine brake stage">
      {#each BRAKE_STAGES as stage (stage.value)}
        <button
          data-testid={`brake-stage-${stage.value}`}
          class:selected={controls.brakeStage === stage.value}
          aria-pressed={controls.brakeStage === stage.value}
          onclick={() => onBrakeStage(stage.value)}
        >
          {stage.label}
        </button>
      {/each}
    </div>
  </div>

  {#if brakeRequested && !brakeActive}
    <p class="muted small" data-testid="brake-inhibited">
      Selected, but not acting: the brake needs the pedal released and more than 1000 rpm.
    </p>
  {/if}

  <p class="muted small">
    The pedal requests fuel, not air. Fuelling is clipped by the trapped air mass, and crank torque
    comes from cylinder pressure through slider-crank geometry.
  </p>

  <p class="muted small">
    The engine brake is a decompression brake, and its torque is produced the same way as firing
    torque: a cam cracks an exhaust valve twice per cycle, once early in compression to pack the
    cylinder from the exhaust manifold and once just before top dead centre to throw that
    compression away. Stage I brakes cylinders 1 to 3, stage II all six, and stage III also shuts
    the wastegate to raise cylinder pressure further.
  </p>

  <p class="muted small">
    Recirculation is on by default, as it is on the real engine across the whole speed range.
    Switching it off shows what it costs: more air reaches the cylinders, so the smoke limit allows
    more fuel and torque rises. What it buys — lower combustion temperature and so lower NOx — is
    the reason the real calibration pays that price. This model does not predict emissions.
  </p>
</section>

<style>
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.9rem;
  }
  .buttons button {
    flex: 1;
  }
  input[type='range'] {
    width: 100%;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.9rem;
  }
  .toggle input {
    width: auto;
  }
  .stages {
    display: flex;
    gap: 0.35rem;
  }
  .stages button {
    flex: 1;
    font-variant-numeric: tabular-nums;
  }
  .stages button.selected {
    outline: 2px solid currentColor;
  }
</style>
