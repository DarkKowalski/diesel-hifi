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

## Status: Milestone 3 — air path and sound

Milestone 1 proved the whole path end to end: Rust core → WASM → Web Worker →
Svelte → static `web/dist`. Milestone 2 replaced its placeholder combustion with
a real one. Milestone 3 stops prescribing the air path and makes it a physical
system — and takes the exhaust sound directly from it.

**In (Milestone 3):** a mean-value wastegate turbocharger (compressor, turbine,
a shaft with inertia, a boost controller driving the wastegate); cooled
high-pressure EGR with a rate controller; both manifolds as real control volumes
with filling dynamics; burned-gas tracking so recirculated exhaust dilutes the
oxygen the smoke limit sees; orifice flow through the exhaust valve, giving real
blowdown; and user-gesture-gated Web Audio whose samples come from that blowdown.

**Boost is now an outcome, not an input.** Milestone 2's `boost_target_schedule`
survives with a more honest meaning: it is the ECU setpoint the wastegate
controller regulates towards, which is what the manual describes the MCM doing.
Turbo lag is no longer a tuned time constant — it emerges from shaft inertia.

**Out (later milestones):** aftertreatment chemistry, the staged decompression
engine brake, driveline and truck load, orifice flow through the *intake* valve,
and any production-quality audio model.

### Calibration result

| | Model | Published | Error |
|---|---:|---:|---:|
| Peak power | 369.2 kW at **1800 rpm** | 375 kW | −1.5% |
| Peak torque | 2509 N·m at **1000 rpm** | 2500 N·m | +0.4% |
| Max cylinder pressure | 20.29 MPa | 23 MPa envelope | 12% margin |
| Idle | 560 rpm | 560 rpm | on target |
| Best fuel consumption | 180 g/kW·h | not published | — |

The **magnitudes** are published. The **engine speeds in bold are an outcome of
our calibration** — the manual does not publish where the real engine makes its
peaks, and these must never be quoted as OEM data. The UI labels them
`CALIBRATED` next to the published reference lines.

Fuel consumption moved from Milestone 2's optimistic 176.5 g/kW·h to 180, in the
direction that milestone predicted it would: recirculation pumping work and
turbine back-pressure both exist now. A real OM 471 is nearer 185–190, so this
remains a little optimistic and is reported rather than tuned away.

### Sound

The exhaust note is not synthesised. The solver's fixed step is 25 µs — a 40 kHz
sample rate — and it runs about 58× faster than real time, so it emits **one
audio sample per solver step**, taken from the computed pressure difference
across the exhaust ports. What you hear is the same cylinder pressure that drives
the crank, and the firing frequency falls out of the firing order rather than
being programmed.

Three things in that chain are easy to get wrong and were:

- **A boundary condition cannot make a sound.** Milestone 2 clamped cylinder
  pressure to the manifold during the exhaust stroke, which makes the pressure
  difference across the port identically zero. The exhaust valve needed real
  orifice flow before there was any pulse to hear.
- **An open pipe radiates the rate of change of flow, not the flow.** A pipe
  mouth is an acoustic monopole. Radiating the flow itself loses 6 dB per octave
  and buries almost all the energy below roughly 80 Hz, where no ordinary
  speaker reproduces anything — a signal that passes every test for being finite,
  bounded and correctly pitched, and is silent in practice.
- **Gain has to be calibrated against the loudest thing the engine does, which
  is starting — not against steady running.** A start transient is louder than
  the governed rev limit. Set the gain on cruise and the transient drives the
  soft clipper flat, and a flat-topped waveform is full of high harmonics: heard
  as a buzz near the filter cutoff rather than as a louder engine. Idle now sits
  around −33 dBFS and the start transient near −3, with no sample against the
  ceiling.

The UI carries a spectrum view off the output path for exactly that reason: it
shows where the energy actually is, which no assertion about sample values can.

## Layout

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, the air path, the exhaust acoustic source, the dynamometer harness, snapshots, native tests. No browser, DOM, audio-device, or filesystem dependency. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering, Web Audio orchestration. |
| `web/src/audio` | The `AudioWorklet` processor: ring buffer, resampler, underrun accounting. Dependency-free plain JavaScript. |
| `scripts/verify-dist.mjs` | Static-build verification. |

`crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json` is the engine
configuration (schema version 3). It is compiled into the WASM module with
`include_str!`, so the deployed site needs no runtime fetch. Regenerate it with
`python3 scripts/gen-om471-config.py`, which holds the authoritative provenance
table; `cargo run --release -p sim-core --example sweep` is the calibration probe
behind the figures above, and `--example audio_probe` reports exhaust levels and
where their energy sits.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # native core: geometry, provenance, catalog,
                                # determinism, limits, combustion, heat transfer,
                                # turbo, EGR, acoustics, dyno calibration,
                                # data-driven, ID-branch guard

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
| `GF14.20-W-3000H` — *Exhaust gas recirculation, function* (471.9 / MODEL 963, 964) | p. 66 / PDF 69 | **EGR cooler duty: about 650 °C in, about 170 °C out**; the cooled high-pressure loop and its throttle valve; EGR active across the whole speed range |
| `GF09.40-W-0001H` — *Boost pressure control and turbocharger protection* | pp. 35–36 / PDF 38–39 | wastegate architecture, boost pressure positioner, charge-air cooler, turbocharger protection function. **No numeric value is taken from this section** |

The later 390 kW / 2600 N m / 2700 bar OM 471 figures are deliberately **not**
mixed into this configuration. The published ≈1200 kg complete-engine mass is
metadata only and is never used as flywheel or rotating inertia.

> **One number in the manual that is deliberately not used.** The turbocharger
> section says the MCM can "influence the pressure (up to 2.8 bar) which should
> be applied to the vacuum cell". That 2.8 bar is the *pneumatic control
> pressure operating the wastegate actuator* — not boost pressure. It happens to
> sit close to this model's calibrated peak boost, which makes it exactly the
> kind of figure that gets miscited as an OEM boost specification. It appears
> nowhere in the configuration, and a test asserts that no parameter claims it.

## Assumptions not supported by the manual

The document describes the boost-control *architecture* (`GF09.40-W-0001H`:
wastegate, MCM, charge-air sensors), the EGR loop *architecture*
(`GF14.20-W-3000H`: cooled high-pressure recirculation, throttle valve,
positioner, active across the whole speed range) and the APCRS injection
*structure* (`GF47.00-W-3013H`: "with or without additional pressure
amplification", up to 2100 bar, MCM-scheduled quantity and timing). Apart from
the EGR cooler temperatures, it publishes **no** boost pressure, recirculation
rate, turbine or compressor geometry, efficiency, inertia, back-pressure,
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
| **Boost setpoint schedule** | 1.28–3.08 bar absolute against speed. No longer written into manifold pressure: it is the target the wastegate controller regulates towards |
| Turbocharger | 100 mm compressor wheel, 0.72/0.70 compressor/turbine efficiency, 3.5e-5 kg m² shaft inertia, 6.0e-4 m² turbine effective area, 2.5e-3 m² wastegate |
| Manifold volumes | 0.020 m³ intake, 0.010 m³ exhaust |
| Charge-air cooler | 0.80 effectiveness against 85 °C coolant |
| Exhaust restriction | 120 kPa per (kg/s)², about 15 kPa at rated flow — muffler and aftertreatment *can* as a flow resistance only |
| **EGR rate** | 0.04–0.14 against speed, ceiling 0.35. Pressure-limited to zero at full load below about 1000 rpm, where the turbine cannot lift exhaust above intake |
| EGR valve | 4.0e-4 m² fully open, discharge coefficient 0.70 |
| Exhaust valve | 2.5e-3 m² at full lift, 35° of crank to ramp between shut and full lift |
| Injection timing | 8–15° BTDC against speed, retarded 0.00075 rad/mg with load — the retard is what holds peak pressure inside the 230 bar envelope |
| Smoke limit | 19.5:1 minimum air/fuel ratio; stoichiometric taken as 14.5:1 |
| Exhaust acoustics | 20 Hz high pass, 6 kHz two-pole muffler roll-off, soft-clip knee 0.85, gain set so the start transient approaches the knee without saturating |
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

- **Only the exhaust valve has orifice flow.** The intake remains a manifold
  boundary condition, because it sits near equilibrium; the exhaust valve opens
  onto a pressure ratio large enough to choke, and blowdown is what the acoustic
  source is made of. Volumetric efficiency is still a calibrated constant rather
  than an emergent result.
- **The compressor map is a simplified ellipse**, not a measured map, and the
  turbine is a fixed effective area rather than a swallowing-capacity table.
- **No aftertreatment chemistry.** The restriction downstream of the turbine is a
  lumped flow resistance. There is no filter loading, no SCR, no dosing and no
  regeneration; SPEC §3 excludes complete aftertreatment from the MVP.
- **EGR is pressure-limited at full load below about 1000 rpm**, where the
  turbine does not raise exhaust manifold pressure above intake. That is real
  behaviour for a fixed-geometry high-pressure loop, not a bug, but it means the
  scheduled rate is a target the hardware cannot always meet.
- Single-zone thermodynamics: no spray, no soot or NOx, no chemical kinetics.
  **The model does not predict emissions**, so the benefit EGR buys can only be
  shown indirectly, as a lower combustion temperature.
- **Audio is basic, per SPEC §2.** Exhaust pulses only: no turbocharger whine, no
  intake noise, no pipe resonance, no mechanical noise.

## Determinism

The core is deterministic: for identical configuration, seed, controls, and step
count, `advance` produces bit-identical snapshots, and `advance(n)` equals `n`
calls of `advance(1)`. Dynamometer sweeps are deterministic too — the harness
holds crank speed rather than running a tuned load controller, so a repeated
sweep is bit-identical. Non-finite state, an out-of-range control, and a breach
of the 23 MPa validation envelope all latch a structured fault instead of
propagating.

Audio is part of that guarantee: the samples are a pure function of state, and
`advance(n)` produces the same signal as `n` calls of `advance(1)`.

Live browser playback derives its *step count* from wall-clock time, so a
real-time session is not bit-reproducible across machines. The worker's
`stepOnce` message advances an exact step count for reproducible runs. Sound
glitches when the simulation stalls, by design — the worklet outputs silence
rather than repeating its last block, so a stall is audible rather than
disguised.

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
