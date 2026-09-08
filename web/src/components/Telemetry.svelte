<script lang="ts">
  import { language as l } from '../lib/i18n.svelte';
  import type { MessageKey } from '../lib/messages';
  import { sim } from '../lib/state.svelte';
  import type { Snapshot } from '../worker/protocol';

  type Reading = { label: MessageKey; key: keyof Snapshot; unit: string; scale?: number; digits?: number; id?: string };
  const groups: { label: MessageKey; id: string; readings: Reading[] }[] = [
    { label: 'cycleAverages', id: 'cycle-averages', readings: [
      { label: 'brakeTorque', key: 'brakeTorqueCycleNm', unit: 'N·m', id: 'brake-torque' },
      { label: 'brakePower', key: 'brakePowerCycleW', unit: 'kW', scale: .001, digits: 1, id: 'brake-power' },
      { label: 'indicatedTorque', key: 'indicatedTorqueCycleNm', unit: 'N·m' },
      { label: 'fuelConsumption', key: 'bsfcGPerKwh', unit: 'g/kW·h' },
      { label: 'fuelPerCycle', key: 'fuelPerCycleMg', unit: 'mg', digits: 1, id: 'fuel' },
      { label: 'peakCycle', key: 'peakPressurePaCycle', unit: 'MPa', scale: 1e-6, digits: 2, id: 'peak-cycle' },
    ] },
    { label: 'airPath', id: 'air-path', readings: [
      { label: 'boost', key: 'boostPressurePa', unit: 'bar', scale: 1e-5, digits: 2, id: 'boost' },
      { label: 'turboShaft', key: 'turboShaftRadPerS', unit: 'krpm', scale: 60 / (2 * Math.PI * 1000), digits: 1, id: 'turbo-shaft' },
      { label: 'intakePressure', key: 'intakePressurePa', unit: 'bar', scale: 1e-5, digits: 2, id: 'intake-pressure' },
      { label: 'exhaustPressure', key: 'exhaustPressurePa', unit: 'bar', scale: 1e-5, digits: 2, id: 'exhaust-pressure' },
      { label: 'wastegate', key: 'wastegatePosition', unit: '%', scale: 100 },
      { label: 'egrRate', key: 'egrRate', unit: '%', scale: 100, digits: 1, id: 'egr-rate' },
    ] },
    { label: 'starter', id: 'starter-data', readings: [
      { label: 'starterCurrent', key: 'starterCurrentA', unit: 'A', id: 'starter-current' },
      { label: 'starterTerminal', key: 'starterTerminalVoltageV', unit: 'V', digits: 1, id: 'starter-volts' },
      { label: 'pinion', key: 'starterEngagement', unit: '%', scale: 100, id: 'starter-engagement' },
      { label: 'starterTorque', key: 'torqueStarterNm', unit: 'N·m', id: 'torque-starter' },
    ] },
    { label: 'brakeDriveline', id: 'brake-driveline', readings: [
      { label: 'absorbedPower', key: 'brakeAbsorbedPowerW', unit: 'kW', scale: .001, digits: 1, id: 'brake-absorbed' },
      { label: 'speed', key: 'vehicleSpeedMPerS', unit: 'km/h', scale: 3.6, digits: 1, id: 'vehicle-speed' },
      { label: 'roadLoad', key: 'torqueDrivelineNm', unit: 'N·m', id: 'torque-driveline' },
      { label: 'reflectedInertia', key: 'reflectedInertiaKgM2', unit: 'kg·m²', digits: 1, id: 'reflected-inertia' },
    ] },
  ];
  const s = $derived(sim.readout);
</script>

<section class="panel" aria-labelledby="telemetry-heading">
  <div class="mb-5 flex items-center justify-between gap-4"><h2 id="telemetry-heading" class="text-lg font-semibold">{l.t('telemetry')}</h2><span class="muted small" data-testid="sim-time">{(s?.simTimeS ?? 0).toFixed(2)} s</span></div>
  {#if !s}<p class="muted">{l.t('noSnapshot')}</p>{:else}
    {#each groups as group, index}
      <details class="border-t border-stone-200 pb-3" open={index === 0} data-testid={`telemetry-${group.id}`}>
        <summary>{l.t(group.label)}</summary>
        {#if index === 0 && !s.cycleValid}<p class="small muted">{l.t('noCycle')}</p>{/if}
        <dl class="grid gap-x-6 sm:grid-cols-2 [&>div]:flex [&>div]:items-baseline [&>div]:justify-between [&>div]:gap-3 [&>div]:border-b [&>div]:border-stone-100 [&>div]:py-3 [&_dt]:text-xs [&_dt]:text-stone-500 [&_dd]:text-right [&_dd]:text-sm [&_dd]:tabular-nums [&_dd_span]:text-[10px] [&_dd_span]:text-stone-500" data-testid={group.id}>
          {#each group.readings as reading}
            <div><dt>{l.t(reading.label)}</dt><dd data-testid={reading.id}>{index === 0 && !s.cycleValid ? '—' : ((s[reading.key] as number) * (reading.scale ?? 1)).toFixed(reading.digits ?? 0)} <span>{reading.unit}</span></dd></div>
          {/each}
          {#if group.id === 'brake-driveline'}<div><dt>{l.t('activeBrakeStage')}</dt><dd data-testid="brake-stage-active">{s.brakeActive ? ['I', 'II', 'III'][s.brakeStageActive - 1] : l.t('off')}</dd></div>{/if}
        </dl>
      </details>
    {/each}
    <details class="border-t border-stone-200 pb-3" data-testid="telemetry-cylinders"><summary>{l.t('cylinderPressure')}</summary>
      <ol class="mb-4 grid gap-3 [&_li]:grid [&_li]:grid-cols-[24px_minmax(0,1fr)_70px] [&_li]:items-center [&_li]:gap-3 [&_li]:text-xs [&_li]:text-stone-500 [&_li]:tabular-nums [&_li>span:last-child]:text-right" data-testid="cylinders">{#each s.cylinderPressurePa as pressure, index}<li><span>0{index + 1}</span><meter class="h-2 w-full accent-brand-600" min="0" max="230" value={pressure / 1e5} aria-label={`${l.t('cylinderPressure')} ${index + 1}`}></meter><span>{(pressure / 1e5).toFixed(1)} bar</span></li>{/each}</ol>
      <p class="muted small">{l.t('pressureHint')}</p>
      <dl class="grid gap-x-6 sm:grid-cols-2 [&>div]:flex [&>div]:items-baseline [&>div]:justify-between [&>div]:gap-3 [&>div]:border-b [&>div]:border-stone-100 [&>div]:py-3 [&_dt]:text-xs [&_dt]:text-stone-500 [&_dd]:text-right [&_dd]:text-sm [&_dd]:tabular-nums [&_dd_span]:text-[10px] [&_dd_span]:text-stone-500"><div><dt>{l.t('peakSession')}</dt><dd data-testid="peak-session">{(s.peakPressurePaSession / 1e6).toFixed(2)} <span>MPa</span></dd></div></dl>
    </details>
  {/if}
</section>
