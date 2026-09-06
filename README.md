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

## Status: Milestone 4 — truck load and engine brake

Milestone 1 proved the whole path end to end: Rust core → WASM → Web Worker →
Svelte → static `web/dist`. Milestone 2 replaced its placeholder combustion with
a real one. Milestone 3 stopped prescribing the air path and made it a physical
system, taking the exhaust sound directly from it. Milestone 4 gives the engine
something to work against, and something to hold it back.

**In (Milestone 4):** the staged decompression engine brake the manual
describes — a two-peak brake cam that packs the cylinder from the exhaust
manifold early in compression and dumps it again just before top dead centre —
with its three stages, and a rigid truck driveline of gear, final drive, road
grade, rolling and aerodynamic resistance, and the vehicle's mass reflected onto
the crankshaft as inertia.

**Braking torque is produced, not prescribed.** The brake computes an *open
area*; gas flows through it; and the resulting cylinder pressure drives the crank
through the same slider-crank geometry as combustion. Nothing looks up a braking
torque, and a test asserts that no solver source so much as mentions the
published brake-power anchors — those are the validation target, and a model
allowed to read its own answer would demonstrate nothing.

**Out (later milestones):** aftertreatment chemistry, clutch slip and
gear-change behaviour, the manual's shift-assist and engine-stop-assist brake
functions, ABS interaction, orifice flow through the *intake* valve, and any
production-quality audio *model* — the switchable cockpit stage added since is a
filter applied to the output, not a second source of sound.

### Engine brake result

The fitted variant is code **M5U**, the standard system. The manual publishes two
brake-power figures for it, and they are the whole acceptance criterion:

| | Model | Published | Error |
|---|---:|---:|---:|
| Brake power at 1300 rpm | 109.0 kW | 100 kW | +9.0% |
| Brake power at 2300 rpm | 274.4 kW | 300 kW | −8.5% |

Held to ±10% rather than the ±3% the fuelled peaks carry, and the reason is
worth stating: the power and torque calibration constrained a model whose
geometry, valve events and injection system were published or tightly bounded,
whereas the manual publishes **no** brake cam contour, lift, timing, effective
area or MCM target. There is far more freedom per constraint here.

A tighter fit was available and was rejected. Moving the release lobe earlier and
widening it reaches ±3% — but it then vents the cylinder from 42° before top dead
centre, and peak cylinder pressure falls to less than half the motored trace.
That is a different machine: it hits the number by charging and venting rather
than by compressing and dumping, and it contradicts the manual's *"the
compression pressure is increased as a result"*. The configuration shipped here
opens the release lobe at 12° before top dead centre, where peak pressure sits
**above** the motored trace, and takes the wider error band.

The manual does not say which stage its figures describe. Maximum brake power is
stage III, and that is the reading validated against — recorded in provenance and
here as an interpretation, not as published fact.

| Stage | Manual | Model at 1300 rpm |
|---|---|---:|
| Motoring | — | 20.5 kW |
| I | "brake power is achieved by cylinders 1...3" | 47.4 kW |
| II | "brake power is provided by all cylinders" | 72.1 kW |
| III | all cylinders, plus wastegate and EGR positioner | 109.0 kW |

Stage I contributes 0.52 of stage II's braking above the motoring baseline, which
is what three cylinders of six should give.

### Truck load

Rigid coupling: in gear, road speed is whatever the crankshaft dictates, and the
truck appears at the crank as inertia — around 210 kg·m² in top gear against the
engine's own 3.5. That ratio is the whole reason a laden truck on a long descent
needs a brake that cannot wear out.

Two consequences are documented rather than hidden. Because road speed is derived
rather than integrated, changing gear changes the truck's speed instantly, and
neutral means no vehicle at all rather than one coasting. Driveline efficiency is
applied on the tractive side only, so on overrun the retarding torque is slightly
overstated; the alternative puts a discontinuity exactly where a descending truck
sits.

### Calibration result

| | Model | Published | Error |
|---|---:|---:|---:|
| Peak power | 369.2 kW at **1800 rpm** | 375 kW | −1.5% |
| Peak torque | 2509 N·m at **1000 rpm** | 2500 N·m | +0.4% |
| Max cylinder pressure | 20.31 MPa | 23 MPa envelope | 12% margin |
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

The engine note is not synthesised. The solver's fixed step is 25 µs — a 40 kHz
sample rate — and it runs about 58× faster than real time, so it emits **one
audio sample per solver step**, taken from quantities it already integrates. What
you hear is the same cylinder pressure that drives the crank, and the firing
frequency falls out of the firing order rather than being programmed.

There are two radiating paths, because a diesel has two:

- **The exhaust.** The net mass flow the exhaust ports actually passed — the same
  clamped, settled flow `flow::exchange` applies to the gas, not a separate
  estimate of it — sent down per-cylinder manifold runners, through the
  turbine's insertion loss, and into a tailpipe modelled as a waveguide with a
  reflecting, lossy open end.
- **The engine itself.** The premixed burn is a near-step pressure rise inside a
  stiff iron box, and the box rings. A bank of four resonators standing for
  bending and breathing modes of the block, head and covers turns cylinder
  pressure into combustion noise, which radiates straight to air and never goes
  near the exhaust. This is the part that makes the ear say *diesel* rather than
  *engine*, and for a heavy-duty one it dominates from roughly 800 Hz to 4 kHz.

Neither has a schedule. The engine rattles hard at light load and mellows under
it because ignition delay lengthens when the cylinder is lightly loaded, which
raises the premixed fraction, which sharpens the pressure rise. The engine brake
barks because its release lobe cracks a valve at the top of compression, the
fastest pressure event the model produces anywhere. The pipe's resonance shifts
as the exhaust heats because the speed of sound is taken from the temperature the
solver integrates. None of that is written down anywhere as a rule; all of it
falls out.

Five things in that chain are easy to get wrong and were:

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
  as a buzz near the filter cutoff rather than as a louder engine.
- **Radiating a proxy for a quantity you have already solved.** The exhaust
  source was once port area times pressure difference, computed beside the call
  that solved the real orifice flow. Real flow goes as the square root of the
  difference and saturates once the throat chokes, so the proxy exaggerated the
  blowdown peak and changed shape against the truth through the choked-to-
  subsonic transition. The pulse shape is the timbre.
- **A resonator with poles and no zeros passes DC.** A real mechanical mode has
  no response to a static pressure. Without zeros, the fraction of a pascal per
  step that a stopped engine's trapped charge gains against its walls came
  through the modal bank as a standing offset — an engine that was quiet rather
  than silent.

An `audio_probe` example reports where the energy sits, band by band and octave
by octave. Its band shares sum to 100% as a self-check, and it reports energy
above 15 kHz separately, because the explicit port transfer leaves a two-sample
limit cycle near Nyquist that is arithmetic rather than sound and must not be
counted as content.

The UI carries a spectrum view off the output path for exactly that reason: it
shows where the energy actually is, which no assertion about sample values can.

### Where you are listening from

That signal is a *tailpipe* signal — what a microphone at the pipe mouth would
hear. A driver is not at the pipe mouth, so the sound stage is switchable:

| Stage | What it is |
|---|---|
| **Raw tailpipe** | The solver's samples, unfiltered. The path the spectrum view was built to verify. |
| **Truck cockpit** (default) | The same samples heard from the driver's seat. |

The cab is a Web Audio chain in `web/src/lib/cabin.ts` and `audioEngine.ts`, not
a change to the physics: a 30 Hz high pass, +6 dB of shell boom at 85 Hz, −1.5 dB
of boxiness at 380 Hz, a 2.6 kHz low pass for glass and insulation, two early
reflections at 7.3 and 11.9 ms panned apart, a short generated impulse response
for the diffuse tail, and a compressor. Both paths always run; switching
crossfades between them over 40 ms.

That low pass was 1.7 kHz while the solver produced nothing above it, which made
it free. It is not free now: clatter is emphatically audible from a driver's
seat, and a cab that removed it would be describing the glass rather than what a
driver hears.

**Nothing is added.** No road noise, no synthesised rumble, no samples. Every
value reaching the speaker is still the solver's cylinder pressure, and a unit
test asserts the stage builds nothing that can produce sound on its own. The
impulse response is generated from a seeded PRNG rather than fetched, so the
build stays self-contained and the room is the same one on every load.

**It is level-matched, and that took two gains rather than one.** A
post-processing switch that is merely louder wins any comparison for the wrong
reason. Matched with an output gain alone, the cab sat level with the dry path
under load and **6.3 dB louder at idle** — the compressor was working at the
loud end and doing nothing at the quiet end, so one gain could only ever match
one of them. Trimming ahead of the compressor moves the quiet end nearly decibel
for decibel and the compressed loud end by much less, so trim-then-make-up
(0.42 in, 1.90 out) brings both together:

| Measured at the output | Idle | Working: 90% pedal, 300 N·m |
|---|---:|---:|
| Cockpit level relative to raw | +1.17 dB | −1.22 dB |
| 2–8 kHz band | — | −33% |
| 60–400 Hz band | — | +5% |

The 2–8 kHz figure was −55% while the cab's low pass sat at 1.7 kHz and the
solver put almost nothing up there. Both numbers have moved for the same reason:
there is now real content in that band, and the cab attenuates it rather than
removing it.

Both ends are asserted to within 3 dB by the browser suite, measured through the
same analyser the spectrum view uses, and repeat to within ±0.05 dB across runs.
The remaining spread is the compressor doing its job, and closing it entirely
would mean squashing the engine's dynamics to win an argument about gain.

None of this is published. The manual says nothing about how the engine sounds
and less about how its cab sounds; these are listening choices, and the UI says
so where you switch them.

## Layout

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, the air path, the engine brake, the truck driveline, the exhaust and structural acoustic sources, the exhaust duct, the dynamometer harness, snapshots, native tests. No browser, DOM, audio-device, or filesystem dependency. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering, Web Audio orchestration. |
| `web/src/audio` | The `AudioWorklet` processor: ring buffer, resampler, underrun accounting. Dependency-free plain JavaScript. |
| `web/src/lib/cabin.ts` | The cockpit listening stage as data: filter table, reflection taps, seeded impulse response. Pure and Web-Audio-free, so it is unit-tested under Node. |
| `scripts/verify-dist.mjs` | Static-build verification. |

`crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json` is the engine
configuration (schema version 4). It is compiled into the WASM module with
`include_str!`, so the deployed site needs no runtime fetch. Regenerate it with
`python3 scripts/gen-om471-config.py`, which holds the authoritative provenance
table. Three calibration probes sit behind the figures above:
`cargo run --release -p sim-core --example sweep` for the fuelled peaks,
`--example brake_sweep` for the engine brake against its published anchors, and
`--example audio_probe` for exhaust levels and where their energy sits.

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
| `GF14.15-W-0002H` — *Engine brake, function* (471.9 / MODEL 963, 964, codes M5U and M5V) | pp. 60–65 / PDF 63–68 | the decompression principle and the two-peak brake cam; the three stages and that **stage I acts on cylinders 1–3**; the **1000 rpm** activation floor; **M5U 100 kW at 1300 rpm and 300 kW at 2300 rpm**; that the two variants share hardware and differ only by data record |

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
amplification", up to 2100 bar, MCM-scheduled quantity and timing). The engine
brake section (`GF14.15-W-0002H`) is the most detailed of all: it describes the
two-peak cam and what each peak is for, all three stages, and the operating
conditions.

Apart from the EGR cooler temperatures and the six engine-brake figures, it
publishes **no** boost pressure, recirculation rate, turbine or compressor
geometry, efficiency, inertia, back-pressure, injection timing, injection
quantity, rate shape, fuel consumption, firing order, rotating inertia, valve
events, brake cam contour or lift, MCM brake target, friction map, vehicle data,
or the speeds of the M3D peaks.

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
| Exhaust restriction | 120 kPa per (kg/s)², about 15 kPa at rated flow. This is what the aftertreatment is to the *gas path* — a flow resistance. What it is to the acoustic path is a separate matter and is modelled separately, below |
| **EGR rate** | 0.04–0.14 against speed, ceiling 0.35. Pressure-limited to zero at full load below about 1000 rpm, where the turbine cannot lift exhaust above intake |
| EGR valve | 4.0e-4 m² fully open, discharge coefficient 0.70 |
| Exhaust valve | 2.5e-3 m² at full lift, 35° of crank to ramp between shut and full lift |
| **Brake cam contour** | charging lobe centred 130° BTDC, release lobe 12° BTDC, half-widths 10° each, 1.3e-3 m² at full lift. The manual describes both peaks and their purpose but publishes no contour, lift or timing |
| **Brake stage air path** | stages I and II leave the wastegate alone; stage III regulates to 2.33 bar absolute with the loop detuned to a tenth of its fuelled gains. The EGR positioner stays shut in every stage |
| Driveline | 40 t gross combination mass, 0.506 m wheels, Cr 0.006, 6.0 m² drag area, 2.61 final drive, twelve ratios from 14.93:1 to 1.00:1, 0.95 efficiency. The source is an engine manual and publishes nothing at all about the vehicle |
| Injection timing | 8–15° BTDC against speed, retarded 0.00075 rad/mg with load — the retard is what holds peak pressure inside the 230 bar envelope |
| Smoke limit | 19.5:1 minimum air/fuel ratio; stoichiometric taken as 14.5:1 |
| Exhaust acoustics | 20 Hz high pass, soft-clip knee 0.85, gain set so the start transient approaches the knee without saturating. There is no longer a muffler roll-off parameter: a two-pole low pass standing in for an entire exhaust system is a tone control, and the duct below replaced it |
| **Exhaust system** | 3.5 m tailpipe of 0.008 m² section; open-end reflection −0.8 with a 2 kHz radiation loss; 12 litres of free aftertreatment volume, which acts as 1.5 m of added acoustic length rather than as a filter; 14 dB of turbine insertion loss above 400 Hz; manifold runners spanning 100–700 mm. **None of this geometry is published.** The manual describes the aftertreatment architecture in detail and gives no dimensions and no acoustics whatever |
| **Combustion noise** | Four structural modes at 900, 1600, 2600 and 3800 Hz, Q 15/20/22/25, weighted 1.0/4.0/6.0/11.0, at an overall gain of 3.0. Plausible for the block, head and covers of an engine this size and nothing more than that. The weights ascend with frequency, which is the opposite of the radiating physics: the model's premixed rise starts with zero slope and so drives the upper modes weakly, and the weights compensate for that shortfall in the drive rather than claiming an engine radiates more at 3.8 kHz than at 900 Hz |
| **Cylinder build scatter** | ±2% on exhaust port area, ±1.5% on injector delivery, drawn once per cylinder at reset from the reset seed and never per step. Six bit-identical cylinders sum to a pure harmonic comb that the ear hears as synthesised; real injectors are matched to a tolerance rather than to each other. Deliberately far too small to move any calibration result, and a test asserts peak power and torque are unchanged by it |
| **Cockpit listening stage** | 30 Hz high pass; +6 dB at 85 Hz, Q 1.1; −1.5 dB at 380 Hz, Q 1.0; 2.6 kHz low pass; reflections at 7.3 ms (−11 dB, left) and 11.9 ms (−13 dB, right); 180 ms seeded impulse response decaying over 130 ms after 6 ms of predelay; compressor at −18 dB, ratio 3, 6/180 ms, trimmed 0.42 in and 1.90 out. Presentation only — it is downstream of everything, changes no state, and is bypassable |
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
  intake noise, no pipe resonance, no mechanical noise. The engine brake is
  audible, but only because its release lobe opens a real port onto a real
  pressure difference — nothing was added to make it bark.
- **The cockpit stage is a listening filter, not an audio model.** It says where
  you are sitting, not what the engine does: it is downstream of the simulation,
  it has no path back into it, and the snapshot's own level reading is measured
  at the source and is unmoved by it. The cab's own transfer function was not
  measured and could not be — there is no such data for this vehicle — so the
  filter is a plausible one rather than a correct one, and it is bypassable for
  precisely that reason.
- **The brake\'s wastegate loop is deliberately slower than the fuelled one.**
  The brake carries its own positive feedback — more boost packs the cylinder
  harder, which dumps more energy into the turbine, which makes more boost — and
  the gains calibrated for the fuelled engine hunt against it above 1800 rpm.
  Absorbed power at the upper anchor then swung between 282 and 300 kW depending
  only on where in the oscillation the average landed, and more settling did not
  help because it was a limit cycle rather than a transient. Commanding a fixed
  valve position instead is worse still: absorbed power swings roughly threefold
  between positions 0.35 and 0.25 as the turbine reaches its speed clamp.
- **Two of the brake\'s three published operating conditions are represented.**
  A released pedal and the 1000 rpm floor are enforced. The manual also requires
  the clutch pedal released and ABS not in closed-loop operation; this model
  carries neither signal, so those conditions are absent rather than
  approximated.
- **The driveline coupling is rigid, with no clutch.** Road speed is derived from
  crank speed rather than integrated, so changing gear changes the truck\'s speed
  instantly, and neutral disengages the vehicle entirely rather than letting it
  coast. There is no gear-change logic and no shift-assist braking.

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
