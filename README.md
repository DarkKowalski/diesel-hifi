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
driveline; three-path engine audio; native deterministic tests; a static Svelte UI
with the simulation off the UI thread.

**Out.** Gasoline engines or any multi-fuel abstraction. Engineering
certification, emissions prediction, ECU reproduction, OEM map reverse
engineering. Backend, SSR, accounts, databases, telemetry collection, runtime
network calls. CFD, finite-element mechanics, injector hydraulics, chemical
kinetics, aftertreatment chemistry.

## Architecture

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, air path, engine brake, driveline, all three acoustic sources and the exhaust duct, dynamometer harness, snapshots, native tests. **No browser, DOM, audio-device, or filesystem dependency.** |
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
| 6 | Making it sound like a truck: a torque-driven body path, a bounded pipe-mouth radiation transfer, a lower and broader modal bank, cycle-to-cycle combustion variation, and acceptance criteria that measure firing orders and pulse dynamics rather than band shares alone. |

Still out: aftertreatment chemistry, clutch slip and gear-change behaviour, the
manual's shift-assist and engine-stop-assist brake functions, ABS interaction,
and orifice flow through the *intake* valve.

## Results

### Calibration

| | Model | Published | Error |
|---|---:|---:|---:|
| Peak power | 369.2 kW at **1800 rpm** | 375 kW | −1.5% |
| Peak torque | 2509 N·m at **1000 rpm** | 2500 N·m | +0.4% |
| Max cylinder pressure | 20.30 MPa | 23 MPa envelope | 12% margin |
| Idle | 560 rpm | 560 rpm | on target |
| Best fuel consumption | 179.9 g/kW·h | not published | — |

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
| Brake power at 2300 rpm | 274.3 kW | 300 kW | −8.6% |

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
integrates. Three paths radiate, each driven by a different quantity, because
they are three different mechanisms:

- **The exhaust**, driven by port mass flow. The net flow the ports actually
  passed — the same clamped, settled flow `flow::exchange` applies to the gas,
  not a second estimate of it — down per-cylinder manifold runners, through the
  turbine's insertion loss, into a tailpipe modelled as a waveguide with a
  reflecting, lossy open end. What leaves at the end is the **volume velocity at
  the mouth**, `p+ − p−`; the aftertreatment substrate takes its cut on each
  traverse. This is the firing comb: 83% of this path's energy sits in the first
  four orders at cruise and 95% at full load.
- **The structure**, driven by cylinder *pressure*. The premixed burn is a
  near-step pressure rise inside a stiff iron box, and the box rings. Six
  resonators standing for bending and breathing modes of the block, head and
  covers turn cylinder pressure into combustion noise that radiates straight to
  air and never goes near the exhaust. This is the clatter — what makes the ear
  say *diesel* rather than *engine* — and only about a quarter of it lands in the
  firing orders at all.
- **The body**, driven by crank *torque*. Gas and pumping torque swing through
  the whole cycle at the firing frequency and its low orders. The engine reacts
  against its mounts, the frame and cab panels take that reaction, and they
  radiate it between roughly 20 and 200 Hz. This is the roar.

**The third path is why this section was rewritten.** Torque and cylinder
pressure are not the same signal and cannot substitute for one another: pressure
spikes once per cylinder per cycle with a fast premixed edge and drives modes in
the hundreds of hertz and above, while torque swings at the firing rate itself.
With only the first two paths there is combustion noise and an exhaust note and
no low-order weight anywhere, and turning up the structural gain buys more
clatter rather than more engine. Measured before the third path existed, the
loudest octave at cruise was **315–630 Hz**, only 21% of the energy sat in the
firing orders, and the clatter path ran **5.2 dB in front of** the exhaust. That
is a motorbike, and it passed every acceptance criterion then written down.

**Nothing is scheduled.** The engine rattles at light load and mellows under it
because ignition delay lengthens when lightly loaded, which raises the premixed
fraction, which sharpens the rise. The roar tracks engine speed because the body
bank straddles the firing frequency across the whole range rather than because
anything sweeps it. The brake barks because its release lobe is the fastest
pressure event in the model, and it thumps because that lobe is a torque event
too. The pipe resonance shifts as the exhaust heats because the speed of sound
comes from the temperature the solver integrates. Permuting the firing order
changes the waveform because the runners differ in length. None of that is
written down as a rule.

Where the energy sits, from `cargo run --release -p sim-core --example audio_probe`:

| | Idle | Cruise | Full load | Target |
|---|---:|---:|---:|---:|
| Firing orders `f0…4·f0` | 42.1% | 71.3% | 91.9% | ≥ 35% loaded |
| Crest factor | 14.7 dB | 10.9 dB | 10.5 dB | ≥ 9 dB |
| Firing-rate modulation | 0.45 | 0.37 | 0.83 | > 0 |
| `<80 Hz` | 17.2% | 18.6% | 4.1% | ≤ 35% |
| `80–300 Hz` | 66.2% | 56.2% | 90.3% | ≥ 45% |
| `300 Hz–2 kHz` | 15.0% | 24.2% | 4.9% | ≤ 30% |
| `150 Hz–15 kHz` | 55.3% | 49.6% | 58.8% | ≥ 35% |
| `>15 kHz` residue | 0.64% | 0.00% | 0.00% | ≤ 1% |
| Peak / dBFS | 0.184 / −29.4 | 0.543 / −16.2 | 0.796 / −12.4 | under the 0.85 knee |

The loudest octave is now `80–160 Hz` at idle and cruise and `160–315 Hz` at full
load, against `315–630 Hz` at idle and cruise before.

The probe reports each path on its own, because the balance between them is the
whole question and a mixed band share cannot say which one moved:

| Alone, at full load | dBFS | `<80 Hz` | `80–300` | `300–2k` | `>2 kHz` | orders |
|---|---:|---:|---:|---:|---:|---:|
| Exhaust | −12.1 | 7.2% | 90.0% | 2.8% | 0.0% | 94.9% |
| Block | −22.0 | 4.1% | 20.6% | 62.9% | 12.5% | 24.4% |
| Body | −23.2 | 84.0% | 16.0% | 0.0% | 0.0% | 99.9% |

The exhaust leads the block by 7.3, 6.3 and 9.9 dB at the three points. That
figure is the single one that decides whether this reads as a truck: the exhaust
carries the firing orders and the block sits on top of them, and a block in front
of the exhaust is a small engine however the bands come out.

**Two measurements exist because band shares are blind to them.** A pulse train
and a tone can hold identical energy in every band. *Crest factor* — peak over
RMS — is 3 dB for a sine and double figures for a series of distinct combustion
events, and it collapses long before saturation measures as distortion: during
calibration it read 3.5 dB with the gains too high, which is a signal squared off
into a buzz. *Firing-rate modulation depth* rectifies the signal, low-passes it
to an envelope, and compares that envelope's content at `f0` and `2·f0` against
its mean; a steady tone gives nearly zero however loud it is.

The audible band starts at 150 Hz because that is where a small speaker begins
reproducing anything, which is the question that criterion asks — and only that
question. Bands run to 15 kHz rather than to Nyquist: the explicit port transfer
leaves a two-sample limit cycle within a whisker of Nyquist which is arithmetic
rather than sound, and the probe reports it separately so it cannot satisfy a
high-frequency target it is not signal for. The probe's four full-range bands sum
to 100% as a self-check.

Eight mistakes this chain invites, all of which were made:

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
- **A monopole only rises to `ka ≈ 1`.** Having established that a pipe mouth
  radiates `dQ/dt` rather than `Q`, the model differentiated *for ever*. A real
  mouth stops behaving like a point source once the wavelength is comparable with
  it — near 1 kHz for this duct's 50 mm radius — and beams down its own axis
  instead, so an off-axis listener sees the rise stop. A bare first difference
  put about 25 dB between the 60 Hz firing fundamental and the 1 kHz clatter
  band, and amplified the port transfer's near-Nyquist residue into a tenth of
  the idle output. Replacing it with a one-pole high pass at that corner — same
  6 dB per octave below, flat above — moved the firing-order share from 11% to
  36% at idle and from 21% to 53% at cruise, on its own. It is also the corner
  the duct already used on the *reflected* wave for exactly the same reason; the
  two sides of one physical corner had been given different treatment.
- **A path driven by the wrong quantity cannot be fixed with gain.** Cylinder
  pressure drives the block's bending and breathing modes and produces clatter.
  No setting of its gain produces low-order weight, because the drive has none:
  a premixed spike is a fast edge once per cylinder per cycle. The roar is
  crank *torque*, which swings at the firing rate itself, and until something was
  driven by it the model had no path that could make the sound. Adding gain to
  the wrong path is how an engine ends up loud and small at the same time.

### Where you are listening from

The solver's output is a *tailpipe* signal. A driver is not at the pipe mouth, so
the stage is switchable: **raw tailpipe** (unfiltered, what the spectrum view was
built to verify) or **truck cockpit** (default).

The cab is a Web Audio chain in `cabin.ts` and `audioEngine.ts`, not a change to
the physics: 30 Hz high pass, a +5 dB low shelf at 150 Hz, +2.5 dB at 200 Hz,
−2 dB at 600 Hz, a 2.6 kHz low pass, two early reflections at 7.3 and 11.9 ms
panned apart, a seeded impulse response, and a compressor. Both paths always run;
switching crossfades over 40 ms. The low pass sits at 2.6 kHz rather than lower
because a cab takes the sharp edge off an exhaust; it does not silence an engine
two feet away through the bulkhead, and clatter is emphatically audible from a
driver's seat.

Three of those stages moved when the source did, because each had been tuned
against a signal that no longer exists:

- **The 85 Hz peak became a 150 Hz shelf.** A bump that narrow is a resonance,
  and picking one frequency out of a firing comb that sweeps from 28 Hz at idle
  to over 100 Hz governed lands the boost on the fundamental at one engine speed
  and between orders everywhere else. A shelf lifts the whole region the orders
  travel through, so the weight tracks the engine.
- **The 380 Hz cut moved to 600 Hz.** The block's modal bank now starts at
  210 Hz rather than 480, so a cut at 380 Hz had stopped removing boxiness and
  started removing the engine.
- **The compressor was eased**, from ratio 3 with a 6 ms attack to ratio 2 with
  25 ms. At 1200 rpm a six fires every 16.7 ms, so a 6 ms attack was biting
  *inside* every pulse: it caught the leading edge of each firing event and
  pulled it down, which is the part worth keeping. An attack longer than the
  firing period lets the transient through and acts on the average instead.

**The +2.5 dB at 200 Hz is for the listener's hardware, not for the cab**, and it
is the one stage here that says so. A laptop speaker reproduces essentially
nothing below about 150 Hz. What carries the pitch on hardware like that is not
the fundamental but its harmonics, from which the ear reconstructs a missing
fundamental — which is why a truck heard through a laptop still sounds like a
truck. Measured on the solver's output, half to nine tenths of the exhaust path's
energy already sits at the second through tenth orders: 90–270 Hz at idle,
120/180/240 Hz at cruise, 140/210/280 Hz at full load.

**Nothing is added.** No road noise, no synthesised rumble, no samples, and **no
harmonic generation** — that last is the line this stage came closest to, and the
distinction it turns on is that emphasising harmonics the solver produced is a
filter while manufacturing harmonics it did not is not. Every value reaching the
speaker is still the solver's cylinder pressure, and a unit test asserts the
stage builds nothing that can produce sound on its own.

**It is level-matched, and that needed two gains.** Matched with an output gain
alone the cab sat level under load and 6.3 dB louder at idle, because the
compressor works at the loud end and does nothing at the quiet end. Trimming
ahead of the compressor moves the quiet end nearly decibel for decibel and the
compressed loud end far less, so trim-then-make-up (0.42 in, 1.55 out) brings
both together:

| Measured at the output | Idle | 90% pedal, 300 N·m |
|---|---:|---:|
| Cockpit level relative to raw | +0.89 dB | −0.74 dB |
| 2–8 kHz band | — | −34% |
| 60–400 Hz band | — | +5% |

Both ends are asserted within 3 dB by the browser suite through the same analyser
the spectrum view uses. The remaining spread is the compressor doing its job.

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
| Exhaust acoustics | 20 Hz rumble filter, soft-clip knee 0.85, exhaust gain 9.5, structural gain 0.40 and body gain 0.09, set so the start transient approaches the knee without saturating and so the exhaust leads the block by 6 to 10 dB across the range. The pipe mouth's radiation transfer is a one-pole high pass at `exhaust_system.radiation_cutoff_hz` rather than a bare difference — the same corner the duct uses on the reflected wave, because it is the same physical corner. There is no muffler roll-off parameter: a two-pole low pass standing in for an entire exhaust system is a tone control, and the duct below replaced it |
| Structural modes | 210, 380, 750, 1400, 2400 and 3600 Hz at Q 9, 10, 11, 12, 13 and 14, weighted 0.25, 0.4, 0.9, 3.0, 7.0 and 12.0. The bank used to start at 480 Hz; a 12.8 L iron structure bends lower than that, and the lowest mode is the cue for how big the engine is. Q is in the low tens throughout rather than up to 25, so the clatter is broadband texture instead of a pitched ring |
| **Exhaust system** | 3.5 m tailpipe of 0.008 m² section; open-end reflection −0.8 with 2 kHz radiation loss; 12 L of free aftertreatment volume acting as 1.5 m of added acoustic length, and a substrate transmission of 0.61 per traverse acting as its flow resistance; 14 dB turbine insertion loss above 400 Hz; runners spanning 100–700 mm. **None of this geometry is published** |
| **Combustion noise** | The weights still *ascend* with frequency, which is the opposite of the radiating physics. This was re-examined and kept: descending weights were tried, on the argument that a big engine should ring low, and they starved the top end to under 1% above 2 kHz because the drive genuinely falls that steeply — the premixed Wiebe rise starts with zero slope, so the model's burn onset drives the upper modes far more weakly than a real one would. The weights compensate for a shortfall in the drive rather than claiming an engine radiates more at 3.6 kHz than at 750 Hz. What changed instead is where the bank *starts* and how sharp it is |
| **Cylinder build scatter** | ±2% exhaust port area, ±1.5% injector delivery, drawn once per cylinder at reset from the reset seed and never per step. Six bit-identical cylinders sum to a pure harmonic comb the ear hears as synthesised. Far too small to move any calibration result, and a test asserts peak power and torque are unchanged by it |
| **Cycle-to-cycle scatter** | ±2% delivered fuel, drawn once per cylinder *per cycle* at intake valve closing from the same seeded stream. Build scatter makes the cylinders differ from each other; this makes a cylinder differ from its own last cycle, without which the engine is a six-event loop on repeat. Its effect is honestly modest — see **Modelling simplifications** — and it is held to the same no-calibration-change standard as the build spreads |
| **Cockpit listening stage** | 30 Hz high pass; +5 dB low shelf at 150 Hz; +2.5 dB at 200 Hz, Q 0.9; −2 dB at 600 Hz, Q 1.0; 2.6 kHz low pass; reflections at 7.3 ms (−11 dB, left) and 11.9 ms (−13 dB, right); 180 ms seeded impulse response decaying over 130 ms after 6 ms predelay; compressor at −18 dB, ratio 2, 25/180 ms, trimmed 0.42 in and 1.55 out. Presentation only — downstream of everything, changes no state, bypassable |
| **Body and mount response** | Four modes at 60, 95, 150 and 220 Hz at Q 4.0, 4.0, 4.5 and 5.0, weighted 1.0, 1.0, 0.8 and 0.5, driven by gas plus pumping torque normalised by rated torque. Q in the single figures because a trimmed cab panel is heavily damped and a sharp bank here jumps in level as the firing frequency sweeps past each mode. This is a *vehicle* response excited by the engine, not an engine property; the source is an engine manual and publishes nothing about either |

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
- **Audio covers the exhaust, the engine's structure and its reaction against
  its mounts, not everything that radiates.** There is no turbocharger whine and
  no intake noise. Blade-pass at the modelled shaft speeds is ultrasonic, so
  audible intake content would be low-order shaft harmonics and diffuser noise —
  the least grounded thing on the list, and deliberately excluded.
- **The body path is a lumped vehicle response, not a structural model.** Four
  damped resonators stand for everything between the engine mounts and the cab
  panels. The forcing is engine state and genuinely belongs in `sim-core`; the
  frame and cab are the *vehicle*, and about those the source manual says nothing
  at all, exactly as it says nothing about the driveline that is modelled on the
  same footing.
- **Cycle-to-cycle variation is physically right and acoustically minor.** It is
  measurable — a cylinder's pulse height varies 1.5 times more with it than
  without, at idle — but it is not what fixed the sound, and saying otherwise
  would be overselling it. Two reasons. The acoustic source is the mass flow
  through the port, and by exhaust valve opening 140° after top dead centre the
  cylinder has expanded far enough that trapped mass, not the last 2% of fuel, is
  setting the pressure. And at full load the smoke limit clamps commanded fuel to
  the trapped air, which erases the trim entirely: measured at full pedal the
  variation is 1.00 times, exactly none. That load dependence matches published
  behaviour — a few percent at light load, under one at high — and it emerges
  from a limiter that is there for another reason rather than from a schedule.
- **The three paths are summed in the solver, not at the listener.** The whole
  chain from the ring buffer to the worklet is mono, and the soft clipper acts on
  the mix. Filtering each path with its own cab transfer would be more correct —
  the exhaust reaches a driver from a stack behind and below, the block through
  the bulkhead, the body through the seat — and is the obvious next milestone. It
  is not done here because it moves the clipper downstream of the mix, which is
  what the "one sample per solver step, finite and inside `[−1, 1]`" criteria are
  written against.
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
  a reset click-free; the note at the firing frequency for any cylinder count; a
  standing torque and a standing cylinder pressure both radiating nothing; the
  figures in **Results → Sound** met.

**The audio criteria were rewritten, and the old ones are recorded here because
they are the reason this shipped sounding wrong.** They were:

| | Old | New |
|---|---|---|
| firing orders `f0…4·f0` | — | **≥ 35%** loaded |
| crest factor | — | **≥ 9 dB** |
| exhaust over block | — | **≥ 6 dB** |
| `80–300 Hz` | ≤ 55% | **≥ 45%** |
| `300 Hz–2 kHz` | — | **≤ 30%** |
| `150 Hz–15 kHz` | ≥ 60% | **≥ 35%** |
| `<80 Hz` | ≤ 35% | ≤ 35%, unchanged |
| `>15 kHz` residue | reported only | **≤ 1%** |

The old set was written to answer one question — *can an ordinary speaker
reproduce any of this* — and was then read as though it described what an engine
sounds like. It does not. A `150 Hz–15 kHz ≥ 60%` floor cannot be met by anything
that sounds heavy, so it was not a criterion the model happened to fail: it was a
criterion enforcing the defect. `<80 Hz ≤ 35%` is a ceiling with no matching
floor, and nothing anywhere asked where the firing orders were, which of the
paths was in front, or whether the output was a pulse train or a tone.

The audibility question is still asked, as its own floor, set low enough to be
about audibility rather than about balance. What is new is everything else.
Chasing a single number is what produced the fault; the replacements are
deliberately a set that cannot all be satisfied by pushing energy in one
direction.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # 257 tests: geometry, provenance, catalog,
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
`--example audio_probe` for levels, where their energy sits, how much of it is in
the firing orders, whether it is a pulse train or a tone, and which of the three
paths is in front.

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
