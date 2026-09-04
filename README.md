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

## Status: Milestone 1 — vertical slice

Milestone 1 proves the whole path end to end: Rust core → WASM → Web Worker →
Svelte → static `web/dist`.

**In:** deterministic fixed-step core; validated, versioned `EngineConfig` with
per-parameter provenance; catalog list / active-ID / select / deterministic
reset; a coarse batched `wasm-bindgen` boundary; WASM stepping in a Web Worker;
engine selector, start/stop/reset, pedal and load controls, live telemetry, and a
provenance panel; a self-contained static build.

**Out (later milestones):** turbocharger and EGR dynamics, aftertreatment, the
staged decompression engine brake, Web Audio, and any power or torque
calibration claim.

## Layout

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, snapshots, native tests. No browser, DOM, audio-device, or filesystem dependency. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering. |
| `scripts/verify-dist.mjs` | Static-build verification. |

`crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json` is the engine
configuration. It is compiled into the WASM module with `include_str!`, so the
deployed site needs no runtime fetch.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # native core: geometry, provenance, catalog,
                                # determinism, limits, data-driven, ID-branch guard

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

The document publishes no firing order, rotating inertia, valve events, friction
map, heat-release law, turbo maps, injector rate shapes, or the engine speeds at
which the M3D peak power and torque occur. Every value below is therefore
classified `calibrated` (or `derived`), carries a stated purpose and safe range,
and is visible in the UI provenance panel.

| Assumption | Milestone 1 value |
|---|---|
| Firing order / crank phase offsets | 1-5-3-6-2-4, the conventional inline-six sequence |
| Rotating inertia | 3.5 kg m² for crank, flywheel and rotating accessories |
| Gas-exchange boundaries | intake valve close 160° BTDC, exhaust valve open 140° ATDC |
| Friction | Chen-Flynn style FMEP: constant, peak-pressure, and piston-speed terms |
| Polytropic exponents | 1.35 compression, 1.30 expansion (lumped wall heat transfer) |
| Manifold conditions | fixed 101 325 Pa / 320 K intake, 106 000 Pa exhaust — **naturally aspirated**, no turbo in Milestone 1 |
| Specific gas constant | *derived*: `R = R_universal / M_air` = 287.0528 J/(kg K) |
| **Placeholder heat release** | single smooth 40° burn from 5° BTDC; **no ignition delay, no premixed/diffusion split, no injector rate shaping** |
| Diesel lower heating value | 42.7 MJ/kg, general reference literature |
| Fuelling limits | 230 mg/cycle pedal ceiling, clipped by a 22:1 smoke-limit air/fuel ratio |
| Idle governor | PI gains; **the 560 rpm target itself is published** |
| Starter | 1500 N m at the crank, tapering to zero at 300 rpm |
| Governed speed | fuelling tapers from 1900 rpm to zero at 2100 rpm |
| Solver step | 25 µs fixed step (≈0.36° crank at 2400 rpm) |

**Milestone 1 makes no power or torque calibration claim.** The engine runs
naturally aspirated, so it cannot and does not reach the published 375 kW. Those
figures are carried as published *rated* metadata only, and the speeds at which
they occur are not modelled. Calibrating against the published magnitudes is
Milestone 2 work.

The combustion path is an explicitly labelled placeholder whose only job is to
route pedal demand to crank torque *through cylinder pressure*, rather than
writing a target torque into crank acceleration. Milestone 2 replaces
`crates/sim-core/src/sim/heat_release.rs` and its provenance entries wholesale.

## Determinism

The core is deterministic: for identical configuration, seed, controls, and step
count, `advance` produces bit-identical snapshots, and `advance(n)` equals `n`
calls of `advance(1)`. Non-finite state, an out-of-range control, and a breach of
the 23 MPa validation envelope all latch a structured fault instead of
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
