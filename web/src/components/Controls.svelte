<script lang="ts">
  import { onMount } from 'svelte';
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';
  import Icon from './Icon.svelte';
  let { expanded = $bindable(false), speedPanelHeader }:
    { expanded?: boolean; speedPanelHeader?: HTMLElement } = $props();
  let mobile = $state(true);
  let toggle = $state<HTMLButtonElement>();
  let drawer = $state<HTMLElement>();
  let heading = $state<HTMLDivElement>();
  let scrollLimit = $state<number>();
  const open = $derived(expanded || !mobile);
  const runState = $derived(sim.snapshot?.state ?? 'stopped');
  const unavailable = $derived(sim.lifecycle !== 'ready' || sim.busy);
  const inputDisabled = $derived(unavailable || (runState === 'fault' && !sim.speedLimitPaused));
  const started = $derived(sim.controls.ignition && (runState === 'running' || runState === 'cranking' || sim.controls.starter));
  // Current UI bounds; all submitted inputs are validated by the simulation core.
  const gears = 12;
  onMount(() => {
    const query = matchMedia('(max-width: 767px)');
    const changed = () => { mobile = query.matches; };
    changed();
    query.addEventListener('change', changed);
    return () => query.removeEventListener('change', changed);
  });
  $effect(() => {
    if (!mobile || !speedPanelHeader || !drawer || !heading) return;
    const speedHeader = speedPanelHeader;
    const panel = drawer;
    const panelHeading = heading;
    const isExpanded = expanded;
    const measure = () => {
      const stripBottom = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 64;
      const speed = speedHeader.getBoundingClientRect();
      const speedVisible = speed.top >= stripBottom;
      // Reserve the compact speed card above the drawer; scrolled pages reserve the top strip.
      const clearUntil = speedVisible ? speed.bottom + 32 : stripBottom + 12;
      const available = innerHeight - parseFloat(getComputedStyle(panel).bottom) - panelHeading.offsetHeight - clearUntil - 2;
      if (isExpanded && speedVisible && available < 160) {
        // Short landscape screens use the normal scrolling handoff to the top strip.
        window.scrollBy(0, speed.bottom - stripBottom + 1);
        return;
      }
      scrollLimit = Math.max(160, available);
    };
    const frame = requestAnimationFrame(measure);
    const resize = new ResizeObserver(measure);
    resize.observe(speedHeader);
    resize.observe(panelHeading);
    window.addEventListener('scroll', measure, { passive: true });
    window.addEventListener('resize', measure);
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      window.removeEventListener('scroll', measure);
      window.removeEventListener('resize', measure);
    };
  });
  function escape(event: KeyboardEvent) {
    if (event.key === 'Escape' && mobile && expanded) {
      event.preventDefault();
      expanded = false;
      toggle?.focus({ preventScroll: true });
    }
  }
</script>

<svelte:window onkeydown={escape} />

<section bind:this={drawer} class="fixed inset-x-3 bottom-[calc(5rem+env(safe-area-inset-bottom))] z-20 overflow-hidden rounded-t-2xl border border-stone-200 bg-white shadow-xl md:static md:rounded-2xl md:shadow-none" id="controls" tabindex="-1" aria-labelledby="controls-heading" data-testid="controls-drawer">
  <div bind:this={heading} class="flex min-h-15 items-center justify-between gap-3 bg-[#30362e] px-4 text-white md:bg-white md:px-5 md:pt-5 md:text-stone-900 lg:px-6 lg:pt-6">
    <h2 id="controls-heading" class="min-w-0 flex-1 text-lg font-semibold">
      <span class="hidden md:inline">{l.t('controls')}</span>
      <button bind:this={toggle} class="w-full justify-start gap-3 rounded-lg px-0 text-base font-semibold focus-visible:outline-offset-0 md:hidden" data-testid="controls-toggle" aria-expanded={expanded} aria-controls="controls-body" onclick={() => { expanded = !expanded; }}>
        <Icon name="details" size={17} /><span>{l.t('controls')}</span>
        <svg class="ml-auto size-4 transition-transform duration-300 motion-reduce:transition-none {expanded ? 'rotate-180' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m6 15 6-6 6 6" /></svg>
      </button>
    </h2>
    <button class="rounded-lg px-3 text-stone-300 hover:bg-white/10 hover:text-white disabled:opacity-60 md:text-stone-500 md:hover:bg-stone-100 md:hover:text-stone-900" data-testid="reset" aria-label={l.t('reset')} title={l.t('reset')} disabled={unavailable} onclick={() => sim.resetSimulation()}><Icon name="reset" size={17} /></button>
  </div>
  <div id="controls-body" class="grid transition-[grid-template-rows,opacity,visibility] duration-300 ease-out motion-reduce:transition-none md:visible md:grid-rows-[1fr] md:opacity-100 {open ? 'visible grid-rows-[1fr] opacity-100' : 'invisible grid-rows-[0fr] opacity-0'}" inert={!open} aria-hidden={!open}>
    <div class="min-h-0 overflow-hidden">
      <div class="max-h-[var(--controls-max-height,calc(100dvh-13rem-env(safe-area-inset-top)-env(safe-area-inset-bottom)))] overflow-y-auto overscroll-contain px-5 pt-4 pb-5 md:max-h-none md:overflow-visible lg:px-6 lg:pb-6" style:--controls-max-height={scrollLimit === undefined ? undefined : `${scrollLimit}px`} data-testid="controls-scroll">
        <div class="grid grid-cols-[1.3fr_1fr] gap-2">
          <button class="btn-primary min-h-14 px-3" data-testid="start" onclick={() => sim.startEngine()} disabled={unavailable || started || sim.speedLimitPaused || runState === 'fault' || (runState === 'running' && !sim.controls.ignition)}><Icon name="power" size={17} />{l.t('start')}</button>
          <button class="btn px-3" data-testid="stop" onclick={() => sim.stopEngine()} disabled={unavailable || !started || sim.speedLimitPaused}>{l.t('stop')}</button>
        </div>
        <div class="mt-5 grid gap-5 divide-y divide-stone-100">
          <div>
            <label class="field"><span class="font-semibold">{l.t('pedal')}<output class="text-2xl font-normal text-brand-600 tabular-nums">{(sim.controls.pedal * 100).toFixed(0)}<small class="ml-1 text-xs">%</small></output></span>
              <input data-testid="pedal" type="range" min="0" max="100" step="1" value={sim.controls.pedal * 100} disabled={inputDisabled} oninput={(event) => sim.setControls({ pedal: Number(event.currentTarget.value) / 100 })} />
            </label>
            <div class="flex items-center justify-between gap-2"><span class="text-[10px] text-stone-400">0 — 100%</span><button class="btn-quiet -mr-2 px-2 text-xs" data-testid="release-pedal" disabled={unavailable || sim.controls.pedal === 0} onclick={() => sim.setControls({ pedal: 0 })}>{l.t('release')}</button></div>
          </div>
          <div class="pt-4">
            <div class="flex justify-between text-sm font-semibold"><span>{l.t('gear')}</span><span class="text-brand-600" data-testid="gear-label">{sim.controls.gear === 0 ? l.t('neutral') : sim.controls.gear}</span></div>
            <div class="mt-2 grid grid-cols-[44px_minmax(0,1fr)_44px] items-center gap-3">
              <button class="btn px-0 text-xl" aria-label={l.t('gearDown')} disabled={inputDisabled || sim.controls.gear === 0} onclick={() => sim.setControls({ gear: sim.controls.gear - 1 })}>−</button>
              <label class="grid"><span class="sr-only">{l.t('gear')}</span><input data-testid="gear" type="range" min="0" max={gears} step="1" value={sim.controls.gear} disabled={inputDisabled} oninput={(event) => sim.setControls({ gear: Number(event.currentTarget.value) })} /></label>
              <button class="btn px-0 text-xl" aria-label={l.t('gearUp')} disabled={inputDisabled || sim.controls.gear === gears} onclick={() => sim.setControls({ gear: sim.controls.gear + 1 })}>+</button>
            </div>
          </div>
          <div class="pt-4">
            <p class="mb-3 text-sm font-semibold">{l.t('brake')}</p>
            <div class="flex gap-2" role="group" aria-label={l.t('brakeStage')}>
              {#each [0, 1, 2, 3] as stage}<button class="flex-1 border px-2 {sim.controls.brakeStage === stage ? 'border-stone-900 bg-stone-900 text-white' : 'border-stone-200 hover:bg-stone-100'}" data-testid={`brake-stage-${stage}`} aria-pressed={sim.controls.brakeStage === stage} disabled={inputDisabled} onclick={() => sim.setControls({ brakeStage: stage })}>{stage === 0 ? l.t('off') : ['I', 'II', 'III'][stage - 1]}</button>{/each}
            </div>
            {#if sim.controls.brakeStage > 0}<p class="small mt-3 {sim.snapshot?.brakeActive ? 'text-brand-700' : 'muted'}" data-testid={sim.snapshot?.brakeActive ? 'brake-engaged' : 'brake-inhibited'}>{l.t(sim.snapshot?.brakeActive ? 'brakeActive' : 'brakeInhibited')}</p>{/if}
          </div>
          <label class="field pt-4"><span class="font-semibold">{l.t('load')}<b class="text-sm font-normal tabular-nums" data-testid="load-label">{sim.controls.loadTorqueNm.toFixed(0)} <small class="text-stone-500">N·m</small></b></span>
            <input data-testid="load" type="range" min="0" max="5000" step="50" value={sim.controls.loadTorqueNm} disabled={inputDisabled} oninput={(event) => sim.setControls({ loadTorqueNm: Number(event.currentTarget.value) })} />
          </label>
          <div class="pt-4">
            <label class="field"><span class="font-semibold">{l.t('grade')}<b class="font-normal tabular-nums" data-testid="grade-label">{sim.controls.roadGradePercent.toFixed(1)}%</b></span>
              <input data-testid="grade" type="range" min="-15" max="15" step="0.5" value={sim.controls.roadGradePercent} disabled={inputDisabled} oninput={(event) => sim.setControls({ roadGradePercent: Number(event.currentTarget.value) })} />
            </label>
            <div class="flex items-center justify-between text-[10px] text-stone-500"><span>{l.t('descent')}</span><button class="btn-quiet text-xs" data-testid="level-road" disabled={inputDisabled} onclick={() => sim.setControls({ roadGradePercent: 0 })}>{l.t('levelRoad')}</button><span>{l.t('climb')}</span></div>
          </div>
        </div>
      </div>
    </div>
  </div>
</section>
