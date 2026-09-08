# Diesel HiFi

A browser-based, real-time **diesel truck** engine simulator. The deterministic
simulation core is Rust, compiled both natively and to WebAssembly, presented
through a static Astro application with an interactive Svelte simulator and
Tailwind CSS. There is no backend.

The built-in engine is a public-data reference model of the 2011 Mercedes-Benz
OM 471.9 M3D.

> **Not an OEM-certified digital twin.** A public-spec reference simulation built
> from published service literature. Mercedes-Benz and OM 471 are factual
> references only; no endorsement is implied and no OEM logos or assets are used.
> Every configuration value is labelled `published`, `derived`, or `calibrated`
> in the configuration. Displayed specifications retain their labels; full
> provenance is documented here and in the configuration. Estimates are never OEM data.

This file is the whole specification and the whole documentation: requirements,
acceptance criteria, results, and every assumption the model makes.

## Scope

**In.** One diesel configuration behind a stable selection API;
crank-angle-resolved four-stroke state; injection, ignition delay, heat release,
cylinder pressure, crank dynamics, friction, load, idle control; a wastegate
turbocharger and cooled EGR; the staged decompression engine brake; a rigid truck
driveline; a series-wound starter motor geared to the flywheel; four-path engine
audio; a tested metric library, named reproducible
listening scenarios and WAV capture; native deterministic tests; a responsive
Astro/Svelte UI with English and Simplified Chinese, driver controls, cockpit/raw
listening, and engine telemetry.

**Out.** Gasoline engines or any multi-fuel abstraction. Engineering
certification, emissions prediction, ECU reproduction, OEM map reverse
engineering. Backend, SSR, accounts, databases, telemetry collection, runtime
network calls. CFD, finite-element mechanics, injector hydraulics, chemical
kinetics, aftertreatment chemistry.

## Architecture

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, air path, engine brake, driveline, all four acoustic sources and the exhaust duct, the starter motor, dynamometer harness, snapshots, native tests. **No browser, DOM, audio-device, or filesystem dependency.** |
| `crates/sim-core/src/sim/starter.rs` | The starter as a machine: a series-wound DC circuit solved in closed form, a rotor with inertia, a compliant one-way drive, two-stage engagement and a latching relay. |
| `crates/sim-core/src/analysis.rs` | The measuring instruments: transform, band shares, comb share, crest factor, modulation depth. Pure, out of the hot loop, and tested against signals whose answer is known. Shared by the probes and the acoustic tests so a metric has one definition. |
| `crates/sim-core/src/scenario.rs` | Named, reproducible listening scenarios: a seed, initial conditions and a script of control phases, returning audio plus the conditions the engine actually reached. A measurement harness like `dyno.rs`, and like it it touches nothing outside the simulation. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src/pages/index.astro` | Static document, metadata, build-time drawing geometry, local stylesheet and client-only simulator island. |
| `web/src` | Svelte controls, engine telemetry, local English/zh-CN messages, Tailwind styles and Web Audio orchestration. |
| `web/src/audio` | `AudioWorklet` processor: ring of interleaved frames, resampler, underrun accounting. Dependency-free plain JavaScript. |
| `web/src/lib/cabin.ts` | Cockpit listening stage as data: per-path transfers, shared filter table, reflection taps, seeded impulse response. Pure and Web-Audio-free, unit-tested under Node. |
| `web/src/lib/compare.ts` | Development comparison arithmetic, retained for audio tooling and unit tests. Pure and Web-Audio-free; no production comparison UI. |
| `scripts/verify-dist.mjs` | Static-build verification. |

The production artifact is `web/dist` and must run from a domain root or a
configurable subpath on ordinary static hosting. Use pnpm, TypeScript, Astro,
Svelte and Tailwind CSS. Astro uses Vite and emits static output; no server adapter,
SSR at deployment, SvelteKit or Node runtime ships. All code, WASM, configuration,
fonts and assets are local to the build. Node.js 22.12+ is required for development.

## Configuration and provenance

`EngineConfig` is versioned (schema 8) and validated before any simulation state
exists. It carries identity, geometry, valvetrain, injection, combustion,
friction, gas, air path, turbo, EGR, exhaust system, engine brake, driveline,
governor, load, starter, inertia, limits, solver and audio sections, plus source
records and parameter-level provenance.

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
- Gas, pumping, friction, accessory, driveline and starter torques are explicit
  terms. The governor requests fuel. Starter crank torque derives from the motor
  circuit and gear ratio.
- 23 MPa is a validation envelope, not a pressure target.
- The 2400 rpm model ceiling is separate from the 2100 rpm fuel cutoff. Road
  torque can accelerate the engine with no fuel. An accelerating ceiling crossing
  pauses the simulation; it does not add braking torque or simulate engine damage.
- Fixed-step deterministic integration. Non-finite state and out-of-range
  configuration are rejected.
- Any randomness is seeded explicitly.
- No heap allocation, blocking, logging, DOM access or I/O in the hot step loop.

## WASM and worker API

Versioned and deliberately coarse: list configuration summaries, return the
active ID, select by ID, reset with an explicit seed and initial conditions,
submit controls, advance many fixed steps, return one compact snapshot. Audio
crosses separately from the snapshot, as **interleaved frames** — one frame per
solver step, `audioPathCount()` floats to a frame — because a batch carries
thousands of them and a snapshot has to stay compact. Errors
are structured for invalid IDs, inputs and numerical state. Memory ownership is
explicit and there are no per-step JS/WASM calls. The module runs in a dedicated
Web Worker and needs neither `SharedArrayBuffer` nor cross-origin isolation.

`SPEED_LIMIT_EXCEEDED` is a recoverable pause, distinct from `NON_FINITE_STATE`.
The crossing step completes, keeping pressure, angle, time and audio consistent;
its finite speed overshoot is retained. Repeating unchanged controls keeps the
pause latched. Changed, validated controls clear only this pause, allowing
deceleration from the preserved state; renewed acceleration above the ceiling
pauses again. Reset speeds above the configured ceiling are rejected before
state changes. Numerical and pressure faults still require a reset.

The browser shows **Engine overspeed** and asks whether to reset road grade to
0% and resume. **Reset grade & resume** applies that change and continues in one
click, preserving the selected gear, other controls and elapsed simulation time.
The grade stays unchanged until the user confirms. Neutral can pause again as
compressed cylinders expand without the truck's reflected inertia. Start remains
disabled while paused; resuming neither engages the starter nor resets the engine. Unplayed samples
from the interrupted batch are discarded before playback resumes.

## Frontend workflow

The application title is **Diesel HiFi** in both languages. A compact toolbar
contains the app name, Drive/Sound/Telemetry navigation, status and language;
there are no promotional headings. Numeric readouts update at 10 Hz, using a
100 ms average for RPM, speed, torque, power and other fluctuating readings. RPM
is rounded to 10 rpm. Pressure peaks retain their actual values; cylinder bars
show the latest sampled pressure. Faults, stops and resets bypass averaging.
Display smoothing never changes the solver state, audio or control decisions. The default **Drive**
view puts RPM, road speed, whole-cycle torque/power, start/stop, accelerator,
release and reset first. One control panel groups the accelerator, gearshift,
engine brake, external resisting load (0–5000 N·m), and road grade. Gear selection
connects the engine to the truck. Controls report when
engine braking is inhibited. The catalog API supplies engine choices and summary
specifications, including the initial ID `mercedes-benz-om471-9-m3d-375kw`.
Selecting any engine resets the session and flushes buffered sound.

A diesel-engine cutaway sits directly below the Drive dial. Pistons, connecting
rods, crank throws and valve movement have a **Slow motion** switch: on (the
default) shows one tenth of engine speed; off shows full engine speed. Switching
preserves animation phase and affects only the drawing. The setting persists
across views, languages and session resets until page reload. Reduced-motion
preferences freeze the drawing and disable the switch. Full-speed stroke
visibility depends on the display refresh rate. Cylinder
count, firing order and rod/stroke ratio come from the engine configuration,
projected by Astro at build time. The illustration is schematic, with simplified
valve timing and power-stroke shading rather than measured geometry or pressure.
It follows live RPM independently of numeric sampling and freezes at rest, on a
fault, offscreen, in a background tab or when reduced motion is requested. The
dial marker and illuminated ticks follow every incoming snapshot without added
smoothing or transition delay. The marker rotates at a fixed radius around the
dial center, remaining on the rail through acceleration, deceleration and reset.

**Sound** exclusively contains the cockpit/raw listening position and volume.
Drive contains no sound panel. Sound is enabled by default: **Start engine** opens the audio
context before cranking, using that button gesture to meet browser playback rules.
There is no separate enable switch; volume zero silences output. Audio does not
autoplay on page load. Listening settings and simulation state persist across
views and languages. Failed startup or device suspension shows **Resume sound**;
failed contexts are closed before retrying. Engine stop retains the audible coast-down.

**Telemetry** contains engine telemetry only: cycle averages, air path, starter
current/voltage/engagement, driveline/brake values, cylinder pressure and elapsed
time. Groups expand on demand. Production contains no sweep, provenance browser,
spectrum, source-isolation, file-comparison or EGR test controls.
The core, WASM APIs, measurement examples and development audio libraries retain
those capabilities for testing. `audioPathCount()` remains four; all source paths
remain enabled in production playback.

Layouts support phone and desktop browsers, with bottom navigation on narrow
screens, touch controls at least 44 CSS pixels high, visible keyboard focus and
reduced-motion support. Phone engine specifications expand on demand. The RPM
readout stays visible below 768 CSS pixels: a compact strip with RPM and engine
status docks to the top as the original number reaches it while scrolling.
The strip is 64 CSS pixels high plus the device's top safe area. It shares the
same live values, works in all three views, and disappears when the original
readout returns or the layout switches to desktop. Docking does not shift page
content. On phones, driving controls live in a bottom drawer above the 80-pixel
navigation bar. It starts folded; tapping its heading opens or folds it, and
Escape folds it and returns focus to the heading. The contents scroll within
the drawer. Opening compacts the main speed panel and sizes the drawer to keep
that readout visible above it. The top strip appears only after scrolling the
original number out of view, so opening controls does not duplicate the speed
panel. Short landscape screens scroll to this handoff when the available control
space would be less than 160 pixels. Switching views folds the drawer while
retaining inputs; desktop keeps the full control panel. A contrasting drawer
heading and solid neutral disabled buttons keep labels readable.
The drawer expands/folds over 300 ms and the speed strip slides/fades over
200 ms. The main speed panel also animates between its dial and compact layout
over 300 ms when opening the mobile drawer or switching views. The phone dial
is capped at 260 CSS pixels wide; desktop uses up to 480 pixels. Reduced-motion
preferences disable these transitions. Closed controls
are excluded from keyboard interaction. Device safe areas and page padding
keep the folded drawer and navigation clear of the content. The RPM
dial uses a presentation scale of 0–2500 rpm; it does not redefine the solver's
2400 rpm limit. The current UI offers neutral plus 12 gears and ±15% grade,
because the catalog summary does not expose gearbox/control limits.

English and Simplified Chinese (`zh-CN`) messages ship locally. A saved language
wins; otherwise browser preferences select English or Simplified Chinese
(`zh`, `zh-CN`, `zh-SG`, `zh-Hans`), with English fallback. Selection updates
document language and saves only the locale in local storage; blocked storage
falls back without breaking the simulator. The phone Specs button shares the
engine selector's row and height. Language and engine choices use
styled listboxes with selection marks, arrow/Home/End navigation, Enter/Space
selection, Escape cancellation and outside dismissal. Model identifiers and SI
unit symbols remain unchanged; the engine menu presents the English `Reference`
suffix as a separate localized model description. Reload resets the simulation;
it is not a saved session.

Responsive behavior is tested in Chromium. Real iOS/Android audio interruptions,
headphone routing and sustained device throughput still require hardware testing.

## Results

### Calibration

Engine, brake, acoustic and starter measurements were regenerated on 2026-09-08
with ring leakage enabled.

| Quantity | Model | Published target | Error |
|---|---:|---:|---:|
| Peak power | 376.9 kW at 1800 rpm | 375 kW | +0.5% |
| Peak torque | 2545.1 N·m at 1000 rpm | 2500 N·m | +1.8% |
| Maximum cylinder pressure | 20.18 MPa | 23 MPa envelope | 12% margin |
| Best fuel consumption | 180.6 g/kW·h | not published | — |
| M5U stage III brake at 1300 rpm | 108.6 kW | 100 kW | +8.6% |
| M5U stage III brake at 2300 rpm | 274.3 kW | 300 kW | −8.6% |

The speeds at the fuelled peaks and the consumption are model outcomes, not OEM
specifications. Fuelled peak magnitudes target ±3%; brake anchors target ±10%.
The manual does not identify the stage for its maximum brake powers; treating
them as stage III is an interpretation. At 1300 rpm, the brake sweep measures
47.6 kW stage I, 71.6 kW stage II and 108.6 kW stage III.

### Sound

Four aligned paths are generated at the 40 kHz solver rate:

| Path | Drive | Transfer in the core |
|---|---|---|
| Exhaust | Settled port mass flows | Runner delays, turbine attenuation, lossy exhaust waveguide, mouth radiation and 20 Hz high pass |
| Block | First difference of ambient-normalised summed cylinder pressure | Six structural resonators |
| Body | Gas/pumping torque reaction plus cylinder-weighted pressure rise | Five damped body/mount resonators |
| Starter | Loaded tooth contact, seating/retraction impact and integrated rotor motion | Four starter/housing resonators |

One common limiter gain bounds both the sum and every individual path to the
0.85 knee. Attack is instantaneous and release is 0.3 s. The final ±1 channel
clamps are safety guards; none engages in the 13 captured scenarios. The
recorded gain recovers the pre-limiter paths within floating-point rounding,
including when channels cancel in the sum.

Measured 2026-09-08 using `audio_capture` and the shipped schema-8 configuration,
seed 0. These are raw dBFS and spectrum measurements, not measured vehicle SPL.

| Scenario | RPM | dBFS | Crest, dB | Orders 1–4 | Below 80 Hz | 80–300 Hz | 300 Hz–2 kHz |
|---|---:|---:|---:|---:|---:|---:|---:|
| `idle` | 551.3 | -31.5 | 15.15 | 54.2% | 48.1% | 38.8% | 12.9% |
| `light-900` | 900.0 | -18.2 | 12.26 | 73.4% | 5.5% | 79.7% | 14.7% |
| `cruise-1200` | 1200.0 | -13.2 | 11.70 | 70.1% | 5.6% | 68.7% | 25.6% |
| `full-1400` | 1400.0 | -14.1 | 11.44 | 89.3% | 0.1% | 92.3% | 7.6% |
| `rated-1800` | 1800.0 | -11.0 | 9.12 | 87.9% | 0.1% | 87.9% | 12.0% |
| `brake-1300` | 1300.0 | -13.8 | 12.38 | 44.2% | 2.2% | 42.9% | 54.9% |

Starter output is exactly silent in these scenarios because its rotor and
housing have never been excited. The idle and brake band exceptions are listed
under **Known deficits**; the existing acceptance thresholds remain visible.

#### Body response

The body modes are 60, 100, 160, 240 and 340 Hz, Q 2–2.5. The drive is

```text
torque_fraction = (torque_gas_nm + torque_pumping_nm) / rated_torque_nm
body_pressure = sum(cylinder_pressure_pa[i] * pressure_weight[i]) / ambient_pa
body_drive = torque_fraction + body_pressure_rate_gain_s * delta(body_pressure) / dt
```

The pressure-rate gain is 0.000016 s; weights by cylinder index are
`[1, 0.35, 0.7, 0.5, 0.25, 0.9]`. This is a calibrated approximation of local
pressure excitation and body radiation, not measured mount geometry. It is
audio only: changing it leaves crank motion, pressure and starter dynamics
identical. Initial filter histories suppress a reset impulse; constant pressure
and torque radiate nothing. The torque normalisation is linear and unclamped.

| Scenario | Body dBFS | Body below 80 Hz | Body at 150 Hz–15 kHz | Exhaust above body |
|---|---:|---:|---:|---:|
| `idle` | -34.0 | 96.1% | 0.66% | -0.4 dB |
| `light-900` | -24.4 | 74.4% | 5.97% | 5.2 dB |
| `cruise-1200` | -17.3 | 83.9% | 4.18% | 3.3 dB |
| `full-1400` | -21.2 | 83.2% | 2.72% | 8.7 dB |
| `rated-1800` | -19.8 | 0.6% | 16.00% | 8.8 dB |
| `brake-1300` | -44.0 | 59.2% | 23.54% | 30.1 dB |

Against the saved review baseline, body energy at 80–300 Hz is 7.5 dB higher at
cruise and 4.1 dB higher at full load. These are band-level comparisons at the
same scripted conditions, including each version's limiter gain. Listening
validation against the truck reference and the user's playback system remains
necessary. The block retains its separate 210/380/750 Hz response and 0.20 gain.

#### What the traces contain

Intake and exhaust gas exchange use orifice flow; the cylinder pressure is not
assigned to manifold pressure at their phase boundaries. The forcing API exposes
the values passed to the acoustic stage. `trace_probe` attributes sharp
changes to valve events and also runs a blind isolation detector.

Use both results: isolation can miss a step on a steep slope and can flag noise
or near-equilibrium port chatter. Interpolating a trace in the probe is a
diagnostic counterfactual, not a solver fix. The focused acoustic suite checks
pressure continuity at gas-exchange transitions.

### The starter

The reference is Bosch HEF109-M, 24 V, 7.8 kW, 12-tooth pinion, part
0 001 330 050. The 145-tooth ring gear, electrical curve, rotor inertia,
drive compliance and sound parameters are calibrated estimates.

The motor drives the pinion, the pinion drives the flywheel ring gear, and the
resulting crank motion compresses and pumps the engine's cylinders. Those
resolved pressures and flows generate the engine sound even with ignition off.
The starter does not substitute a recording or an audio oscillator for engine
motion.

The saturating series-field circuit solves current from rotor speed. A rotor
inertia of 0.004 kg·m² integrates motor torque and drag. The seated drive uses
500 N·m/rad stiffness and 1 N·m/(rad/s) damping, integrated implicitly at the
rotor; positive drive torque reaches the crank through the 145/12 ratio and 0.9
efficiency. The one-way clutch releases instead of back-driving the armature.
Elastic energy discarded when that clutch opens is dissipated in the drive.
The synthetic four-cylinder fixture uses 0.0016 kg·m² rotor inertia. Its running
acceptance test starts unloaded and applies 200 N·m after exceeding 600 rpm.

Seating takes 8 ms. Main current starts ramping 60 ms after the key request and
ramps over 8 ms. The relay latches out at 420 rpm and rearms when the key is
released. After retraction, motor drag produces a coast-down. Sound follows
integrated rotor angle at 24 calibrated commutator contacts per revolution,
weighted by electromagnetic torque and rotational drag. Its relative gain is
0.12. Unresolved motor orders fade before Nyquist. Below 1e-6 rad/s, an unpowered
rotor is set to rest to terminate an inaudible numerical decay.

Total circuit resistance is 0.01263 Ω, of which 0.0035 Ω represents the battery
and cables. Motor terminal voltage is `24 - current_a * 0.0035`; the remaining
resistance is inside the motor. This follows the series-motor voltage balance
described by [MathWorks](https://www.mathworks.com/help/sps/ref/universalmotor.html).

| Measurement | Result | Conditions |
|---|---:|---|
| Peak shaft output | 7.80 kW at 149 equivalent crank rpm | Unchanged nominal-power anchor |
| Stall | 1900 A, 964 N·m at crank, 17.3 V at motor terminals | Calibrated steady circuit |
| Ignition-off crank speed | 257 rpm mean; sampled range 231–279 rpm | Last third of the 1 s sweep, sampled every 25 ms |
| End-of-run current and crank torque | 885 A, 57 N·m | One snapshot; 20.9 V terminals |
| Mesh frequency at sampled mean | 621 Hz | 145 teeth × crank rpm / 60 |
| First combustion in `start` | 0.39085 s | First positive heat release, 25 µs resolution |
| Pinion fully retracted in `start` | 0.6200 s | Observed at a 2.5 ms capture boundary |
| Engine speed after 5 s | 553 rpm | Instantaneous governed speed; target 560 rpm |

The unfuelled `crank` capture at 0.25–1.20 s measures starter −36.8 dBFS,
exhaust −43.4, body −43.6 and block −73.5. No combustion occurs. In the fuelled
`start` window at 0.10–0.58 s, starter/exhaust/body/block measure
−33.2/−37.8/−41.5/−48.7 dBFS. These windows have different operating conditions.
At 0.65–0.75 s the released starter remains audible at −57.6 dBFS from motor
coast-down and housing decay; retraction no longer means immediate silence.

Ring leakage uses 0.10 mm² effective area per cylinder, including discharge loss.
`air_path.ring_leakage_area_m2` is a calibrated estimate, not an OEM clearance
or leak-down measurement. The compressible orifice law exchanges mass and
enthalpy with a vented crankcase at the configured crankcase pressure and ambient
temperature. It runs at every speed, including rest, and also updates the
motored reference. Leakage does not contribute to exhaust-port flow.

In `restart`, the crank stops with compression remaining. After the retry at
1.0 s, first combustion occurs at 1.527825 s and the pinion retracts at 1.6750 s.
The engine reaches 656 rpm at 4 s with zero starter current. Fifteen combinations
of unfuelled crank duration (0.15–0.85 s) and pause (0.1–2.0 s), plus three
successive running/shutdown/restart cycles, settle within 500–620 rpm after
six seconds without starter assistance. Zero leakage reproduces the permanent
compression stall; a 2000 N·m external load still stalls the normal model.

The ring-pack flow mechanism is supported by
[Antony et al.](https://arxiv.org/abs/2406.02702); that study does not establish
this engine’s leakage area. Reverse crank rocking, ring motion, wear and crankcase
thermal/pressure dynamics remain outside this lumped model. It transfers gas
enthalpy; unburnt fuel transport and oil dilution are not resolved.

### Known deficits

- **Starter calibration:** matching peak power does not validate the complete
  current/torque curve, cranking speed or timbre. Inductance, battery state of
  charge, temperature, Coulomb brush friction and detailed contact mechanics are
  omitted. Coolant starts at 293.15 K, friction has no temperature input and the
  wall stays at 480 K; these are not validated cold starts.
- **Idle and brake bands:** idle has 48.2% below 80 Hz and 38.8% at 80–300 Hz;
  brake has 42.9% at 80–300 Hz and 54.9% at 300 Hz–2 kHz. These miss the retained
  35% ceiling, 45% floor and 30% ceiling where applicable. Body bass still
  concentrates below 80 Hz at idle and the subjective result needs listening.
- **Other retained limitations:** a loaded air-path limit cycle was previously
  recorded near 1900 rpm, with boost about 25–155 kPa. Consumption remains a
  calibration estimate. No OEM recording validates the isolated source gains.
- **Playback limits:** offline exports share the cockpit graph but use browser
  resampling, not the live worklet's interpolation, buffering and clock trim.
  Passing numerical checks does not establish the response of the user's speakers.

## Further sound improvements

The implemented foundation covers path headroom, broader body response,
pressure-driven compression texture, independent starter rotation, terminal
voltage, ring leakage and repeatable raw/cockpit exports. Requirements,
measurements and remaining assumptions live here.

Next, compare the exported cranking and running passages at matched level with
the recorded truck reference. Tune motor/mesh balance and body modes against
that evidence. Obtain leak-down, voltage/current and cranking-speed measurements
to validate restart timing before changing
the torque curve or adding cold-friction multipliers. Keep device compensation
separate from engine physics. Injection shaping, intake/turbo sound and
driveline clutch dynamics remain later work.

## Measurement and listening workflow

### Reproducible scenarios

`sim-core::scenario` defines reset state, seed, scripted controls and capture
windows. Held phases pin speed every 100 steps (2.5 ms); free phases integrate
crank dynamics. Idle is governed. Captures exclude settle phases and retain
actual operating conditions. Firing-order and modulation metrics are null for
transients because their speed changes within the capture.

| Scenario | Conditions |
|---|---|
| `idle` | No pedal/load; 4 s settle, 1.2 s capture, free governed speed |
| `light-900` | 25% pedal, held 900 rpm |
| `cruise-1200` | 60% pedal, held 1200 rpm |
| `full-1400` | Full pedal, held 1400 rpm |
| `rated-1800` | Full pedal, held 1800 rpm |
| `crank` | Rest; ignition disabled throughout, 1.2 s motoring and 0.8 s key release |
| `crank-release` | Rest; 0.15 s unfuelled cranking and 0.85 s release |
| `restart` | Rest; 0.35 s unfuelled cranking, 0.65 s release, 1.2 s fuelled retry and 1.8 s released |
| `start` | Rest; starter requested 1.2 s with ignition on and no pedal, then 2.8 s released |
| `stop` | Idle, then ignition off; 3 s capture |
| `accelerate` | Sixth gear, part to full pedal; 5 s capture |
| `release` | Ninth gear on a 3% climb, pedal released; 3 s capture |
| `brake-1300` | Held 1300 rpm, stage III decompression brake |

### Capturing scenarios as files

`audio_capture` exports 40 kHz IEEE-float WAVs into
`target/audio-capture/<scenario>/`. The root manifest records the exact config
checksum (FNV-1a-64), config schema, seed, controls, measured phase statistics,
events and track metrics; `config.json` preserves the configuration bytes.
An optional `--config <path>` selects another validated configuration.

| File | Contents |
|---|---|
| `paths.wav` | All four paths interleaved from the same run, including silent channels |
| `mix.wav` | Raw sum before device resampling |
| `exhaust.wav`, `block.wav`, `body.wav`, `starter.wav` | Individual paths; exact-silent paths omitted and listed in `silentPaths` |
| `*-unsaturated.wav`, `mix-unsaturated.wav` | Recorded common gain divided out; written where the limiter engages |
| `raw-48000.wav`, `cockpit-48000.wav` | Stereo comparison exported with `pnpm audio:render <capture directory>` |
| `render-48000.json` | Input and graph SHA-256, cabin specification, browser version, rates, common gain, peaks and RMS |

The development-only renderer uses Chromium's `OfflineAudioContext` and the
shipped `buildStageGraph`. It includes the room tail and applies one shared
headroom trim to both stages if needed. It excludes master volume and live
worklet scheduling. Pass `44100` as the last argument for 44.1 kHz. Installed
Playwright Chromium is required; no export service ships in the static site.

Starter events and phase electrical statistics are observed every 2.5 ms;
first positive combustion heat is recorded at the 25 µs solver step. Event
frames refer to boundaries in concatenated captured audio, or null for an
excluded settling phase. No event is inferred from the scripted phase name.
Artifacts are gitignored; the current set is in
`target/audio-implementation-2026-09-08/`.

### Measuring the sound

`analysis.rs` supplies RMS, peak, crest, spectrum/band shares, firing-order
share, analytic-envelope modulation, firing-rate estimation and jump detection.
The metric tests use signals with known answers: a steady tone has 3 dB crest
and zero modulation; a 50% AM tone has modulation 0.5; pulse trains and noise
exercise distinct spectral/envelope properties. Report near-Nyquist residue
separately from the 150 Hz–15 kHz playback-oriented band.

`reference_probe` estimates speed from a WAV's harmonic spacing and reports
confidence and windows in `target/reference-annotation/annotation.json`.
Inferred RPM and unknown load must stay labelled. A lone tone, a half-order or
sample looping can mislead the estimator. Use known-speed simulator captures as
controls, and do not assign steady metrics to a changing-speed passage.

### Playback audit and comparison

The worklet buffers aligned frames and resamples 40 kHz to the device rate.
Tests exercise 44.1 and 48 kHz, pitch, alignment, imaging below −38 dBc in the
tested band, fades around underruns, whole-frame overruns, malformed-block
rejection, flush and drift correction bounded to ±1%. Underruns output silence
and are counted. This is not a sustained real-time throughput test.

Production provides listening position and volume only. Developer comparison
arithmetic and the audio graph retain RMS matching with ±24 dB trim and a
peak-based full-scale limit; unit tests cover clipping and unreachable matches.
Files exported by the capture/render commands can be compared outside the app.
RMS matching is not a perceptual loudness guarantee for different spectra.

### Where you are listening from

Cockpit is the default. “Raw engine mix” sums all four engine paths.
Both graphs always run; switching
crossfades over 40 ms. All cockpit values are calibrated listening assumptions.

| Path | Level | Filters | Delay | Room send |
|---|---:|---|---:|---:|
| Exhaust | 0 dB | 1.6 kHz low pass; −3 dB at 400 Hz | 12 ms | 1.0 |
| Block | −3 dB | 3.2 kHz low pass; +2 dB at 1 kHz | 2 ms | 0.4 |
| Body | +1.5 dB | 450 Hz low pass; +1 dB at 120 Hz | 0 ms | 0 |
| Starter | −1 dB | 3.8 kHz low pass; +2 dB at 1.2 kHz | 3 ms | 0.3 |

After per-path filters/delays, the direct gain is 0.75. Room sends feed taps at
7.3 ms (gain 0.28, pan −0.6) and 11.9 ms (gain 0.22, pan 0.55), through a
1.8 kHz biquad low pass, and a generated stereo tail at gain 0.20. The tail lasts
180 ms with 6 ms predelay; its source noise is band-limited at 5 kHz and its
envelope decays by 60 dB over 130 ms below 900 Hz and 35 ms above it. Explicit
seeds make it repeatable; normalization uses undamped reference energy.

The summed room output receives a 35 Hz high pass, +5 dB low shelf at 150 Hz,
+2.5 dB peak at 200 Hz (Q 0.9), and −2 dB at 600 Hz (Q 1). Compressor settings:
input gain 0.30, threshold −18 dB, knee 12 dB, ratio 2, attack/release 25/180 ms,
makeup gain 2.15. Both stages then receive a common 0.8 playback gain for
filter/reverb headroom; default user volume is 1.0. The 200 Hz boost currently includes
small-speaker compensation in the vehicle response. There is no shared low pass.

The stage adds no independent sound, samples or harmonic generator; silence in
produces silence out. The body route currently represents mount/seat transmission
and has no room send. The browser suite requires cockpit/raw levels within
±3 dB at its idle and loaded points and stronger low/high weighting in the cab.
The ±3 dB requirement applies to those browser test points, not every possible
RPM/load combination. Export metadata records the actual level difference;
compare exported recordings at matched RMS when judging different recordings.
The final browser measurements are +2.6 dB at idle and −2.4 dB at the loaded
test point on both root and subpath builds. Some held scenarios differ more:
the full-1400 cockpit export is 4.4 dB above raw at 48 kHz and 4.8 dB at 44.1 kHz.
Across all 52 stereo exports at 44.1/48 kHz, the largest absolute sample is
0.920573 with the common 0.8 playback gain; no additional export trim is needed.

### Acoustic reference

The existing local benchmark is the Max2712 ETS2 OM471 sound-mod demonstration,
video ID `l3NGWdoA-_I`. Project reference notes identify a later 2018 Arocs
3248 as the sampled engine. Files reside in gitignored
`datasheet/audio-references/max2712-om471/` and are never distributed.

The interior chapter, 1:33–2:49, is 76 s of stereo 48 kHz audio. Previously
measured steady windows at chapter-relative 6.1–9.6, 9.6–15.0 and 64.2–73.0 s
gave inferred idle speeds of 507, 502 and 499 rpm, and 92.1%, 90.6% and 89.1%
below 80 Hz. These are retained reference measurements, not rerun results.
Other interior passages are stationary rev sweeps. The test drive from 6:17 has
ambiguous half-rate content; load and brake events were not established.

This is a pitch-shifted, looped game mix of a later engine with other sound
layers, not calibrated or isolated microphones from this configuration. It can
inform a preferred character but cannot establish OEM band limits, cranking
conditions or per-path balance. Comparing its interior mix with raw simulator
output is also a mismatch of listening positions. No blinded, level-matched
listening validation is recorded.

## Reference engine and sources

Stable ID `mercedes-benz-om471-9-m3d-375kw`, display name *OM 471.9 M3D 375 kW
Reference*. Primary source: **Introduction of engine OM 471 and exhaust
aftertreatment**, Mercedes-Benz service literature, technical status 2011-09-01,
order number 6517 1260 02. Scope: engine series 471.9 in model 963/964, power
code M3D, plus documented M5Z Euro VI subsystems where the document states them.

Second source, for the starter only: the **Bosch parts catalogue entry for
HEF109-M 24 V, part number 0 001 330 050**, plus Bosch Off-Highway product
literature for the HEF/HEP 109 family. Scope: nominal voltage, nominal power,
pinion tooth count, flange diameter, rotation, and the two-stage engagement
sequence. It publishes **no torque-speed curve, no circuit resistance and no
current figures**, so every electrical parameter in the `starter` section is
calibrated rather than read — and neither source publishes the engine-side ring
gear, which is the dominant lever on both cranking speed and the mesh frequency.

The two documents do not overlap. The engine manual says nothing about the
starter and the starter catalogue says nothing about the engine, so nothing in
the `starter` section can be cross-checked against anything in the rest of the
configuration.

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

The configuration contains parameter-level purpose, safe range and provenance;
this document and the configuration expose them. Unless identified as published in **Reference engine and
sources**, these quantities are calibrated or derived. Acoustic and cockpit
assumptions are specified above and below.

| Assumption | Current value or model |
|---|---|
| Firing order | 1-5-3-6-2-4 |
| Rotating inertia | 3.5 kg·m²; complete engine mass is metadata only |
| Valve events | Intake closes 40° ABDC; exhaust opens 140° ATDC; 35° ramps |
| Effective port areas | 0.0025 m² per cylinder on intake and exhaust, including discharge coefficient |
| Trapping | Integrated port flow; 0.92 volumetric efficiency applies only to mean manifold flow |
| Boost targets | 1.28–3.08 bar absolute against speed; wastegate regulation, not assigned pressure |
| Turbo | 100 mm compressor wheel; efficiencies 0.72/0.70; inertia 3.5e-5 kg·m²; turbine/wastegate areas 6.0e-4/2.5e-3 m² |
| Manifolds and cooler | Intake/exhaust volumes 0.020/0.010 m³; charge-air cooler effectiveness 0.80 against 85 °C coolant |
| Exhaust gas restriction | 120 kPa per (kg/s)² |
| EGR | Scheduled 0.04–0.14; ceiling 0.35; valve 4.0e-4 m², discharge coefficient 0.70 |
| Brake cam | Charge centre 128° BTDC, release centre 12° BTDC; half-widths 10°; full-lift area 0.0013 m² |
| Brake air path | I/II leave wastegate control alone; III targets 1.85 bar absolute at 0.1 times fuelled loop gains; EGR positioner shut |
| Driveline | 40 t, wheel radius 0.506 m, rolling coefficient 0.006, drag area 6.0 m², final drive 2.61, twelve ratios 14.93–1.00, efficiency 0.95 |
| Injection | 8–15° BTDC schedule; load retard 0.00075 rad/mg; 8 holes ×175 µm, discharge coefficient 0.75; amplified variant above 120 mg/cycle |
| Fuel and smoke | 42.7 MJ/kg, 832 kg/m³, 350 mg/cycle ceiling; minimum air/fuel 19.5, stoichiometric 14.5 |
| Ignition and burning | Hardenberg-Hase delay, cetane 50; double Wiebe, premixed 10°, diffusion 0.0028 rad/mg, premixed fraction capped at 0.85 |
| Ring leakage | 1e-7 m² effective area per cylinder; allowed 0–1e-6 m²; vented crankcase uses configured pressure and ambient temperature |
| Heat and gas | Woschni transfer; fixed wall 480 K; cv(T) = 718 + 0.18(T − 300) J/(kg·K) |
| Friction | FMEP: 60 kPa constant +0.005 × peak pressure +1200 × piston speed +130 × piston speed², SI units; no oil-temperature or breakaway term |
| Governors | PI idle control at published 560 rpm; fuel tapers from 1900 to zero at 2100 rpm |
| Accessories and controls | Accessory torque 25 N·m +0.02 × absolute crank rad/s; external load limited to 5000 N·m; road grade to ±15% |
| Model speed ceiling | 2400 rpm; recoverable pause, separate from the fuel cutoff; no vehicle service brake or overspeed-damage model |
| Starter circuit | 0.01263 Ω total, 0.0035 Ω battery/cable resistance; 9.9752e-5 N·m/A² torque constant; 620 A saturation knee; 0.063 N·m/(rad/s) drag; 0.9 mesh efficiency |
| Starter engagement and contact | 145 ring teeth; 8 ms seating; power starts at 60 ms; 420 rpm release; 0.15 tooth-pitch contact half-width; seating impulse 0.12 |
| Exhaust duct | 3.5 m, area 0.008 m²; mouth reflection −0.8 and radiation corner 2 kHz; 12 L aftertreatment adds 1.5 m equivalent length; transmission 0.61 per traverse; 14 dB turbine loss above 400 Hz; runners 100–700 mm |
| Audio levels | Common gain 1.41; exhaust 9.5, block 0.20, body 0.09, starter 0.25; limiter knee 0.85, release 0.3 s; exhaust high pass 20 Hz |
| Block modes | Frequencies 210/380/750/1400/2400/3600 Hz; Q 9/10/7/8/13/14; weights 1.15/1.3/2.2/1.5/2.0/2.5 |
| Body modes | Frequencies 60/100/160/240/340 Hz; Q 2/2/2.2/2.4/2.5; weights 0.65/1/1.1/0.9/0.5 |
| Body pressure excitation | Rate gain 0.000016 s; cylinder weights 1/0.35/0.7/0.5/0.25/0.9, audio only |
| Starter rotor and drive | 0.004 kg·m² rotor; 500 N·m/rad stiffness, 1 N·m/(rad/s) damping; 24 commutator contacts, relative rotor sound gain 0.12 |
| Starter modes | Frequencies 210/430/850/1750 Hz; Q 4.5/5.5/6/7; weights 1/1/0.55/0.22 |
| Seeded scatter | Per-cylinder exhaust area ±2%, injector delivery ±1.5% at reset; delivered fuel ±2% per cycle before smoke limiting |
| Solver and audio meter | Fixed 25 µs step; audio reference offset 94 dB is an uncalibrated display reference, not measured SPL |

## Modelling simplifications

- Both manifolds use mean speed-density flow rather than sums of cylinder port
  flows. Cylinder gas exchange is resolved but manifold pulsation is not; their
  mass balances are not fully coupled. The compressor map is a simplified ellipse
  and the turbine a fixed effective area. There is no aftertreatment chemistry.
- Thermodynamics are single-zone. No spray, emissions, chemical kinetics,
  temperature-dependent cold-start friction or detailed ring-pack model is provided.
  Fixed wall temperature prevents interpreting the reset as a calibrated cold start.
- EGR is pressure-limited at high load and low speed. Brake regulation uses slower
  gains to limit positive feedback between charging and turbine power.
- Fuel scatter precedes the smoke limiter, which can erase it at full load.
  Integrated trapping introduces residual/cycle coupling even with scatter zero.
- The exhaust duct is one-dimensional; aftertreatment equivalent length is a
  low-frequency approximation. Block/body/starter resonators are lumped transfers,
  not measured structural modes. Summed cylinder pressure uses identical coupling
  for the block; the body uses configured cylinder weights. Intake/turbo sources
  and individual brush impacts are omitted; starter motor orders follow its rotor.
- Starter electrical and mechanical omissions are listed under **The starter**.
  A correctly matched power anchor does not validate current, voltage, clutch
  behaviour or transient motor motion.
- The four channels are mono source paths, not a surround layout. Interleaving
  and a common resampling cursor keep them aligned. Stereo is generated by the
  cockpit reflections and tail.
- The limiter bounds the pre-cockpit sum and each source; filters and delays can change
  cancellation downstream. The cockpit compressor and volume are not proof of
  final output headroom. Offline comparison exports apply a common headroom trim.
- The cabin is estimated and includes speaker compensation. RMS matching can
  differ from perceived loudness. Arithmetic playback tests do not establish
  browser throughput or latency under load.
- The brake enforces released pedal and its published 1000 rpm floor. Clutch and
  ABS conditions from the manual are absent. Rigid driveline coupling has no
  clutch or independent vehicle-speed integration: shifts change road speed
  immediately and neutral disengages rather than coasts. Efficiency applies only
  in traction, slightly overstating overrun retardation.
- Descents that drive the engine past the modelled speed ceiling pause for a
  control change. There is no service-brake control or engine-damage simulation;
  a fuel cutoff alone cannot prevent road-driven overspeed.

## Determinism

Identical configuration, seed, controls and step count produce bit-identical
snapshots and audio; advancing a batch equals advancing its steps individually.
Sweeps hold crank speed deterministically. Invalid/non-finite state and pressure
envelope breaches latch structured faults.
Speed-limit pauses are deterministic too: one completed crossing step produces
one audio frame, regardless of the requested batch size.

Live browser sessions derive step count from elapsed wall time and therefore do
not reproduce identical counts across machines. The worker's `stepOnce` advances
an exact count. Underruns fade to silence rather than repeat old audio.

## Acceptance criteria

These requirements remain in force. Known failures and gaps are listed under
**Known deficits**.

- Deterministic native stepping, snapshots and audio for equal seed/configuration.
- Geometry matches 12.8 L within 0.05 L for six cylinders at 132 ×156 mm.
- Published values and provenance round-trip unchanged; calibrated targets are
  visible in metadata and this document. Nominal pressure stays below 23 MPa.
- Catalog list, active/unknown ID, selection and deterministic reset work.
- Gear 12 on a −15% descent pauses at the model speed ceiling with a specific
  overspeed notice. Confirming **Reset grade & resume** levels the road, preserves
  gear and elapsed time, and permits deceleration. The core keeps the pause latched
  when controls are unchanged.
- WASM loads and advances through a worker; UI stays responsive and selection is
  populated from the catalog. Drive, Sound and Telemetry work at phone
  and desktop sizes in English and zh-CN; language changes preserve a live session. Complete static output loads at root and subpath
  without external runtime requests.
- Exactly one four-path frame per step; finite samples within ±1; summed output and each path
  within the limiter knee; batch invariance in every path; reset silence and no
  reset click; correct firing pitch for the configured cylinder count.
- Constant pressure/torque radiate nothing. A resting, unexcited starter emits
  exact silence. After release, motor rotation and housing ringing may decay;
  pinion retraction stops mesh forcing and torque. Engaged mesh frequency follows
  ring-gear tooth rate, and motor sound follows rotor motion.
- Below-knee limiter transparency; 20× gain on the synthetic pulse test changes
  crest by less than 1 dB; gain swings under 10% in its settled passage. Reported
  gain must recover unlimited output within rounding even when paths cancel.
- Metrics pass known tone/AM/pulse/noise tests; jump detection identifies inserted
  steps at their location and size, and its noise/steep-slope limitations remain
  measured. Forcings equal the actual stage inputs. No gas-exchange pressure
  transition exceeds a tenth of its trace's typical step by the test's metric.
- Pressure held at a stopped crank relaxes by ring mass/enthalpy flow without
  resetting angle or adding torque. Unloaded retries reach running idle; a
  perfectly sealed diagnostic and excessive external load retain genuine stalls.
- Scenarios reproduce samples and recorded conditions, exclude settling from
  captures, hold where requested and drain without losing frames.
- Worklet pitch/alignment, tested imaging below −38 dBc, counted silent underruns,
  whole-frame overruns, malformed-block rejection and ±1% bounded drift at both
  44.1 and 48 kHz.
- Comparison trim is bounded and cannot clip a file; an unreachable match is
  reported; one comparison source plays at a time. Cockpit/raw levels remain
  within ±3 dB at the browser test points and cockpit low/high weighting is at
  least 1.5 times raw in its loaded test.

| Existing raw audio criterion | Bound |
|---|---:|
| Firing orders 1–4, loaded | ≥35% |
| Crest | ≥9 dB |
| Firing-rate modulation | >0 |
| Exhaust above block | ≥6 dB |
| 80–300 Hz | ≥45% |
| 300 Hz–2 kHz | ≤30% |
| 150 Hz–15 kHz | ≥35% |
| Below 80 Hz | ≤35% |
| Above 15 kHz residue | ≤1% |

These bands are diagnostic targets, not established OEM sound specifications.
Passing them alone does not establish fidelity. Idle and brake exceptions remain
visible; transient windows require separate interpretation.

## Commands

Run relevant checks for the changed areas. Documentation-only edits do not need
a full test run. Never report an unrun or failed check as passed.

```text
pnpm install --frozen-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p sim-core --test acoustics
pnpm wasm:build
pnpm wasm:test
pnpm check
pnpm test:unit
pnpm test
pnpm build
pnpm build:subpath
pnpm dev
pnpm preview
```

`pnpm test` builds WASM and runs WASM, unit and browser tests. Install Chromium
once with `pnpm --filter web exec playwright install chromium`.
Generated WASM is under `web/src/wasm/`; static output is `web/dist`.
Astro builds for `/` by default; `VITE_BASE` (retained for compatibility) or
Astro's `--base` selects an absolute deployment prefix. The subpath build verifies
`/diesel-hifi/`. Rebuild with the matching base when changing the hosting prefix.
The browser test servers use Astro's programmatic API so Playwright owns their
lifetime even in environments where the Astro CLI starts a background process.

GitHub Pages publishes through the manually dispatched **Pages** workflow on
`main`. It uses Node.js 22, runs frontend checks, and builds with the Pages base
path before uploading `web/dist`. Pages must use GitHub Actions as its source;
private repositories need an account plan that supports GitHub Pages.
After pushing the intended commit, run `gh workflow run pages.yml --ref main`
and inspect its result with `gh run view`.

Focused selection, mobile drawer, scrolling and overspeed browser checks:

```text
pnpm --filter web exec playwright test e2e/slice.spec.ts --grep "selection menus|mobile speed|mobile drawer|overspeed"
```

```text
cargo run --release -p sim-core --example sweep
cargo run --release -p sim-core --example brake_sweep
cargo run --release -p sim-core --example starter_sweep
cargo run --release -p sim-core --example audio_probe
cargo run --release -p sim-core --example trace_probe
cargo run --release -p sim-core --example audio_capture
cargo run --release -p sim-core --example audio_capture -- --list
pnpm audio:render target/audio-capture
pnpm audio:render target/audio-capture 44100
cargo run --release -p sim-core --example audio_capture -- --only idle,start,full-1400 --out target/comparison
cargo run --release -p sim-core --example reference_probe -- --file datasheet/audio-references/max2712-om471/interior-engine-01m33s-02m49s.wav
cargo run --release -p sim-core --example reference_probe -- --window 32768 --file target/audio-capture/idle/mix.wav
```

The reference probe accepts PCM 16/24/32-bit and float WAV, mono or stereo.
`--windows` prints every window; `--peaks` prints spectral lines;
`--rpm-min` changes the search floor; `--hop` changes spacing between
analysis windows. Record these options with any reference measurement.

### Verification, 2026-09-08

| Command | Result |
|---|---|
| `cargo test --release --workspace --quiet` | Exit 0; 349 passed, 0 failed, including 56 acoustic tests |
| `cargo test -p sim-core --test overspeed --quiet` | Exit 0; 7 passed, including recovery at 12 different crank phases |
| `cargo test -p sim-core --test restart --quiet` | Exit 0; all 6 restart regression tests passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt --all -- --check` | Exit 0 |
| `pnpm install --frozen-lockfile` | Exit 0; lockfile current |
| `pnpm check` | Exit 0; 0 errors, 0 warnings |
| `pnpm test:unit` | Exit 0; 93 passed |
| Focused frontend browser command above | Exit 0; 14 passed at root and subpath, including localized selection menus, Specs alignment, mobile RPM docking, drawer animation, a single unobstructed speed readout, and one-click overspeed recovery on phone and desktop |
| `pnpm test` | Exit 0; 18 WASM, 93 unit and 52 browser tests passed; root/subpath coverage includes touch, zh-CN, automatic audio, grade-reset recovery, engine animation, pointer tracking, menus, mobile drawer and reduced motion |
| `pnpm build` and `pnpm build:subpath` | Both exit 0; each static output verified |
| `cargo run --release -p sim-core --example sweep` | Exit 0; 376.9 kW, 2545.1 N·m peaks; maximum pressure 20.18 MPa |
| `cargo run --release -p sim-core --example brake_sweep` | Exit 0; 108.6/274.3 kW stage III at 1300/2300 rpm |
| `cargo run --release -p sim-core --example starter_sweep` | Exit 0; 7.80 kW peak, 257 rpm sampled cranking mean, 17.3 V stall terminals |
| `cargo run --release -p sim-core --example audio_capture -- --out target/audio-restart-2026-09-08` | Exit 0; all 13 scenarios; zero full-scale individual-path samples |
| `pnpm audio:render target/audio-restart-2026-09-08` and the same command with `44100` | Both exit 0; 52 finite stereo WAVs, peak 0.920573 |
| `git diff --check` | Exit 0 |

Cargo emitted non-fatal incremental-cache access notes on Windows. Numerical
passes do not establish measured leak-down timing, band acceptance or listening fidelity.

## Legal and fidelity boundaries

- The gitignored `engine-sim/` checkout (Ange Yaghi, MIT) is reference only;
  no code or assets from it are used in `crates/` or `web/`. Any future
  adaptation must preserve its MIT notice and be documented here.
- No closed-source or unlicensed assets, OEM logos or implied endorsement.
- Not an engineering certification tool: no emissions-compliance prediction,
  ECU reproduction or OEM map reverse engineering.
