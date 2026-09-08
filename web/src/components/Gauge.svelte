<script lang="ts">
  import { onMount } from 'svelte';
  import MobileSpeedBar from './MobileSpeedBar.svelte';
  import { language as l } from '../lib/i18n.svelte';
  import { sim } from '../lib/state.svelte';
  let { compact = false, controlsExpanded = false, onHeader }:
    { compact?: boolean; controlsExpanded?: boolean; onHeader?: (element: HTMLDivElement | undefined) => void } = $props();
  let header = $state<HTMLDivElement>();
  let largeReadout = $state<HTMLSpanElement>();
  let smallReadout = $state<HTMLSpanElement>();
  let mobile = $state(false);
  const minimized = $derived(compact || (mobile && controlsExpanded));
  $effect(() => { onHeader?.(header); });
  onMount(() => {
    const query = matchMedia('(max-width: 767px)');
    const changed = () => { mobile = query.matches; };
    changed();
    query.addEventListener('change', changed);
    return () => query.removeEventListener('change', changed);
  });
  const s = $derived(sim.readout);
  const runState = $derived(sim.speedLimitPaused ? 'paused' : s?.state ?? 'stopped');
  // Display scale only; the solver owns the model's speed envelope.
  const scaleRpm = 2500;
  const ticks = Array.from({ length: 51 }, (_, i) => i);
  const liveRpm = $derived(Math.max(0, Math.min(scaleRpm, sim.snapshot?.rpm ?? 0)));
  const needleAngle = $derived(135 + liveRpm / scaleRpm * 270);
  const railStart = point(0, 174);
  const railEnd = point(50, 174);
  function point(index: number, radius: number) {
    const angle = (135 + index / 50 * 270) * Math.PI / 180;
    return { x: 240 + Math.cos(angle) * radius, y: 200 + Math.sin(angle) * radius };
  }
</script>

<section class="relative min-w-0 overflow-hidden rounded-2xl border border-stone-800 bg-[#202320] p-5 text-white md:px-7" aria-label={l.t('rpmLabel')} data-testid="speed-panel" data-compact={minimized}>
  <div bind:this={header} class="flex min-h-10 items-center justify-between gap-4">
    <div class="grid gap-2">
      <span class="text-[10px] font-medium tracking-[0.15em] text-stone-400">{l.t('rpmLabel')}</span>
      <span class="flex items-center gap-2 text-[10px] {s?.state === 'running' && !sim.speedLimitPaused ? 'text-orange-300' : 'text-stone-400'}" data-testid="run-state" data-state={runState}>
        <span class="size-1.5 rounded-full bg-current" aria-hidden="true"></span>{l.t(runState)}
      </span>
    </div>
    <div class="flex shrink-0 items-baseline gap-2 transition-[opacity,visibility] duration-200 motion-reduce:transition-none {minimized ? 'visible opacity-100' : 'invisible opacity-0'}" aria-hidden={!minimized}>
      <span bind:this={smallReadout} class="text-3xl font-light tabular-nums" data-testid={minimized ? 'rpm' : undefined}>{Math.round((s?.rpm ?? 0) / 10) * 10}</span><span class="text-xs text-stone-400">rpm</span>
    </div>
  </div>
  <div class="grid transition-[grid-template-rows,opacity,visibility] duration-300 ease-out motion-reduce:transition-none {minimized ? 'invisible grid-rows-[0fr] opacity-0' : 'visible grid-rows-[1fr] opacity-100'}" aria-hidden={minimized}>
    <div class="min-h-0 overflow-hidden">
  <div class="relative mx-auto max-w-[260px] md:max-w-[480px]" data-testid="rpm-block">
      <svg class="block w-full overflow-visible" viewBox="0 0 480 345" aria-hidden="true">
        <circle cx="240" cy="200" r="183" fill="none" stroke="#454a43" stroke-width=".7" stroke-dasharray="1 5" />
        <path data-testid="rpm-rail" d={`M ${railStart.x} ${railStart.y} A 174 174 0 1 1 ${railEnd.x} ${railEnd.y}`} fill="none" stroke="#3b4039" stroke-width="3" />
        {#each ticks as tick}
          {@const a = point(tick, 157)}{@const b = point(tick, tick % 10 === 0 ? 141 : 149)}
          <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={tick / 50 <= liveRpm / scaleRpm && liveRpm > 0 ? '#f07843' : tick % 10 === 0 ? '#c5c9c1' : '#697063'} stroke-width={tick % 10 === 0 ? 2 : 1.5} />
          {#if tick % 10 === 0}
            {@const label = point(tick, 119)}
            <text x={label.x} y={label.y + 4} text-anchor="middle" fill="#a8afa2" font-size="11">{tick * 50}</text>
          {/if}
        {/each}
        <g transform={`rotate(${needleAngle} 240 200)`}>
          <circle data-testid="rpm-pointer" cx="414" cy="200" r="5" fill="#f07843" />
        </g>
      </svg>
    <div class="absolute inset-x-0 top-[44%] flex flex-col items-center">
      <span bind:this={largeReadout} class="text-5xl leading-none font-light tracking-tight tabular-nums md:text-6xl lg:text-7xl" data-testid={minimized ? undefined : 'rpm'}>{Math.round((s?.rpm ?? 0) / 10) * 10}</span>
      <span class="mt-2 text-xs text-stone-400">rpm</span>
    </div>
    <div class="absolute inset-x-0 bottom-[3%] hidden justify-items-center gap-2 md:grid"><b class="grid size-10 place-items-center rounded-xl border border-stone-600 bg-white/5 text-xl font-medium text-orange-300">{sim.controls.gear === 0 ? 'N' : sim.controls.gear}</b><span class="text-[10px] text-stone-400">{l.t(sim.controls.gear === 0 ? 'neutral' : 'gear')}</span></div>
  </div>
    <dl class="mt-2 grid grid-cols-3 divide-x divide-white/10 border-t border-white/10 pt-4 pb-2 md:mt-5 md:pt-5">
      <div><dt class="text-[11px] text-stone-400">{l.t('speed')}</dt><dd class="mt-2 text-2xl font-light tabular-nums"><span data-testid="driveline-speed">{s?.gearEngaged ? (s.vehicleSpeedMPerS * 3.6).toFixed(1) : '—'}</span><small class="mt-1 block text-[10px] text-stone-400 lg:ml-2 lg:inline">km/h</small></dd></div>
      <div class="pl-5"><dt class="text-[11px] text-stone-400">{l.t('torque')}</dt><dd class="mt-2 text-2xl font-light tabular-nums">{s?.cycleValid && s.state !== 'stopped' ? s.brakeTorqueCycleNm.toFixed(0) : '—'}<small class="mt-1 block text-[10px] text-stone-400 lg:ml-2 lg:inline">N·m</small></dd></div>
      <div class="pl-5"><dt class="text-[11px] text-stone-400">{l.t('power')}</dt><dd class="mt-2 text-2xl font-light tabular-nums">{s?.cycleValid && s.state !== 'stopped' ? (s.brakePowerCycleW / 1000).toFixed(1) : '—'}<small class="mt-1 block text-[10px] text-stone-400 lg:ml-2 lg:inline">kW</small></dd></div>
    </dl>
    </div>
  </div>
  <div class="absolute inset-x-0 bottom-0 h-1 bg-white/5 transition-opacity duration-300 {minimized ? 'opacity-100' : 'opacity-0'}" aria-hidden="true"><div class="h-full bg-brand-500" style:width={`${liveRpm / scaleRpm * 100}%`}></div></div>
</section>

<MobileSpeedBar target={minimized ? smallReadout : largeReadout} />
