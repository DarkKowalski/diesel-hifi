<script lang="ts">
  import { sim } from '../lib/state.svelte';

  // The options come from the worker's `listConfigs` call, which is the real
  // catalog API. Nothing about the engine is written into this component.
  const configs = $derived(sim.configs);
  const active = $derived(sim.activeConfig);

  async function onChange(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    await sim.selectConfig(select.value);
  }
</script>

<section class="panel" aria-labelledby="engine-heading">
  <h2 id="engine-heading">Engine</h2>

  {#if configs.length === 0}
    <p class="muted" data-testid="selector-empty">Waiting for the configuration catalog…</p>
  {:else}
    <label class="field">
      <span>Configuration</span>
      <select
        data-testid="engine-select"
        data-source="catalog-api"
        value={sim.activeId}
        onchange={onChange}
      >
        {#each configs as config (config.id)}
          <option value={config.id}>{config.displayName}</option>
        {/each}
      </select>
    </label>

    {#if active}
      <dl class="spec" data-testid="engine-spec">
        <div><dt>Stable ID</dt><dd data-testid="active-id">{active.id}</dd></div>
        <div><dt>Reference</dt><dd>{active.manufacturerReference}</dd></div>
        <div><dt>Cylinders</dt><dd>{active.cylinders} in line</dd></div>
        <div><dt>Displacement</dt><dd>{active.displacementL.toFixed(2)} L</dd></div>
        <div><dt>Rated output</dt><dd>{active.ratedPowerKw.toFixed(0)} kW (published)</dd></div>
        <div><dt>Rated torque</dt><dd>{active.ratedTorqueNm.toFixed(0)} N m (published)</dd></div>
        <div><dt>Idle speed</dt><dd>{active.idleRpm.toFixed(0)} rpm (published)</dd></div>
      </dl>
      <p class="disclaimer" data-testid="disclaimer">{active.disclaimer}</p>
      <p class="muted small">
        Rated power and torque are published figures. The engine speeds at which they occur are not
        published and are not modelled here.
      </p>
    {/if}
  {/if}
</section>

<style>
  .spec {
    display: grid;
    gap: 0.25rem 1rem;
    margin: 0.75rem 0 0;
  }
  .spec > div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding-bottom: 0.2rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .disclaimer {
    margin-top: 0.75rem;
    font-size: 0.78rem;
    line-height: 1.45;
    color: var(--muted);
    border-left: 2px solid var(--warn);
    padding-left: 0.6rem;
  }
</style>
