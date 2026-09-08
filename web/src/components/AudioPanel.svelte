<script lang="ts">
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';
  import Icon from './Icon.svelte';
</script>

<section class="panel" aria-labelledby="audio-heading">
  <h2 id="audio-heading" class="mb-4 text-base font-semibold">{l.t('listeningPosition')}</h2>
  <div class="grid grid-cols-2 gap-3" role="group" aria-label={l.t('listeningPosition')} data-testid="audio-stage" data-stage={sim.audioStage}>
    {#each ['cockpit', 'raw'] as stage}
      <button class="min-h-28 flex-col gap-4 border p-4 text-left text-sm {sim.audioStage === stage ? 'border-brand-600 bg-brand-50 text-brand-700' : 'border-stone-200 text-stone-500 hover:bg-stone-50'}" data-testid={`audio-stage-${stage}`} aria-pressed={sim.audioStage === stage} onclick={() => sim.setAudioStage(stage as 'cockpit' | 'raw')}>
        <Icon name={stage === 'cockpit' ? 'drive' : 'engine'} size={28} />
        {l.t(stage as 'cockpit' | 'raw')}
      </button>
    {/each}
  </div>
  <label class="field mt-5"><span>{l.t('volume')}<b class="font-medium tabular-nums">{(sim.audioVolume * 100).toFixed(0)}%</b></span><input data-testid="audio-volume" type="range" min="0" max="100" step="1" value={sim.audioVolume * 100} oninput={(event) => sim.setAudioVolume(Number(event.currentTarget.value) / 100)} /></label>
  <p class="small muted mt-3">{l.t('audioEstimate')}</p>
</section>
