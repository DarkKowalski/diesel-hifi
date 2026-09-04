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

  <p class="muted small">
    The pedal requests fuel, not air. Fuelling is clipped by the trapped air mass, and crank torque
    comes from cylinder pressure through slider-crank geometry.
  </p>

  <p class="muted small">
    Audio is not part of this milestone. When it arrives it will require an explicit user action
    before any Web Audio context is created.
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
</style>
