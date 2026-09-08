<script lang="ts">
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';
  import Icon from './Icon.svelte';
  import SelectMenu from './SelectMenu.svelte';
  let showSpecs = $state(false);
  const options = $derived(sim.configs.map((config) => ({ value: config.id,
    label: config.displayName.replace(/ Reference$/, ''),
    detail: `${l.t('referenceModel')} · ${config.displacementL.toFixed(1)} L · ${config.ratedPowerKw.toFixed(0)} kW`,
  })));
</script>

<section class="flex flex-col gap-4 rounded-2xl border border-stone-200 bg-white p-4 md:flex-row md:items-center md:justify-between md:gap-6 md:px-6" aria-label={l.t('engine')}>
  <div class="flex min-w-0 items-center gap-3">
    <span class="hidden text-stone-400 lg:block"><Icon name="engine" size={28} /></span>
    <div class="grid min-w-0 grow gap-2"><span class="eyebrow">{l.t('configuration')}</span>
      <SelectMenu id="engine-select" label={l.t(sim.configs.length ? 'configuration' : 'catalogLoading')} value={sim.activeId} {options} source="catalog-api" disabled={sim.lifecycle !== 'ready' || sim.busy} onchange={(value) => sim.selectConfig(value)} />
    </div>
    <button class="btn-quiet shrink-0 self-end px-2 text-xs md:hidden" data-testid="specs-toggle" aria-expanded={showSpecs} aria-controls="engine-specifications" onclick={() => showSpecs = !showSpecs}>{l.t('specs')}</button>
  </div>
  {#if sim.activeConfig}
    <dl id="engine-specifications" class="shrink-0 justify-between gap-4 border-t border-stone-100 px-3 pt-3 md:flex md:gap-6 md:border-0 md:p-0 lg:gap-9 {showSpecs ? 'flex' : 'hidden'}" data-testid="engine-spec">
      <div><dt class="text-[10px] text-stone-500">{l.t('displacement')}<small class="ml-1 text-[9px] md:ml-0 md:block">{l.t('derived')}</small></dt><dd class="mt-1 text-xl font-medium tabular-nums">{sim.activeConfig.displacementL.toFixed(1)} <span class="text-[10px] font-normal text-stone-500">L</span></dd></div>
      <div><dt class="text-[10px] text-stone-500">{l.t('ratedPower')}<small class="ml-1 text-[9px] md:ml-0 md:block">{l.t('published')}</small></dt><dd class="mt-1 text-xl font-medium tabular-nums">{sim.activeConfig.ratedPowerKw.toFixed(0)} <span class="text-[10px] font-normal text-stone-500">kW</span></dd></div>
      <div><dt class="text-[10px] text-stone-500">{l.t('ratedTorque')}<small class="ml-1 text-[9px] md:ml-0 md:block">{l.t('published')}</small></dt><dd class="mt-1 text-xl font-medium tabular-nums">{sim.activeConfig.ratedTorqueNm.toFixed(0)} <span class="text-[10px] font-normal text-stone-500">N·m</span></dd></div>
    </dl>
  {/if}
</section>
