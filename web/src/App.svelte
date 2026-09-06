<script lang="ts">
  import AudioPanel from './components/AudioPanel.svelte';
  import Controls from './components/Controls.svelte';
  import DrivelinePanel from './components/DrivelinePanel.svelte';
  import DynoPanel from './components/DynoPanel.svelte';
  import EngineSelector from './components/EngineSelector.svelte';
  import ProvenancePanel from './components/ProvenancePanel.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Telemetry from './components/Telemetry.svelte';
  import { sim } from './lib/state.svelte';

  $effect(() => {
    void sim.start();
    return () => sim.destroy();
  });

  // Disengage the starter as soon as the engine sustains itself.
  $effect(() => {
    if (sim.snapshot?.state === 'running') {
      void sim.releaseStarterIfRunning();
    }
  });
</script>

<main>
  <header>
    <h1>Diesel Truck Engine Simulator</h1>
    <p class="muted">
      Deterministic Rust simulation core compiled to WebAssembly, stepped in a Web Worker, rendered
      by a static Svelte build. Public-spec reference model, not an OEM calibration.
    </p>
  </header>

  <StatusBar />

  <div class="layout">
    <div class="column">
      <EngineSelector />
      <Controls />
      <DrivelinePanel />
      <AudioPanel />
    </div>
    <div class="column">
      <Telemetry />
      <DynoPanel />
    </div>
  </div>

  <ProvenancePanel />

  <footer class="muted small">
    Mercedes-Benz and OM 471 are used as factual references only. No OEM endorsement is implied.
    Milestone 4 adds the staged decompression engine brake and a rigid truck driveline. Braking
    torque comes out of cylinder pressure, exactly as firing torque does, and is validated against
    the published M5U anchors of 100 kW at 1300 rpm and 300 kW at 2300 rpm. Aftertreatment
    chemistry, clutch slip and gear-change behaviour are not modelled.
  </footer>
</main>

<style>
  main {
    max-width: 1180px;
    margin: 0 auto;
    padding: 1.5rem 1.25rem 3rem;
    display: grid;
    gap: 1rem;
  }
  header h1 {
    margin: 0 0 0.35rem;
    font-size: 1.35rem;
  }
  header p {
    margin: 0;
    max-width: 70ch;
    font-size: 0.85rem;
    line-height: 1.5;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.35fr);
    gap: 1rem;
    align-items: start;
  }
  .column {
    display: grid;
    gap: 1rem;
    align-content: start;
  }
  footer {
    border-top: 1px solid var(--rule);
    padding-top: 0.9rem;
    line-height: 1.5;
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
