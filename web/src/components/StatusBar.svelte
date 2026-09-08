<script lang="ts">
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';
</script>

<div class="hidden shrink-0 items-center gap-2 text-xs text-stone-300 xl:inline-flex" data-testid="status-bar" data-lifecycle={sim.lifecycle} role="status"><span class="size-1.5 rounded-full {sim.lifecycle === 'ready' ? 'bg-emerald-600' : 'bg-amber-500'}" aria-hidden="true"></span><span data-testid="lifecycle">{l.t(sim.lifecycle !== 'ready' ? sim.lifecycle : sim.speedLimitPaused ? 'paused' : sim.snapshot?.state ?? 'stopped')}</span></div>

{#if sim.speedLimitPaused || sim.errorCode || sim.audioError || (sim.audioRequested && !sim.audioRunning && !sim.audioStarting && sim.running)}
  <div class="fixed inset-x-4 bottom-[calc(5.5rem+env(safe-area-inset-bottom))] z-40 mx-auto max-w-xl rounded-2xl border border-orange-200 bg-white p-5 text-stone-900 shadow-xl md:bottom-6" role="alert" data-testid={sim.speedLimitPaused ? 'speed-limit-banner' : sim.errorCode ? 'error-banner' : 'audio-error'}>
    {#if sim.speedLimitPaused}
      <strong class="text-sm text-brand-700">{l.t('overspeed')}</strong><p class="mt-2 mb-4 text-sm leading-relaxed text-stone-600">{l.t('overspeedHint')}</p>
      <button class="btn-primary" data-testid="resume" disabled={sim.busy} onclick={() => sim.resumeSimulation()}>{l.t('resume')}</button>
    {:else if sim.errorCode}
      <strong class="text-sm text-brand-700">{l.t('errorTitle')}</strong><p class="mt-2 mb-4 text-sm leading-relaxed text-stone-600">{l.t(sim.lifecycle === 'error' ? 'loadError' : 'errorHint')}</p>
      {#if sim.lifecycle === 'error'}<button class="btn-primary" onclick={() => location.reload()}>{l.t('reload')}</button>{:else}<button class="btn-primary" disabled={sim.busy} onclick={() => sim.resetSimulation()}>{l.t('reset')}</button>{/if}
    {:else}<p class="small text-stone-600">{l.t(sim.audioError ? 'audioError' : 'audioInterrupted')}</p><button class="btn mt-3" data-testid="resume-sound" disabled={sim.audioStarting} onclick={() => sim.enableAudio()}>{l.t('resumeSound')}</button>{/if}
  </div>
{/if}
