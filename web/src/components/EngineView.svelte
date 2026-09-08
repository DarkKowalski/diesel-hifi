<script lang="ts">
  import { onMount } from 'svelte';
  import type { EngineGeometry } from '../lib/engineGeometry';
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';

  let { geometry, slowMotion = $bindable(true) }: { geometry: EngineGeometry; slowMotion?: boolean } = $props();
  let element: HTMLElement;
  let phase = $state(0);
  let reducedMotion = $state(false);
  let visible = $state(true);
  let foreground = $state(true);
  const cycle = 4 * Math.PI;
  const radius = 22;
  const rod = $derived(radius * geometry.connecting_rod_m / (geometry.stroke_m / 2));
  const width = $derived(geometry.cylinders * 76 + 40);
  const offsets = $derived(Array.from({ length: geometry.cylinders }, (_, index) =>
    geometry.firing_order.indexOf(index + 1) * cycle / geometry.cylinders));
  const moving = $derived(sim.running && (sim.snapshot?.rpm ?? 0) > 0
    && sim.snapshot?.state !== 'fault' && !sim.speedLimitPaused
    && !reducedMotion && visible && foreground);
  const firing = $derived(sim.snapshot?.state === 'running' && sim.controls.ignition
    && (sim.snapshot?.fuelDemandMg ?? 0) > 0 && !sim.speedLimitPaused);
  let previousTime = 0;
  let previousConfig = '';

  $effect(() => {
    const snapshot = sim.snapshot;
    if (!snapshot) return;
    if (snapshot.simTimeS < previousTime || snapshot.configId !== previousConfig) phase = 0;
    previousTime = snapshot.simTimeS;
    previousConfig = snapshot.configId;
  });

  $effect(() => {
    if (!moving) return;
    let last = performance.now();
    let frame: number;
    const animate = (now: number) => {
      // Only the drawing clock changes; preserve phase when switching speed.
      phase = (phase + Math.min(now - last, 50) / 1000
        * (sim.snapshot?.rpm ?? 0) / 60 * Math.PI * 2 * (slowMotion ? .1 : 1)) % cycle;
      last = now;
      frame = requestAnimationFrame(animate);
    };
    frame = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(frame);
  });

  onMount(() => {
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    const motionChanged = () => { reducedMotion = preference.matches; };
    const visibilityChanged = () => { foreground = !document.hidden; };
    motionChanged();
    visibilityChanged();
    preference.addEventListener('change', motionChanged);
    document.addEventListener('visibilitychange', visibilityChanged);
    const observer = new IntersectionObserver(([entry]) => { visible = entry?.isIntersecting ?? false; });
    observer.observe(element);
    return () => {
      preference.removeEventListener('change', motionChanged);
      document.removeEventListener('visibilitychange', visibilityChanged);
      observer.disconnect();
    };
  });
</script>

<section bind:this={element} class="panel overflow-hidden" aria-labelledby="engine-view-heading" data-testid="engine-animation" data-moving={moving}>
  <div class="flex items-center justify-between gap-3">
    <h2 id="engine-view-heading" class="text-sm font-semibold">{l.t('engineView')}</h2>
    <button class="-my-2 -mr-2 gap-2.5 px-2 text-xs text-stone-600" type="button" role="switch" aria-checked={slowMotion} data-testid="engine-slow-motion" disabled={reducedMotion} title={reducedMotion ? l.t('reducedMotion') : undefined} onclick={() => { slowMotion = !slowMotion; }}>
      <span>{l.t('slowMotion')}</span>
      <span class="flex h-5 w-9 items-center rounded-full p-0.5 {slowMotion ? 'bg-brand-600' : 'bg-stone-300'}" aria-hidden="true">
        <span class="size-4 rounded-full bg-white transition-transform {slowMotion ? 'translate-x-4' : ''}"></span>
      </span>
    </button>
  </div>
  <svg class="mx-auto mt-3 block max-h-[280px] w-full" viewBox={`0 0 ${width} 242`} role="img" aria-label={l.t('engineCutaway')}>
    <defs>
      <linearGradient id="piston-metal" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" stop-color="#a6ada5" /><stop offset=".4" stop-color="#f4f5ef" /><stop offset="1" stop-color="#90998e" />
      </linearGradient>
      <linearGradient id="engine-block" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#eaede5" /><stop offset="1" stop-color="#f6f7f2" />
      </linearGradient>
    </defs>
    <path d={`M 23 50 H ${width - 23} V 154 L ${width - 32} 168 V 213 H 32 V 168 L 23 154 Z`} fill="url(#engine-block)" stroke="#d1d6cb" />
    <path d={`M 34 216 H ${width - 34} L ${width - 49} 227 H 49 Z`} fill="#ced4c7" />
    <rect x="17" y="38" width={width - 34} height="17" rx="5" fill="#3d4739" />
    <path d={`M 28 190 H ${width - 28}`} stroke="#c2c9ba" stroke-width="10" stroke-linecap="round" />
    {#each offsets as offset, index}
      {@const x = 58 + index * 76}
      {@const angle = (phase - offset + cycle) % cycle}
      {@const pinX = x + Math.sin(angle) * radius}
      {@const pinY = 190 - Math.cos(angle) * radius}
      {@const pistonY = pinY - Math.sqrt(rod * rod - (pinX - x) ** 2)}
      {@const power = angle < Math.PI && firing ? Math.sin(angle) * .65 : 0}
      {@const intake = angle > 2 * Math.PI && angle < 3 * Math.PI}
      {@const exhaust = angle > Math.PI && angle < 2 * Math.PI}
      <g>
        <rect x={x - 25} y="55" width="50" height="99" rx="2" fill="#fff" stroke="#bdc5b6" />
        <rect x={x - 23} y="56" width="46" height={Math.max(0, pistonY - 70)} fill="#ed763d" opacity={power} />
        <line x1={x - 14} y1="30" x2={x - 14} y2={intake ? 63 : 57} stroke="#929d8a" stroke-width="3" />
        <line x1={x + 14} y1="30" x2={x + 14} y2={exhaust ? 63 : 57} stroke="#929d8a" stroke-width="3" />
        <path d={`M ${x - 20} ${intake ? 63 : 57} h 12 M ${x + 8} ${exhaust ? 63 : 57} h 12`} stroke="#5f6b56" stroke-width="3" stroke-linecap="round" />
        <rect x={x - 3} y="28" width="6" height="28" rx="2" fill="#6b7563" />
        <circle cx={x} cy="23" r="2" fill="#f07843" />
        <circle cx={x} cy="190" r="25" fill="#eef0e8" stroke="#c6cebc" />
        <path d={`M ${x} 190 L ${pinX} ${pinY}`} stroke="#727f66" stroke-width="14" stroke-linecap="round" />
        <line x1={x} y1={pistonY} x2={pinX} y2={pinY} stroke="#89967d" stroke-width="9" stroke-linecap="round" />
        <line x1={x} y1={pistonY} x2={pinX} y2={pinY} stroke="#cbd3c1" stroke-width="4" />
        <g transform={`translate(${x}, ${pistonY})`} data-testid="engine-piston">
          <rect x="-23" y="-14" width="46" height="24" rx="3" fill="url(#piston-metal)" stroke="#8f9b84" />
          <path d="M -22 -9 H 22 M -22 -5 H 22" stroke="#77846b" stroke-width="1.5" />
          <circle r="3.5" fill="#6f7d63" stroke="#d9dfd2" />
        </g>
        <circle data-testid="engine-crank-pin" cx={pinX} cy={pinY} r="4" fill="#4c5a40" stroke="#e7ebdf" stroke-width="2" />
        <circle cx={x} cy="190" r="3" fill="#dbe2d2" />
      </g>
    {/each}
  </svg>
</section>
