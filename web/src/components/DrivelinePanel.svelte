<script lang="ts">
  import { sim } from '../lib/state.svelte';

  const controls = $derived(sim.controls);
  const snapshot = $derived(sim.snapshot);

  // Twelve forward ratios plus neutral. The count is fixed here rather than read
  // from the configuration because the worker does not expose the gearbox; a
  // gear the engine does not have is rejected at the boundary, which is the
  // check that actually matters.
  const GEARS = 12;
  const MAX_GRADE = 15;

  async function onGear(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    await sim.setControls({ gear: Number(input.value) });
  }

  async function onGrade(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    await sim.setControls({ roadGradePercent: Number(input.value) });
  }

  const speedKph = $derived((snapshot?.vehicleSpeedMPerS ?? 0) * 3.6);
  const engaged = $derived(snapshot?.gearEngaged ?? false);
  const grade = $derived(controls.roadGradePercent);
</script>

<section class="panel" aria-labelledby="driveline-heading">
  <h2 id="driveline-heading">Driveline</h2>

  <label class="field">
    <span>
      Gear <b data-testid="gear-label">{controls.gear === 0 ? 'Neutral' : controls.gear}</b>
    </span>
    <input
      data-testid="gear"
      type="range"
      min="0"
      max={GEARS}
      step="1"
      value={controls.gear}
      oninput={onGear}
    />
  </label>

  <label class="field">
    <span>
      Road grade <b data-testid="grade-label">{grade.toFixed(1)}%</b>
      {#if grade < 0}<span class="muted">(descent)</span>{:else if grade > 0}<span class="muted"
          >(climb)</span
        >{/if}
    </span>
    <input
      data-testid="grade"
      type="range"
      min={-MAX_GRADE}
      max={MAX_GRADE}
      step="0.5"
      value={grade}
      oninput={onGrade}
    />
  </label>

  <dl class="grid" data-testid="driveline-readout">
    <div>
      <dt>Road speed</dt>
      <dd data-testid="driveline-speed">{engaged ? `${speedKph.toFixed(1)} km/h` : '—'}</dd>
    </div>
    <div>
      <dt>Reflected inertia</dt>
      <dd data-testid="driveline-inertia">
        {(snapshot?.reflectedInertiaKgM2 ?? 0).toFixed(1)} kg m²
      </dd>
    </div>
  </dl>

  <p class="muted small">
    The coupling is rigid: in gear, road speed is whatever the crankshaft dictates, and the truck's
    forty tonnes appear at the crankshaft as inertia. In a high gear that is around two hundred
    kilogram-metres-squared against the engine's own three and a half, which is the whole reason a
    laden truck on a long descent needs a brake that cannot wear out.
  </p>

  <p class="muted small">
    Because road speed is slaved to the crank rather than integrated, changing gear changes the
    truck's speed instantly, and neutral means no vehicle at all rather than one coasting. Nothing
    about the vehicle is published in the source manual; every value here is our estimate of a laden
    tractor-trailer.
  </p>
</section>

<style>
  input[type='range'] {
    width: 100%;
  }
</style>
