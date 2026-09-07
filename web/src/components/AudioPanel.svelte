<script lang="ts">
  import type { AudioPath } from '../lib/audioEngine';
  import { sim } from '../lib/state.svelte';
  import SpectrumView from './SpectrumView.svelte';

  /**
   * The radiating paths, in the order the solver emits them.
   *
   * Named for the mechanism rather than for the frequency range, because that is
   * what they are: the exhaust is port flow out of a pipe, the block is cylinder
   * pressure ringing iron, and the body is crank torque shaking the mounts.
   */
  const PATH_LABELS: Array<{ path: AudioPath; label: string }> = [
    { path: 'exhaust', label: 'Exhaust' },
    { path: 'block', label: 'Block' },
    { path: 'body', label: 'Body' },
  ];

  const snapshot = $derived(sim.snapshot);

  // The level meter is driven by the simulation's own RMS, reported against the
  // configured reference. It is a relative indicator, not a claim about how loud
  // the real engine is.
  const levelDb = $derived(snapshot?.audioLevelDb ?? 0);
  const meterPercent = $derived(Math.max(0, Math.min(100, ((levelDb - 40) / 60) * 100)));

  /**
   * Level measured after everything, including the stage and the volume control.
   *
   * The meter above it is the solver's own RMS at the source, which is unmoved
   * by any of that. Showing both is the point: it is how you can see that the
   * cockpit stage changes the character rather than simply turning the sound up.
   */
  let outputDb = $state<number | null>(null);

  $effect(() => {
    if (!sim.audioRunning) {
      outputDb = null;
      return undefined;
    }
    // Ten hertz. This is a readout, not a meter ballistics exercise, and it has
    // no business on an animation frame.
    const timer = setInterval(() => {
      outputDb = sim.readAudioOutputLevelDb();
    }, 100);
    return () => clearInterval(timer);
  });

  async function onVolume(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    sim.setAudioVolume(Number(input.value) / 100);
  }
</script>

<section class="panel" aria-labelledby="audio-heading">
  <h2 id="audio-heading">Sound</h2>

  {#if !sim.audioRunning}
    <p class="muted small">
      The exhaust note is not synthesised. The solver's 25&nbsp;µs step is a 40&nbsp;kHz sample rate,
      so it emits one sample per step taken from the computed blowdown across the exhaust ports —
      the same cylinder pressure that drives the crank.
    </p>
    <button
      data-testid="enable-audio"
      onclick={() => sim.enableAudio()}
      disabled={sim.audioStarting || sim.lifecycle !== 'ready'}
    >
      {sim.audioStarting ? 'Starting…' : 'Enable sound'}
    </button>
    <p class="muted small">Audio starts only from this explicit action.</p>
  {:else}
    <div class="meter" data-testid="audio-meter" aria-hidden="true">
      <div class="fill" style="width: {meterPercent}%"></div>
    </div>

    <SpectrumView />

    <div
      class="stage"
      role="group"
      aria-label="Sound stage"
      data-testid="audio-stage"
      data-stage={sim.audioStage}
    >
      <button
        type="button"
        data-testid="audio-stage-raw"
        aria-pressed={sim.audioStage === 'raw'}
        onclick={() => sim.setAudioStage('raw')}
      >
        Raw tailpipe
      </button>
      <button
        type="button"
        data-testid="audio-stage-cockpit"
        aria-pressed={sim.audioStage === 'cockpit'}
        onclick={() => sim.setAudioStage('cockpit')}
      >
        Truck cockpit
      </button>
    </div>

    <p class="muted small">
      {#if sim.audioStage === 'raw'}
        The tailpipe signal as the solver produces it, unfiltered — the three radiating paths simply
        added back up. This is the path the spectrum view was built to verify.
      {:else}
        The same samples heard from the driver's seat, each path by its own route: the exhaust from a
        stack metres behind and below, the block two feet away through the bulkhead, the body through
        the mounts and the seat. <b>Nothing is added</b> — no road noise, no synthesised rumble, no
        harmonic generation. It is a filter, and every value in it is a listening choice, not a
        measurement. The manual publishes nothing about how this cab sounds.
      {/if}
    </p>

    <div class="stage" role="group" aria-label="Radiating paths" data-testid="audio-paths">
      {#each PATH_LABELS as { path, label } (path)}
        <button
          type="button"
          data-testid={`audio-path-${path}`}
          aria-pressed={sim.audioPaths[path]}
          onclick={() => sim.setAudioPath(path, !sim.audioPaths[path])}
        >
          {label}
        </button>
      {/each}
    </div>

    <p class="muted small">
      The three sources, separately. Muting one is a listening control: the engine goes on producing
      all three and nothing about the simulation changes. The balance between them is what decides
      whether this sounds like a truck, and it is calibrated by measurement — the
      <code>audio_probe</code> example — rather than by ear.
    </p>

    <label class="field">
      <span>Volume <b>{(sim.audioVolume * 100).toFixed(0)}%</b></span>
      <input
        data-testid="audio-volume"
        type="range"
        min="0"
        max="100"
        step="1"
        value={sim.audioVolume * 100}
        oninput={onVolume}
      />
    </label>

    <dl class="grid" data-testid="audio-status">
      <div>
        <dt>Source rate</dt>
        <dd data-testid="audio-source-rate">{(sim.audioSampleRateHz / 1000).toFixed(1)} kHz</dd>
      </div>
      <div>
        <dt>Device rate</dt>
        <dd data-testid="audio-device-rate">{(sim.audioDeviceRateHz / 1000).toFixed(1)} kHz</dd>
      </div>
      <div>
        <dt>Samples received</dt>
        <dd data-testid="audio-received">{sim.audioReceived}</dd>
      </div>
      <div>
        <dt>Buffered</dt>
        <dd data-testid="audio-buffered">{sim.audioBuffered}</dd>
      </div>
      <div>
        <dt>Underruns</dt>
        <dd data-testid="audio-underruns">{sim.audioUnderruns}</dd>
      </div>
      <div>
        <dt>Source level</dt>
        <dd data-testid="audio-level">{levelDb.toFixed(0)} dB</dd>
      </div>
      <div>
        <dt>Output level</dt>
        <dd data-testid="audio-output-level">
          {outputDb === null ? '—' : `${outputDb.toFixed(1)} dBFS`}
        </dd>
      </div>
    </dl>

    <button data-testid="disable-audio" onclick={() => sim.disableAudio()}>Stop sound</button>

    <p class="muted small">
      Underruns count render blocks that ran dry. Because the samples <em>are</em> the simulation,
      audio glitches when the simulation stalls — the worklet outputs silence rather than repeating
      the last block, so a stall is audible instead of disguised.
    </p>
  {/if}
</section>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 0.15rem 1.25rem;
    margin: 0 0 0.9rem;
  }
  .grid > div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding: 0.15rem 0;
  }
  .grid dt {
    color: var(--muted);
  }
  .grid dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .meter {
    height: 0.5rem;
    border-radius: 0.25rem;
    background: var(--surface-sunken, #1b1f27);
    overflow: hidden;
    margin-bottom: 0.9rem;
  }
  .fill {
    height: 100%;
    background: var(--series-torque, #3987e5);
    transition: width 80ms linear;
  }
  input[type='range'] {
    width: 100%;
  }
  button {
    width: 100%;
  }
  .stage {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.5rem;
  }
  .stage button {
    flex: 1;
  }
  .stage button[aria-pressed='true'] {
    border-color: var(--series-torque, #3987e5);
    color: var(--series-torque, #3987e5);
  }
</style>
