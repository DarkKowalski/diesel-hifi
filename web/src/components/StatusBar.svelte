<script lang="ts">
  import { sim } from '../lib/state.svelte';

  const lifecycle = $derived(sim.lifecycle);
  const ready = $derived(sim.ready);
  const hasError = $derived(sim.errorMessage.length > 0);
</script>

<div class="status" data-testid="status-bar" data-lifecycle={lifecycle}>
  <span class="dot" data-state={lifecycle} aria-hidden="true"></span>
  <span data-testid="lifecycle">{lifecycle}</span>

  {#if ready}
    <span class="muted small">
      api v{ready.apiVersion} · snapshot v{ready.snapshotVersion} · protocol v{ready.protocolVersion}
      · step {(ready.fixedStepS * 1e6).toFixed(0)} µs
    </span>
    <span class="muted small worker-note">simulation runs in a Web Worker</span>
  {/if}
</div>

{#if hasError}
  <div class="error" role="alert" data-testid="error-banner">
    <strong data-testid="error-code">{sim.errorCode}</strong>
    <span data-testid="error-message">{sim.errorMessage}</span>
    <button onclick={() => sim.clearError()}>Dismiss</button>
  </div>
{/if}

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    flex-wrap: wrap;
    padding: 0.5rem 0.9rem;
    border: 1px solid var(--rule);
    border-radius: 8px;
    background: var(--panel);
    font-size: 0.8rem;
  }
  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot[data-state='ready'] {
    background: var(--ok);
  }
  .dot[data-state='loading'] {
    background: var(--warn);
  }
  .dot[data-state='error'] {
    background: var(--bad);
  }
  .worker-note {
    margin-left: auto;
  }
  .error {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    flex-wrap: wrap;
    margin-top: 0.6rem;
    padding: 0.6rem 0.9rem;
    border: 1px solid var(--bad);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bad) 12%, transparent);
    font-size: 0.85rem;
  }
  .error button {
    margin-left: auto;
  }
</style>
