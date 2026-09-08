<script lang="ts">
  import AudioPanel from './components/AudioPanel.svelte';
  import Controls from './components/Controls.svelte';
  import EngineSelector from './components/EngineSelector.svelte';
  import EngineView from './components/EngineView.svelte';
  import Gauge from './components/Gauge.svelte';
  import Icon from './components/Icon.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import SelectMenu from './components/SelectMenu.svelte';
  import Telemetry from './components/Telemetry.svelte';
  import { language as l } from './lib/i18n.svelte';
  import { sim } from './lib/state.svelte';
  import type { EngineGeometries } from './lib/engineGeometry';

  let { geometries }: { geometries: EngineGeometries } = $props();

  let view = $state<'drive' | 'sound' | 'details'>('drive');
  let engineSlowMotion = $state(true);
  let controlsExpanded = $state(false);
  let speedPanelHeader = $state<HTMLDivElement>();
  const views = ['drive', 'sound', 'details'] as const;
  function navigate(next: typeof view) { view = next; controlsExpanded = false; window.scrollTo(0, 0); }
  $effect(() => { void sim.start(); return () => sim.destroy(); });
  $effect(() => {
    if (sim.snapshot?.state === 'running') void sim.releaseStarterIfRunning();
  });
  $effect(() => { document.documentElement.lang = l.locale; document.title = l.t('appTitle'); });
</script>

<a class="sr-only z-50 bg-brand-600 p-4 text-white focus:not-sr-only focus:fixed focus:top-2 focus:left-2" href="#controls" onclick={() => { controlsExpanded = true; }}>{l.t('skip')}</a>
<header class="border-b border-stone-800 bg-[#202320] text-white">
  <div class="mx-auto flex h-16 max-w-7xl items-center justify-between gap-4 px-5 md:h-18 md:px-8">
    <a class="flex shrink-0 items-center gap-3 no-underline" href="#main" onclick={(event) => { event.preventDefault(); navigate('drive'); }} aria-label="Diesel HiFi">
      <span class="grid size-9 place-items-center rounded-lg border border-white/15 text-brand-500"><Icon name="engine" size={22} /></span>
      <span class="text-base font-semibold tracking-tight">Diesel <span class="font-normal text-stone-300">HiFi</span></span>
    </a>
    <nav class="fixed inset-x-0 bottom-0 z-30 flex h-[calc(5rem+env(safe-area-inset-bottom))] justify-around gap-1 border-t border-stone-200 bg-white p-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] md:static md:ml-5 md:mr-auto md:h-auto md:border-0 md:bg-transparent md:p-0" aria-label={l.t('navigation')}>
      {#each views as item}
        <button data-testid={`nav-${item}`} class="flex-1 flex-col gap-1 rounded-lg px-5 text-xs md:flex-row md:gap-2 md:px-4 md:text-sm {view === item ? 'bg-brand-50 text-brand-700 md:bg-white/10 md:text-orange-300' : 'text-stone-500 hover:bg-stone-100 md:text-stone-300 md:hover:bg-white/5'}" aria-current={view === item ? 'page' : undefined} onclick={() => navigate(item)}>
          <Icon name={item} size={17} /><span>{l.t(item)}</span>
        </button>
      {/each}
    </nav>
    <StatusBar />
    <div class="flex min-w-0 items-center gap-2 text-stone-300"><span class="hidden sm:block"><Icon name="globe" size={16} /></span>
      <SelectMenu id="language-select" label={l.t('language')} value={l.locale} options={[{ value: 'en', label: 'English' }, { value: 'zh-CN', label: '简体中文' }]} dark align="right" onchange={(value) => l.set(value === 'zh-CN' ? 'zh-CN' : 'en')} />
    </div>
  </div>
</header>

<main id="main" class="mx-auto max-w-7xl px-4 pt-5 pb-[calc(10rem+env(safe-area-inset-bottom))] md:px-8 md:pt-7 md:pb-8">
  <h1 class="sr-only">{l.t(view)}</h1>
  <EngineSelector />
  <div class="mt-5 grid items-start gap-5 md:grid-cols-[minmax(0,1fr)_300px] lg:grid-cols-[minmax(0,1fr)_340px]">
    <div class="contents min-w-0 md:grid md:gap-5">
      <Gauge compact={view !== 'drive'} {controlsExpanded} onHeader={(element) => { speedPanelHeader = element; }} />
      {#if view === 'drive' && geometries[sim.activeId]}
        <EngineView geometry={geometries[sim.activeId]!} bind:slowMotion={engineSlowMotion} />
      {/if}
      {#if view === 'sound'}<AudioPanel />{/if}
      <div hidden={view !== 'details'}><Telemetry /></div>
    </div>
    <aside class="contents md:sticky md:top-5 md:block">
      <Controls bind:expanded={controlsExpanded} {speedPanelHeader} />
    </aside>
  </div>
  <footer class="mt-9 flex flex-col justify-between gap-2 border-t border-stone-200 pt-5 text-[11px] text-stone-500 md:flex-row"><span>{l.t('disclaimer')}</span></footer>
</main>
