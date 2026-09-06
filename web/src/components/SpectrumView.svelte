<script lang="ts">
  /**
   * Output spectrum of the exhaust audio.
   *
   * This exists as a verification tool, not decoration. The signal can measure
   * perfectly well — finite, bounded, right fundamental — and still be inaudible
   * because all its energy sits below what a speaker reproduces. That failure is
   * invisible to every assertion in the test suite and obvious here.
   *
   * Read off the output path, so it shows what is actually being played.
   *
   * Log frequency axis, because pitch is logarithmic: a linear axis would spend
   * most of its width on the 10-20 kHz region where an engine has nothing, and
   * squeeze the entire firing series into the first few pixels.
   */
  import { sim } from '../lib/state.svelte';

  const MIN_HZ = 20;
  const MAX_HZ = 20_000;
  const BUCKETS = 64;

  const WIDTH = 320;
  const HEIGHT = 120;
  const PAD_LEFT = 26;
  const PAD_BOTTOM = 16;
  const PAD_TOP = 6;

  /** Bucket magnitudes, 0..1. */
  let levels = $state<number[]>(new Array(BUCKETS).fill(0));
  let frame = 0;

  const logMin = Math.log10(MIN_HZ);
  const logMax = Math.log10(MAX_HZ);

  /** Position of a frequency along the axis, 0..1. */
  function axisPosition(hz: number): number {
    return (Math.log10(Math.max(hz, MIN_HZ)) - logMin) / (logMax - logMin);
  }

  function xOf(hz: number): number {
    return PAD_LEFT + axisPosition(hz) * (WIDTH - PAD_LEFT - 4);
  }

  /** Firing frequency: one exhaust event per cylinder per two revolutions. */
  const firingHz = $derived.by(() => {
    const rpm = sim.snapshot?.rpm ?? 0;
    const cylinders = sim.activeConfig?.cylinders ?? 0;
    if (rpm <= 0 || cylinders <= 0) return 0;
    return (rpm / 120) * cylinders;
  });

  const plotHeight = HEIGHT - PAD_BOTTOM - PAD_TOP;
  const barWidth = $derived((WIDTH - PAD_LEFT - 4) / BUCKETS);

  function sample() {
    const reading = sim.readAudioSpectrum();
    if (reading) {
      const { magnitudes, binHz } = reading;
      const sums = new Array(BUCKETS).fill(0);
      const counts = new Array(BUCKETS).fill(0);

      for (let bin = 1; bin < magnitudes.length; bin += 1) {
        const hz = bin * binHz;
        if (hz < MIN_HZ || hz > MAX_HZ) continue;
        const bucket = Math.min(BUCKETS - 1, Math.floor(axisPosition(hz) * BUCKETS));
        sums[bucket] += magnitudes[bin]!;
        counts[bucket] += 1;
      }

      levels = sums.map((sum, i) => (counts[i] > 0 ? sum / counts[i]! / 255 : 0));
    }
    frame = requestAnimationFrame(sample);
  }

  $effect(() => {
    if (sim.audioRunning) {
      frame = requestAnimationFrame(sample);
      return () => cancelAnimationFrame(frame);
    }
    levels = new Array(BUCKETS).fill(0);
    return undefined;
  });

  const ticks = [50, 100, 500, 1000, 5000, 10_000];
  function tickLabel(hz: number): string {
    return hz >= 1000 ? `${hz / 1000}k` : `${hz}`;
  }
</script>

<figure class="spectrum">
  <figcaption>Output spectrum</figcaption>
  <svg
    data-testid="spectrum"
    viewBox="0 0 {WIDTH} {HEIGHT}"
    role="img"
    aria-label="Exhaust audio output spectrum against frequency"
  >
    <!-- Recessive frequency grid. -->
    {#each ticks as hz (hz)}
      <line
        x1={xOf(hz)}
        y1={PAD_TOP}
        x2={xOf(hz)}
        y2={HEIGHT - PAD_BOTTOM}
        class="grid"
      />
      <text x={xOf(hz)} y={HEIGHT - 4} class="tick" text-anchor="middle">{tickLabel(hz)}</text>
    {/each}
    <text x={4} y={HEIGHT - 4} class="tick">Hz</text>

    {#each levels as level, i (i)}
      <rect
        x={PAD_LEFT + i * barWidth}
        y={PAD_TOP + plotHeight * (1 - level)}
        width={Math.max(1, barWidth - 1)}
        height={Math.max(0, plotHeight * level)}
        rx="1"
        class="bar"
      />
    {/each}

    <!-- Where the firing fundamental should be, so the two can be compared. -->
    {#if firingHz > 0}
      <line
        x1={xOf(firingHz)}
        y1={PAD_TOP}
        x2={xOf(firingHz)}
        y2={HEIGHT - PAD_BOTTOM}
        class="firing"
        data-testid="spectrum-firing-marker"
      />
      <!--
        Anchored to whichever side has room and given a dark halo, because the
        bars immediately around the fundamental are the tallest on the chart and
        a plain label lands right on top of them.
      -->
      <text
        x={xOf(firingHz) + (axisPosition(firingHz) > 0.6 ? -4 : 4)}
        y={HEIGHT - PAD_BOTTOM - 4}
        class="firing-label"
        text-anchor={axisPosition(firingHz) > 0.6 ? 'end' : 'start'}
      >
        firing {firingHz.toFixed(0)} Hz
      </text>
    {/if}
  </svg>
  <p class="muted small">
    The marker is the firing frequency computed from engine speed and cylinder count. The strongest
    low peak should sit on it. Energy far below the marker is real but inaudible on small speakers.
  </p>
</figure>

<style>
  .spectrum {
    margin: 0 0 0.9rem;
  }
  figcaption {
    font-size: 0.78rem;
    color: var(--text-muted, #98a2b3);
    margin-bottom: 0.3rem;
  }
  svg {
    width: 100%;
    height: auto;
    display: block;
    background: var(--surface-sunken, #1b1f27);
    border-radius: 0.25rem;
  }
  .bar {
    fill: var(--series-torque, #3987e5);
  }
  .grid {
    stroke: var(--grid, #2b313c);
    stroke-width: 1;
  }
  .tick {
    fill: var(--text-muted, #98a2b3);
    font-size: 7px;
  }
  .firing {
    stroke: var(--series-power, #d95926);
    stroke-width: 1.5;
    stroke-dasharray: 3 2;
  }
  .firing-label {
    fill: var(--series-power, #d95926);
    font-size: 7px;
    /* Halo so the label stays readable over the bars behind it. */
    stroke: var(--surface-sunken, #1b1f27);
    stroke-width: 2.5px;
    paint-order: stroke fill;
  }
</style>
