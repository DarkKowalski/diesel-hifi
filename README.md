# Diesel HiFi

An experimental diesel truck engine and sound simulator for the browser.
The current engine is a 12.8 L, six-cylinder Mercedes-Benz OM 471.9 M3D
reference model, rated at 375 kW and 2500 N·m in the published specifications.

[Open Diesel HiFi](https://darkkowalski.github.io/diesel-hifi/)

## Current features

- **Drive:** start/stop, accelerator, 12-speed gearbox, engine brake, external
  load and road grade, with a live speed gauge and animated engine cutaway.
- **Sound:** synthesized exhaust, block, body and starter audio, with cockpit/raw
  listening positions and volume control. Starting the engine enables sound.
- **Telemetry:** engine, cylinder, air-path, starter and driveline readings.
- Phone and desktop layouts, foldable mobile controls, a speed readout that
  stays visible while scrolling, and optional slow-motion animation.
- English and Simplified Chinese (`zh-CN`), keyboard controls and reduced-motion
  support. Production includes telemetry without developer debugging panels.

The deterministic Rust simulation runs as WebAssembly in a Web Worker. The
frontend uses Astro, Svelte, TypeScript and Tailwind CSS. The built application
is a self-contained static site, with no backend or external runtime assets.
Engine parameters and their published, derived or calibrated provenance are in
[the engine configuration](crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json).

## Run locally

Requires Node.js 22.12+, pnpm 11 and Rust installed through rustup.
The repository's `rust-toolchain.toml` selects the Rust components and WASM target.

```sh
cargo install wasm-pack --locked
pnpm install --frozen-lockfile
pnpm dev
```

Open [localhost:5173](http://localhost:5173/). The development command builds WASM
before starting the frontend.

## Build and test

```sh
pnpm --filter web exec playwright install chromium
pnpm test
pnpm check
cargo test --workspace
pnpm build
```

`pnpm test` builds WASM and runs WASM, unit and browser tests. `pnpm build`
creates and verifies `web/dist`; `pnpm preview` serves it locally.
`pnpm build:subpath` verifies a build for `/diesel-hifi/`. Set `VITE_BASE` to
build for another hosting path.

The latest application verification (2026-09-08) passed 18 WASM, 93 unit and
52 browser tests, frontend checks, and static builds at both root and subpath.
The site is deployed through the manual
[GitHub Pages workflow](.github/workflows/pages.yml).

## Known limitations

- This is a public-spec reference model, not an OEM calibration or certified
  digital twin. Engine, starter and cabin acoustics include estimates;
  Mercedes-Benz does not endorse the project.
- Thermodynamics and the driveline are simplified. There is no clutch, service
  brake, emissions or engine-damage model, or validated cold-start behavior.
  Overspeed pauses the simulation and offers to level the road and resume.
- Audio fidelity remains experimental, including idle/brake tonal balance and
  high-speed air-path oscillation. Browser coverage uses Chromium; real
  iOS/Android audio interruptions and sustained device performance remain unverified.
