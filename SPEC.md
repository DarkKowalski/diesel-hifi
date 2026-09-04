# Diesel Truck Engine Simulator Specification

Status: implementation baseline  
Version: 0.1

## 1. Goal

Build a browser-based, real-time diesel truck engine simulator inspired by the interaction and sound-oriented goals of `ange-yaghi/engine-sim`, without retaining gasoline-engine behavior. The deterministic simulation core is written in Rust, compiled both natively and to WebAssembly, and presented through a deployable static Svelte application.

The first built-in engine is a public-data reference model of the 2011 Mercedes-Benz OM 471.9 M3D. It is not an OEM-certified digital twin.

## 2. Scope

### MVP

- One diesel engine configuration with a stable selection API.
- Crank-angle-resolved four-stroke state for six cylinders.
- Diesel fuel scheduling, ignition delay, simplified heat release, cylinder pressure, crankshaft dynamics, friction, load, and idle control.
- Native deterministic tests and a WASM browser integration.
- Static Svelte UI with engine selection, start/stop, pedal/load controls, and live telemetry.
- Simulation off the UI thread.
- Basic user-gesture-gated Web Audio only after the simulation slice is stable.

### Non-goals

- Gasoline engines or a generic multi-fuel abstraction.
- Engineering certification, emissions compliance prediction, ECU reproduction, or OEM map reverse engineering.
- A backend, SSR, accounts, databases, telemetry collection, or runtime network calls.
- Full CFD, finite-element mechanics, detailed injector hydraulics, chemical kinetics, or complete aftertreatment in the MVP.

## 3. Architecture

| Area | Responsibility |
|---|---|
| `crates/sim-core` | Engine configuration, validation, deterministic physics, control, snapshots, and native tests |
| `crates/sim-wasm` | Serialization and coarse `wasm-bindgen` boundary only |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, and message protocol |
| `web/src` | Svelte UI, user input, telemetry rendering, and Web Audio orchestration |
| `web/public` | Locally bundled static assets and built-in configuration data when not compiled into WASM |

`sim-core` must remain free of browser, DOM, audio-device, and filesystem dependencies. The production artifact is `web/dist` and must run from root or a configurable subpath on ordinary static hosting.

## 4. Reference Engine

### Identity and source

- Stable ID: `mercedes-benz-om471-9-m3d-375kw`
- Display name: `OM 471.9 M3D 375 kW Reference`
- Primary source: *Introduction of engine OM 471 and exhaust aftertreatment*
- Technical status: 2011-09-01
- Publication/order number: 6517 1260 02
- Source scope: engine series 471.9 in model 963/964; M3D power code and documented M5Z Euro VI subsystems where stated

### Published parameters

| Parameter | Published value | Internal value |
|---|---:|---:|
| Layout | Inline six | 6 cylinders |
| Displacement | 12.8 L | `0.0128 m^3` reference |
| Bore | 132 mm | `0.132 m` |
| Stroke | 156 mm | `0.156 m` |
| Compression ratio | 17.3:1 | `17.3` |
| Connecting-rod length | 268 mm | `0.268 m` |
| Valve train | DOHC, 2 intake + 2 exhaust per cylinder | 4 valves/cylinder |
| Idle speed | 560 rpm | convert at API boundary as needed |
| Complete-engine mass | approximately 1200 kg | metadata only |
| M3D maximum output | 375 kW / 510 hp | `375000 W` |
| M3D maximum torque | 2500 N m | `2500 N m` |
| Maximum rail pressure | 900 bar | `90 MPa` |
| Amplified injector pressure | up to 2100 bar | `210 MPa` limit |
| Maximum combustion pressure | up to 230 bar | `23 MPa` envelope |

The complete-engine mass must not be used as flywheel or rotating inertia.

### Published subsystem behavior

- APCRS injectors support injection with or without local pressure amplification. Quantity, timing, and mode depend on operating state.
- A single exhaust turbocharger feeds a charge-air cooler. The MCM controls boost through a wastegate and monitors relevant pressure, temperature, and, on M5Z, turbo-speed signals.
- Cooled, regulated EGR is controlled across the operating range.
- The decompression engine brake has three operating stages. Stage I operates cylinders 1-3; stages II and III operate all cylinders, with stage III also increasing cylinder pressure through air-path control.
- M5U brake anchors: 100 kW at 1300 rpm and 300 kW at 2300 rpm.
- M5V brake anchors: 150 kW at 1300 rpm and 400 kW at 2300 rpm.

### Data gaps

The manual does not publish the RPM positions of the M3D maximum power and torque, firing order, complete torque or fuel maps, valve events, turbo maps, injector rate shapes, heat-release law, friction map, or rotating inertias. These values require another generation-specific primary source or an explicit `calibrated` classification.

Do not mix the later 390 kW, 2600 N m, or 2700 bar OM 471 figures into this configuration.

## 5. Configuration Model

`EngineConfig` must be versioned and validated before simulation state is created. It must contain:

- identity and display metadata;
- cylinder and crank geometry;
- valve, injection, combustion, friction, air-path, governor, load, and audio calibration sections;
- physical limits and safe numerical ranges;
- one or more source records; and
- parameter-level provenance with `published`, `derived`, or `calibrated` status.

Published values must name a source record and locator such as a manual page or section. Derived values must state their formula and inputs. Calibrated values must state their purpose and safe range.

The configuration catalog must expose list, active-ID, and select-by-ID operations. Selecting any configuration, including the active one, performs a documented deterministic reset. Solver code must not branch on the OM 471 ID.

## 6. Simulation Model

- Use SI units and radians internally. One four-stroke cycle spans `4 * PI` radians.
- Represent cylinder phase offsets independently of engine-specific solver logic.
- Pedal input requests fuel or torque; it is not a gasoline throttle plate.
- Model injection scheduling, ignition delay, premixed and diffusion heat-release phases, heat transfer, gas exchange, and cylinder pressure at an intentionally simplified level.
- Derive crank torque from cylinder pressure and slider-crank geometry. Do not write a target torque directly into crank acceleration.
- Include friction, pumping, accessory, driveline/load, starter, and idle-governor torque terms explicitly.
- Treat 23 MPa as a validation/safety envelope, not a normal pressure target.
- Use fixed-step or bounded deterministic integration. Reject non-finite state and invalid configuration ranges.
- Avoid heap allocation, blocking, logging, DOM access, and I/O in the hot step loop.

Turbocharging, EGR, engine braking, and production-quality audio may be delivered after the first vertical slice, but their interfaces must not require redesigning `EngineConfig` or the snapshot protocol.

## 7. WASM and Worker API

Expose a versioned, coarse-grained API capable of:

- listing configuration summaries;
- returning the active configuration ID;
- selecting a configuration by stable ID;
- resetting with an explicit seed and initial conditions;
- submitting controls;
- advancing multiple fixed steps; and
- returning a compact telemetry snapshot.

Return structured errors for invalid configuration IDs, inputs, or numerical state. Define memory ownership explicitly and avoid per-step JS/WASM calls. Run the module in a dedicated Web Worker without relying on `SharedArrayBuffer` or cross-origin-isolation headers.

## 8. Static UI

Use pnpm, TypeScript, Svelte, and Vite; do not use SvelteKit. The UI must include:

- an engine selector populated through the real configuration API;
- start/stop and reset controls;
- pedal and load controls;
- RPM plus a compact set of pressure, torque, and state telemetry;
- visible error and worker lifecycle states; and
- an explicit user action before starting Web Audio.

All runtime code, WASM, configurations, fonts, and assets must be local to the build. Configure Vite so the output works at a domain root and a supplied subpath.

## 9. Milestones

### Milestone 1 - Vertical slice

Create the Cargo workspace, validated OM 471 configuration, minimal deterministic crank/cylinder state, WASM adapter, worker protocol, and Svelte controls/telemetry. The single-option selector must use the actual catalog API. Audio, detailed turbo/EGR, aftertreatment, and engine-brake physics are excluded.

### Milestone 2 - Diesel combustion and calibration

Implement injection/ignition/heat-release and pressure-derived torque, then calibrate the published maximum power and torque magnitudes without claiming unpublished RPM locations as OEM facts.

### Milestone 3 - Air path and sound

Add wastegate turbo and EGR dynamics, exhaust pulse output, and user-gesture-gated Web Audio.

### Milestone 4 - Truck load and engine brake

Add driveline/load behavior and the documented staged decompression brake, tested against the M5U or M5V published anchors selected in configuration.

## 10. Acceptance Criteria

- Native Rust tests are deterministic for identical configuration, seed, inputs, and step count.
- Geometry tests recompute displacement from six cylinders and 132 x 156 mm geometry and match 12.8 L within 0.05 L.
- Configuration tests preserve all published values and provenance classifications.
- Nominal tests stay below the 23 MPa combustion-pressure envelope and reject invalid/non-finite state.
- Catalog tests cover list, active ID, unknown ID, select, and deterministic reset.
- WASM loads and advances in a browser smoke test through the worker.
- The UI remains responsive while the simulation runs and obtains its selector entries from the real API.
- `web/dist` contains a complete static deployment with no runtime external requests.
- Root and configurable-subpath previews both load the UI, worker, WASM, and assets.
- Any calibration target not directly published is visible in source metadata and documentation.

## 11. Required Repository Commands

The implementation must provide root-level pnpm scripts so these commands are valid:

```bash
pnpm install --frozen-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm wasm:build
pnpm check
pnpm test
pnpm build
```

Do not report a command as passed unless it was run and exited successfully.

## 12. Legal and Fidelity Boundaries

- Preserve notices for any MIT-licensed code adapted from `ange-yaghi/engine-sim`; document adaptations.
- Do not copy code or assets from closed-source successors or unlicensed projects.
- Use Mercedes-Benz and OM 471 names only as factual references. Do not use OEM logos or imply endorsement.
- Display the model as a public-spec reference simulation, not an OEM calibration or engineering tool.
