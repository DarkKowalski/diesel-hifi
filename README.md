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
driveline; three-path engine audio; a tested metric library, named reproducible
listening scenarios and WAV capture; native deterministic tests; a static Svelte
UI with the simulation off the UI thread and a level-matched A/B against local
recordings.

**Out.** Gasoline engines or any multi-fuel abstraction. Engineering
certification, emissions prediction, ECU reproduction, OEM map reverse
engineering. Backend, SSR, accounts, databases, telemetry collection, runtime
network calls. CFD, finite-element mechanics, injector hydraulics, chemical
kinetics, aftertreatment chemistry.

## Architecture

| Path | Responsibility |
|---|---|
| `crates/sim-core` | Configuration, validation, provenance, deterministic physics, air path, engine brake, driveline, all three acoustic sources and the exhaust duct, dynamometer harness, snapshots, native tests. **No browser, DOM, audio-device, or filesystem dependency.** |
| `crates/sim-core/src/analysis.rs` | The measuring instruments: transform, band shares, comb share, crest factor, modulation depth. Pure, out of the hot loop, and tested against signals whose answer is known. Shared by the probes and the acoustic tests so a metric has one definition. |
| `crates/sim-core/src/scenario.rs` | Named, reproducible listening scenarios: a seed, initial conditions and a script of control phases, returning audio plus the conditions the engine actually reached. A measurement harness like `dyno.rs`, and like it it touches nothing outside the simulation. |
| `crates/sim-wasm` | `wasm-bindgen` adapter. Serialization and boundary only; no physics. |
| `web/src/worker` | WASM lifecycle, fixed-step scheduling, batching, message protocol. |
| `web/src` | Svelte UI, input, telemetry rendering, Web Audio orchestration. |
| `web/src/audio` | `AudioWorklet` processor: ring of interleaved frames, resampler, underrun accounting. Dependency-free plain JavaScript. |
| `web/src/lib/cabin.ts` | Cockpit listening stage as data: per-path transfers, shared filter table, reflection taps, seeded impulse response. Pure and Web-Audio-free, unit-tested under Node. |
| `web/src/lib/compare.ts` | Level-matching arithmetic for the A/B against local recordings. Pure and Web-Audio-free. |
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
submit controls, advance many fixed steps, return one compact snapshot. Audio
crosses separately from the snapshot, as **interleaved frames** — one frame per
solver step, `audioPathCount()` floats to a frame — because a batch carries
thousands of them and a snapshot has to stay compact. Errors
are structured for invalid IDs, inputs and numerical state. Memory ownership is
explicit and there are no per-step JS/WASM calls. The module runs in a dedicated
Web Worker and needs neither `SharedArrayBuffer` nor cross-origin isolation.

The UI provides an engine selector populated through the real catalog API,
start/stop and reset, pedal and load controls, RPM with pressure/torque/state
telemetry, visible error and worker lifecycle states, a solo toggle per
radiating path, a level-matched A/B against local audio files, and **an explicit
user action before Web Audio starts**.

## Milestones

| # | Delivered |
|---|---|
| 1 | Vertical slice: workspace, validated configuration, minimal crank/cylinder state, WASM adapter, worker protocol, Svelte controls and telemetry. |
| 2 | Diesel combustion: injection, ignition delay, double-Wiebe heat release, pressure-derived torque, calibrated to the published power and torque magnitudes. |
| 3 | Air path and sound: wastegate turbo and EGR dynamics, exhaust pulse output, gesture-gated Web Audio. |
| 4 | Truck load and engine brake: rigid driveline and the staged decompression brake, against the published M5U anchors. |
| 5 | Engine acoustics: structural combustion noise, per-cylinder build scatter, radiated port flow, an exhaust duct with runners and turbine loss, and a retuned cockpit stage. |
| 6 | Making it sound like a truck: a torque-driven body path, a bounded pipe-mouth radiation transfer, a lower and broader modal bank, cycle-to-cycle combustion variation, and acceptance criteria that measure firing orders and pulse dynamics rather than band shares alone. |
| 7 | Three paths to the listener: the radiating paths cross the boundary interleaved instead of summed, each gets its own cab transfer, and the UI can solo them. |
| 8 | **Measuring before changing**: a tested metric library, a broken modulation detector replaced, named reproducible scenarios covering idle through engine braking, a WAV capture tool, a level-matched A/B against local recordings, and an audit of the playback chain at both device rates. No acoustic source or calibration was changed. |
| 9 | **The limiter stops shaping the timbre**: the memoryless soft clipper is replaced by a level follower with an instantaneous attack and a calibrated release, the ceiling becomes transparent below the knee, and the capture tool reports the gain each frame carried. Every operating point gains crest factor; the engine brake gains 5 dB of it and passes the criterion it was failing. No acoustic *source* changed. |
| 10 | **The reference gets measured**: a firing-rate estimator, so a recording that does not report its speed can still be measured; a probe that annotates any WAV into operating-point segments with a confidence attached; the benchmark's idle characterised from three independent stretches; and its test drive shown to be dominated by a sampled source's loop rather than by an engine. No acoustic source, calibration or criterion changed. |
| 11 | **The drive gets looked at, before it is amplified**: the three forcings are readable where they enter the acoustic stage, a detector says whether a trace was integrated to or assigned to, and a probe attributes every step in them to the crank event that caused it. It finds that two of the three gas-exchange transitions are boundary assignments, and that the impulse they hand the modal bank is up to **13 of the 21 percentage points** the block path holds above 2 kHz at idle. No acoustic source, calibration or criterion changed. |
| 12 | **The intake valve becomes a valve**: the cylinder stops being assigned its manifold's pressure during induction and starts drawing gas through a port, so the trapped charge comes from what flowed rather than from a calibrated multiplier. The two assigned gas-exchange transitions become continuous — the largest of them falls from 0.929 to 0.002 — and the artefact they were feeding the modal bank goes with them: the block path's content above 2 kHz drops from **20.9% to 8.6%** at governed idle against a counterfactual bound of 8.1%. Trapping efficiency, valve timing and the brake's charging lobe are recalibrated with it; two open deficits close. |

Milestone 8 deliberately delivered **no change to how the engine sounds**, and
every figure it measured at an operating point milestone 7 also measured was
unchanged to the last digit. What it changed was what can be measured, what can
be regenerated, and what is known to be wrong.

Milestone 9 is the first fix taken from that list, and it is a fix to the
*listening stage* rather than to a source: the measurements said the pulses were
being squashed after they were produced. The figures below therefore move for the
first time since milestone 7, and **Results → Sound** records what moved, by how
much, and what the fix exposed underneath.

Milestone 10 changes nothing about the sound either, and for a different reason:
three of the open criteria were recorded as **waiting on the reference
recording**, which had never been measured. It is measured now — see
**Characterising the reference** — and what that settles, what it does not, and
which of the three it cannot settle even in principle are recorded there and
under **Known deficits**. No criterion was changed on the strength of it.
Amending an acceptance criterion in the same breath as first measuring the
evidence for it is how the previous set came to enforce the defect it was meant
to catch.

Milestone 11 changes nothing about the sound either, and it is the last of the
three that will: the acoustic plan puts *inspect the traces for discontinuities
before amplifying their high-frequency content* ahead of any source work, and
until now nothing could see the traces. It can now, and there is something in
them — see **What the traces contain**. Every source experiment the plan lists
next would have amplified it.

| 13 | **The seat gets a balance, and finds out why it cannot have much of one**: each radiating path gets a level of its own in the cab, applied cab-side so the raw stage stays the solver's own output. Acting on a listening report — body up, the others down — the body goes up 1.5 dB and the block down 3, taking about 2 dB more of the top end out on the way to the driver. The request could not be met in full, and the measurement that stopped it is the result: the body path sits **1.1 dB behind the exhaust at idle and 12.8 dB behind at full load**, so no constant trim describes both ends. No physics or calibration changed. |

| 14 | **The cab stops reflecting treble it should swallow, and the bass is found to have no harmonics**: the generated room tail gets two decay rates instead of one — a quarter as long above 900 Hz, because a cab lined with seats, carpet and a headliner absorbs several times more there than at 125 Hz — and the early reflections get a damping filter, because what bounces off a seat back is not a full-bandwidth copy. Measuring the forcings to explain a listening report then found the larger problem: the **torque drive is 87% fundamental with a crest factor of 4.8 dB**, so the body path can only ever be a tone. Cab change delivered; the bass finding recorded. |

Milestone 14 is two things that arrived together because one was found while
looking into the other. The cab change is the listener's own suggestion — a
cabin is full of soft things and soft things absorb treble — and it is
straightforwardly right: the tail had a single decay constant for every
frequency, which is a hard box. The bass finding came from asking why raising
the body path in milestone 13 had made the sound boomy rather than fuller, and
the answer is under **What the drive contains**: there are no harmonics in the
bass to raise.

Milestone 13 is a listening-stage change and the first thing in this document
driven by someone hearing it rather than by an instrument. It is also the first
time a listening request has been **partly refused by a measurement**: the level
match between the cockpit and raw stages is what stopped the body trim at 1.5 dB
rather than the 6 dB that was tried first, and chasing why turned up a source
deficit nobody had looked for. What the ear asked for and what the cab could give
are recorded separately, under **Where you are listening from** and **Known
deficits**, because they are not the same size.

Milestone 12 is the first change to the acoustic *source* since milestone 6, and
it is the fix milestone 11 blocked the rest of phase 4 on. It is also the first
change since milestone 9 to move a figure in **Results → Calibration** as well as
in **Results → Sound**, because the trapped charge stopped coming from a
calibrated number and started coming from a flow. What it recovered, what it cost
to recalibrate, and the one residual step it left are recorded under **What the
traces contain**.

It is worth saying plainly what did *not* happen. The counterfactual in milestone
11 was recorded as an **upper bound** on what a real port could recover, on the
grounds that interpolating a step away removes an edge entirely while a real valve
still makes a fast one. The bound turned out to be within half a percentage point
of the answer at every operating point. The reasoning offered for expecting that —
an argument from the exhaust side's 35° ramp — was explicitly labelled "a reason,
and it is not evidence". It is evidence now.

Still out: aftertreatment chemistry, clutch slip and gear-change behaviour, the
manual's shift-assist and engine-stop-assist brake functions, ABS interaction, and
pulsating manifold coupling. **The last of those is the simplification milestone
12 deliberately kept**: both ports now flow for real, but both manifolds are still
drained and filled by a mean-value speed-density estimate rather than by the summed
port flows. That is the treatment the exhaust side has had since milestone 5, so
the two sides are now symmetric rather than one being a special case — see
**Modelling simplifications**.

## Results

### Calibration

| | Model | Published | Error | Milestone 11 |
|---|---:|---:|---:|---:|
| Peak power | 377.6 kW at **1800 rpm** | 375 kW | +0.7% | −1.5% |
| Peak torque | 2550.6 N·m at **1000 rpm** | 2500 N·m | +2.0% | +0.4% |
| Max cylinder pressure | 20.20 MPa | 23 MPa envelope | 12% margin | 20.30 MPa |
| Idle | 568 rpm | 560 rpm | +1.4% | 560 rpm |
| Best fuel consumption | 180.1 g/kW·h | not published | — | 179.9 |

The **magnitudes** are published; the **speeds in bold are an outcome of our
calibration** and must never be quoted as OEM data. The UI labels them
`CALIBRATED`. A real OM 471 is nearer 185–190 g/kW·h, so consumption remains
slightly optimistic; it is reported rather than tuned away.

**The last column is milestone 11, and every entry in it moved for one reason.**
Trapping used to be `air_path.volumetric_efficiency`, a constant 0.92 multiplied
into the charge at the instant the intake valve shut. It is now whatever flowed
through an intake port, so it varies with speed, boost and valve timing on its
own — which is what a trapping efficiency does, and is why the numbers had to be
refitted rather than merely re-read. Two calibrated values carried the refit:

| Value | Was | Now | Why |
|---|---:|---:|---|
| `valvetrain.intake_effective_area_m2` | — | 0.0025 | New. Matched to the exhaust port; both are *effective* areas, so a discharge coefficient is already inside them |
| `valvetrain.intake_valve_close_rad` | −2.7925 (20° ABDC) | −2.4435 (40° ABDC) | Was a boundary switch with no effect on trapping, and is now the dominant lever on it. 40° ABDC is inside the ordinary heavy-duty range |
| `engine_brake.charge_center_rad` | −2.2689 | −2.2340 | Not a free choice. The validator requires the brake's charging lobe to open after the intake valve shuts, and retarding IVC left 1e-7 rad of margin — an accident, not a calibration. Moved to restore 2° |

A fixed area throttles more the faster the piston asks for gas, so peak power fell
away with area while peak torque at 1000 rpm barely moved — the port is generous
enough to reach equilibrium at that speed whatever its size. That is why valve
timing rather than area is what carried the fit, and it is a property of the
physics rather than a convenience.

**Idle now settles at 568 rpm rather than on 560.** It is governed, not held, so
this is where the governor and the new pumping work balance; the scenario test
that asserts idle is governed rather than pinned still passes. It is reported
rather than tuned, and it is a 1.4% error against a published figure.

### Engine brake

Fitted variant **M5U**. The manual publishes two brake-power figures for it, and
they are the whole acceptance criterion:

| | Model | Published | Error | Milestone 11 |
|---|---:|---:|---:|---:|
| Brake power at 1300 rpm | 108.6 kW | 100 kW | +8.6% | +9.0% |
| Brake power at 2300 rpm | 274.4 kW | 300 kW | −8.5% | −8.6% |

Both moved in milestone 12, and the charging lobe moving is why — see
**Calibration** above for what forced it. The fit is marginally better balanced
than the one it replaced and neither anchor was targeted; the lobe position was
chosen for the margin it leaves against intake valve closing, and these are what
came out.

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
the solver emits **one audio frame per step** from quantities it already
integrates. Three paths radiate, each driven by a different quantity, because
they are three different mechanisms — and they cross the boundary **separately**,
interleaved into that frame, because they do not reach a listener by the same
route either:

- **The exhaust**, driven by port mass flow. The net flow the ports actually
  passed — the same clamped, settled flow `flow::exchange` applies to the gas,
  not a second estimate of it — down per-cylinder manifold runners, through the
  turbine's insertion loss, into a tailpipe modelled as a waveguide with a
  reflecting, lossy open end. What leaves at the end is the **volume velocity at
  the mouth**, `p+ − p−`; the aftertreatment substrate takes its cut on each
  traverse. This is the firing comb: 82% of this path's energy sits in the first
  four orders at cruise and 92% at full load.
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

Where the energy sits, from `cargo run --release -p sim-core --example audio_probe`.
Every row is a named scenario from `sim-core::scenario`, so it can be regenerated
rather than reproduced by hand:

| | `idle` | `light-900` | `cruise-1200` | `full-1400` | `rated-1800` | `brake-1300` | Target |
|---|---:|---:|---:|---:|---:|---:|---:|
| Speed, rpm | 551 governed | 900 held | 1200 held | 1400 held | 1800 held | 1300 held | — |
| Firing orders `f0…4·f0` | 48.1% | 60.0% | 72.0% | 88.2% | 72.2% | 43.3% | ≥ 35% loaded |
| Crest factor | 15.5 dB | 12.0 dB | 11.9 dB | 12.7 dB | 11.5 dB | 12.5 dB | ≥ 9 dB |
| Modulation depth | 0.66 | 1.11 | 0.77 | 1.55 | 1.08 | 2.31 | > 0 |
| `<80 Hz` | **39.3%** | 1.9% | 17.8% | 5.4% | 0.0% | 2.0% | ≤ 35% |
| `80–300 Hz` | 45.4% | 80.1% | 57.8% | 84.9% | 73.9% | **42.1%** | ≥ 45% |
| `300 Hz–2 kHz` | 14.7% | 16.3% | 23.6% | 9.3% | 24.9% | **55.9%** | ≤ 30% |
| `>2 kHz` | 0.6% | 1.7% | 0.9% | 0.4% | 1.2% | 0.0% | — |
| `150 Hz–15 kHz` | 38.5% | 65.3% | 50.1% | 59.7% | 77.2% | 94.4% | ≥ 35% |
| `>15 kHz` residue | 0.02% | 0.01% | 0.00% | 0.00% | 0.00% | 0.01% | ≤ 1% |
| Peak | 0.105 | 0.367 | 0.652 | 0.850 | 0.850 | 0.850 | at or under the 0.85 knee |
| Level, dBFS | −35.1 | −20.7 | −15.6 | −14.1 | −12.9 | −13.9 | — |
| Exhaust over block | 10.8 dB | **5.2 dB** | 7.1 dB | 12.2 dB | 6.3 dB | 26.8 dB | ≥ 6 dB |

Bold entries miss their target and are discussed under **Known deficits** below.

**Milestone 12 moved every column, and two bold entries stopped being bold.**
`80–300 Hz` at governed idle went from 41.7% to 45.4% and now clears its floor,
and `rated-1800` went from 5.8 dB to 6.3 dB of exhaust over block and now clears
the 6 dB balance criterion. Neither criterion was touched. Both were failing
because the block path was carrying an artefact, and removing the artefact moved
the share and the balance in the same motion.

| | Was, milestone 11 | Now | Target |
|---|---:|---:|---:|
| `idle` `<80 Hz` | 40.8% | 39.3% | ≤ 35%, still failing |
| `idle` `80–300 Hz` | **41.7%** | 45.4% | ≥ 45%, **now passing** |
| `rated-1800` exhaust over block | **5.8 dB** | 6.3 dB | ≥ 6 dB, **now passing** |
| `light-900` exhaust over block | **5.2 dB** | 5.2 dB | ≥ 6 dB, unmoved |
| `brake-1300` `300 Hz–2 kHz` | **55.2%** | 55.9% | ≤ 30%, still undecided |

Crest factor fell between 0.1 and 0.8 dB at five of the six points, and that
needs saying rather than burying: part of what was being counted as pulse
dynamics was an impulse train at the gas-exchange rate. `idle` lost 1.2 dB, which
is almost exactly the 1.27 dB milestone 11 measured the steps as contributing
there. The figure is lower and it is now measuring combustion.

**Milestone 9 moved these figures, and the crest row is why.** The limiter was a
memoryless soft clipper — `knee · tanh(mix / knee)` — applied to each sample as it
was produced. That reduces a tall sample more than a short one, which is the
definition of a waveshaper and the opposite of what a limiter is for. The capture
tool now measures the mix on both sides of it, and the cost was not subtle:

| Crest factor | before, saturated | before, limiter divided out | now |
|---|---:|---:|---:|
| `idle` | 16.6 dB | 16.7 dB | 16.7 dB |
| `light-900` | 11.7 dB | 12.1 dB | 12.1 dB |
| `cruise-1200` | 10.9 dB | 11.9 dB | 11.9 dB |
| `full-1400` | 10.4 dB | 13.8 dB | 13.1 dB |
| `rated-1800` | 10.0 dB | 12.6 dB | 12.3 dB |
| `accelerate` | 10.7 dB | 14.6 dB | 11.9 dB |
| `brake-1300` | **7.4 dB** | 12.9 dB | 12.4 dB |

The middle column is the signal the solver had *already produced* and the clipper
then flattened. `brake-1300` was failing the 9 dB criterion at 7.4 dB while
carrying 12.9 dB of crest one stage upstream: the release lobe was never the
problem, the stage after it was. Three of the four known deficits recorded in
milestone 8 were this one defect seen from different angles.

Two consequences of replacing it, both visible in the table. The knee is now
**transparent below itself**, so `idle`, `light-900`, `cruise-1200`, `start` and
`stop` are no longer touched at all where the clipper had been rounding them; and
where it does engage the peak sits *on* the knee rather than a little under it,
because the ceiling is now reached by turning the passage down instead of by
squashing its peaks. The levels fall with it — `brake-1300` by 5 dB — since a
uniform gain reduction costs RMS where a waveshaper had been quietly making up
the difference in harmonics.

**The modulation column is not comparable with milestone 7's.** It is a different
measurement; see **Measuring the sound** below.

Three columns arrived with milestone 8 and two of them were not measurable
before. `idle` is **governed** — the engine sits where the governor puts it, at
551 rpm against the published 560 — where the old "idle" column was 600 rpm at
15% pedal with the crank re-pinned every 2.5 ms. That is a held speed with an
idle-ish pedal, and it is a different signal: it was reading 17.2% below 80 Hz
where governed idle reads 40.8%, because 600 rpm puts the second order above
80 Hz and 551 rpm does not. The old number was not wrong about the signal it
measured. It was measuring the wrong signal.

The loudest octave is `40–80 Hz` at governed idle, `160–315 Hz` at 900, 1400 and
1800, `80–160 Hz` at cruise, and `315–630 Hz` under brake. The brake's moved up
an octave in milestone 9: the release lobe's crack is the fastest event in the
model and the clipper had been taking the top off it.

The probe reports each path on its own, because the balance between them is the
whole question and a mixed band share cannot say which one moved:

| Alone, at `full-1400` | dBFS | `<80 Hz` | `80–300` | `300–2k` | `>2 kHz` | orders |
|---|---:|---:|---:|---:|---:|---:|
| Exhaust | −13.9 | 7.9% | 86.3% | 5.8% | 0.0% | 92.1% |
| Block | −26.1 | 4.3% | 21.9% | 68.2% | 5.6% | 25.8% |
| Body | −26.8 | 84.3% | 15.7% | 0.0% | 0.0% | 99.8% |

The block's `>2 kHz` share is the row milestone 12 was for: 12.8% became 5.6%,
against the 5.7% a perfect removal of the steps predicted.

The exhaust leads the block by 5 to 27 dB at every point measured. That figure is
the single one that decides whether this reads as a truck: the exhaust carries the
firing orders and the block sits on top of them, and a block in front of the
exhaust is a small engine however the bands come out.

#### What the traces contain

Everything above measures what leaves the boundary. This measures what goes in,
and the two are not the same question asked twice: each path filters, and a
resonator bank spreads a single defective step over tens of milliseconds, so by
the time a sample is emitted a one-step artefact and a combustion event look
alike — broadband, decaying, arriving at the firing rate.

The question is **whether a trace got where it is by being integrated or by
being assigned.** Anything a fixed-step solver integrates is continuous, because
a derivative bounds how far it can move in 25 µs. A boundary that clamps a
cylinder to a manifold pressure, or a charge recomputed from a fresh gas-law
evaluation, moves it by whatever the two descriptions disagree by, in one step.
In the gas model that is a legitimate simplification making a small bounded
error. It stops being small the moment the same trace is radiated, because the
structural path differentiates it, and the derivative of a step is an impulse,
and an impulse is white.

From `cargo run --release -p sim-core --example trace_probe`. Every cylinder
makes three gas-exchange transitions per cycle, and **as of milestone 12 all
three are modelled as orifice flow**. Mean excess at each, in atmospheres of
summed cylinder pressure, where excess is how far the trace moved beyond what the
differences either side of it implied:

| Transition | `idle` | `light-900` | `cruise-1200` | `full-1400` | `rated-1800` | `brake-1300` |
|---|---:|---:|---:|---:|---:|---:|
| exhaust valve opens | 0.0000 | 0.0002 | 0.0002 | 0.0004 | 0.0010 | 0.0000 |
| TDC overlap | 0.0001 | 0.0030 | 0.0011 | 0.0001 | 0.0017 | 0.0000 |
| intake valve closes | 0.0000 | 0.0000 | 0.0001 | 0.0001 | 0.0001 | **0.0371** |

And what it was before, on the same measurement, with the two assignments in
place:

| Transition, milestone 11 | `idle` | `light-900` | `cruise-1200` | `full-1400` | `rated-1800` | `brake-1300` |
|---|---:|---:|---:|---:|---:|---:|
| TDC overlap | 0.0406 | 0.0457 | 0.1513 | 0.6060 | **0.9288** | 0.0702 |
| intake valve closes | 0.0701 | 0.0683 | 0.0959 | 0.1049 | 0.0666 | 0.0609 |

**The largest assignment in the model fell from 0.929 to 0.002, and the blind
detector now finds no steps in the pressure forcing at any operating point.** The
exhaust row is the control and is unchanged, which is what distinguishes the
intake side being fixed from the measurement having stopped working.

What that was worth, from the same probe — the block path's own band shares, as
built now, against what it was and against the counterfactual that interpolated
every step away:

| Block path `>2 kHz` | `idle` | `light-900` | `cruise-1200` | `full-1400` | `rated-1800` | `brake-1300` |
|---|---:|---:|---:|---:|---:|---:|
| Milestone 11, as built | 20.9% | 7.5% | 5.6% | 12.8% | 7.6% | 0.9% |
| Milestone 11 counterfactual | 8.1% | 7.6% | 5.8% | 5.7% | 4.0% | 0.1% |
| **Milestone 12, as built** | **8.6%** | **7.6%** | **5.7%** | **5.6%** | **4.1%** | **0.2%** |
| `300 Hz–2 kHz` now | 39.5% | 68.1% | 71.7% | 67.9% | 78.9% | 81.3% |

**The middle row was recorded as an upper bound and it landed within half a
percentage point of the answer everywhere.** That was the open question milestone
11 could not settle — how much of the artefact a *finite-rate* transition removes,
as against the perfect removal an interpolation measures — and the answer is
essentially all of it. At idle the port recovered 12.3 of the 12.8 points that
were available; at full load and at rated it landed a tenth of a point *past* the
bound, which is within the run-to-run reach of a different 32 768 samples rather
than a real overshoot. A real valve does still make a fast edge. At a 35° ramp it
is worth under half a percentage point.

The counterfactual is now flat. Removing every gas-exchange step from the pressure
forcing changes its level by 0.00 dB and no band share by more than 0.1 of a
point at five of the six points, which is the same statement as the first table
made twice.

**One residual step remains and it is a calibration consequence, not a leftover.**
`brake-1300` reads 0.0371 at intake valve closing — a factor of 25 below the worst
assignment it replaced, invisible to the blind detector, worth 0.17 dB of crest
and no band share. Its cause is the recalibration: retarding intake valve closing
to 40° ABDC left the brake's charging lobe opening only 2° later, so during
braking a cylinder shuts its intake valve and is immediately charged from the
exhaust manifold through a lobe facing a large pressure difference. That is a real
fast flow event rather than an assignment, and widening the gap between the two
events is the lever on it if it ever matters.

Idle was the worst case before for a reason that was not the step size — the steps
there were the smallest measured. It was that idle has the least combustion noise
to hide them behind: no boost, the gentlest premixed rise, and a block path nearly
29 dB below the one at rated. That is also why it is where the fix shows most.

The other two forcings are clean, and that matters as much:

- **The exhaust source.** No valve event moves it by more than 0.003, because
  what it radiates *is* the flow through the port and the flow is what the
  transition is about. The blind detector does flag 0.8 to 1.7% of it at four of
  the six points, at magnitudes down to 0.0001 — that is the near-Nyquist limit
  cycle the clamped port transfer is documented to carry. Removing every one of
  them changes the path's level by at most 0.02 dB and no band share by more than
  0.1 of a point. The claim that this residue is real and inaudible has been in
  this document for three milestones; it is now measured.
- **The body path.** No steps at any operating point, and the largest valve-event
  excess anywhere is 0.0007. Torque is a sum over six cylinders of pressure times
  a geometric factor that goes to zero at the dead centres where the transitions
  happen, so a gas-exchange event arrives weighted by almost nothing. This was
  true when the transitions were assignments too, which is why the body path never
  needed fixing.

**The blind detector is why this cannot be checked by the blind detector alone,
and milestone 11 said so before there was a fix to check.** A difference standing
three times above the differences either side of it is an *isolation* test, and
isolation cannot see a discontinuity that lands on a steep enough slope: at
1800 rpm the crank covers more angle per step, so the neighbouring differences
grow and the ratio falls under three while the step itself grows. It found nothing
at `rated-1800` in milestone 11, where the largest assignment in the model sat. It
finds nothing there now either — and the two readings mean opposite things.

Which is exactly why the probe reports both, and why the row that carries the
result here is the valve-event attribution rather than the blind one: 0.9288
against 0.0017 at the transition the solver *knows* happened. A measurement that
reads "nothing" both before and after a fix has told you nothing about the fix.
Pointing a new instrument at a signal whose answer is already known is what found
the modulation detector in milestone 8 and the octave error in milestone 10; here
it is what stopped a clean blind result from being mistaken for evidence.

#### What the drive contains

**What the traces contain** asks whether each forcing is continuous. This asks
what is *in* it, and the two questions have different answers.

The measurement is the per-path captures and the forcings themselves, both run
through `reference_probe` — which estimates a firing rate from the signal and
reports the share of energy in each of the first four orders. Pointing it at a
capture whose speed is already known is the same trick that caught the octave
error in milestone 10.

| Energy in orders 1 / 2 / 3 / 4 | `idle` | `cruise-1200` | `full-1400` | `rated-1800` |
|---|---|---|---|---|
| **Torque forcing** (drives the body) | 87.8 / 11.0 / 1.1 / 0.1 | 86.8 / 11.4 / 1.4 / 0.3 | 89.3 / 9.7 / 0.8 / 0.1 | 87.2 / 11.2 / 1.3 / 0.2 |
| **Pressure forcing** (drives the block) | 87.5 / 10.2 / 1.4 / 0.4 | 85.4 / 9.9 / 2.5 / 1.2 | 91.4 / 6.2 / 1.2 / 0.6 | 85.5 / 10.4 / 2.4 / 1.0 |
| **Mouth flow** (drives the exhaust) | 19.7 / 31.9 / 9.3 / 10.8 | 49.1 / 29.9 / 13.0 / 4.0 | 38.3 / 34.5 / 21.0 / 4.4 | 74.9 / 12.4 / 10.3 / 0.8 |
| Crest, torque / pressure / mouth | 4.0 / 7.1 / 11.5 dB | 4.9 / 7.1 / 8.8 dB | 4.8 / 6.2 / 8.6 dB | 4.8 / 6.9 / 7.4 dB |

**The torque forcing is a sine wave.** It reads 87 to 89% in its fundamental at
every operating point, with a crest factor of 4.0 to 4.9 dB against the 3.0 dB a
pure sine gives. The exhaust drive, by contrast, is a genuine pulse train — 20 to
49% in the fundamental, energy spread across four orders, crest up to 11.5 dB —
because it *is* a train of blowdown events.

That propagates straight through. The body path as radiated:

| Body path | `idle` | `cruise-1200` | `full-1400` | `rated-1800` |
|---|---|---|---|---|
| Orders 1 / 2 / 3 / 4 | 33.0 / **64.1** / 2.5 / 0.3 | **97.4** / 2.0 / 0.4 / 0.1 | **84.2** / 14.8 / 0.8 / 0.1 | **95.8** / 3.8 / 0.4 / 0.0 |
| Crest | 7.0 dB | 4.8 dB | 5.7 dB | 5.1 dB |

**So the low end of this engine is a hum and not a growl**, and that is a
statement about a harmonic series rather than about a level. A diesel's bass is
the firing frequency *plus* a long series above it; ours is one order carrying
95% of the path.

Idle gives away the mechanism. There the *second* order dominates at 64.1%,
because at 550 rpm the second order is 55 Hz and the body modal bank's lowest
resonance is at 60 Hz. **The bank is selecting whichever order lands nearest its
bottom mode**, which is what a bank of four resonators spanning less than two
octaves at Q 4 to 5 does. Its provenance note says it is "placed to straddle the
firing frequency across the whole speed range", and it does exactly that — a
design that passes the fundamental is a design that rejects the harmonics.

And there is nowhere for those harmonics to go even if the drive had them:

| Frequency | 60–220 Hz | **220–750 Hz** | 1.4–3.6 kHz |
|---|---|---|---|
| Body bank weights | 1.0, 1.0, 0.8, 0.5 | *(bank ends at 220 Hz)* | — |
| Block bank weights | — | **0.25, 0.4, 0.9** | 3.0, 7.0, 12.0 |

The body bank fades out where the block bank has not yet arrived, so the
structure-borne response has a trough from roughly 220 Hz to 750 Hz — precisely
where the third through eighth firing orders sit. The block bank's ascending
weights are deliberate and documented, but their side effect is that the one
band a diesel growls in is the quietest thing in the model.

Recorded rather than fixed, under **Known deficits**. It is a source change with
two independent parts — a drive that has harmonics, and a path that passes them —
and it lands on the `<80 Hz` criterion that is already failing.

#### Known deficits

Recorded here rather than tuned away, because tuning a number the day it is first
measured is how the previous round of acceptance criteria came to enforce the
defect they were meant to catch.

**Fixed in milestone 9**, and kept here because what they turned out to be is the
point:

- ~~The engine brake is being clipped, hard.~~ It was, and it was the *stage*
  rather than the source: 7.4 dB of crest at the output against 12.9 dB one stage
  upstream. The level follower passes the release lobe and `brake-1300` now reads
  12.4 dB. It is still held down by 12 dB of gain reduction — that figure did not
  move, and was never the problem.
- ~~The limiter engages at every fuelled point above idle.~~ It engaged because
  the clipper acted on every sample it saw, so "engaged" included rounding a
  signal that was nowhere near the ceiling. With a transparent knee it engages at
  three of the ten scenarios, and where it does it changes the level rather than
  the shape.

**Closed in milestone 12**, and what closed them is the point:

- ~~`80–300 Hz` at governed idle is 41.7% against a 45% floor.~~ It now reads
  45.4%. Nothing was done to the band and no criterion was touched: the block path
  was ringing a boundary assignment, that artefact lived above 2 kHz, and the
  shares are shares. Removing it moved weight back into the band that was short.
- ~~`rated-1800` has the exhaust only 5.8 dB in front of the block.~~ It now reads
  6.3 dB. Same cause, read a different way — the block was 0.5 dB louder than the
  engine warranted because part of what it radiated was an impulse train, and the
  balance criterion is a ratio against exactly that path.

Both were listed as failing for two milestones and neither was ever a balance
problem. They were one defect in the source, seen through two criteria.

**Still open:**

- **Governed idle sits 39.3% below 80 Hz, against a 35% ceiling.** A six at
  551 rpm has its fundamental at 27.5 Hz and its second order at 55 Hz, so the
  energy is genuinely there and the model is not obviously wrong. What is wrong is
  the criterion: a fixed ceiling asks "can ordinary hardware reproduce this" at a
  frequency that sweeps by a factor of four across the range, and at the bottom of
  the range the honest answer is *not the fundamental, no*.

  Milestone 12 moved it from 40.8% to 39.3%, which is a side effect rather than an
  attempt and does not change the situation.

  **There are now two independent pieces of evidence against the criterion and
  none for it.** Milestone 10 measured the reference this was waiting on, and it
  reads **89 to 92% below 80 Hz at idle** across three independent stretches — so
  the model is the *less* low-heavy of the two, by a wide margin. And the first
  listening report on the model, recorded in milestone 12, is that it has *too
  much high frequency and not enough low* — which points the same way as the
  reference and the opposite way from the ceiling. See **What listening has said
  so far**.

  The number is still left as it stands, failing, and the criterion is still not
  widened, because that reference is a **sampled source played back by a game** and
  one listening report is one listening report. What has changed is that the
  question has evidence on one side of it and a convention on the other, and that
  is the state in which a criterion normally gets changed. It is the next decision
  rather than this milestone's.
- **`light-900` has the exhaust only 5.2 dB in front of the block**, under the
  6 dB floor, and it did not move in milestone 12 while `rated-1800` did. Light
  load is where ignition delay is longest and the premixed fraction largest, so
  the block is loudest relative to the exhaust exactly where the balance criterion
  is tightest — and that loudness is combustion rather than artefact, which is
  why removing the artefact did nothing here. This is the one balance figure that
  is asking a real question about the model.
  **It is not waiting on the reference, because the reference cannot ever answer
  it.** A ratio between two radiating paths needs the two paths measured
  separately, and the benchmark is a stereo mix in which they arrived added
  together. Milestone 8 recorded this as one of three things blocked on
  characterisation; milestone 10 establishes that it is blocked on something else.
  Settling it needs either a multi-microphone measurement of a real engine or an
  argument from mechanism, not more analysis of this file.

**Still visible, because the clipper had been flattering it:**

- **`brake-1300` puts 55.9% in `300 Hz–2 kHz` against a 30% ceiling**, and 42.1%
  in `80–300 Hz` against a 45% floor. Under the old stage these read 40.4% and
  54.4%, and that was attributed to clipping being broadband. It was the reverse.
  The clipper was reducing the tall fast release-lobe pulses — which is where this
  path's high-frequency energy lives — more than the quieter body of the signal
  between them, so it was moving share *out* of the upper band. Passing the pulses
  intact shows what the source actually produces, and its loudest octave is
  `315–630 Hz`.

  Whether that is wrong is genuinely undecided. A decompression brake's bark is a
  hard high crack, and a criterion of 30% above 300 Hz was written against fuelled
  operation where it is not one. This is the first measurement of what the brake
  path really contains, and it belongs with the reference characterisation rather
  than with a gain adjustment.

  Milestone 10 went looking and came back empty. The benchmark's only loaded
  material is a 14-minute test drive dominated by a component at half the firing
  rate, over a firing comb holding 18% of the energy — the signature of a sampled
  and looped source rather than of an engine — so no passage in it can be
  identified as engine braking with any confidence. The question stands where it
  stood, now with the search on record.

**Fixed in milestone 12**, and kept because the shape of the diagnosis is worth
more than the fix:

- ~~The block path is partly ringing a boundary assignment.~~ It was, and it was
  worth 13 of the 21 percentage points above 2 kHz at governed idle. Both assigned
  transitions became orifice flow, the largest of them fell from 0.929 to 0.002,
  and the block path's `>2 kHz` share went to 8.6% against the 8.1% a perfect
  removal predicted. See **What the traces contain**.

  Three things were written down before the fix and all three can now be checked
  against it, which is the only reason to record predictions:

  - The counterfactual was labelled an **upper bound**, on the grounds that a real
    valve still makes a fast edge. It was within half a percentage point
    everywhere. The supporting argument — that the exhaust side's 35° ramp is
    about 130 solver steps at 1800 rpm — was labelled "a reason, and it is not
    evidence". Correct reason.
  - The **cheaper alternative was rejected on principle**: relaxing intake
    pressure towards the manifold while still trapping by volumetric efficiency,
    which would have removed the discontinuity without moving a calibrated peak.
    Rejected because pressure and mass would then disagree at a known volume. That
    rejection cost what it was predicted to cost — every peak in
    **Results → Calibration** moved and two valve-timing values had to be refitted
    — and the model is coherent with the gas law instead of quietly not.
  - The **objection** was that the intake sits near equilibrium, which is where an
    explicit orifice solver overshoots into a two-sample Nyquist limit cycle. It
    was answered by pointing at the settle clamp `flow::exchange` already carries.
    That held: the pressure forcing carries no detectable residue at any operating
    point, and the intake side contributes none to the exhaust source's documented
    0.8–1.7%.

  One thing was **wrong**, and it was a scope estimate rather than a physical
  claim: this was recorded as taking "the intake manifold's own mass balance" with
  it. It did not have to. Both manifolds are still mean-value, which is what the
  exhaust side had always done — see **Modelling simplifications**.

**Newly measured, in milestone 13, and the reason a listening request could only
be half delivered:**

- **The body path's level relative to the exhaust swings 12 dB across the
  operating range.** It sits 1.1 dB behind the exhaust at governed idle, 2.8 dB
  behind at cruise, and **12.8 dB** behind at full load. So the roar recedes as
  the engine works harder, which is the wrong way round: a truck's cab gets more
  boomy under load, not less.

  | Exhaust leads the body by | `idle` | `light-900` | `cruise-1200` | `full-1400` | `rated-1800` |
  |---|---:|---:|---:|---:|---:|
  | | 1.1 dB | 6.8 dB | 2.8 dB | 12.8 dB | 9.8 dB |

  The mechanism is not mysterious. The exhaust source is port mass flow, which
  grows steeply with fuelling and boost. The body source is gas plus pumping
  torque **normalised by rated torque**, which is bounded above by construction —
  it cannot exceed about one however hard the engine is worked. One source has a
  ceiling and the other does not, so their ratio has to move with load.

- **The bass has no harmonic series, so raising it produces a hum.** Measured in
  milestone 14 and recorded in full under **What the drive contains**: the torque
  forcing is 87 to 89% fundamental at every operating point with a crest factor
  of 4.0 to 4.9 dB, which is a sine wave, and the body path it drives reads 84 to
  97% in a single order. The exhaust drive over the same captures is 20 to 49%
  fundamental with crest up to 11.5 dB, so this is a property of one source
  rather than of the measurement.

  **This is the deficit behind the one above, and it is why the milestone 13
  request could not simply be granted.** Turning up a path that carries one
  order gives more of that order. What a diesel's low end actually is — the
  reason it reads as a large engine rather than as a tone at the right pitch — is
  the *series*: the firing frequency and a long tail of harmonics above it.

  Two independent things are missing and a fix needs both:

  - **A drive with harmonics.** Six cylinders firing every 120° of crank, each
    contributing a hump of pressure times a geometric factor that lasts most of
    an expansion stroke, overlap into something very close to a sinusoid. Some of
    that smoothness is real — a large six does have smooth crank torque — but 11%
    of energy at the second order and 1% at the third is at the bottom of the
    plausible range, and a real cab is also excited by each combustion event
    shaking the block, which the torque *reaction* does not represent at all.
  - **A path that passes them.** The body modal bank spans 60 to 220 Hz at Q 4
    to 5, so it selects whichever order lands nearest its bottom mode — visible
    at idle, where the second order carries 64% because 55 Hz sits next to the
    60 Hz resonance. Above 220 Hz the body bank has ended and the block bank's
    weights are 0.25 and 0.4, so the structure-borne response has a trough from
    220 to 750 Hz. That is exactly the band the third through eighth firing
    orders occupy.

  Neither is attempted here. Both move band shares, both move the `<80 Hz`
  figure that is already failing its criterion, and the second one interacts
  with the block bank's ascending weights that **Modelling simplifications**
  records as compensating for a different shortfall. Changing three coupled
  calibrations in the milestone that first measured any of them is how the
  previous acceptance criteria came to enforce the defect they were meant to
  catch.

  **This was found by trying to fix something else.** A listener asked for the
  body path to come up; the obvious answer is a cab trim; and a trim large enough
  to satisfy the request at *load* makes the cab 6 dB louder than the raw stage at
  *idle*, because the same path is 43% of the idle mix and 5% of the loaded one.
  The level-match assertion caught it. What the cab ships is the part that fits
  inside that constraint, and it is modest.

  **The fix is a source change and it is not a gain.** Raising `audio.body_gain`
  moves both ends together and would break the idle end first — it is the same
  constant meeting the same problem one stage earlier. What is wrong is the
  *scaling*, so the candidates are a body forcing that is not normalised by a
  fixed rated torque, or one driven by torque fluctuation rather than torque
  level. Either changes a radiating source, so it moves the band shares, the
  crest factors and the `<80 Hz` figure above — and that last one is already
  failing its criterion in the direction this would push it. Which is why it is
  recorded here and not attempted in the same milestone as the measurement.

**Newly measured in milestone 12, and not caused by it:**

- **The air path limit-cycles at high speed under load.** At full pedal against
  1500 N·m the engine settles near 1900 rpm, and there the wastegate loop does not
  converge: boost swings between roughly 25 and 155 kPa over tens of seconds. The
  full-load sweep has been reporting `conv false` at 1900 rpm for several
  milestones, which is the same fact in one column.

  **It is not new and it was not found by looking for it.** Milestone 12 shifted
  the *phase* of the oscillation, which made a turbo test that sampled boost at one
  instant fail — and measuring the previous solver at the same operating point
  found the same oscillation at the same amplitude. `tests/turbo.rs` now averages
  boost over a second rather than reading it once, so the suite no longer depends
  on where in the cycle a run happens to stop, and the deficit is recorded here
  instead of being absorbed by a lucky assertion.

  It does not touch any figure in **Results → Calibration**, which is measured at
  held speeds where the loop is stable, and it is upstream of nothing acoustic:
  boost that swings over tens of seconds is not a sound. It is a control
  calibration defect and belongs with the wastegate gains.

None of the milestone 8 deficits was visible before it, for three separate
reasons: two of the four operating points did not exist as scenarios, the
limiter's engagement was never reported, and idle was being measured somewhere the
engine does not idle. What made this round's diagnosis possible was one further
measurement — the same mix with the limiter divided back out — which turns "the
gain fell to 0.247" into "and it cost 5.5 dB of crest doing it". The first figure
alone had been on record for a milestone and read as evidence about the brake.

**The paths are summed at the listener, not in the solver.** Each is a channel
of its own from the ring buffer through to the Web Audio graph, where `cabin.ts`
gives it its own transfer. What the solver still does is decide the *saturation*,
because that is a property of the mix: it derives one gain from the summed signal
and scales all three paths by it. The gain is never above one, so the three
channels sum to exactly the single sample this used to emit — which is why
splitting the paths in milestone 7 left the **raw** listening stage
bit-comparable to milestone 6.

**The gain follows the level rather than the sample.** When the mix needs less
gain than it is getting it gets exactly what it needs, on that sample: an
instantaneous attack, which is what bounds the output to the knee with no
lookahead and no latency. Recovery has a time constant — a calibrated 0.3 s,
about twenty firing periods — so over a steady passage the gain settles to very
nearly a constant, and a constant gain preserves pulse shape and crest factor
exactly. Below the knee nothing is touched at all.

The predecessor did none of that. It was `knee · tanh(mix / knee)` evaluated per
sample, which reduces tall samples more than short ones and reduces small ones
too: 2.4 dB down *at* the knee and rounding everything below it. See the crest
table above for what that cost, and the tenth entry in the list below for why the
mistake is not specific to this model.

Three honest limits. The gain decision is made on the *pre-filter* mix, so once
the cab filters each path differently the filtered sum can exceed the knee; the
volume control and the cockpit compressor sit downstream of it. Each channel
carries a final clamp to `[−1, 1]`, because bounding the sum does not by itself
bound three signed terms — two paths in opposition could in principle each exceed
it. Measurement says that never happens, and a test says so too, but a contract
that holds only in practice is not a contract. And a gain with a release time is
a function of the run rather than of the sample, so it cannot be recovered from
the output by arithmetic: the solver records what it applied to each frame, and
the capture tool divides *that* out. The `atanh` inverse that did this job for two
milestones is gone rather than left in place describing a stage that no longer
exists.

**Two measurements exist because band shares are blind to them.** A pulse train
and a tone can hold identical energy in every band. *Crest factor* — peak over
RMS — is 3 dB for a sine and double figures for a series of distinct combustion
events, and it collapses long before saturation measures as distortion: during
calibration it read 3.5 dB with the gains too high, which is a signal squared off
into a buzz. *Firing-rate modulation depth* asks whether the signal's envelope
swings at the firing rate, which is the difference between an engine and a hum at
the right pitch.

The audible band starts at 150 Hz because that is where a small speaker begins
reproducing anything, which is the question that criterion asks — and only that
question. Bands run to 15 kHz rather than to Nyquist: the explicit port transfer
leaves a two-sample limit cycle within a whisker of Nyquist which is arithmetic
rather than sound, and the probe reports it separately so it cannot satisfy a
high-frequency target it is not signal for. The probe's four full-range bands sum
to 100% as a self-check.

Eleven mistakes this chain invites, all of which were made:

- **A boundary condition cannot make a sound.** Clamping cylinder pressure to the
  manifold during the exhaust stroke makes the pressure difference across the
  port identically zero. Real orifice flow had to come first.
- **And a boundary condition can make a sound it should not.** The mirror of the
  one above, found ten milestones later and fixed in milestone 12. A clamp does
  not merely fail to produce the pulse it should — it produces a step it should
  not, because the two descriptions either side of it do not agree, and a step
  differentiated is an impulse and an impulse is white. The lesson is not about
  clamps. It is that the moment a trace is *radiated*, every simplification in it
  is promoted from an approximation with a bounded error to a signal with a
  spectrum, and those are judged by completely different standards. Nothing about
  the intake clamp changed when the structural path was added in milestone 5; what
  changed was what it was being asked to be, and it took six more milestones to
  notice because the pressure trace it appears in looks perfectly reasonable.
- **A calibration constant applied at an instant is a step.**
  `air_path.volumetric_efficiency` was 0.92, multiplied into the charge at the
  moment the intake valve shut, and the intake-closing step measured 6 to 8% of
  cylinder pressure. Those are the same number: the step *was* the constant,
  arriving all at once. A trapping efficiency is the integral of a throttling
  loss over an induction stroke, and the difference between modelling it that way
  and multiplying by the answer is invisible in every quantity except the one that
  gets radiated. Its replacement is a port area, and trapping is now an outcome
  that varies with speed on its own — which is also why it could not be refitted
  by scaling the new area, and valve timing had to carry it.
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
- **A boundary that has already added things up cannot be un-added.** The three
  paths were summed in the solver for two milestones, so one cab transfer acted
  on a mixture of a tailpipe several metres away, an engine two feet away through
  the bulkhead, and a frame arriving through the seat. The compromise is legible
  in what the shared low pass had to be: 1.7 kHz, then 2.6 kHz, neither right,
  because one number cannot describe two routes. Splitting the paths was not a
  refactor in search of a benefit — it was the only way to stop the filter
  contradicting itself.
- **A path driven by the wrong quantity cannot be fixed with gain.** Cylinder
  pressure drives the block's bending and breathing modes and produces clatter.
  No setting of its gain produces low-order weight, because the drive has none:
  a premixed spike is a fast edge once per cylinder per cycle. The roar is
  crank *torque*, which swings at the firing rate itself, and until something was
  driven by it the model had no path that could make the sound. Adding gain to
  the wrong path is how an engine ends up loud and small at the same time.
- **A ceiling applied to the sample is a waveshaper, not a limiter.**
  `knee · tanh(x / knee)` bounds the output, keeps it continuous, keeps it
  differentiable, sounds like the right answer, and reduces a tall sample by more
  than a short one — which is *precisely* the operation that turns a train of
  distinct combustion events into a buzz. It also acts below the knee, so it was
  shaping the entire operating range rather than catching the top of it. A
  limiter changes the level: it works out how much the passage needs turning down
  and turns all of it down by that, which is why crest factor survives.
  Diagnosing this needed a measurement that did not exist for two milestones —
  the same mix with the gain divided back out — because how far the gain fell is
  the same number either way. It read 0.247 on the brake in both cases; in one of
  them the waveform came through and in the other it did not.

### Reproducible scenarios

Every acoustic figure above names a scenario, and a scenario is a seed, initial
conditions and a script of control phases. The point is that a comparison can be
*regenerated*. Two recordings of "roughly full load" are not a comparison; two
runs of `full-1400` are.

| Scenario | Kind | What it is |
|---|---|---|
| `idle` | steady, **governed** | No pedal, no load, the governor holding it. 551 rpm against the published 560 target |
| `light-900` | steady, held | 25% pedal at 900 rpm: long ignition delay, large premixed fraction, the rattle |
| `cruise-1200` | steady, held | 60% pedal at 1200 rpm |
| `full-1400` | steady, held | Full pedal at 1400 rpm, the peak-torque region |
| `rated-1800` | steady, held | Full pedal at 1800 rpm, the calibrated power peak |
| `start` | transient | Cranking from rest at 0 rpm, catching, settling to idle at 571 |
| `stop` | transient | Idling, then ignition off: the run-down to rest |
| `accelerate` | transient | Sixth gear, part pedal to full: 1110 → 2075 rpm over five seconds |
| `release` | transient | Ninth gear on a 3% climb, pedal released: 1428 → 1301 rpm over three |
| `brake-1300` | steady, held | Decompression brake, stage III, pedal released, at its published anchor speed |

Three distinctions the table encodes, each of which was previously implicit:

- **Held speed and free running are different measurements.** A held phase pins
  the crank every 2.5 ms as a dynamometer does; a free phase lets it go where the
  physics takes it. Holding is how a steady spectrum is measured — an unloaded
  engine at full pedal leaves the point being measured inside a few hundred
  milliseconds, and a spectrum taken across that is smeared over whatever it
  swept — and it is *not* a claim that a truck behaves this way. Every result
  reports the speed the engine actually reached, and whether it was held.
- **Idle is governed, not held.** The governor is part of how an idling diesel
  sounds. Pinning a chosen speed and calling it idle measures something else, and
  did.
- **The two free transients are driven in gear.** The engine's own inertia is
  3.5 kg·m²; a full-pedal pull against a resisting torque either stalls it or
  slams it into the governor inside a tenth of a second, and neither is what a
  truck accelerating sounds like. In gear the forty-tonne combination appears at
  the crank as tens to hundreds of kilogram-metres-squared and the speed rises at
  a rate the ear can follow. The first attempt at `accelerate` — full pedal
  against 900 N·m from idle, out of gear — stalled the engine to 22 rpm, which is
  a scenario that measures a stall.

A transient's metrics are reported as **null** rather than as numbers. The
firing-rate metrics need a firing rate that stands still for the length of the
window, and a run-down passes through every speed below idle; a comb share
against the mean speed of a sweep is a measurement of a frequency the engine held
for a fraction of the capture. Reporting a number there invites it to be compared
with a steady one.

### Capturing scenarios as files

`cargo run --release -p sim-core --example audio_capture` writes every scenario to
`target/audio-capture/<id>/` as 32-bit float WAV at 40 kHz, with a
`manifest.json` recording the conditions and every metric for every track.

| File | What it is |
|---|---|
| `mix.wav` | the three paths added up: the raw listening stage |
| `exhaust.wav`, `block.wav`, `body.wav` | each radiating path on its own |
| `*-unsaturated.wav` | the same paths with the limiter's gain divided out |
| `mix-unsaturated.wav` | those paths summed: the mix before the limiter |

**The unsaturated set is exact, because the solver reports the gain it applied.**
The limiter derives one gain from the mix and scales all three paths by it, so
dividing each sample by the gain its frame carried recovers what the solver
produced before the limiter touched it. The gain rides alongside the frames in
`Acoustics`, indexed off the same cursors so it cannot slide out of step with
them, and `drain_audio_with_gains` hands both to the capture harness. The
playback path does not drain it: the gain is already in the samples, and a fourth
interleaved value would be a channel the listening stage has to know about and
does not need.

This used to be arithmetic instead. The clipper was a memoryless function of the
mix, so `knee · atanh(sum / knee)` recovered the pre-saturation value from the
emitted one, and no solver change was needed. A gain with a release time is a
function of the whole passage, and nothing can be done to one output sample to
find out what it was scaled by.

The files are written only for scenarios where the limiter engaged, and the
manifest records how far it did *and what it cost*: a set of files identical to
the ones beside them is a set someone will later compare and draw a conclusion
from. **The pair is also how the limiter itself is measured**, and it is what
found the defect milestone 9 fixed. How far the gain fell says nothing about
whether the waveform survived — a slow gain and a per-sample one report the same
minimum. The crest factor of the two mixes is the difference.

**There is no cockpit-stage file.** The cab is a Web Audio graph in `cabin.ts`;
reimplementing it in Rust to export it would be a second copy of a listening
stage that would immediately start drifting from the one people actually hear.
Cockpit comparison happens in the browser, against these files, through the
comparison controls below.

Float WAV rather than 16-bit PCM: quantising would put a dither decision, or a
truncation artefact, between the solver and the comparison.

### Measuring the sound

Everything above is a number produced by `sim_core::analysis`, and this section
is about that module rather than about the engine. It exists because the previous
arrangement — the metrics defined inside the probe that printed them, and again
inside the tests that asserted on them — produced a metric that was wrong for two
milestones without anything noticing.

**A metric that only ever sees engine output has nothing to fail against.** So
`tests/analysis.rs` points every metric at four signals whose answer is known
before the code runs, and asserts what each must read:

| | steady tone | 50% AM tone | pulse train | white noise |
|---|---:|---:|---:|---:|
| Crest factor | 3.0 dB | 6.0 dB | 14.2 dB | ~4.8 dB |
| Modulation depth | 0.00 | 0.50 | 2.59 | < 0.15 |
| Firing orders | 100% at `f0`, 0% off it | 0% | 48.5% | < 1% |

The probe prints that table on every run, above the engine figures, so the scale
the engine numbers sit on is visible rather than assumed.

**The modulation detector was replaced, and it was giving false positives.** The
old one rectified the signal, low-passed it to an "envelope", and looked for
content at `f0` and `2·f0`. But rectification *manufactures* what it then finds:
`|A cos ωt|` is not constant — it is `2A/π` plus even harmonics of the carrier,
the first of which sits at `2ω` with amplitude `4A/3π`. Point that detector at a
steady 60 Hz sine and it reports **0.42**, against documentation promising
"nearly zero however loud it is". It was measuring its own rectification, and
0.42 comfortably satisfied the `> 0` criterion written against it.

The replacement takes the **analytic envelope** — the magnitude of the signal
after a frequency-domain Hilbert transform — for which a tone is exactly flat at
any frequency, and reports the envelope's fractional swing at `f0` and `2·f0`
against its mean. A tone reads 0. A tone modulated to depth *d* reads *d*. That
last property is what makes it checkable rather than merely plausible, and it is
why the figures in the table above are larger than milestone 7's: they are a
different measurement, not a change in the engine.

The same module now supplies crest factor, RMS and the firing frequency to
`tests/acoustics.rs`, which had its own copies. Two definitions of one metric is
how a test and the probe it is meant to agree with come to disagree.

**Milestone 10 added the one metric that has to find its own reference
frequency.** Every measurement above needs an `f0`, and for the model that comes
free from the crank speed the solver is already integrating. A recording does not
come with one, so `estimate_firing_hz` recovers it from the signal — and it
arrived with the same defect as everything else in this file's history, caught
the same way. Pointed at captures whose speed was known it read two of the six an
octave high, because it was asking whether a candidate's fundamental was loud
rather than whether the lines between its lines were empty. **Characterising the
reference** has the case, the fix and the table of all six recovered to within
0.5%.

### Playback audit

The worklet is the last thing between the solver and the speaker, and until
milestone 8 nothing tested it. `web/test/worklet.test.ts` loads
`exhaust-processor.js` — the file that ships, through the same `?raw` mechanism
the app uses — evaluates it with the three globals a worklet realm provides, and
drives it at **both** common device rates. Neither divides the solver's 40 kHz,
which is the whole reason there is a resampler to audit.

Eleven properties, at 48 kHz and at 44.1 kHz: silence until the cushion fills; a
300 Hz tone in is a 300 Hz tone out; the three paths stay sample-aligned through
the resampling; imaging stays below −38 dBc; a dry ring outputs silence rather
than repeating itself, and counts it; both ends of a dry spell fade rather than
cut; an overrun drops whole frames and stays aligned; a block with the wrong path
count is refused rather than de-interleaved on a guess; the drift trim pushes
towards the buffer target and not away from it, by no more than 1%; the source
rate is taken from the message; and a flush clears the interpolator's state as
well as the ring.

Resampler imaging, measured with the buffer held still so the drift trim is not
sweeping the read rate:

| Input | 48 kHz worst product | 44.1 kHz worst product |
|---|---|---|
| 900 Hz | −64.3 dBc at 8.9 kHz | −65.3 dBc at 5.0 kHz |
| 2.6 kHz | −46.0 dBc at 10.6 kHz | −47.0 dBc at 6.7 kHz |
| 3.8 kHz | −39.7 dBc at 11.8 kHz | −38.0 dBc at 7.9 kHz |

The 48 kHz column reproduces the figures recorded by hand in the worklet's own
header, landing at the frequency it said they would. **44.1 kHz had never been
measured**, and is no worse. Above about 16 kHz the products rise — the worst at
900 Hz is −54.8 dBc at 23.1 kHz, a third-order image — which is inaudible and is
excluded from the table on that basis rather than overlooked.

Two things the audit found in its own harness rather than in the worklet, both
worth recording because they would mislead the next person to measure this:
feeding the ring one large buffer and rendering against it makes the buffer drain
throughout, which sweeps the drift trim, which frequency-modulates the output and
puts sidebands 10 to 25 dB above the real imaging. And any assertion on a counter
has to render at least 32 quanta, because that is the reporting interval.

**Not audited.** Sustained real-time throughput and latency drift under load are
browser measurements, and this suite is not a browser. What is measured here is
that the chain is arithmetically correct at both rates; what a machine can sustain
is reported by the underrun counter in the sound panel, live.

### Comparing against a recording

The sound panel carries a **level-matched A/B** between three sources: the live
engine, a `Reference` file, and a `Candidate` file. The two file slots are the
same mechanism with different names — reference is what the model is compared
*against*, candidate is what it is compared *with*, and the intended candidate is
a `mix.wav` from `audio_capture` taken before a change.

**An untrimmed comparison is a comparison of loudness.** Louder wins, and it does
not take much: a decibel is enough to swing a preference and far too little to
notice as a level difference. So the comparison is *switched* rather than mixed —
one source at a time, because a crossfade would spend its duration playing a
mixture — and every clip is trimmed to the engine's own measured output level
before it plays. The clip's RMS and peak are measured from its decoded samples,
the gain is shown in decibels, and it is clamped two ways: 24 dB of travel at
most, and never enough to lift a clip past full scale. A clip that cannot reach
the target says so rather than presenting a clamped gain as a match.

Matching is explicit, not continuous. An automatic match would be a compressor
keyed to the engine and would flatten the loudness differences between operating
points, which is one of the things being judged — so it re-matches on load and on
request, and the panel says to re-match after changing speed or load.

The reference joins the graph **after** the cab stage and before the volume
control. A recording made in a real cab has already been through one; running it
through ours would filter a cab through a cab.

**Files are read in the browser and never leave it.** There is no backend to send
them to, and the reference material is somebody else's recording.

### Where you are listening from

The solver's output is a *tailpipe* signal. A driver is not at the pipe mouth, so
the stage is switchable: **raw tailpipe** (unfiltered, what the spectrum view was
built to verify) or **truck cockpit** (default).

The cab is a Web Audio chain in `cabin.ts` and `audioEngine.ts`, not a change to
the physics. **Each radiating path takes its own route to the driver**, and then
everything shared happens once:

| | own EQ | delay | room send |
|---|---|---:|---:|
| Exhaust | low pass 1.6 kHz, −3 dB at 400 Hz | 12 ms | 1.0 |
| Block | low pass 3.2 kHz, +2 dB at 1 kHz | 2 ms | 0.4 |
| Body | low pass 250 Hz, +1 dB at 120 Hz | 0 ms | 0.0 |

The exhaust leaves a stack several metres behind and below and arrives through
the rear wall and a length of outside air: the most muffled of the three, and the
only one with a propagation delay worth having — 12 ms is about four metres, and
it is what stops the tailpipe and the block sounding like one source in one
place. The block is two feet away through the bulkhead, the one thing genuinely
in the cab with the driver, so it keeps its top end. The body arrives through the
mounts, the frame and the seat with no air path at all, so it gets **no delay and
no room whatever**: a structure-borne path does not arrive as an early reflection
or a diffuse tail, and reverberating it would be inventing an acoustic route it
does not take.

**There is no shared low pass any more, and its absence is the point.** It was
1.7 kHz while the solver produced nothing above that, then 2.6 kHz once the
structural path put real content at 2.4 and 3.6 kHz — because a cab takes the
sharp edge off an exhaust several metres away but does not silence an engine two
feet away through the bulkhead. Both figures were one number trying to describe
two different routes, and neither could.

Then, shared: 30 Hz high pass, a +5 dB low shelf at 150 Hz, +2.5 dB at 200 Hz,
−2 dB at 600 Hz, two early reflections at 7.3 and 11.9 ms panned apart, a seeded
impulse response, and a compressor. Those describe the cab as a box and the
listener's speaker rather than any one source, so they act once on everything
that reaches the driver rather than once per path and again through the
reflections. Both stages always run; switching crossfades over 40 ms.

Three of the shared stages moved when the *source* changed in milestone 6,
because each had been tuned against a signal that no longer exists:

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
compressed loud end far less, so trim-then-make-up (0.42 in, 1.39 out) brings
both together:

| Measured at the output | Idle | 90% pedal, 300 N·m |
|---|---:|---:|
| Cockpit level relative to raw | +2.6 dB | −2.6 dB |
| Low-band over high-band ratio | — | 4.2 against raw's 2.4 |
| 2 kHz and above | — | −45% |

Both ends are asserted within 3 dB by the browser suite through the same analyser
the spectrum view uses, and the suite now *prints* them: a number that appears
only when it fails is a number nobody can calibrate against.

That spread was ±0.8 dB before the paths were split, and it widened for a real
reason rather than through carelessness. Each path now has its own filters, and
the two operating points put their energy in different places — 90% of full load
sits in 80–300 Hz against 66% at idle — so the cab removes different amounts at
the two ends, and no pair of gains can zero both. The pair is centred on the
spread instead: zeroing idle put load at −3.3 dB, and chasing one end is the
exact mistake the input trim was introduced to stop.

**Each path also has a level of its own now, and it is the smallest part of this
document with the largest story behind it.** A listener in the seat reported that
the body path wanted bringing up and the other two down. The body is 1.5 dB up,
the block 3 dB down and the exhaust untouched — and those numbers are small
because the measurement refused to let them be large.

The first attempt was +6 dB on the body, which is what the request sounds like.
It put the cab **+6.0 dB over raw at idle** against −1.9 dB under load, failing
the level match at one end, and every intermediate value traded one end against
the other. The cause is not in this stage: the body path sits 1.1 dB behind the
exhaust at governed idle and **12.8 dB** behind it at full load, so a constant
trim is a large change to the idle mix and nearly nothing to the loaded one. The
window is 6 dB wide and a rebalance opens the spread to about 5 on its own.

What the cab could honestly deliver is above: the low-to-high ratio at the
driver's ear went from 3.7 to 4.2 against a raw stage sitting at 2.4, and about
2 dB more of the top end is now taken out on the way in. The rest of that request
is a **source** change and is recorded under **Known deficits**.

**The cab is lined with soft things, and as of milestone 14 it behaves like it.**
Porous absorbers — seats, headliner, carpet, door trim, a bunk — take around 0.1
of the energy out at 125 Hz and 0.6 to 0.9 by 2 kHz, so reverberation time in a
lined cabin falls steeply with frequency. That is the single most characteristic
thing about sitting in one, and the model had none of it: the generated tail had
**one decay constant for every frequency**, which describes a hard box, and the
early reflections were full-bandwidth delayed copies, which describes bouncing an
exhaust pulse off glass rather than off a seat back.

Three changes, all of them the same physical statement:

| | Was | Now |
|---|---|---|
| Tail decay above 900 Hz | same as below, 0.13 s | **0.035 s**, about a quarter |
| Early reflections | full bandwidth | one-pole low pass at **1.8 kHz** |
| Tail bandwidth | to Nyquist | one-pole at **5 kHz** |
| Tail level | 0.30 | **0.20** |

The last two need their reasons stating, because neither is about absorption.

The tail's bandwidth is there for an arithmetic reason: white noise runs to
Nyquist, so the share of it sitting above the damping corner depends on the
device rate, and the tail came out **11% quieter at 96 kHz than at 44.1 kHz** —
a real defect, caught by the test that has asserted rate-independence since
milestone 7. Giving the noise a corner in hertz fixes its shape and the split
stops depending on the machine.

The tail's level fell because absorbing the top end and then renormalising to
unit energy is a *tone control*, not a soft furnishing: it takes the treble out
and hands the same energy to the bass. Measured, that made the cab **louder at
idle the more of its top end it swallowed** — the opposite of the intended
change. The tail is now normalised against the energy it would have carried
undamped, so absorption genuinely absorbs, and `reverbGain` carries what is left.

Level-matching held through all of it: the cab sits +2.7 dB over raw at idle and
−2.5 dB under load, inside the ±3 dB the browser suite asserts.

**What it does not fix is the treble reaching the driver directly**, and that is
correct rather than a shortfall. Soft furnishings absorb *reflected* sound; the
direct path's losses are the per-path low passes above, which have been there
since milestone 7. The reflections and the tail are a minority of the room mix,
so this is a change of a decibel or so in the output — audibly less brittle,
not a different engine.

None of this is published. The manual says nothing about how the engine sounds
and less about how its cab sounds; these are listening choices and the UI says so
where you switch them.

### What listening has said so far

Every figure in **Results → Sound** is a signal measurement, and the acoustic plan
has recorded from milestone 8 onward that no measurement substitutes for hearing
it. This section exists so that what has actually been heard is on the record at
the same standard as what has been measured — which means it is short.

**Milestone 12, on the build immediately before it.** The report was that there is
**too much high frequency and not enough low**. One listener, one unrecorded
playback chain, no level matching, no blind comparison, no operating point
specified. That is the weakest kind of evidence this document accepts, and it is
recorded rather than discarded because it is the only evidence of its kind and
because it agreed with two instruments that had already been pointed at the same
thing:

- The high-frequency half was **already measured and already attributed**.
  Milestone 11 had found that 13 of the 21 percentage points the block path held
  above 2 kHz at governed idle were a boundary assignment being rung rather than
  combustion. Milestone 12 removed it. Whether that is audible has not been
  reported back on.
- The low-frequency half **agrees with the reference and disagrees with the
  acceptance criterion**. Milestone 10 measured the benchmark at 89–92% below
  80 Hz at idle against the model's 39.3%, and the criterion demands ≤ 35%. Two
  independent sources now say the model wants more low end and one convention says
  less. See **Known deficits**.

**Milestone 13, from the driver's seat.** The second report: **the body path
should be brought up and the other two down**, heard from the seat. Same listener,
same unrecorded conditions, so the same weight — but it is a sharper statement
than the first, because it names a path rather than a band, and the two reports
are consistent: the body path is where the low end lives, carrying 84% of its
energy below 80 Hz.

It also survived contact with a measurement, which is the part worth recording.
Acting on it directly — a large boost on the body path in the cab — broke the
level match between the cockpit and raw stages, and *why* it broke is a finding
about the model rather than about the request. See **Where you are listening
from** for what the cab could deliver and **Known deficits** for the rest.

**Milestone 14, two suggestions rather than a report.** The listener asked
whether the bass was short of a harmonic series, and pointed out that a cabin is
surrounded by soft things that absorb high frequencies. Both were checked and
both were right, and they are the most productive entries in this section so far
because neither was a verdict on the output — they were hypotheses about the
model, and hypotheses can be measured.

- The soft furnishings are now modelled: see **Where you are listening from**.
- The bass harmonic series is **absent and now measured**: the torque forcing is
  87 to 89% fundamental with a crest factor of 4.8 dB. See **What the drive
  contains**. It is the deficit behind the milestone 13 balance request, and it
  is why turning the body path up made the sound boomy rather than fuller.

Worth noting what this does to the weight of the earlier reports. A listener
saying *too much treble, not enough bass* was consistent with two instruments,
which was reassuring but not informative. A listener saying *the bass has no
harmonics* named a mechanism, and checking it took one afternoon and found a
defect nobody had gone looking for in fourteen milestones. The lesson is about
which questions to ask a listener, not about whether to trust one.

What none of this is: a validation. Phase 7 of the acoustic plan asks for blinded,
level-matched comparisons across operating conditions with the playback equipment
recorded, and none of that has happened. A listening report that happens to
confirm the instruments is worth exactly as much as one that contradicts them, and
the reason to write it down is that the next one may.

### The acoustic reference, and what it is not

The comparison workflow is built to be used against a benchmark recording. The
one it was built for is an ETS2 sound mod's demonstration video, whose author
identifies the source as a **2018 Mercedes Arocs 3248 with a 12.8-litre OM 471**,
recorded with synchronised microphones around the engine and exhaust. Its
interior-engine chapter is the driver-seat segment of interest.

Four things it is not, all of which bound what it can settle:

- **It is a later engine.** This configuration is the 2011 OM 471.9 M3D; the
  later 390 kW / 2600 N·m / 2700 bar figures are deliberately kept out of it, and
  a recording of that engine cannot be treated as ground truth for this one.
- **It is a processed game mix**, not isolated microphone tracks and not a
  calibrated sound-pressure measurement. Levels in it mean nothing absolute, which
  is why the comparison matches levels rather than reading them.
- **Its description lists other sound mods.** Anything not the engine has to be
  identified before a window is chosen.
- **Nothing in it is annotated.** RPM and load are inferred, and stationary
  revving does not establish what the engine sounds like under load.
- **It is a sampled source heard from a game's point of view, not a rendering.**
  This is the limit that bounds the others, and it is easy to lose sight of
  because the samples themselves are genuine recordings of a real Arocs. What the
  video contains is those samples *played back by ETS2* — pitch-shifted to the
  current speed, looped, crossfaded between speed layers, and mixed into an
  interior camera position along with a road-and-wind mod.

  Four things follow, and every figure below inherits all of them. **Resampling
  moves what should not move**: a block mode or a pipe resonance sits at a fixed
  frequency in a real engine and does not scale with speed, but pitch-shift a
  recording and it does, so a spectrum taken anywhere other than a sample's own
  speed is not the spectrum of an engine at that speed. **A loop repeats at its
  own rate**, which need not be the firing rate, and whatever was in the sample
  once is then in the signal periodically. **A crossfade is two speeds at once.**
  And **the balance is a mix decision** — where the low rumble sits against the
  clatter is where the mod author and the game's interior attenuation put it.

  So this benchmark is evidence about a sound *design* that a lot of people find
  convincing. It is not a measurement of an engine, and no amount of care with
  the analysis converts it into one.

The files live in the gitignored `datasheet/audio-references/`, are loaded
through the panel's file input at listening time, and are **not** part of the
distributed build. The application ships no audio assets and fetches nothing at
runtime.

**No listening comparison against it has been made yet.** Milestone 8 delivered
the apparatus — reproducible scenarios, exported captures, corrected metrics,
level-matched A/B — and no ear has been applied to the result. Nothing below or
above is a listening result, and a passing signal check is not one either. That
includes everything in the next section, which is measurement.

### Characterising the reference

Milestone 8 built the instruments and pointed all of them at the model. This
points the same ones at the other side. `cargo run --release -p sim-core --example
reference_probe` reads a WAV, cuts it into 1.365 s windows, and measures each one
with `sim_core::analysis` — the module the engine's own figures come from.

**The one thing it has to do that the model's probe does not is find the speed.**
`audio_probe` asks the solver what the crank is doing; a recording does not
answer that question, so `analysis::estimate_firing_hz` recovers the firing rate
from the signal and everything downstream hangs off it. Every rpm below is
therefore **inferred**, and every one is printed beside the comb share that is
its confidence.

#### The estimator, measured against known answers

`tests/analysis.rs` puts it at the traps a pitch estimator falls into, because
both of them produce a wrong answer that looks right — an engine reported at half
or double its speed is still a plausible rpm. A weighted harmonic sum handles the
easy direction: a lone tone at 120 Hz is not a 60 Hz comb, because 60 Hz is empty.

**It does not handle the hard direction, and the model's own captures are what
proved it.** Pointed at `idle`, whose speed is known to be 551 rpm, the first
version reported 1100. The firing fundamental at 27.5 Hz holds 5.4% of that
signal's energy while its second order at 55 Hz holds 19.8%, so the sum preferred
the second order. `light-900` was worse: 1.3% at 45 Hz against 13.6% at 90, and
it read 1805.

The fix is to stop asking whether the fundamental is loud and start asking what a
comb *is*, which is its spacing. If the lines halfway between a candidate's lines
are also there — at `f0/2`, `3f0/2`, `5f0/2` — then the spacing is the half. On
`light-900` those interleaved lines hold half again as much energy as the
candidate's own comb, because they are its real third and fifth orders; on a comb
genuinely spaced `f0` they hold nothing. The two populations differ by a factor of
ten, and the case that exposed it is now a synthetic test with those proportions
rather than a note about a capture.

With that in place, the estimator recovers every steady scenario's speed from its
audio alone:

| Scenario | Speed run at | Estimated from the audio | Error |
|---|---:|---:|---:|
| `idle` | 551 | 550 | −0.3% |
| `light-900` | 900 | 903 | +0.3% |
| `cruise-1200` | 1200 | 1206 | +0.5% |
| `full-1400` | 1400 | 1408 | +0.5% |
| `rated-1800` | 1800 | 1807 | +0.4% |
| `brake-1300` | 1300 | 1297 | −0.2% |

This is the cross-check that makes the reference figures worth reading, and it
was worth running for its own sake: two of those six were being read an octave
out by a tool that would have annotated the recording with the same error and no
way to notice.

#### What the interior chapter contains

The 76-second interior-engine chapter, 1:33 to 2:49, stereo float at 48 kHz,
channels correlated at +1.00 so the mono downmix cancels nothing. 110 windows, 90
of them engine-dominated. Three of those are steady, and they are all the same
thing:

| From–to, s | rpm | comb | dBFS | crest | mod | `150 Hz–15 k` | `<80 Hz` | `80–300` | `300–2k` | `>2 kHz` | octave |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 6.1–9.6 | 507 ±10 | 83.7% | −24 | 8.2 dB | 0.23 | 6.3% | **92.1%** | 4.0% | 3.9% | 0.0% | 20–40 Hz |
| 9.6–15.0 | 502 ±8 | 81.1% | −24 | 8.6 dB | 0.40 | 6.9% | **90.6%** | 5.4% | 4.0% | 0.0% | 20–40 Hz |
| 64.2–73.0 | 499 ±7 | 83.2% | −26 | 9.4 dB | 0.36 | 8.3% | **89.1%** | 6.1% | 4.8% | 0.0% | 20–40 Hz |

Three stretches, minutes apart, agreeing on 499 to 507 rpm. That sits about 10%
under the 560 rpm this engine's manual publishes and under the 551 the model's
governor delivers — near enough to be the same thing, far enough to be a reminder
that it is a later engine in a game. Everything
between them is a rev sweep and is reported as a run of one-window segments
rather than averaged into an operating point, which is the honest answer: a
spectrum of a sweep is a spectrum of everywhere it went.

**The first 3.4 seconds are not the engine**, and how they were excluded is worth
recording. They are the transition out of the previous chapter: a lone line near
108 Hz with nothing above it, which scored 17 to 26% comb share — clear of any
noise floor — and was duly reported as a confident 2170 rpm. A comb share cannot
tell one line from four. So a window has to clear a second test to count, and the
test is that its second order is there at all: those windows hold at most 0.5% of
their energy at the second order and every window with an engine in it holds at
least 1.4%.

**These numbers were checked outside this codebase**, because a band share that
surprising should not rest on one implementation. `ffmpeg` on the same 6.1–15.0 s
stretch reads −23.8 dBFS overall, −24.6 through a six-pole low pass at 80 Hz and
−36.8 through a six-pole high pass at 150 Hz. That is 84% of the energy below
80 Hz and 5% above 150 Hz, against this probe's 91% and 6.5%; the gap is the
filters' skirts, and the agreement is the point.

#### What the test drive contains, and why it is not used

The 14-minute test drive from 6:17 was extracted and scanned in full: 618
windows, 528 of them clearing both confidence tests, median comb share 55%. And
**96% of those read between 400 and 900 rpm**, which a truck under way cannot do
for fourteen minutes — an OM 471 idles at 560.

Running the scan again with the search floor lifted to 900 rpm says what is
happening:

| Search range | Windows accepted | Median comb share | Inferred speeds |
|---|---:|---:|---|
| from 400 rpm | 528 of 618 | 54.8% | piled up at 400–900, impossible |
| from 900 rpm | 352 of 618 | 18.4% | 1000–1600, exactly a cruise distribution |

**The loudest thing in the drive sits at half the firing rate, and the firing
comb itself is weak.** Forced upward the estimator finds a perfectly plausible
set of speeds, and finds them in a comb holding 18% of the energy where the low
line alone holds 15 to 77%. That low line — 25 to 37 Hz, tracking speed, with
harmonics down at 1 to 2% — is not the shape of a firing comb.

For a *sampled* source there are several explanations and this measurement cannot
choose between them: a loop repeating every two firing events, a sub-bass layer
the mod lays under the engine, or a real half-order that resampling and looping
have exaggerated. All three are properties of sample playback rather than of an
engine, which is the point. The drive is a recording of a sound engine, and the
model is a simulation of a diesel one.

That it is a mixture is visible too: `ffmpeg` over 30 s of it reads 48% of the
energy below 80 Hz and 20% above 150 Hz, against 90% and 6% for the stationary
idle. The description names a road-and-wind noise mod and a sound fixes pack, and
this is what they look like.

So the reference characterises **one sound designer's idle, stationary, on a
later engine, through a game's interior mix**. It does not characterise loaded
operation, and it offers no engine-brake passage that could be identified as one.

#### What it settles

| Open question | What the reference says |
|---|---|
| Is `<80 Hz ≤ 35%` right at idle? | Something, but less than it first appears: a well-liked sound mod puts 89–92% there, against the model's 40.8% and a ceiling of 35% |
| Is the exhaust-over-block floor right? | **Nothing, ever.** A stereo mix has no path separation in it; no measurement of this file can produce a per-path ratio |
| Is the brake's 55.2% above 300 Hz a defect? | Nothing yet. No engine-brake passage could be identified, and the material that would contain one is the unusable drive |

The first row is the one worth being careful about, and the criterion is left at
35%, failing, exactly as it was. **The reference is a sampled source in a game
mix**, so its band balance is a design decision — where a mod author and a game's
interior attenuation put the rumble against the clatter — and not a measurement
of an engine. On top of that the two figures are taken at different points in
their chains: the model's is its raw pre-cab mix, the reference's has been through
an interior mix and an Opus encoder.

What survives both caveats is worth having anyway, because the question the
criterion asks is itself a listening question. A sound design that a lot of
people accept as a truck puts nine tenths of its idle energy below 80 Hz. A
ceiling of 35% is not slightly tight there; nothing anyone finds convincing is
anywhere near it. That is a reason to expect the criterion to change, and not yet
a number to change it to.

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
| Valve events | intake valve close 140° BTDC (40° after intake BDC), exhaust valve open 140° ATDC. Each opens and shuts over a 35° ramp. **The intake closing angle moved 20° later in milestone 12** — it used to be a boundary switch with no effect on trapping and is now the dominant lever on it, so the old value was calibrated against a question it was not being asked |
| Port effective areas | 2.5e-3 m² per cylinder at full lift, intake and exhaust alike. Both are *effective*, so a discharge coefficient is inside them; the manual publishes valve counts and nothing else |
| Trapping efficiency | **not assumed — an outcome.** It falls out of port area, valve timing, speed and manifold pressure. `air_path.volumetric_efficiency` survives at 0.92 as the coefficient of the manifolds' mean-value flow, which is a different quantity that happens to share a name |
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
| **Cycle-to-cycle scatter** | ±2% delivered fuel, drawn once per cylinder *per cycle* at intake valve closing from the same seeded stream. Build scatter makes the cylinders differ from each other; this was the only thing making a cylinder differ from its own last cycle until milestone 12, and is now the smaller of two such things — trapping through a real port carries the residual forward, which does the same job at roughly twice the size. Its effect is honestly modest, it is kept because it is still a working lever, and see **Modelling simplifications** for the measurement |
| **Cockpit listening stage** | Per path: exhaust at 0 dB, low-passed at 1.6 kHz with −3 dB at 400 Hz, delayed 12 ms, full room send; block at −3 dB, low-passed at 3.2 kHz with +2 dB at 1 kHz, delayed 2 ms, 0.4 room send; body at **+1.5 dB**, low-passed at 250 Hz with +1 dB at 120 Hz, no delay and **no room send at all**. The three path levels say where the driver is sitting and are applied inside the cab chain only, so the raw stage stays exactly the sum the solver emitted. Then shared: 30 Hz high pass; +5 dB low shelf at 150 Hz; +2.5 dB at 200 Hz, Q 0.9; −2 dB at 600 Hz, Q 1.0; reflections at 7.3 ms (−11 dB, left) and 11.9 ms (−13 dB, right), both through a 1.8 kHz low pass because what they bounce off is upholstery; a 180 ms seeded impulse response after 6 ms of predelay, band-limited at 5 kHz and decaying over **130 ms below 900 Hz and 35 ms above it** — a lined cabin absorbs several times more at 2 kHz than at 125 Hz — at a level of 0.20; compressor at −18 dB, ratio 2, 25/180 ms, trimmed 0.42 in and 1.39 out. No shared low pass: one figure could not describe a tailpipe metres away and an engine through the bulkhead at once. Presentation only — downstream of everything, changes no state, bypassable |
| **Body and mount response** | Four modes at 60, 95, 150 and 220 Hz at Q 4.0, 4.0, 4.5 and 5.0, weighted 1.0, 1.0, 0.8 and 0.5, driven by gas plus pumping torque normalised by rated torque. Q in the single figures because a trimmed cab panel is heavily damped and a sharp bank here jumps in level as the firing frequency sweeps past each mode. This is a *vehicle* response excited by the engine, not an engine property; the source is an engine manual and publishes nothing about either |

## Modelling simplifications

- **Both valves have orifice flow; both manifolds are mean-value.** Every cylinder
  draws and expels gas through a port area under a pressure ratio, so all three
  gas-exchange transitions are integrated rather than assigned and trapping
  efficiency is an emergent result. But neither *manifold* is drained or filled by
  the summed port flows: both still use the speed-density estimate
  `eta_v · V_d · (N/120) · rho`, with `air_path.volumetric_efficiency` as its
  coefficient.

  So `volumetric_efficiency` is still a calibrated 0.92 and still means something,
  but it means a different thing than it did before milestone 12: it is the
  coefficient of the manifold's mean-value flow, not a multiplier on the trapped
  charge. The two numbers no longer have to agree, and they do not — the port traps
  what it traps.

  **This is a real inconsistency, deliberately kept, and it is symmetric.** The
  exhaust side has worked exactly this way since milestone 5: real orifice flow at
  the cylinder, mean-value flow at the manifold. Milestone 12 gave the intake the
  same treatment rather than inventing a third arrangement, so what remains is one
  simplification stated once instead of an asymmetry needing explanation. Closing
  it means feeding both manifolds their summed port flows, which makes manifold
  pressure ripple at the firing rate — genuine induction and exhaust pulsation, a
  plausible future sound source, and a change to the turbo and EGR loops rather
  than to the acoustic source. It is not a prerequisite for anything currently
  measured.

  The acoustic cost that *was* here has been paid: see **Results → Sound → What
  the traces contain**.
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
- **Cycle-to-cycle variation is physically right, acoustically minor, and as of
  milestone 12 mostly no longer the configured parameter's doing.** The acoustic
  source is the mass flow through the port, and by exhaust valve opening 140° after
  top dead centre the cylinder has expanded far enough that trapped mass, not the
  last 2% of fuel, is setting the pressure. And at full load the smoke limit clamps
  commanded fuel to the trapped air, which erases the trim entirely: measured at
  full pedal the variation is exactly none. That load dependence matches published
  behaviour — a few percent at light load, under one at high — and it emerges from
  a limiter that is there for another reason rather than from a schedule.

  What changed is where the variation comes from. With
  `injection.cycle_delivery_spread` set to zero the model used to be a six-event
  loop played on repeat, and that parameter was the only thing breaking it. It now
  measures 0.58% pulse-height variation at idle *with the parameter at zero*,
  because the intake side traps by integrating flow: what a cylinder traps depends
  on the residual it kept, which depends on what its previous cycle burned. That is
  a cycle-to-cycle feedback path the assigned charge did not have, and it is worth
  roughly what a spread of 0.04 used to buy — twice the shipped 0.02.

  The parameter is kept, at 0.02, and is still a working lever: 0.05 gives 1.6
  times the intrinsic variation and 0.10 gives 2.9 times. But it is no longer the
  mechanism, and `tests/acoustics.rs` now asserts both halves separately — that
  the engine varies with the knob at zero, and that the knob still does something —
  because only the second of those used to be true.
- **The three paths are aligned by construction, not by agreement.** They share
  one ring buffer and one fractional read position from the solver through to the
  worklet's resampler, interleaved a frame at a time. Three buffers with three
  sets of cursors could be filled or drained unevenly and slide apart, and a
  fixed delay between the exhaust and the block is not something a listener would
  hear as anything but wrong. Interleaving makes misalignment unrepresentable
  rather than merely unlikely, which is why an overrun drops a whole frame and a
  drain refuses to write a partial one.
- **Three channels are three mono paths, not a spatial layout.** The worklet's
  output carries them as discrete channels because that is how aligned signals
  travel through one Web Audio node; the stereo image is still made downstream by
  the panned reflection taps and the stereo convolver.
- **The exhaust duct is one-dimensional.** A waveguide with a lumped turbine loss
  and the aftertreatment box as an equivalent added length. The added-length
  approximation is a low-frequency one and stops describing the box once a
  wavelength approaches its own dimensions.
- **The cockpit stage is a listening filter, not an audio model.** It says where
  you are sitting, not what the engine does. The cab's transfer function was not
  measured and could not be — no such data exists for this vehicle — so the
  filter is plausible rather than correct, and bypassable for that reason.
- **The saturation is shared, and the engine brake still pays for it in level.**
  One limiter acts on the mix, and the path gains were set against fuelled
  operation. The brake drives the summed signal about four times past the knee, so
  it runs with 12 dB of gain reduction — `brake-1300` sits at −13.8 dBFS where the
  raw mix would be around −2. Milestone 9 made that a level reduction rather than
  a deformation, which is the part that was audible; what remains is that the
  loudest thing the engine does is the thing furthest from its own natural level,
  and one static gain into one ceiling cannot hold the range from governed idle to
  a decompression brake. Whether to spend the level or move the gains is a
  calibration question waiting on the reference characterisation.
- **A comparison is level-matched by RMS, not by loudness in the broadcast
  sense.** Both sides are engine noise in the same rough spectral region, which is
  the case where the simple measure and a weighted one agree. Comparing an engine
  against something with a different spectrum is not what this is for.
- **The playback audit is not a throughput measurement.** It establishes that the
  worklet is arithmetically correct at both device rates. What a given machine can
  sustain in real time is a browser question, and is reported live by the underrun
  counter rather than asserted here.
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
- Audio: exactly one **frame** per solver step, three paths interleaved;
  bit-identical across batch boundaries in every path; every sample of every path
  finite and inside `[−1, 1]`, and their **sum** inside the limiter's knee; a
  reset engine silent and a reset click-free; the note at the firing frequency for
  any cylinder count; a standing torque and a standing cylinder pressure both
  radiating nothing; the figures in **Results → Sound** met, except those recorded
  there as **Known deficits**.
- **The ceiling changes the level, not the shape.** Below the knee it is exactly
  transparent; above it, twenty times the gain on the same signal changes its
  crest factor by less than 1 dB, and the gain does not swing by more than 10%
  across a settled passage. Reaching the knee at full load is expected — the old
  criterion required headroom *under* it, which was a true statement about a
  waveshaper and is why that one was replaced rather than relaxed.
- **A reported limiter gain is exact.** Dividing it out of a limited run
  reproduces an unlimited run of the same input to within rounding, so the
  unsaturated captures are the solver's own signal rather than an estimate of it.
- **Every metric gives the wrong-looking answer for the wrong signal.** A steady
  tone reads unmodulated at any carrier and any level; a tone modulated to a
  stated depth reads that depth; a pulse train reads deeply modulated and a
  high crest factor; noise reads neither; a comb at the firing rate reads as
  orders and a tone between them does not; a ramp, a tone and a fast pulse train
  contain no discontinuities and a step added to any of them is found at the
  index and the size it was added. Passing on engine audio is not evidence that a
  metric is correct.
- **A metric's limitations are measured, not assumed.** Where an instrument has a
  signal class it cannot handle, a test states what it reads for that class and
  what that number is. Isolation cannot distinguish white noise from a trace full
  of assignments, and cannot see a discontinuity that lands on a steep enough
  slope; both are on the record with figures, and neither was resolved by moving a
  threshold until the awkward signal went quiet.
- **The forcings are readable where they enter the acoustic stage**, and are the
  arguments the stage was given rather than a second estimate of them: the
  reported structural forcing equals the snapshot's summed cylinder pressure over
  ambient, and the reported body forcing equals its gas plus pumping torque over
  rated, at every step.
- **A radiated trace is integrated to, not assigned to.** No gas-exchange
  transition moves the radiated pressure forcing by more than a tenth of that
  trace's own typical step. This is the durable form of milestone 12: an
  approximation with a small bounded error in the gas becomes an impulse with a
  white spectrum once the same trace is differentiated by a structural path, so a
  simplification that is acceptable in the pressure trace is not automatically
  acceptable in the sound. Measured against the trace's own step so it does not
  depend on the operating point's loudness.
- **A scenario is reproducible.** The same scenario run twice is bit-identical in
  every sample and in every recorded condition; a capture contains its measurement
  phases and not the settling before them; a held phase holds; nothing is dropped
  on the way out.
- **Playback is arithmetically correct at 44.1 and 48 kHz**: the resampled pitch
  is right, the three paths stay sample-aligned through it, imaging stays below
  −38 dBc in band, an underrun outputs silence and is counted, an overrun drops
  whole frames, a malformed block is refused, and the drift trim moves towards the
  buffer target by no more than 1%.
- **A listening comparison is level-matched**: clips are trimmed to the engine's
  measured output level, the trim is bounded and cannot drive a clip into
  clipping, a match that cannot reach its target reports the shortfall, and one
  source plays at a time.

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

**And that replacement set is itself now known to be incomplete.** Members of it
were measured in milestone 8 against operating points they had never been applied
to, and failed — see **Known deficits**. The `<80 Hz ≤ 35%` ceiling in particular
is a fixed number asked of a firing fundamental that sweeps from 27.5 Hz at
governed idle to over 100 Hz governed, which is a factor of four; it is not
obviously a criterion that can hold at both ends. It is left failing at idle, and
un-widened, because a tolerance should come from characterising the reference
recordings rather than from whatever the model happens to produce on the day the
question is first asked. That is exactly the mistake recorded above.

**Milestone 10 did that characterisation, and the criteria still did not move.**
The reference reads 89 to 92% below 80 Hz at idle against the model's 40.8%,
which is evidence that the ceiling is wrong at the bottom of the range and is not
proof of what should replace it: a sampled source played back by a game, on a
later engine, measured at a different point in the chain, sets no tolerance.
Rewriting a criterion the day its first evidence arrives is the same move as
writing one the day the model is first measured. What that milestone did settle
is that the exhaust-over-block floor is not waiting on this reference at all — a
stereo mix has no per-path ratio in it — and that the brake's band bounds are
waiting on material the benchmark does not contain.

The band criteria have a second problem milestone 9 exposed: **they were all
written against fuelled operation and are applied to the engine brake as well.**
Under brake the model now reads 55.2% in `300 Hz–2 kHz` and 42.8% in
`80–300 Hz`, both outside their bounds, and a decompression brake genuinely is a
hard high crack rather than a low roar. One set of band bounds for combustion and
for a valve cracking at the top of compression is one number describing two
mechanisms, which is the shape of every mistake in the list above. They are left
failing rather than split, for the same reason as the idle ceiling: the split
should come from the reference and not from the model.

The crest-factor criterion is the one member of the set that has now caught a real
defect rather than been fitted to one. It read 7.4 dB under brake against its
9 dB floor while every band criterion in the table was being *satisfied* by the
same clipping that caused it.

## Commands

```bash
pnpm install --frozen-lockfile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # 309 tests: geometry, provenance, catalog,
                                # determinism, limits, combustion, heat transfer,
                                # turbo, EGR, acoustics, the limiter, brake,
                                # driveline, dyno calibration, ID-branch guard,
                                # the metric library against known signals,
                                # the firing-rate estimator against its octave
                                # traps, the jump detector against continuous
                                # signals, scenario reproducibility

pnpm wasm:build                 # wasm-pack -> web/src/wasm (generated, gitignored)
pnpm wasm:test                  # wasm-pack test --node: WASM API smoke test
pnpm check                      # svelte-check
pnpm test                       # wasm smoke + vitest units (83) + Playwright browser suite
pnpm build                      # wasm + vite build + verify-dist
pnpm build:subpath              # the same build under an absolute /diesel-hifi/ base
```

**Do not report a command as passed unless it was run and exited successfully.**

The Playwright suite needs a browser once:
`pnpm --filter web exec playwright install chromium`.

### Measuring a recording

`reference_probe` takes any WAV — 16-, 24- or 32-bit, PCM or float, mono or
stereo — and prints the operating-point segments it can find in it:

```bash
# The benchmark's interior chapter, already extracted.
cargo run --release -p sim-core --example reference_probe -- \
  --file datasheet/audio-references/max2712-om471/interior-engine-01m33s-02m49s.wav

# Add --windows for every window rather than the merged segments, and --peaks
# for the strongest spectral lines under each: that is how a suspicious rpm is
# argued with.

# The loaded material has to come out of the video first. Mono and 16-bit
# because this one is a scan, and it lands under target/ rather than beside the
# reference because it is regenerable in one line.
mkdir -p target/reference-scan
ffmpeg -ss 377 -i datasheet/audio-references/max2712-om471/l3NGWdoA-_I.webm \
  -ac 1 -ar 48000 -c:a pcm_s16le target/reference-scan/test-drive.wav
cargo run --release -p sim-core --example reference_probe -- \
  --file target/reference-scan/test-drive.wav --hop 65536

# --rpm-min is how a suspected octave error is settled: lift the floor above the
# suspect reading and see whether what is left is coherent or merely plausible.
cargo run --release -p sim-core --example reference_probe -- \
  --file target/reference-scan/test-drive.wav --hop 65536 --rpm-min 900

# And pointed at the model's own captures it re-derives their speed from the
# audio, which is what makes the reference figures worth reading. 32768 samples
# is 0.82 s at the solver's 40 kHz.
cargo run --release -p sim-core --example reference_probe -- --window 32768 \
  --file target/audio-capture/idle/mix.wav --file target/audio-capture/full-1400/mix.wav
```

The annotation lands in `target/reference-annotation/annotation.json`, with every
window's metrics, order shares and strongest lines. The reference recordings are
gitignored and are not in this repository; without them the tool says so and
names this section.

### Inspecting the drive

`trace_probe` runs each steady scenario and reports the three forcings *before*
any filtering: how many samples the solver assigned rather than integrated to,
which crank event each one coincides with, and what removing them would do to
the path they drive.

```bash
cargo run --release -p sim-core --example trace_probe
```

It replays each scenario a step at a time at the same chunk cadence
`scenario::run` uses, and `advance(n)` equals `n` calls of `advance(1)`, so these
are the traces behind the very captures `audio_probe` reports rather than a
similar run at a similar speed.

Read the two halves of each block against each other. **Blind** is what
[`analysis::jumps`] finds without being told where to look — a difference
standing three times above the differences either side of it. **By cause** is
every valve event, whether or not it stands out.

**The by-cause half is the one that carries a result, and milestone 12 is why.**
Blind found nothing in the pressure forcing at `rated-1800` in milestone 11, where
the largest assignment in the model sat, because isolation cannot see a
discontinuity that lands on a steep enough slope. It finds nothing there now
either, and the assignment is gone. A measurement that reads the same before and
after a fix has said nothing about the fix; the by-cause row went from 0.9288 to
0.0017 and did.

The counterfactual line is the same path run again with the steps interpolated
away. It is a measurement device. Interpolating a solver's output is not a
proposed fix and the probe's own documentation says so — and having now been
checked against a real fix, it turned out to predict one to within half a
percentage point.

[`analysis::jumps`]: crates/sim-core/src/analysis.rs

### Verification, milestone 14

Run on Windows 11. A listening-stage change plus a measurement. No Rust source,
configuration or acoustic source was touched, so the native suite and the probes
are unchanged and were not re-run; the bass measurement used the existing
`reference_probe` against the existing per-path captures, plus a throwaway
example to dump the three forcings as WAV, which was deleted rather than kept —
if that measurement wants repeating it belongs in `trace_probe`.

| Command | Result |
|---|---|
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test` | 86 unit passed, 34 browser passed at root and subpath, 0 failed |

Three unit tests changed and one is new, and two of the three changes are worth
reading because they are cases where an assertion was measuring a proxy:

| Test | What it asserted | What it asserts now |
|---|---|---|
| `carries the same energy whatever the device rate` | both rates give energy ≈ 1 | the two rates agree with **each other**, and the figure is below one. Absorption legitimately removes energy; equality to one was the old invariant, rate-independence was always the claim |
| `decorrelates the two channels without changing their energy` | channel energies equal to six decimal places | equal within a quarter. The tail's energy is dominated by its first twenty milliseconds, which at this bandwidth is a few hundred independent samples, so two seeds differ by about a decibel at any setting |
| `builds every filter chain in the order the specification gives` | six per-path plus four shared biquads | plus the reflection damping filter, which is not a shared stage — it sits on the reflection bus only, so the direct sound and the tail bypass it |

The new one, `gets darker as it decays`, is the change stated as a measurement:
the tail's high-to-low energy ratio falls by a factor of seven between its first
and second quarters.

One test was **rewritten because its metric was wrong, not because it failed
usefully.** A first attempt asserted that equal decay rates reproduce a
flat-spectrum tail, measured through the same brightness proxy. It failed — the
first quarter reads 0.26 against 0.31 for the rest, an artefact of a decaying
envelope inside the window rather than of the code. The property is exactly
algebraic: the band split is `low` and `noise − low`, which reconstructs the
noise, so with equal decays the corner between them cannot matter. It is now
asserted as bit-equality between two very different corners, which is what it
actually claims.

### Verification, milestone 13

Run on Windows 11. A listening-stage change: no Rust source, no configuration and
no acoustic source was touched, so the native suite and the probes are unchanged
from milestone 12 and were not re-run. The checks that apply are the web ones.

| Command | Result |
|---|---|
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test` | 84 unit passed, 34 browser passed at root and subpath, 0 failed |

Two unit tests changed and one is new. The changed pair walk each path's chain and
now step over the level trim between the solo gain and the filters. The new one
asserts the invariant the whole design turns on: **a cab trim must not reach the
raw stage.** The raw stage is the sum the solver emitted and is what every figure
under **Results → Sound** is measured from, so a trim leaking into it would move
the numbers this project validates against while looking like a listening choice.

The browser test gained an assertion and a print. The assertion is the measurable
form of the request — the cab must weight low over high at least 1.5 times as
heavily as the raw stage does, which is what "the seat is a low-frequency
listening position" means when both stages are level-matched. The print is the
cab spread at both ends, because it is the number `makeupGain` and the path levels
are calibrated against and it previously existed only inside a failure message.

Calibration ran to seven measured iterations, and the record of what failed is
more useful than the value that passed:

| Body trim | Other trims | Idle vs raw | Load vs raw |
|---:|---|---:|---:|
| +6 dB | exhaust −2, block −3 | **+6.0 dB** | −1.9 dB |
| +3 dB | exhaust −1.5, block −3 | **+3.4 dB** | −2.7 dB |
| +4 dB | exhaust −1.5, block −3, shelf 5→2.5 | +2.0 dB | **−3.5 dB** |
| +3 dB | exhaust −1, block −3 | **+3.6 dB** | −2.6 dB |
| **+1.5 dB** | **block −3, makeup 1.41→1.39** | **+2.6 dB** | **−2.6 dB** |

Every row above the last fails the ±3 dB level match at one end or the other, and
moving weight out of the shared low shelf into the body path — the physically
better description — failed at the *loud* end instead. The window is 6 dB wide and
a rebalance opens the spread to about 5 on its own.

### Verification, milestone 12

Run on Windows 11.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace` | 310 passed, 0 failed |
| `cargo run --release -p sim-core --example sweep` | passed; the figures under **Results → Calibration** |
| `cargo run --release -p sim-core --example brake_sweep` | passed; the figures under **Results → Engine brake** |
| `cargo run --release -p sim-core --example audio_probe` | passed; the figures under **Results → Sound** |
| `cargo run --release -p sim-core --example trace_probe` | passed; the figures under **What the traces contain** |
| `cargo run --release -p sim-core --example audio_capture` | passed; 10 scenarios written, limiter engaging at three of them and costing 0.3 to 0.7 dB of crest |
| `pnpm install --frozen-lockfile` | passed |
| `pnpm wasm:build` | passed; the blob carries the new configuration |
| `pnpm wasm:test` | 17 passed, 0 failed |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test` | 83 unit passed, 34 browser passed at root and subpath, 0 failed |
| `pnpm build` | passed; `verify-dist` OK, 6 files, 590 KiB |
| `pnpm build:subpath` | passed; `verify-dist` OK under an absolute base |

**The browser suite was failing before this milestone and the fix is in the
test.** Running it turned up five failures, all reading `stopped` where a speed
was expected. The suspicion was the obvious one — retarding intake valve closing
cost low-speed torque, so the engine can no longer pull away — and it was wrong.
Reproduced natively, applying 500 N·m at 395 rpm dips the crank to **269 rpm and
then pulls away**; on the previous solver the same manoeuvre dipped to **256 rpm**,
so milestone 12 made that margin slightly *better*.

The suite was then run against the previous solver, which failed **six** of the
same tests — in a different combination, with the identical symptom. The defect is
that `run-state` reads `running` while the starter is still dragging the crank
through a few hundred rpm, and the test helper applied load there. A 269 rpm dip is
close enough to a stall that how many steps the worker completed in the last
animation frame decides it, and browser step counts come from wall-clock time by
design. `pullAway` now waits for governed idle before loading, which is what a
driver does; the suite went from 4.8 minutes with six failures to 2.4 minutes with
none, the difference being timeouts it is no longer waiting out.

This is the third time in three milestones that a test failing alongside a change
turned out not to be caused by it, and the method was the same each time: measure
the previous version at the same operating point before believing the new one
broke something.

**Four tests changed, and none of them changed to accommodate a regression.** The
distinction matters enough to itemise:

| Test | Was | Now | Why |
|---|---|---|---|
| `no_gas_exchange_transition_steps_the_radiated_pressure_trace` | asserted two transitions step and one does not | asserts none of the three steps | It was written in milestone 11 to pin the defect, and its own failure message said to invert it when the intake side was fixed. Renamed from `the_intake_side_boundaries_step_and_the_exhaust_side_does_not` |
| `the_structural_path_is_what_puts_energy_above_the_firing_harmonics` | modal bank dominates above 1 kHz by 4× | by 3× | The 4× was fitted while a fifth of the block path's content above 2 kHz was the artefact. It measured 3.98 after the fix. The claim is dominance; the constant was carrying a defect |
| `the_per_cycle_spread_makes_a_cylinder_differ_from_its_own_last_cycle` | the spread at 0.02 raises variation 1.3× over zero | the engine varies at zero, **and** 0.05 raises it 1.3× | The model now has intrinsic cycle coupling larger than the shipped spread contributes. Split into two assertions because only one of them used to be true |
| `running_under_load_spins_the_shaft_up_and_makes_real_boost`, `boost_decays_back_toward_ambient_when_fuelling_stops` | boost at one instant > 30 kPa | mean boost over a second > 30 kPa | The plant limit-cycles there and always has. Verified by measuring the previous solver at the same point. See **Known deficits** |
| `e2e/slice.spec.ts`, `pullAway` and the responsiveness test | load applied as soon as `run-state` reads `running` | load applied once the engine reaches governed idle | The engine was being loaded at a few hundred rpm while still on the starter. Failed six tests on the previous solver and five on this one, in different combinations. See the note below |

One test is new: `the_intake_port_is_shut_at_overlap_and_at_valve_closing`, which
holds the intake window shut at both ends. An intake port still open at valve
closing would trap the charge through a discontinuity in *area* instead of one in
pressure, which is the same defect wearing a different hat.

The middle two rows are the ones to be suspicious of, because relaxing a
threshold the day it fails is how the previous round of acceptance criteria came
to enforce the defect they were meant to catch. What distinguishes these: both
thresholds were fitted against a signal that contained the artefact, milestone 11
measured the artefact and **predicted in advance** that removing it would take
that content with it, and both new values are stated with the measurement that
moved them. Neither is an acceptance criterion from **Acceptance criteria** — none
of those changed, and two of them stopped failing.

### Verification, milestone 11

Run on Windows 11. This change adds a measurement tool, one cached struct on the
step, and tests; it touches neither the physics nor the browser audio, so the web
checks were not run — `AGENTS.md` scopes the full suite to changes that span
them.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace` | 309 passed, 0 failed |
| `cargo run --release -p sim-core --example audio_probe` | passed; **every engine figure identical to milestones 9 and 10**, which is the check that this milestone changed nothing |
| `cargo run --release -p sim-core --example trace_probe` | passed; the figures under **What the traces contain** |

Nine tests are new. Seven hold the jump detector against signals whose answer is
known before the code runs — a ramp, a tone, a fast pulse train, a lone step
added to each, and white noise — and one of those seven asserts the instrument's
*limitation* rather than its strength: white noise defeats isolation, registering
a few percent with ratios into the hundreds, and that is recorded as a measured
fact rather than tuned away. Raising the threshold until noise went quiet would
fit the number to a signal nobody measures.

The two in `tests/acoustics.rs` are the ones that matter to the finding. The
first checks that the reported forcing is what the paths were actually driven by,
against the snapshot's cylinder pressures and torque terms — data the solver
publishes by an entirely different route — so a probe cannot be reading a
plausible copy. The second pins the defect: the transition modelled as orifice
flow is continuous against the trace's own typical step, and the two modelled as
assignments are at least ten times larger. **That second assertion is expected to
fail when the intake side is fixed**, and it says so in its own failure message.
It failed in milestone 12 and was inverted; see **Verification, milestone 12**.

No sweep was re-run. Three stores into a struct after the acoustic push cannot
reach combustion, the crank or the brake, and `audio_probe` reproducing every
figure to the last digit is the stronger statement: identical audio requires
identical cylinder pressure at every one of 48 000 steps.

### Verification, milestone 10

Run on Windows 11. This change adds a measurement tool and touches neither the
physics nor the browser audio, so the web checks were not run — `AGENTS.md`
scopes the full suite to changes that span them.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace` | 300 passed, 0 failed |
| `cargo run --release -p sim-core --example audio_probe` | passed; **every engine figure identical to milestone 9**, which is the check that this milestone changed nothing |
| `cargo run --release -p sim-core --example audio_capture` | passed; 10 scenarios, all figures unchanged |
| `cargo run --release -p sim-core --example reference_probe` | passed on the interior chapter, the test-drive scan at two search ranges, and six model captures; figures under **Characterising the reference** |
| `ffmpeg` band levels on the same windows | agrees with the probe within the filters' skirts; the cross-check is described in that section |

### Verification, milestone 9

Run on Windows 11, and recorded with what actually happened rather than with what
was expected:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace` | 290 passed, 0 failed |
| `pnpm wasm:build` | passed |
| `pnpm wasm:test` | 17 passed, 0 failed |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test:unit` | 83 passed, 0 failed, across 6 files |
| `pnpm test:e2e` | **27 passed, 7 failed** — the same seven as milestone 8 |
| `pnpm build` | passed; `verify-dist: OK`, 6 files, 587 KiB |
| `pnpm build:subpath` | passed; `verify-dist: OK`, absolute `/diesel-hifi/` prefix |
| `--example sweep` | ran; peak power −1.5%, peak torque +0.4%, 20.30 MPa peak — all unchanged |
| `--example brake_sweep` | ran; stage III +9.0% at 1300 rpm, −8.6% at 2300 — unchanged |
| `--example audio_probe` | ran; the figures in **Results → Sound** |
| `--example audio_capture` | ran; 10 scenarios, no dropped frames |

**The seven browser failures are the same seven recorded under milestone 8**, by
name and by failure mode: the tests that drive the simulation in real time
through `pullAway`, failing because the engine does not reach 1100 rpm within the
30-second poll on this machine. Each one fails at `slice.spec.ts:58`, inside
`pullAway`, before it reaches its own assertions — including the exhaust-output
test, which never gets as far as measuring audio. Nothing in this milestone
touches the worker or the pacing. The remaining 27, including the three audio
tests and path soloing, pass.

The three new native tests are the ones that pin the mechanism: the ceiling is
exactly transparent below its knee, twenty times the gain on the same pulse train
changes its crest factor by under 1 dB, and dividing a reported gain out of a
limited run reproduces an unlimited run of the same input. The last of those is
checked against *linearity* rather than against the output it came from, so it
fails if the recorded gain is off by a frame, off by a scale, or read from the
wrong ring position.

**The fuelled and brake sweeps were re-run to show the change is audio-only**,
and they are identical to milestone 8's to every digit reported. Nothing in this
milestone touches combustion, the crank or the brake mechanism.

**One acceptance criterion was replaced rather than relaxed.** The old form
required full load to peak at or below 99% of the knee, on the grounds that the
louder start transient needed room underneath the ceiling to arrive in. That was
true of a static gain feeding a waveshaper and is not true of a level follower,
which reaches the ceiling by turning the passage down. The question it was really
asking — *is the loud end squared off* — is not answered by proximity to the
ceiling at all, and is now asked directly as a crest-factor floor. The old form
would have passed the very defect this milestone fixed: `full-1400` sat at 0.794
under a 0.85 knee, comfortably inside the threshold, with its crest factor down
3.4 dB.

### Verification, milestone 8

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace` | 287 passed, 0 failed |
| `pnpm wasm:build` | passed |
| `pnpm wasm:test` | 17 passed, 0 failed |
| `pnpm check` | 0 errors, 0 warnings |
| `pnpm test:unit` | 83 passed, 0 failed, across 6 files |
| `pnpm test:e2e` | **27 passed, 7 failed** — see below |
| `pnpm build` | passed; `verify-dist: OK`, 6 files, 584 KiB |
| `pnpm build:subpath` | passed; `verify-dist: OK`, absolute `/diesel-hifi/` prefix |
| `--example audio_probe` | ran; the figures in **Results → Sound** |
| `--example audio_capture` | ran; 10 scenarios, no dropped frames |

**The seven browser failures are pre-existing and were reproduced with every
milestone 8 change stashed.** They are the tests that drive the simulation in real
time through `pullAway` — the air path, the EGR comparison, the exhaust output,
and UI responsiveness — and they fail because the engine does not pick up speed
within the 30-second poll on this machine. Nothing in milestone 8 touches the
solver, the worker or the pacing. The remaining 27, including the three audio
tests and path soloing, pass.

**Two fixes were needed to run the documented commands at all**, both the same
defect. `playwright.config.ts` and `pnpm build:subpath` each set the subpath
build's base with a `VITE_BASE=… command` prefix, which is POSIX shell syntax.
Both are spawned through the platform shell, and on Windows that is `cmd.exe`,
which reads the prefix as a program name and fails before Vite is reached — so
the browser suite and the subpath build were unrunnable there. The Playwright
server now passes the base through Playwright's own `env` option, and the script
passes Vite's own `--base` flag; neither needs shell syntax. Both produce the
same artifact, and `verify-dist` confirms the absolute prefix reaches the emitted
HTML.

`vite.config.ts` uses a relative base by default, so one build works at a domain
root *and* under any subpath. Set `VITE_BASE`, or pass Vite's own `--base`, for
hosts needing an absolute prefix.

Four probes sit behind the figures above:

```bash
cargo run --release -p sim-core --example sweep         # the fuelled peaks
cargo run --release -p sim-core --example brake_sweep   # the brake's published anchors
cargo run --release -p sim-core --example audio_probe   # every steady scenario, measured
cargo run --release -p sim-core --example audio_capture # every scenario, as WAV files

cargo run --release -p sim-core --example audio_capture -- --list
cargo run --release -p sim-core --example audio_capture -- --only idle,brake-1300 --out captures
```

`audio_probe` reports levels, where the energy sits, how much of it is in the
firing orders, whether it is a pulse train or a tone, and which of the three paths
is in front — and prints the metrics' readings for a tone, a modulated tone and a
pulse train first, so the scale the engine figures sit on is visible. It reads the
paths as channels of one run rather than re-running the engine with gains zeroed,
so the figures it compares cannot have drifted apart.

`audio_capture` writes the same runs, plus the transients the probe skips, to
`target/audio-capture/` with a manifest. Captures are build artefacts of a
particular working tree and are not committed.

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
