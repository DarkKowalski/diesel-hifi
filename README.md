# Diesel Truck Engine Simulator

A browser-based, real-time **diesel truck** engine simulator. The deterministic
simulation core is written in Rust, compiled both natively and to WebAssembly,
and presented through a static Svelte application with no backend.

The first built-in engine is a public-data reference model of the 2011
Mercedes-Benz OM 471.9 M3D.

> **This is not an OEM-certified digital twin.** It is a public-spec reference
> simulation built from published service literature. Mercedes-Benz and OM 471
> are used as factual references only; no OEM endorsement is implied, and no OEM
> logos or assets are used. Every configuration value is labelled `published`,
> `derived`, or `calibrated`, and the UI shows those labels. An estimate is never
> presented as OEM data.

Requirements and acceptance criteria live in [`SPEC.md`](SPEC.md).

## Status: Milestone 2 — diesel combustion and calibration

Milestone 1 proved the whole path end to end: Rust core → WASM → Web Worker →
Svelte → static `web/dist`. Milestone 2 replaced its placeholder combustion with
a real one and calibrated the engine against its published output.

**In (Milestone 2):** injection scheduling driven by the published APCRS rail
pressures; Hardenberg-Hase ignition delay; double-Wiebe heat release whose
premixed fraction emerges from the fuel injected during the delay; Woschni wall
heat transfer with temperature-dependent specific heats; residual-gas tracking;
a prescribed, calibrated boost schedule; whole-cycle averaged torque, power,
BMEP and fuel consumption; and a steady-state dynamometer harness with a curve
view in the UI.

**Out (later milestones):** wastegate turbocharger and EGR dynamics,
orifice-based valve flow, aftertreatment, the staged decompression engine brake,
and Web Audio.

### Calibration result

| | Model | Published | Error |
|---|---:|---:|---:|
| Peak power | 374.6 kW at **1600 rpm** | 375 kW | −0.1% |
| Peak torque | 2521 N·m at **1100 rpm** | 2500 N·m | +0.8% |
| Max cylinder pressure | 21.89 MPa | 23 MPa envelope | 4.8% margin |
| Best fuel consumption | 177 g/kW·h | not published | — |

The **magnitudes** are published. The **engine speeds in bold are an outcome of
our calibration** — the manual does not publish where the real engine makes its
peaks, and these must never be quoted as OEM data. The UI labels them
`CALIBRATED` next to the published reference lines.

177 g/kW·h is optimistic against a real OM 471 (roughly 185–190 g/kW·h at best).
The model has no EGR pumping penalty and no aftertreatment back-pressure; both
arrive in Milestone 3 and both worsen real fuel consumption. It is reported here
rather than tuned away.

## Layout

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, the dynamometer harness, snapshots, native tests. No browser, DOM, audio-device, or filesystem dependency. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering. |
| `scripts/verify-dist.mjs` | Static-build verification. |

`crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json` is the engine
configuration (schema version 2). It is compiled into the WASM module with
`include_str!`, so the deployed site needs no runtime fetch. Regenerate it with
`python3 scripts/gen-om471-config.py`, which holds the authoritative provenance
table; `cargo run --release -p sim-core --example sweep` is the calibration probe
behind the figures above.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # native core: geometry, provenance, catalog,
                                # determinism, limits, combustion, heat transfer,
                                # dyno calibration, data-driven, ID-branch guard

pnpm wasm:build                 # wasm-pack -> web/src/wasm (generated, gitignored)
pnpm wasm:test                  # wasm-pack test --node: WASM API smoke test
pnpm check                      # svelte-check
pnpm test                       # wasm smoke + vitest units + Playwright browser suite
pnpm build                      # wasm + vite build + verify-dist
pnpm build:subpath              # the same build under VITE_BASE=/diesel-hifi/
```

The Playwright suite needs a browser once: `pnpm --filter web exec playwright
install chromium`.

`vite.config.ts` uses a relative base by default, so one build works at a domain
root *and* under any subpath. Set `VITE_BASE` for hosts that need an absolute
prefix.

## Reference engine and sources

Stable ID `mercedes-benz-om471-9-m3d-375kw`, display name
*OM 471.9 M3D 375 kW Reference*.

Primary source: **Introduction of engine OM 471 and exhaust aftertreatment**,
Mercedes-Benz service literature, technical status 2011-09-01, order number
6517 1260 02. Scope: engine series 471.9 in model 963/964, power code M3D, plus
documented M5Z Euro VI subsystems where the document states them.

Locators used by the published parameters:

| Locator | Printed / PDF page | Supplies |
|---|---|---|
| `SN00.00-W-0002-05H` — *Technical data of diesel engine OM 471* | p. 11 / PDF 14 | 12,8 l; 6 in line; DOHC 2/2; idle 560 rpm; CR 17,3; stroke 156 mm; stroke:bore 1,18; ≈1200 kg; M3D 375 kW / 510 hp / 2500 N m; rail 900 bar; conrod 268 mm; bore 132 mm |
| `SN00.00-W-0002-04H` — *Engine OM 471* | p. 10 / PDF 13 | combustion pressures up to 230 bar; 900 bar rail |
| `GF47.00-W-3013H` — *Fuel high pressure circuit function* (471.9 / MODEL 963, code M5Z) | p. 100 / PDF 103 | injection pressure up to 2100 bar |

The later 390 kW / 2600 N m / 2700 bar OM 471 figures are deliberately **not**
mixed into this configuration. The published ≈1200 kg complete-engine mass is
metadata only and is never used as flywheel or rotating inertia.

## Assumptions not supported by the manual

The document describes the boost-control *architecture* (`GF09.40-W-0001H`:
wastegate, MCM, charge-air sensors) and the APCRS injection *structure*
(`GF47.00-W-3013H`: "with or without additional pressure amplification", up to
2100 bar, MCM-scheduled quantity and timing). It publishes **no** boost pressure,
injection timing, injection quantity, rate shape, fuel consumption, firing order,
rotating inertia, valve events, friction map, or the speeds of the M3D peaks.

Every value below is therefore `calibrated` (or `derived`), carries a stated
purpose and safe range, and is visible in the UI provenance panel. The published
structure is what justifies the *shape* of the model; none of it supplies numbers.

| Assumption | Value |
|---|---|
| Firing order / crank phase offsets | 1-5-3-6-2-4, conventional inline-six |
| Rotating inertia | 3.5 kg m²; the published ≈1200 kg engine mass is **never** used here |
| Gas-exchange boundaries | intake valve close 160° BTDC, exhaust valve open 140° ATDC |
| **Boost schedule** | 1.25–2.66 bar absolute against speed. With the smoke limit this is what sets the full-load torque curve, and it is exactly what Milestone 3 replaces with turbocharger dynamics |
| Boost response | 0.6 s first-order lag, standing in for turbo inertia |
| Charge temperature | 315 K at ambient pressure, +12 K per bar of boost |
| Exhaust back-pressure | +100 kPa per bar of boost, representing the turbine |
| Injection timing | 8–15° BTDC against speed, retarded 0.00045 rad/mg with load — the retard is what holds peak pressure inside the 230 bar envelope |
| Nozzle | 8 holes × 175 µm, discharge coefficient 0.75 |
| Amplified-variant threshold | 120 mg/cycle |
| Ignition delay | Hardenberg-Hase at cetane number 50 |
| Heat release | double Wiebe, premixed 10°, diffusion 0.0028 rad/mg, premixed fraction capped at 0.85 |
| Wall heat transfer | Woschni with standard coefficients, 480 K lumped wall temperature |
| Specific heats | `cv(T) = 718 + 0.18 (T − 300)` J/(kg K) |
| Friction | Chen-Flynn style FMEP: constant, peak-pressure, and piston-speed terms |
| Fuel | 42.7 MJ/kg, 832 kg/m³; smoke limit 21:1 AFR; 350 mg/cycle pedal ceiling |
| Idle governor | PI gains; **the 560 rpm target itself is published** |
| Starter | 1500 N·m at the crank, tapering to zero at 300 rpm |
| Governed speed | fuelling tapers from 1900 rpm to zero at 2100 rpm |
| Solver step | 25 µs fixed step |

### Modelling simplifications

- **Gas exchange is a manifold boundary condition**, not orifice flow through the
  valves. Residual gas is carried cycle to cycle, but volumetric efficiency is a
  calibrated constant rather than an emergent result. Orifice flow is deferred to
  Milestone 3, where EGR needs real mass flows anyway.
- **Boost is prescribed, not computed.** There is no turbine, no compressor map,
  and no wastegate. The `air_path` fields the solver reads are the same ones
  Milestone 3 will fill from turbo dynamics.
- Single-zone thermodynamics: no spray, no soot or NOx, no chemical kinetics.

## Determinism

The core is deterministic: for identical configuration, seed, controls, and step
count, `advance` produces bit-identical snapshots, and `advance(n)` equals `n`
calls of `advance(1)`. Dynamometer sweeps are deterministic too — the harness
holds crank speed rather than running a tuned load controller, so a repeated
sweep is bit-identical. Non-finite state, an out-of-range control, and a breach
of the 23 MPa validation envelope all latch a structured fault instead of
propagating.

Live browser playback derives its *step count* from wall-clock time, so a
real-time session is not bit-reproducible across machines. The worker's
`stepOnce` message advances an exact step count for reproducible runs.

## Legal and fidelity boundaries

- The vendored `engine-sim/` checkout (Ange Yaghi, MIT) is a **reference only**.
  No code or assets from it are used in `crates/` or `web/`; if any are adapted
  later, its MIT notice will be preserved and the adaptation documented here.
- No code or assets are taken from closed-source successors or unlicensed
  projects.
- Manufacturer and engine names appear as factual references. No OEM logos are
  used and no endorsement is implied.
- This is not an engineering tool: no emissions-compliance prediction, no ECU
  reproduction, no OEM map reverse engineering.
