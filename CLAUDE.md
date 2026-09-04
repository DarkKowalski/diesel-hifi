# Diesel Truck Engine Simulator

## Project Rules

- Build only diesel truck engines. Do not add gasoline support or a generic multi-fuel framework.
- Product requirements and acceptance criteria live in `SPEC.md`. Read it before planning a feature; report conflicts instead of silently changing the specification.
- The initial engine ID is `mercedes-benz-om471-9-m3d-375kw`. Keep engine discovery and selection data-driven even while only one configuration exists.

## Architecture

- `crates/sim-core` owns deterministic, platform-independent simulation. It must not depend on browser, DOM, audio-device, or filesystem APIs.
- `crates/sim-wasm` is a thin `wasm-bindgen` adapter. Keep physics and validation in `sim-core`.
- `web` uses pnpm, TypeScript, Svelte, and Vite. Production output is a self-contained static site in `web/dist`; no backend, SSR, CDN, or Node.js runtime is allowed.
- Run the simulation in a Web Worker. Do not require `SharedArrayBuffer` or cross-origin-isolation headers.
- Keep the WASM boundary coarse-grained: set inputs, advance a batch, and return a compact snapshot.

## Simulation Invariants

- Use SI units and radians internally; one four-stroke cycle is `4 * PI` radians.
- Construct simulation state from a validated, versioned `EngineConfig`; never embed engine-specific calibration in the solver.
- Label configuration values as `published`, `derived`, or `calibrated`, with a source for every published value. Never present an estimate as OEM data.
- Pedal input requests fuel or torque. Combustion pressure drives crankshaft torque; do not inject a target torque directly into crank dynamics.
- Keep stepping deterministic. Seed any randomness explicitly, reject non-finite state, and avoid allocation, blocking, logging, and I/O in the hot loop.

## Workflow

- Before editing, inspect relevant files, tests, and `git status --short`.
- Make the smallest coherent change and add focused tests for changed behavior.
- Use pnpm exclusively for JavaScript dependencies. Preserve unrelated changes and existing licenses.
- Run the relevant checks documented in `SPEC.md`; never claim a check passed unless it was run successfully.
- Do not commit, push, rewrite history, change licenses, or add major dependencies unless explicitly requested.
- Finish with changed behavior, files, exact verification results, and known limitations.
