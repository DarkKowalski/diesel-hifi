<script lang="ts">
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';

  let { target }: { target: HTMLElement | undefined } = $props();
  let bar = $state<HTMLDivElement>();
  let docked = $state(false);
  const shown = $derived(docked);
  const s = $derived(sim.readout);
  const runState = $derived(sim.speedLimitPaused ? 'paused' : s?.state ?? 'stopped');

  $effect(() => {
    if (!target || !bar) return;
    const readout = target;
    const strip = bar;
    const mobile = matchMedia('(max-width: 767px)');
    let observer: IntersectionObserver | undefined;
    const observe = () => {
      observer?.disconnect();
      if (!mobile.matches) { docked = false; return; }
      const height = strip.getBoundingClientRect().height;
      // Hand off before the original number passes underneath the fixed strip.
      observer = new IntersectionObserver(([entry]) => {
        docked = !!entry && entry.boundingClientRect.top < (entry.rootBounds?.top ?? height);
      }, { rootMargin: `-${height}px 0px 0px`, threshold: [0, 1] });
      observer.observe(readout);
    };
    observe();
    mobile.addEventListener('change', observe);
    const size = new ResizeObserver(observe);
    size.observe(strip);
    return () => {
      observer?.disconnect();
      size.disconnect();
      mobile.removeEventListener('change', observe);
    };
  });
</script>

<div bind:this={bar} class="fixed inset-x-0 top-0 z-30 border-b border-white/10 bg-[#202320] px-5 pt-[env(safe-area-inset-top)] text-white shadow-md transition-[transform,opacity,visibility] duration-200 ease-out motion-reduce:transition-none md:hidden {shown ? 'visible translate-y-0 opacity-100' : 'invisible -translate-y-full opacity-0 pointer-events-none'}" aria-hidden={!shown} data-testid="mobile-speed-bar">
  <div class="flex h-16 items-center justify-between gap-4">
    <div class="grid gap-1">
      <span class="text-[10px] tracking-[0.12em] text-stone-400">{l.t('rpmLabel')}</span>
      <span class="flex items-center gap-1.5 text-[11px] {runState === 'running' ? 'text-orange-300' : 'text-stone-400'}" data-testid="mobile-run-state" data-state={runState}>
        <span class="size-1.5 rounded-full bg-current" aria-hidden="true"></span>{l.t(runState)}
      </span>
    </div>
    <div class="flex shrink-0 items-baseline gap-2">
      <span class="text-3xl font-light tabular-nums" data-testid="mobile-rpm">{Math.round((s?.rpm ?? 0) / 10) * 10}</span>
      <span class="text-xs text-stone-400">rpm</span>
    </div>
  </div>
</div>
