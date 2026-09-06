# Diesel Truck Engine Simulator

A browser-based, real-time **diesel truck** engine simulator. The deterministic
simulation core is Rust, compiled both natively and to WebAssembly, presented
through a static Svelte application with no backend.

The built-in engine is a public-data reference model of the 2011 Mercedes-Benz
OM 471.9 M3D.

> **Not an OEM-certified digital twin.** A public-spec reference simulation built
> from published service literature. Mercedes-Benz and OM 471 are factual
> references only; no endorsement is implied and no OEM logos or assets are used.
> Every configuration value is labelled `published`, `derived`, or `calibrated`
> and the UI shows those labels. An estimate is never presented as OEM data.

This file is the whole specification and the whole documentation: requirements,
acceptance criteria, results, and every assumption the model makes.

## Scope

**In.** One diesel configuration behind a stable selection API;
crank-angle-resolved four-stroke state; injection, ignition delay, heat release,
cylinder pressure, crank dynamics, friction, load, idle control; a wastegate
turbocharger and cooled EGR; the staged decompression engine brake; a rigid truck
driveline; two-path engine audio; native deterministic tests; a static Svelte UI
with the simulation off the UI thread.

**Out.** Gasoline engines or any multi-fuel abstraction. Engineering
certification, emissions prediction, ECU reproduction, OEM map reverse
engineering. Backend, SSR, accounts, databases, telemetry collection, runtime
network calls. CFD, finite-element mechanics, injector hydraulics, chemical
kinetics, aftertreatment chemistry.

## Architecture

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, air path, engine brake, driveline, both acoustic sources and the exhaust duct, dynamometer harness, snapshots, native tests. **No browser, DOM, audio-device, or filesystem dependency.** |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering, Web Audio orchestration. |
| `web/src/audio` | `AudioWorklet` processor: ring buffer, resampler, underrun accounting. Dependency-free plain JavaScript. |
| `web/src/lib/cabin.ts` | Cockpit listening stage as data: filter table, reflection taps, seeded impulse response. Pure and Web-Audio-free, unit-tested under Node. |
| `scripts/verify-dist.mjs` | Static-build verification. |

The production artifact is `web/dist` and must run from a domain root or a
configurable subpath on ordinary static hosting. Use pnpm, TypeScript, Svelte and
Vite; not SvelteKit. All runtime code, WASM, configuration, fonts and assets are
local to the build.

## Configuration and provenance

`EngineConfig` is versioned (schema 5) and validated before any simulation state
exists. It carries identity, geometry, valvetrain, injection, combustion,
friction, gas, air path, turbo, EGR, exhaust system, engine brake, driveline,
governor, load, inertia, limits, solver and audio sections, plus source records
and parameter-level provenance.

- **`published`** values must name a source record and a locator.
- **`derived`** values must state their formula and inputs.
- **`calibrated`** values must state their purpose and a safe range containing
  the value.

`config/paths.rs` is the authoritative parameter list. Adding a field without
adding it there and giving it provenance is a validation failure, not a silent
omission. The catalog exposes list, active-ID and select-by-ID; selecting any
configuration, including the active one, performs a deterministic reset. **Solver
code must not branch on the engine ID**, and a test scans the sources to prove it.

`crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json` is compiled into the
WASM module with `include_str!`, so the deployed site needs no runtime fetch. It
is the authoritative copy, edited directly and validated on load; there is no
generator standing between it and the model.

## Simulation invariants

- SI units and radians internally; one four-stroke cycle is `4 * PI` radians.
- Cylinder phase offsets derive from the configured firing order, never from
  engine-specific solver logic.
- Pedal input requests **fuel**, not a throttle plate. Crank torque comes from
  cylinder pressure through slider-crank geometry; no term writes a target torque
  into crank acceleration.
- Friction, pumping, accessory, driveline, starter and governor torques are all
  explicit terms.
- 23 MPa is a validation envelope, not a pressure target.
- Fixed-step deterministic integration. Non-finite state and out-of-range
  configuration are rejected.
- Any randomness is seeded explicitly.
- No heap allocation, blocking, logging, DOM access or I/O in the hot step loop.

## WASM and worker API

Versioned and deliberately coarse: list configuration summaries, return the
active ID, select by ID, reset with an explicit seed and initial conditions,
submit controls, advance many fixed steps, return one compact snapshot. Errors
are structured for invalid IDs, inputs and numerical state. Memory ownership is
explicit and there are no per-step JS/WASM calls. The module runs in a dedicated
Web Worker and needs neither `SharedArrayBuffer` nor cross-origin isolation.

The UI provides an engine selector populated through the real catalog API,
start/stop and reset, pedal and load controls, RPM with pressure/torque/state
telemetry, visible error and worker lifecycle states, and **an explicit user
action before Web Audio starts**.

## Milestones

| # | Delivered |
|---|---|
| 1 | Vertical slice: workspace, validated configuration, minimal crank/cylinder state, WASM adapter, worker protocol, Svelte controls and telemetry. |
| 2 | Diesel combustion: injection, ignition delay, double-Wiebe heat release, pressure-derived torque, calibrated to the published power and torque magnitudes. |
| 3 | Air path and sound: wastegate turbo and EGR dynamics, exhaust pulse output, gesture-gated Web Audio. |
| 4 | Truck load and engine brake: rigid driveline and the staged decompression brake, against the published M5U anchors. |
| 5 | Engine acoustics: structural combustion noise, per-cylinder build scatter, radiated port flow, an exhaust duct with runners and turbine loss, and a retuned cockpit stage. |

Still out: aftertreatment chemistry, clutch slip and gear-change behaviour, the
manual's shift-assist and engine-stop-assist brake functions, ABS interaction,
and orifice flow through the *intake* valve.

## Results

### Calibration

| | Model | Published | Error |
|---|---:|---:|---:|
| Peak power | 369.2 kW at **1800 rpm** | 375 kW | −1.5% |
| Peak torque | 2509 N·m at **1000 rpm** | 2500 N·m | +0.4% |
| Max cylinder pressure | 20.31 MPa | 23 MPa envelope | 12% margin |
| Idle | 560 rpm | 560 rpm | on target |
| Best fuel consumption | 180 g/kW·h | not published | — |

The **magnitudes** are published; the **speeds in bold are an outcome of our
calibration** and must never be quoted as OEM data. The UI labels them
`CALIBRATED`. A real OM 471 is nearer 185–190 g/kW·h, so consumption remains
slightly optimistic; it is reported rather than tuned away.

### Engine brake

Fitted variant **M5U**. The manual publishes two brake-power figures for it, and
they are the whole acceptance criterion:

| | Model | Published | Error |
|---|---:|---:|---:|
| Brake power at 1300 rpm | 109.0 kW | 100 kW | +9.0% |
| Brake power at 2300 rpm | 274.4 kW | 300 kW | −8.5% |

Held to ±10% rather than the ±3% the fuelled peaks carry: the manual publishes no
brake cam contour, lift, timing, effective area or MCM target, so there is far
more freedom per constraint. A ±3% fit was available and rejected — it reaches
the numbers by venting from 42° BTDC, which halves peak cylinder pressure against
the motored trace and contradicts the manual's *"the compression pressure is
increased as a result"*. The shipped release lobe opens at 12° BTDC, where peak
pressure sits **above** the motored trace.

Braking torque is produced, not prescribed: the brake computes an open area, gas
flows through it, and the resulting pressure drives the crank through the same
geometry as combustion. A test asserts no solver source so much as mentions the
published anchors — those are the validation target, and a model allowed to read
its own answer proves nothing.

| Stage | Manual | Model at 1300 rpm |
|---|---|---:|
| Motoring | — | 20.5 kW |
| I | "brake power is achieved by cylinders 1...3" | 47.4 kW |
| II | "brake power is provided by all cylinders" | 72.1 kW |
| III | all cylinders, plus wastegate and EGR positioner | 109.0 kW |

Stage I gives 0.52 of stage II's braking above the motoring baseline, which is
what three cylinders of six should give. The manual does not say which stage its
figures describe; maximum brake power is stage III, and that reading is recorded
as an interpretation, not as published fact.

### Sound

The engine note is not synthesised. The 25 µs step is a 40 kHz sample rate, so
the solver emits **one audio sample per step** from quantities it already
integrates. Two paths radiate, because a diesel has two:

- **The exhaust.** The net mass flow the ports actually passed — the same
  clamped, settled flow `flow::exchange` applies to the gas, not a second
  estimate of it — down per-cylinder manifold runners, through the turbine's
  insertion loss, into a tailpipe modelled as a waveguide with a reflecting,
  lossy open end. What leaves at the end is the **volume velocity at the mouth**,
  `p+ − p−`; the aftertreatment substrate takes its cut on each traverse.
- **The engine itself.** The premixed burn is a near-step pressure rise inside a
  stiff iron box, and the box rings. Five resonators standing for bending and
  breathing modes of the block, head and covers turn cylinder pressure into
  combustion noise that radiates straight to air and never goes near the exhaust.
  The lowest is the whole block bending at 480 Hz — a 12.8 L iron structure bends
  in the low hundreds of hertz — and the four above it carry the clatter from
  roughly 900 Hz to 4 kHz, which is what makes the ear say *diesel* rather than
  *engine*.

**Nothing is scheduled.** The engine rattles at light load and mellows under it
because ignition delay lengthens when lightly loaded, which raises the premixed
fraction, which sharpens the rise. The brake barks because its release lobe is
the fastest pressure event in the model. The pipe resonance shifts as the exhaust
heats because the speed of sound comes from the temperature the solver
integrates. Permuting the firing order changes the waveform because the runners
differ in length. None of that is written down as a rule.

Where the energy sits, from `cargo run --release -p sim-core --example audio_probe`:

| Band | Idle | Cruise | Full load | Target |
|---|---:|---:|---:|---:|
| `<80 Hz` | 3.4% | 7.2% | 8.8% | ≤ 35% |
| `80–300 Hz` | 38.2% | 15.0% | 45.4% | ≤ 55% |
| `150 Hz–15 kHz` | 76.8% | 82.0% | 67.9% | ≥ 60% |
| `2–15 kHz` | 3.7% | 3.1% | 6.5% | ≥ 3% idle |
| Peak / dBFS | 0.292 / −25.1 | 0.697 / −12.2 | 0.833 / −10.5 | under the 0.85 knee |

The probe also reports each path on its own, because the balance between them is
the whole question and a mixed band share cannot say which of the two moved:

| Alone, at full load | dBFS | `<80 Hz` | `80–300` | `300–2k` | `>2 kHz` |
|---|---:|---:|---:|---:|---:|
| Exhaust | −13.0 | 7.1% | 89.3% | 3.5% | 0.0% |
| Block | −12.3 | 3.5% | 11.9% | 70.0% | 14.5% |

The audible band starts at 150 Hz because that is where a small speaker begins
reproducing anything, which is the question the criterion asks. Bands run to
15 kHz rather than to Nyquist: the explicit port transfer leaves a
two-sample limit cycle within a whisker of Nyquist which is arithmetic rather
than sound, and the probe reports it separately so it cannot satisfy a
high-frequency target it is not signal for. The probe's four full-range bands sum
to 100% as a self-check.

Six mistakes this chain invites, all of which were made:

- **A boundary condition cannot make a sound.** Clamping cylinder pressure to the
  manifold during the exhaust stroke makes the pressure difference across the
  port identically zero. Real orifice flow had to come first.
- **An open pipe radiates the rate of change of flow, not the flow.** A pipe
  mouth is a monopole. Radiating flow loses 6 dB per octave and buries the energy
  below 80 Hz — a signal that passes every test for being finite, bounded and
  correctly pitched, and is inaudible.
- **Do not radiate a proxy for something you have already solved.** The source
  was once port area times pressure difference, computed beside the call that
  solved the real flow. Real flow goes as the square root of the difference and
  saturates once choked, so the proxy exaggerated the blowdown peak and distorted
  its shape through the choked-to-subsonic transition. The pulse shape is the
  timbre.
- **A resonator with poles and no zeros passes DC.** A real mode has no response
  to static pressure. Without zeros, the fraction of a pascal per step a stopped
  engine's trapped charge gains against its walls came through as a standing
  offset — an engine that was quiet rather than silent.
- **A pipe mouth is a pressure node, so its pressure is not what radiates.**
  `p+ + p−` at an open end is nearly zero at low frequency by definition.
  What radiates is the mouth's volume velocity, `p+ − p−`, at a maximum exactly
  where the pressure is at a minimum. The factor between them is
  `(1 + R) / (1 − R)`, which at the calibrated −0.8 is −19 dB across everything
  below the radiation corner: the entire exhaust note.
- **A duct damped only at its mouth is an organ pipe.** An unflanged pipe returns
  nearly all of a 200 Hz wave, so the mouth is not where a real exhaust system's
  damping lives — the aftertreatment is. Modelled as a pure compliance with no
  loss, the duct rings for a T60 near 360 ms and whichever firing order lands on
  a resonance swallows the rest of the note.

### Where you are listening from

The solver's output is a *tailpipe* signal. A driver is not at the pipe mouth, so
the stage is switchable: **raw tailpipe** (unfiltered, what the spectrum view was
built to verify) or **truck cockpit** (default).

The cab is a Web Audio chain in `cabin.ts` and `audioEngine.ts`, not a change to
the physics: 30 Hz high pass, +5 dB at 85 Hz, −3 dB at 380 Hz, a 2.6 kHz low
pass, two early reflections at 7.3 and 11.9 ms panned apart, a seeded impulse
response, and a compressor. Both paths always run; switching crossfades over
40 ms. The low pass sits at 2.6 kHz rather than lower because a cab takes the
sharp edge off an exhaust; it does not silence an engine two feet away through
the bulkhead, and clatter is emphatically audible from a driver's seat.

**Nothing is added.** No road noise, no synthesised rumble, no samples. Every
value reaching the speaker is still the solver's cylinder pressure, and a unit
test asserts the stage builds nothing that can produce sound on its own.

**It is level-matched, and that needed two gains.** Matched with an output gain
alone the cab sat level under load and 6.3 dB louder at idle, because the
compressor works at the loud end and does nothing at the quiet end. Trimming
ahead of the compressor moves the quiet end nearly decibel for decibel and the
compressed loud end far less, so trim-then-make-up (0.42 in, 1.90 out) brings
both together:

| Measured at the output | Idle | 90% pedal, 300 N·m |
|---|---:|---:|
| Cockpit level relative to raw | +1.46 dB | −1.17 dB |
| 2–8 kHz band | — | −30% |
| 60–400 Hz band | — | +2% |

Both ends are asserted within 3 dB by the browser suite through the same analyser
the spectrum view uses, repeating to within ±0.05 dB. The remaining spread is the
compressor doing its job.

None of this is published. The manual says nothing about how the engine sounds
and less about how its cab sounds; these are listening choices and the UI says so
where you switch them.

## Reference engine and sources

Stable ID `mercedes-benz-om471-9-m3d-375kw`, display name *OM 471.9 M3D 375 kW
Reference*. Primary source: **Introduction of engine OM 471 and exhaust
aftertreatment**, Mercedes-Benz service literature, technical status 2011-09-01,
order number 6517 1260 02. Scope: engine series 471.9 in model 963/964, power
code M3D, plus documented M5Z Euro VI subsystems where the document states them.

### Published parameters

| Parameter | Published | Internal |
|---|---:|---:|
| Layout | Inline six | 6 cylinders |
| Displacement | 12.8 L | `0.0128 m³` reference |
| Bore × stroke | 132 × 156 mm | `0.132` / `0.156 m` |
| Compression ratio | 17.3:1 | `17.3` |
| Connecting rod | 268 mm | `0.268 m` |
| Valve train | DOHC, 2 + 2 per cylinder | 4 valves/cylinder |
| Idle speed | 560 rpm | `560` |
| Complete-engine mass | ≈1200 kg | **metadata only** |
| M3D maximum output | 375 kW / 510 hp | `375000 W` |
| M3D maximum torque | 2500 N·m | `2500 N·m` |
| Maximum rail pressure | 900 bar | `90 MPa` |
| Amplified injector pressure | up to 2100 bar | `210 MPa` limit |
| Maximum combustion pressure | up to 230 bar | `23 MPa` envelope |

The ≈1200 kg mass **must not** be used as flywheel or rotating inertia.

Published subsystem behaviour: APCRS injectors with or without local pressure
amplification, quantity and timing by operating state; a single turbocharger
feeding a charge-air cooler with MCM wastegate boost control; cooled regulated
EGR across the operating range; a three-stage decompression brake with stage I on
cylinders 1–3; M5U anchors 100 kW at 1300 rpm and 300 kW at 2300 rpm (M5V:
150/400 kW).

### Locators

| Locator | Page | Supplies |
|---|---|---|
| `SN00.00-W-0002-05H` — *Technical data of diesel engine OM 471* | 11 / PDF 14 | 12,8 l; 6 in line; DOHC 2/2; idle 560 rpm; CR 17,3; stroke 156 mm; stroke:bore 1,18; ≈1200 kg; M3D 375 kW / 510 hp / 2500 N m; rail 900 bar; conrod 268 mm; bore 132 mm |
| `SN00.00-W-0002-04H` — *Engine OM 471* | 10 / PDF 13 | combustion pressures up to 230 bar; 900 bar rail |
| `GF47.00-W-3013H` — *Fuel high pressure circuit function* | 100 / PDF 103 | injection pressure up to 2100 bar |
| `GF14.20-W-3000H` — *Exhaust gas recirculation, function* | 66 / PDF 69 | **EGR cooler duty: ≈650 °C in, ≈170 °C out**; cooled high-pressure loop and throttle valve; EGR active across the whole speed range |
| `GF09.40-W-0001H` — *Boost pressure control and turbocharger protection* | 35–36 / PDF 38–39 | wastegate architecture, boost positioner, charge-air cooler, protection function. **No numeric value is taken from this section** |
| `GF14.15-W-0002H` — *Engine brake, function* (M5U, M5V) | 60–65 / PDF 63–68 | decompression principle and two-peak cam; three stages and **stage I on cylinders 1–3**; the **1000 rpm** floor; **M5U 100 kW at 1300 rpm, 300 kW at 2300 rpm**; that both variants share hardware and differ only by data record |

### Data gaps

The manual publishes **no** RPM positions for the M3D peaks, firing order, torque
or fuel maps, valve events, turbo maps, injector rate shapes, heat-release law,
friction map, rotating inertias, boost pressure, recirculation rate, brake cam
contour, MCM brake target, vehicle data, or anything at all about acoustics.
Everything in the table below is therefore `calibrated` or `derived`.

The later **390 kW / 2600 N·m / 2700 bar** OM 471 figures are deliberately not
mixed into this configuration.

> **One number in the manual that is deliberately not used.** The turbocharger
> section says the MCM can "influence the pressure (up to 2.8 bar) which should
> be applied to the vacuum cell". That is the *pneumatic control pressure
> operating the wastegate actuator* — not boost pressure. It sits close to this
> model's calibrated peak boost, which makes it exactly the kind of figure that
> gets miscited as an OEM boost specification. It appears nowhere in the
> configuration, and a test asserts no parameter claims it.

## Assumptions not supported by the manual

Every value carries a stated purpose and safe range and is visible in the UI
provenance panel. The published *structure* justifies the shape of the model;
none of it supplies these numbers.

| Assumption | Value |
|---|---|
| Firing order / phase offsets | 1-5-3-6-2-4, conventional inline-six |
| Rotating inertia | 3.5 kg·m²; the ≈1200 kg engine mass is **never** used here |
| Gas-exchange boundaries | intake valve close 160° BTDC, exhaust valve open 140° ATDC |
| **Boost setpoint schedule** | 1.28–3.08 bar absolute against speed — the target the wastegate controller regulates towards, not a written-in manifold pressure |
| Turbocharger | 100 mm compressor wheel, 0.72/0.70 compressor/turbine efficiency, 3.5e-5 kg·m² shaft inertia, 6.0e-4 m² turbine area, 2.5e-3 m² wastegate |
| Manifold volumes | 0.020 m³ intake, 0.010 m³ exhaust |
| Charge-air cooler | 0.80 effectiveness against 85 °C coolant |
| Exhaust restriction | 120 kPa per (kg/s)², ≈15 kPa at rated flow. This is what the aftertreatment is to the *gas path*; what it is to the acoustic path is modelled separately below |
| **EGR rate** | 0.04–0.14 against speed, ceiling 0.35. Pressure-limited to zero at full load below ≈1000 rpm, where the turbine cannot lift exhaust above intake |
| EGR valve | 4.0e-4 m² fully open, discharge coefficient 0.70 |
| Exhaust valve | 2.5e-3 m² at full lift, 35° of crank to ramp between shut and full lift |
| **Brake cam contour** | charging lobe 130° BTDC, release lobe 12° BTDC, half-widths 10°, 1.3e-3 m² at full lift |
| **Brake stage air path** | stages I and II leave the wastegate alone; stage III regulates to 2.33 bar absolute with the loop detuned to a tenth of its fuelled gains. The EGR positioner stays shut in every stage |
| Driveline | 40 t gross combination mass, 0.506 m wheels, Cr 0.006, 6.0 m² drag area, 2.61 final drive, twelve ratios from 14.93:1 to 1.00:1, 0.95 efficiency. The source is an engine manual and publishes nothing about the vehicle |
| Injection timing | 8–15° BTDC against speed, retarded 0.00075 rad/mg with load — the retard holds peak pressure inside the 230 bar envelope |
| Smoke limit | 19.5:1 minimum air/fuel ratio; stoichiometric taken as 14.5:1 |
| Nozzle | 8 holes × 175 µm, discharge coefficient 0.75 |
| Amplified-variant threshold | 120 mg/cycle |
| Ignition delay | Hardenberg-Hase at cetane number 50 |
| Heat release | double Wiebe, premixed 10°, diffusion 0.0028 rad/mg, premixed fraction capped at 0.85 |
| Wall heat transfer | Woschni with standard coefficients, 480 K lumped wall temperature |
| Specific heats | `cv(T) = 718 + 0.18 (T − 300)` J/(kg·K) |
| Friction | Chen-Flynn style FMEP: constant, peak-pressure and piston-speed terms |
| Fuel | 42.7 MJ/kg, 832 kg/m³; 350 mg/cycle pedal ceiling |
| Idle governor | PI gains; **the 560 rpm target itself is published** |
| Starter | 1500 N·m at the crank, tapering to zero at 300 rpm |
| Governed speed | fuelling tapers from 1900 rpm to zero at 2100 rpm |
| Solver step | 25 µs fixed |
| Exhaust acoustics | 20 Hz high pass, soft-clip knee 0.85, exhaust gain 26 and structural gain 2.2, set so the start transient approaches the knee without saturating and so the exhaust carries the firing orders while the block carries the clatter. There is no muffler roll-off parameter: a two-pole low pass standing in for an entire exhaust system is a tone control, and the duct below replaced it |
| Structural modes | 480, 900, 1600, 2600 and 3800 Hz at Q 11, 15, 20, 22 and 25, weighted 0.55, 1.0, 4.0, 6.0 and 11.0. The 480 Hz mode is the block's own bending mode. The four above it ascend, which is the opposite of the radiating physics and is deliberate: the premixed Wiebe rise starts with zero slope, so the model's burn onset drives the upper modes more weakly than a real one would |
| **Exhaust system** | 3.5 m tailpipe of 0.008 m² section; open-end reflection −0.8 with 2 kHz radiation loss; 12 L of free aftertreatment volume acting as 1.5 m of added acoustic length, and a substrate transmission of 0.61 per traverse acting as its flow resistance; 14 dB turbine insertion loss above 400 Hz; runners spanning 100–700 mm. **None of this geometry is published** |
| **Combustion noise** | Four modes at 900/1600/2600/3800 Hz, Q 15/20/22/25, weighted 1.0/4.0/6.0/11.0, overall gain 3.0. The weights *ascend* with frequency, which is the opposite of the radiating physics: the premixed rise starts with zero slope and drives the upper modes weakly, so the weights compensate for a shortfall in the drive rather than claiming an engine radiates more at 3.8 kHz than at 900 Hz |
| **Cylinder build scatter** | ±2% exhaust port area, ±1.5% injector delivery, drawn once per cylinder at reset from the reset seed and never per step. Six bit-identical cylinders sum to a pure harmonic comb the ear hears as synthesised. Far too small to move any calibration result, and a test asserts peak power and torque are unchanged by it |
| **Cockpit listening stage** | 30 Hz high pass; +6 dB at 85 Hz, Q 1.1; −1.5 dB at 380 Hz, Q 1.0; 2.6 kHz low pass; reflections at 7.3 ms (−11 dB, left) and 11.9 ms (−13 dB, right); 180 ms seeded impulse response decaying over 130 ms after 6 ms predelay; compressor at −18 dB, ratio 3, 6/180 ms, trimmed 0.42 in and 1.90 out. Presentation only — downstream of everything, changes no state, bypassable |

## Modelling simplifications

- **Only the exhaust valve has orifice flow.** The intake stays a manifold
  boundary because it sits near equilibrium; the exhaust valve opens onto a
  pressure ratio large enough to choke. Volumetric efficiency is a calibrated
  constant, not an emergent result.
- **The compressor map is a simplified ellipse** and the turbine a fixed
  effective area, not measured maps.
- **No aftertreatment chemistry.** The restriction downstream of the turbine is a
  lumped flow resistance; no filter loading, SCR, dosing or regeneration.
- **Single-zone thermodynamics**: no spray, soot, NOx or chemical kinetics. **The
  model does not predict emissions**, so EGR's benefit shows only indirectly as a
  lower combustion temperature.
- **EGR is pressure-limited at full load below ≈1000 rpm**, where the turbine
  cannot raise exhaust above intake. Real behaviour for a fixed-geometry
  high-pressure loop, but it means the scheduled rate is a target the hardware
  cannot always meet.
- **Audio covers the exhaust and the engine's structure, not everything that
  radiates.** There is no turbocharger whine and no intake noise. Blade-pass at
  the modelled shaft speeds is ultrasonic, so audible intake content would be
  low-order shaft harmonics and diffuser noise — the least grounded thing on the
  list, and deliberately excluded.
- **The exhaust duct is one-dimensional.** A waveguide with a lumped turbine loss
  and the aftertreatment box as an equivalent added length. The added-length
  approximation is a low-frequency one and stops describing the box once a
  wavelength approaches its own dimensions.
- **The cockpit stage is a listening filter, not an audio model.** It says where
  you are sitting, not what the engine does. The cab's transfer function was not
  measured and could not be — no such data exists for this vehicle — so the
  filter is plausible rather than correct, and bypassable for that reason.
- **The brake's wastegate loop is deliberately slower than the fuelled one.** The
  brake carries positive feedback — more boost packs the cylinder harder, which
  dumps more energy into the turbine — and the fuelled gains hunt against it
  above 1800 rpm, swinging absorbed power between 282 and 300 kW as a limit cycle
  rather than a transient. A fixed valve position is worse: absorbed power swings
  roughly threefold between positions 0.35 and 0.25 as the turbine hits its speed
  clamp.
- **Two of the brake's three published operating conditions are represented.** A
  released pedal and the 1000 rpm floor are enforced. The manual also requires
  the clutch released and ABS not in closed-loop operation; the model carries
  neither signal, so those are absent rather than approximated.
- **The driveline coupling is rigid, with no clutch.** Road speed is derived from
  crank speed rather than integrated, so changing gear changes the truck's speed
  instantly and neutral disengages the vehicle rather than letting it coast. In
  gear the truck appears at the crank as inertia — around 210 kg·m² in top gear
  against the engine's own 3.5, which is why a laden truck on a long descent
  needs a brake that cannot wear out. Driveline efficiency applies on the
  tractive side only, so overrun retarding torque is slightly overstated; the
  alternative puts a discontinuity exactly where a descending truck sits.

## Determinism

For identical configuration, seed, controls and step count, `advance` produces
bit-identical snapshots, and `advance(n)` equals `n` calls of `advance(1)`. Audio
is part of that guarantee. Dynamometer sweeps are deterministic too: the harness
holds crank speed rather than running a tuned load controller. Non-finite state,
an out-of-range control, and a breach of the 23 MPa envelope each latch a
structured fault rather than propagating.

Live browser playback derives its *step count* from wall-clock time, so a
real-time session is not bit-reproducible across machines; the worker's
`stepOnce` message advances an exact count for reproducible runs. Sound glitches
when the simulation stalls, by design — the worklet outputs silence rather than
repeating its last block, so a stall is audible rather than disguised.

## Acceptance criteria

- Native tests are deterministic for identical configuration, seed, inputs and
  step count.
- Geometry recomputes displacement from six cylinders at 132 × 156 mm and matches
  12.8 L within 0.05 L.
- All published values and provenance classifications round-trip unchanged.
- Nominal operation stays below the 23 MPa envelope; invalid and non-finite state
  is rejected.
- Catalog covers list, active ID, unknown ID, select, and deterministic reset.
- WASM loads and advances in a browser through the worker.
- The UI stays responsive while the simulation runs and populates its selector
  from the real API.
- `web/dist` is a complete static deployment with no runtime external requests,
  loading at both a domain root and a configurable subpath.
- Any calibration target not directly published is visible in source metadata and
  in this document.
- Audio: exactly one sample per solver step; bit-identical across batch
  boundaries; every sample finite and inside `[−1, 1]`; a reset engine silent and
  a reset click-free; the note at the firing frequency for any cylinder count;
  the band shares in **Results → Sound** met.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # 246 tests: geometry, provenance, catalog,
                                # determinism, limits, combustion, heat transfer,
                                # turbo, EGR, acoustics, brake, driveline,
                                # dyno calibration, ID-branch guard

pnpm wasm:build                 # wasm-pack -> web/src/wasm (generated, gitignored)
pnpm wasm:test                  # wasm-pack test --node: WASM API smoke test
pnpm check                      # svelte-check
pnpm test                       # wasm smoke + vitest units + Playwright browser suite
pnpm build                      # wasm + vite build + verify-dist
pnpm build:subpath              # the same build under VITE_BASE=/diesel-hifi/
```

**Do not report a command as passed unless it was run and exited successfully.**

The Playwright suite needs a browser once:
`pnpm --filter web exec playwright install chromium`.

`vite.config.ts` uses a relative base by default, so one build works at a domain
root *and* under any subpath. Set `VITE_BASE` for hosts needing an absolute prefix.

Three calibration probes sit behind the figures above:
`cargo run --release -p sim-core --example sweep` for the fuelled peaks,
`--example brake_sweep` for the brake against its published anchors, and
`--example audio_probe` for levels and where their energy sits.

## Legal and fidelity boundaries

- The vendored `engine-sim/` checkout (Ange Yaghi, MIT) is a **reference only**.
  No code or assets from it are used in `crates/` or `web/`; if any are adapted
  later its MIT notice will be preserved and the adaptation documented here.
- No code or assets are taken from closed-source successors or unlicensed
  projects.
- Manufacturer and engine names appear as factual references. No OEM logos are
  used and no endorsement is implied.
- Not an engineering tool: no emissions-compliance prediction, no ECU
  reproduction, no OEM map reverse engineering.
