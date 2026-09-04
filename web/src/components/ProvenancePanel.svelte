<script lang="ts">
  import { sim } from '../lib/state.svelte';
  import type { ProvenanceStatus } from '../worker/protocol';

  const report = $derived(sim.provenance);
  let filter = $state<ProvenanceStatus | 'all'>('all');

  const entries = $derived(
    (report?.entries ?? []).filter((entry) => filter === 'all' || entry.status === filter),
  );
</script>

<section class="panel" aria-labelledby="provenance-heading">
  <h2 id="provenance-heading">Provenance</h2>

  {#if !report}
    <p class="muted">No provenance loaded.</p>
  {:else}
    <p class="muted small">
      Every parameter is labelled. <b>Published</b> values are printed in the source document and
      name a locator. <b>Derived</b> values state their formula. <b>Calibrated</b> values are our
      estimates and are never OEM data.
    </p>

    <div class="counts" data-testid="provenance-counts">
      <span>published <b data-testid="count-published">{report.published_count}</b></span>
      <span>derived <b data-testid="count-derived">{report.derived_count}</b></span>
      <span>calibrated <b data-testid="count-calibrated">{report.calibrated_count}</b></span>
    </div>

    <h3>Sources</h3>
    <ul class="sources">
      {#each report.sources as source (source.id)}
        <li>
          <b>{source.title}</b>
          <div class="muted small">
            {source.publisher} · technical status {source.technical_status} · order
            {source.order_number}
          </div>
          <div class="muted small">{source.scope}</div>
        </li>
      {/each}
    </ul>

    <h3>Parameters</h3>
    <div class="filters">
      {#each ['all', 'published', 'derived', 'calibrated'] as option (option)}
        <button
          class:active={filter === option}
          onclick={() => (filter = option as ProvenanceStatus | 'all')}
        >
          {option}
        </button>
      {/each}
    </div>

    <ul class="entries" data-testid="provenance-entries">
      {#each entries as entry (entry.path)}
        <li>
          <div class="row">
            <code>{entry.path}</code>
            <span class="badge" data-status={entry.status}>{entry.status}</span>
          </div>
          <div class="muted small">{entry.value_note}</div>
          {#if entry.locator}
            <div class="muted small">Locator: {entry.locator}</div>
          {/if}
          {#if entry.formula}
            <div class="muted small">Formula: {entry.formula} ({entry.inputs.join('; ')})</div>
          {/if}
          {#if entry.purpose}
            <div class="muted small">Purpose: {entry.purpose}</div>
          {/if}
          {#if entry.safe_range}
            <div class="muted small">
              Safe range: {entry.safe_range[0]} … {entry.safe_range[1]}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .counts {
    display: flex;
    gap: 1rem;
    margin: 0.6rem 0;
    font-size: 0.8rem;
  }
  h3 {
    margin: 1rem 0 0.4rem;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .sources,
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.6rem;
  }
  .entries {
    max-height: 22rem;
    overflow-y: auto;
    padding-right: 0.4rem;
  }
  .entries li {
    border-bottom: 1px solid var(--rule);
    padding-bottom: 0.4rem;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }
  code {
    font-size: 0.78rem;
  }
  .badge {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    border-radius: 999px;
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--rule);
  }
  .badge[data-status='published'] {
    color: var(--ok);
    border-color: var(--ok);
  }
  .badge[data-status='derived'] {
    color: var(--accent);
    border-color: var(--accent);
  }
  .badge[data-status='calibrated'] {
    color: var(--warn);
    border-color: var(--warn);
  }
  .filters {
    display: flex;
    gap: 0.35rem;
    margin-bottom: 0.6rem;
  }
  .filters button {
    font-size: 0.72rem;
    padding: 0.2rem 0.6rem;
  }
  .filters button.active {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
