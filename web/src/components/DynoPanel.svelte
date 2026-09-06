<script lang="ts">
  import { sim } from '../lib/state.svelte';
  import type { OperatingPoint } from '../worker/protocol';

  /*
   * Torque and power against engine speed are two measures on different scales.
   * The conventional dyno chart puts them on one plot with two y-axes, which
   * invents a crossing point that means nothing. These are two small multiples
   * sharing one x-scale instead: one measure per plot, one y-scale each.
   *
   * Each plot carries a single series, so the title names it and no legend box
   * is needed. The published reference is a dashed hairline in muted ink rather
   * than a second colour, so it never reads as another series.
   */

  const PUBLISHED_TORQUE_NM = 2500;
  const PUBLISHED_POWER_KW = 375;

  const W = 560;
  const H = 150;
  const M = { l: 54, r: 18, t: 22, b: 8 };
  const AXIS_H = 24;

  const points = $derived(sim.sweepPoints);
  const peaks = $derived(sim.sweepPeaks);
  const hasCurve = $derived(points.length > 1);

  let hoverIndex = $state<number | null>(null);
  let showTable = $state(false);

  const xMin = $derived(hasCurve ? points[0]!.rpm : 0);
  const xMax = $derived(hasCurve ? points[points.length - 1]!.rpm : 1);

  /**
   * Round the axis top up to a readable step (1, 2, 2.5 or 5 x 10^n) so the
   * ticks land on numbers a reader can actually use.
   */
  function niceScale(rawMax: number, count = 5): { top: number; ticks: number[] } {
    if (!Number.isFinite(rawMax) || rawMax <= 0) return { top: 1, ticks: [0, 1] };
    const magnitude = 10 ** Math.floor(Math.log10(rawMax / count));
    const normalised = rawMax / count / magnitude;
    const step =
      (normalised <= 1
        ? 1
        : normalised <= 2
          ? 2
          : normalised <= 2.5
            ? 2.5
            : normalised <= 5
              ? 5
              : 10) * magnitude;
    const top = Math.ceil(rawMax / step) * step;
    const ticks: number[] = [];
    for (let v = 0; v <= top + step * 1e-9; v += step) ticks.push(v);
    return { top, ticks };
  }

  const torqueScale = $derived(
    niceScale(Math.max(PUBLISHED_TORQUE_NM, ...points.map((p) => p.brakeTorqueNm)) * 1.1),
  );
  const powerScale = $derived(
    niceScale(Math.max(PUBLISHED_POWER_KW, ...points.map((p) => p.brakePowerW / 1000)) * 1.1),
  );
  const torqueMax = $derived(torqueScale.top);
  const powerMax = $derived(powerScale.top);

  /** Keep a centred label inside the plot area. */
  function labelX(rpm: number): number {
    return Math.min(Math.max(sx(rpm), M.l + 46), W - M.r - 46);
  }

  function sx(rpm: number): number {
    const span = xMax - xMin || 1;
    return M.l + ((rpm - xMin) / span) * (W - M.l - M.r);
  }
  function sy(value: number, max: number): number {
    const plot = H - M.t - M.b;
    return M.t + plot - (value / (max || 1)) * plot;
  }

  function path(values: (p: OperatingPoint) => number, max: number): string {
    return points
      .map((p, i) => `${i === 0 ? 'M' : 'L'}${sx(p.rpm).toFixed(2)},${sy(values(p), max).toFixed(2)}`)
      .join(' ');
  }

  const xTicks = $derived(
    hasCurve
      ? points.filter((_, i) => i % Math.max(1, Math.round(points.length / 6)) === 0).map((p) => p.rpm)
      : [],
  );

  const hovered = $derived(hoverIndex === null ? null : (points[hoverIndex] ?? null));

  function onMove(event: MouseEvent) {
    if (!hasCurve) return;
    const target = event.currentTarget as SVGSVGElement;
    const box = target.getBoundingClientRect();
    const x = ((event.clientX - box.left) / box.width) * W;
    // Nearest point, not nearest pixel: the reader is asking about a measurement.
    let best = 0;
    let bestDistance = Infinity;
    points.forEach((p, i) => {
      const distance = Math.abs(sx(p.rpm) - x);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = i;
      }
    });
    hoverIndex = best;
  }

  const torqueError = $derived(
    peaks ? ((peaks.peakTorqueNm - PUBLISHED_TORQUE_NM) / PUBLISHED_TORQUE_NM) * 100 : 0,
  );
  const powerError = $derived(
    peaks ? ((peaks.peakPowerW / 1000 - PUBLISHED_POWER_KW) / PUBLISHED_POWER_KW) * 100 : 0,
  );
</script>

<section class="panel viz-root" aria-labelledby="dyno-heading">
  <h2 id="dyno-heading">Dynamometer</h2>

  <div class="actions">
    <button data-testid="run-sweep" onclick={() => sim.runSweep()} disabled={sim.sweepRunning}>
      {sim.sweepRunning ? 'Measuring…' : 'Run full-load sweep'}
    </button>
    {#if sim.sweepRunning && sim.sweepTotal > 0}
      <span class="muted small" data-testid="sweep-progress">
        {sim.sweepDone} / {sim.sweepTotal} points
      </span>
    {/if}
    {#if hasCurve}
      <button class="ghost" onclick={() => (showTable = !showTable)}>
        {showTable ? 'Show chart' : 'Show table'}
      </button>
    {/if}
  </div>

  {#if !hasCurve}
    <p class="muted small">
      Measure a full-load curve. Each point holds the crankshaft at a fixed speed and averages
      torque over whole four-stroke cycles, the way an engine dynamometer does.
    </p>
  {:else if showTable}
    <div class="table-wrap">
      <table data-testid="sweep-table">
        <caption class="muted small">Measured full-load operating points</caption>
        <thead>
          <tr>
            <th scope="col">rpm</th>
            <th scope="col">Torque N·m</th>
            <th scope="col">Power kW</th>
            <th scope="col">Peak p MPa</th>
            <th scope="col">Fuel mg</th>
            <th scope="col">g/kW·h</th>
          </tr>
        </thead>
        <tbody>
          {#each points as p (p.rpm)}
            <tr>
              <td>{p.rpm.toFixed(0)}</td>
              <td>{p.brakeTorqueNm.toFixed(0)}</td>
              <td>{(p.brakePowerW / 1000).toFixed(1)}</td>
              <td>{(p.peakPressurePa / 1e6).toFixed(2)}</td>
              <td>{p.fuelMgPerCycle.toFixed(0)}</td>
              <td>{p.bsfcGPerKwh.toFixed(0)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <figure data-testid="dyno-chart">
      <!-- Brake torque -->
      <figcaption class="chart-title">Brake torque <span class="unit">N·m</span></figcaption>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label="Brake torque against engine speed"
        onmousemove={onMove}
        onmouseleave={() => (hoverIndex = null)}
      >
        {#each torqueScale.ticks as t (t)}
          <line class="grid" x1={M.l} x2={W - M.r} y1={sy(t, torqueMax)} y2={sy(t, torqueMax)} />
          <text class="tick" x={M.l - 8} y={sy(t, torqueMax) + 4} text-anchor="end">
            {t.toFixed(0)}
          </text>
        {/each}

        <line
          class="reference"
          x1={M.l}
          x2={W - M.r}
          y1={sy(PUBLISHED_TORQUE_NM, torqueMax)}
          y2={sy(PUBLISHED_TORQUE_NM, torqueMax)}
        />
        <text
          class="reference-label"
          x={M.l + 4}
          y={sy(PUBLISHED_TORQUE_NM, torqueMax) - 6}
          text-anchor="start">PUBLISHED 2500</text
        >

        <path class="series torque" d={path((p) => p.brakeTorqueNm, torqueMax)} />

        {#if peaks}
          <circle
            class="peak torque"
            cx={sx(peaks.peakTorqueRpm)}
            cy={sy(peaks.peakTorqueNm, torqueMax)}
            r="5"
          />
          <text
            class="peak-label"
            x={labelX(peaks.peakTorqueRpm)}
            y={sy(peaks.peakTorqueNm, torqueMax) - 12}
            text-anchor="middle"
            data-testid="peak-torque-label"
          >
            {peaks.peakTorqueNm.toFixed(0)} N·m · CALIBRATED
          </text>
        {/if}

        {#if hovered}
          <line class="crosshair" x1={sx(hovered.rpm)} x2={sx(hovered.rpm)} y1={M.t} y2={H - M.b} />
          <circle
            class="marker torque"
            cx={sx(hovered.rpm)}
            cy={sy(hovered.brakeTorqueNm, torqueMax)}
            r="4.5"
          />
        {/if}
      </svg>

      <!-- Brake power -->
      <figcaption class="chart-title">Brake power <span class="unit">kW</span></figcaption>
      <svg
        viewBox={`0 0 ${W} ${H + AXIS_H}`}
        role="img"
        aria-label="Brake power against engine speed"
        onmousemove={onMove}
        onmouseleave={() => (hoverIndex = null)}
      >
        {#each powerScale.ticks as t (t)}
          <line class="grid" x1={M.l} x2={W - M.r} y1={sy(t, powerMax)} y2={sy(t, powerMax)} />
          <text class="tick" x={M.l - 8} y={sy(t, powerMax) + 4} text-anchor="end">
            {t.toFixed(0)}
          </text>
        {/each}

        <line
          class="reference"
          x1={M.l}
          x2={W - M.r}
          y1={sy(PUBLISHED_POWER_KW, powerMax)}
          y2={sy(PUBLISHED_POWER_KW, powerMax)}
        />
        <text
          class="reference-label"
          x={M.l + 4}
          y={sy(PUBLISHED_POWER_KW, powerMax) - 6}
          text-anchor="start">PUBLISHED 375</text
        >

        <path class="series power" d={path((p) => p.brakePowerW / 1000, powerMax)} />

        {#if peaks}
          <circle
            class="peak power"
            cx={sx(peaks.peakPowerRpm)}
            cy={sy(peaks.peakPowerW / 1000, powerMax)}
            r="5"
          />
          <text
            class="peak-label"
            x={labelX(peaks.peakPowerRpm)}
            y={sy(peaks.peakPowerW / 1000, powerMax) - 12}
            text-anchor="middle"
            data-testid="peak-power-label"
          >
            {(peaks.peakPowerW / 1000).toFixed(1)} kW · CALIBRATED
          </text>
        {/if}

        {#if hovered}
          <line class="crosshair" x1={sx(hovered.rpm)} x2={sx(hovered.rpm)} y1={M.t} y2={H - M.b} />
          <circle
            class="marker power"
            cx={sx(hovered.rpm)}
            cy={sy(hovered.brakePowerW / 1000, powerMax)}
            r="4.5"
          />
        {/if}

        <line class="axis" x1={M.l} x2={W - M.r} y1={H - M.b} y2={H - M.b} />
        {#each xTicks as t (t)}
          <text class="tick" x={sx(t)} y={H - M.b + 16} text-anchor="middle">{t.toFixed(0)}</text>
        {/each}
        <text class="axis-title" x={(M.l + W - M.r) / 2} y={H + AXIS_H - 2} text-anchor="middle">
          engine speed (rpm)
        </text>
      </svg>
    </figure>

    {#if hovered}
      <div class="readout" data-testid="dyno-readout">
        <b>{hovered.rpm.toFixed(0)} rpm</b>
        <span>{hovered.brakeTorqueNm.toFixed(0)} N·m</span>
        <span>{(hovered.brakePowerW / 1000).toFixed(1)} kW</span>
        <span>{(hovered.peakPressurePa / 1e6).toFixed(2)} MPa peak</span>
        <span>{hovered.bsfcGPerKwh.toFixed(0)} g/kW·h</span>
        <span>AFR {hovered.airFuelRatio.toFixed(1)}</span>
      </div>
    {/if}

    {#if peaks}
      <dl class="peaks" data-testid="dyno-peaks">
        <div>
          <dt>Peak torque</dt>
          <dd>
            {peaks.peakTorqueNm.toFixed(0)} N·m at {peaks.peakTorqueRpm.toFixed(0)} rpm
            <span class="delta">({torqueError >= 0 ? '+' : ''}{torqueError.toFixed(1)}%)</span>
          </dd>
        </div>
        <div>
          <dt>Peak power</dt>
          <dd>
            {(peaks.peakPowerW / 1000).toFixed(1)} kW at {peaks.peakPowerRpm.toFixed(0)} rpm
            <span class="delta">({powerError >= 0 ? '+' : ''}{powerError.toFixed(1)}%)</span>
          </dd>
        </div>
        <div>
          <dt>Max cylinder pressure</dt>
          <dd>{(peaks.maxPeakPressurePa / 1e6).toFixed(2)} MPa of the 23.0 MPa envelope</dd>
        </div>
        <div>
          <dt>Best fuel consumption</dt>
          <dd>{peaks.bestBsfcGPerKwh.toFixed(0)} g/kW·h</dd>
        </div>
      </dl>
    {/if}

    <p class="muted small" data-testid="calibration-note">
      The 375 kW and 2500 N·m <b>magnitudes</b> are published. The engine <b>speeds</b> at which
      this model reaches them are an outcome of our calibration and are marked CALIBRATED — the
      source manual does not publish them, and they are not OEM data.
    </p>
  {/if}
</section>

<style>
  /*
   * Series hues validated against this panel surface (#161b22) with the
   * dataviz palette validator: CVD ΔE 26.8, normal-vision ΔE 31.8, both above
   * 3:1 contrast. Text never wears a series colour.
   */
  .viz-root {
    --series-torque: #3987e5;
    --series-power: #d95926;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-bottom: 0.8rem;
    flex-wrap: wrap;
  }
  .ghost {
    margin-left: auto;
    font-size: 0.75rem;
  }

  figure {
    margin: 0;
  }
  .chart-title {
    font-size: 0.78rem;
    color: var(--muted);
    margin: 0.5rem 0 0.1rem;
  }
  .chart-title .unit {
    opacity: 0.7;
  }
  svg {
    display: block;
    width: 100%;
    height: auto;
    overflow: visible;
  }

  .grid {
    stroke: var(--rule);
    stroke-width: 1;
  }
  .axis {
    stroke: #383835;
    stroke-width: 1;
  }
  .tick,
  .axis-title {
    fill: var(--muted);
    font-size: 10px;
  }
  .axis-title {
    font-size: 10px;
    letter-spacing: 0.04em;
  }

  .reference {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 4 4;
  }
  .reference-label {
    fill: var(--muted);
    font-size: 9px;
    letter-spacing: 0.09em;
  }

  .series {
    fill: none;
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .series.torque {
    stroke: var(--series-torque);
  }
  .series.power {
    stroke: var(--series-power);
  }

  /* A 2px surface ring keeps the marker legible where it crosses the line. */
  .peak,
  .marker {
    stroke: var(--panel);
    stroke-width: 2;
  }
  .peak.torque,
  .marker.torque {
    fill: var(--series-torque);
  }
  .peak.power,
  .marker.power {
    fill: var(--series-power);
  }
  .peak-label {
    fill: var(--ink);
    font-size: 10px;
    letter-spacing: 0.03em;
  }

  .crosshair {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 2 3;
  }

  .readout {
    display: flex;
    gap: 0.9rem;
    flex-wrap: wrap;
    margin-top: 0.6rem;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--rule);
    border-radius: 6px;
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .readout b {
    color: var(--ink);
  }

  .peaks {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 0.15rem 1.25rem;
    margin: 0.9rem 0 0.6rem;
    font-size: 0.82rem;
  }
  .peaks > div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding: 0.15rem 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .delta {
    color: var(--muted);
  }

  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }
  caption {
    text-align: left;
    padding-bottom: 0.4rem;
  }
  th,
  td {
    text-align: right;
    padding: 0.2rem 0.5rem;
    border-bottom: 1px solid var(--rule);
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
</style>
