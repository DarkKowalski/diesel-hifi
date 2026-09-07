# OM 471 Sound Improvement Plan

Status: phases 1-3 delivered 2026-09-07 as milestone 8. The first item of phase 5
— the shared saturation stage — delivered 2026-09-07 as milestone 9, ahead of
phase 4, because the measurements said the listening stage was flattening the
pulses the source already produced. Phase 2's carried-forward reference
annotation delivered 2026-09-07 as milestone 10, by measurement rather than by
ear. Phases 4, 6 and 7 remain proposed.

Delivered behaviour, the measurements it produced and the deficits it exposed are
recorded in `README.md`, which stays the specification. See
**Results → Sound → Known deficits** there for what is open.

## Objective

Make the simulator convincingly resemble a stock OM 471 heard from the driver's
seat, with distinct idle, loaded running, acceleration, overrun and engine-brake
character. The user reports that the current sound resembles a motorcycle or
electric motor.

The mechanical reference remains the 2011 OM 471.9 M3D configuration,
`mercedes-benz-om471-9-m3d-375kw`. The downloaded sound benchmark informs acoustic
character; it does not replace this engine's published specifications.

This document records proposed work. `README.md` remains the specification for
delivered behaviour, calibration, assumptions and acceptance criteria. Update it
as each change is implemented. The scope changes below are explicit proposals,
not changes already delivered.

## Available Reference

[Max2712: Mercedes New Actros OM471 Sound Mod, V1 release](https://www.youtube.com/watch?v=l3NGWdoA-_I).
The creator identifies a 2018 Mercedes Arocs 3248 with a 12.8-litre OM 471 as the
recording source, using synchronized microphones around the engine and exhaust.
The video demonstrates the resulting ETS2 sound mix.

| Segment | Use, after milestone 10 measured it |
|---|---|
| 0:05-1:33, exterior engine | Supporting source-character comparison |
| 1:33-2:49, interior engine | Primary driver-seat benchmark. Three steady idle stretches at 499-507 rpm; everything else in it is a stationary rev sweep |
| 6:17 onward, test drive | **Not usable.** Dominated by a component at half the firing rate; no operating point can be inferred from it |

The video is the mod running **in the game**: recorded samples pitch-shifted,
looped and crossfaded to an interior camera, not a rendering and not raw
microphone tracks. Every measurement of it inherits that.

Local files are in the existing gitignored directory
`datasheet/audio-references/max2712-om471/`:

- `l3NGWdoA-_I.webm`: full audio, approximately 20:21, stereo Opus at 48 kHz.
- `interior-engine-01m33s-02m49s.wav`: 76-second stereo floating-point WAV.
- `l3NGWdoA-_I.info.json` and `.description`: source details and chapter markers.

The full audio decoded without errors. The extracted chapter's decoded samples
match the same source interval exactly; no EQ or loudness normalization was
applied.

Operating-point annotation was delivered as milestone 10, by signal measurement.
The interior chapter yields three steady idle stretches at 499-507 rpm and
nothing else steady; the test drive yields no usable operating point at all. See
`README.md`, **Results → Characterising the reference**. Listening review remains
pending and is not substitutable by measurement.

This is a processed stereo benchmark from a later engine, not isolated microphone
tracks or a calibrated sound-pressure measurement. Its description also lists
additional game sound mods. Identify unrelated sounds before selecting analysis
windows. Keep the reference local to the comparison workflow; adding it to the
application's distributed assets is outside this plan. Further public-reference
research can proceed alongside implementation.

## Established Baseline

Investigation runs on 2026-09-07:

| Check | Result |
|---|---|
| `cargo run --release -p sim-core --example audio_probe` | Passed; raw mix at 1400 rpm/full pedal has 91.9% of energy in its first four firing harmonics |
| `cargo test -p sim-core --test acoustics` | 46 passed, 0 failed |
| Numerical reproduction of the probe's modulation calculation | A steady 60 Hz sine scores approximately 0.42, contradicting the stated near-zero expectation |

These results establish a working baseline, not acoustic fidelity. The following
implementation limits motivate the work:

- The probe labels 600 rpm at 15% pedal with repeated speed pinning as idle;
  actual governed idle is 560 rpm.
- A constant-rate injection event and double-Wiebe burn supply the combustion
  model. Summed cylinder pressure drives one shared structural modal bank.
- Per-cycle fuel scatter precedes the smoke limiter and can disappear at full
  load. Increasing that scatter alone cannot reliably restore acoustic texture.
- The cab transfers are estimated. The body path excludes room response, while
  audible panel radiation and felt seat vibration need distinct interpretations.
- Fixed exhaust dominance and band-share thresholds do not establish the correct
  driver-seat sound balance.
- The exhaust uses runner delays, fixed turbine attenuation and an equivalent
  pipe length for aftertreatment volume. These are acoustic approximations.

The relative contribution of each limit to the reported sound remains to be
tested through controlled comparisons.

## Scope and Invariants

Keep deterministic physics and acoustic source generation in `sim-core`, a thin
`sim-wasm` adapter, and browser/device orchestration in `web`. Preserve fixed
stepping, SI units, radians, validated configuration, explicit seeds, data-driven
engine selection, pressure-derived crank torque and the allocation-free hot loop.
Use pnpm for JavaScript dependencies. Preserve static deployment, the Web Worker,
coarse batching and operation without `SharedArrayBuffer`.

Revisit these README constraints before implementing dependent changes:

| Current constraint | Proposed treatment |
|---|---|
| No added or synthesized acoustic content | Permit calibrated, state-driven procedural acoustic sources with explicit provenance |
| Exactly three source paths | Extend the channel contract when a demonstrated source requires it; preserve aligned interleaved frames |
| Exhaust must exceed block by at least 6 dB | Establish listener- and operating-point-specific balance targets |
| No room response for the body path | Model audible panel radiation into the cabin separately from tactile vibration |
| Intake flow, turbo/intake sound and clutch behaviour excluded | Reopen as evidence-driven work packages |
| Fixed spectral shares establish realism | Use joint signal measurements and controlled listening comparisons |

Gasoline support, multi-fuel abstractions, ECU reproduction, emissions prediction,
full CFD, finite-element mechanics and aftertreatment chemistry remain outside
scope. Engine-specific calibration stays in configuration with published,
derived or calibrated provenance. No major dependency is required by this plan.

## Implementation Phases

### 1. Update the Specification

Rewrite the README around current behaviour, the driver-seat target, supported
conditions, assumptions and validation. Replace historical correction narratives
with concise descriptions. Record the reference's later-engine and game-mix
limitations. Treat closed windows as a provisional listening condition until the
selected segments support it.

Completion: delivered behaviour and proposed scope are distinguishable, and the
constraints needed for the next phase are explicit.

### 2. Build Repeatable Comparisons

Annotate reference segments with timestamps, steady/transient state, identifiable
accessory sounds and RPM confidence. Inspect the video for a tachometer where
available. Label inferred RPM and unknown load; stationary revving does not
establish loaded sound. Reserve some suitable segments for validation after
tuning.

Create reproducible simulator scenarios for governed idle, steady operation at
several speeds and loads, starting/stopping, acceleration, pedal release and
engine braking. Record configuration, seed, controls, actual RPM/load and settling
conditions. Distinguish held-speed dynamometer measurements from free-running
transients.

Export raw source tracks before saturation and final cockpit output from the
same run. Extend existing listening controls with local reference/current/candidate
comparison, path soloing and repeatable level matching. Keep unmodified captures
alongside comparison gain settings. File writing belongs in tools/examples, not
the simulation core's hot loop.

Completion: a scenario can be regenerated and compared at matched loudness without
manually reproducing pedal movements. Steady-state measurement windows exclude
startup and unrelated events.

### 3. Repair Metrics and Audit Playback

Define the intended meaning of each metric and validate it against tones, pulse
trains, amplitude-modulated signals and noise. Repair or replace the modulation
detector so rectification-generated harmonics cannot establish pulse realism.

Measure firing orders, harmonic-to-broadband balance, spectral shape, pulse
envelopes, crest factor, persistent resonances and numerical residue together.
Use appropriate windows for steady and changing speed. Establish tolerances from
reference characterization; do not optimize toward one spectral percentage.

Check the worker/worklet chain at 44.1 and 48 kHz for rate accuracy, channel
alignment, interpolation artifacts, underruns, overflow and latency drift. Compare
processing with saturation, compression and cab stages individually bypassed.
Report sustained real-time throughput and buffer behaviour under stated workloads.

Completion: synthetic counterexamples expose incorrect metrics, and playback
defects are either corrected or separately identified before source tuning.

### 4. Improve Source Excitation

Run independently switchable experiments with cylinder-specific structural
coupling, bounded variation in combustion timing/burn shape, and crank-synchronized
mechanical excitation. Inspect pressure and port-flow traces for discontinuities
and numerical artifacts before amplifying their high-frequency content.

Evaluate injection rate shaping and pilot/main events where evidence supports
their use. Retain mass/energy accounting and recalibrate against existing engine
targets when combustion changes. Acoustic-only randomness gets a separate seeded
stream so new sound sources cannot change fuelling randomness.

Completion: retained changes improve idle and loaded comparisons, preserve
determinism and meet physical acceptance criteria. Source improvements must remain
audible without relying on additional background noise.

### 5. Calibrate the Driver-Seat Transfer

**The saturation review is done; the rest of this phase is not.** The shared
stage was a memoryless soft clipper, which is a waveshaper: it cost every fuelled
point above idle 1 to 3.4 dB of crest factor and the engine brake 5.5 dB, which
was that scenario's entire acceptance failure. It is now a level follower with an
instantaneous attack, a calibrated release and a ceiling that is transparent below
its knee, and the solver reports the gain each frame carried so a capture can
recover the pre-limiter mix exactly. Headroom at the final output is measured and
recorded. Source contributions were already identifiable; no channel changed, so
no descriptor, adapter, protocol, worklet or routing change was needed.

What remains here is the transfer itself: source-to-ear balance, filtering,
delays, cabin response, panel radiation against tactile vibration, and the
cockpit compressor's interaction with the paths.

Tune source-to-ear balance, filtering, delays and cabin response using individually
inspectable sources. Represent mount-driven panel radiation and cabin modes;
separate optional speaker compensation from the vehicle response. Keep raw output
available and rename the current combined raw mix so it is not labelled solely
as tailpipe sound.

Review shared saturation and compression for altered pulse shape and unintended
coupling between sources. Measure headroom at the final output. If source channels
change, update their descriptors, adapter, protocol, worklet, routing and tests
together; retain one aligned frame per solver step.

Completion: final cockpit output improves at matched loudness across operating
conditions, with source contributions identifiable and no new routing artifacts.

### 6. Add Mechanisms Where Evidence Justifies Them

| Candidate | Trigger for implementation |
|---|---|
| Intake and turbo airflow sound | Missing load-dependent texture identified in comparisons |
| Fan and accessory sound | A source is identifiable and its driving state can be represented |
| Better exhaust/aftertreatment impedance | Persistent incorrect resonance or pulse shaping survives source and transfer work |
| Intake-valve flow and manifold coupling | Gas-exchange traces demonstrate a source limitation |
| Clutch and driveline compliance | Pull-away, shifting and overrun require independent engine/vehicle dynamics |

Use bounded reduced models and documented calibration. New channels are a reason
to extend the existing protocol coherently, not to introduce a generic audio or
multi-fuel framework. Optional road/wind ambience is separate from engine-fidelity
acceptance.

Completion: each addition has an observed deficit, a measurable state dependency
and a demonstrated benefit that justifies its cost.

### 7. Validate and Record Results

Run blinded, level-matched baseline/candidate comparisons across multiple
operating conditions, including reference segments held back from tuning. Record
listener feedback with conditions and playback equipment. Include headphones and
ordinary speakers; record unsupported conditions and remaining fidelity gaps.

Preserve the README's physical calibration tolerances: fuelled peak magnitudes
within 3%, engine-brake anchors within 10%, and nominal operation below the 23 MPa
pressure envelope. Maintain deterministic snapshots and audio across batch
boundaries within the same configuration and software version.

Run focused checks for each changed behaviour. Before completing changes spanning
physics and browser audio, run the applicable README commands: formatting, Clippy,
workspace tests, WASM tests, Svelte checks, unit/browser tests, and root/subpath
static builds. Re-run the fuelled, brake and audio probes when their behaviour or
calibration changes. Record successful commands and actual results in the README;
passing signal checks alone is not a listening result.

Completion: evidence covers sound quality, physical calibration, determinism,
real-time playback and deployment. Documentation describes the delivered result
and its remaining limitations.

## First Implementation Package

Phases 1-3, delivered. The updated README, ten reproducible scenarios covering
governed idle through engine braking, WAV exports with a manifest, level-matched
local comparison playback, a repaired and tested metric library, and a playback
audit at both device rates. No acoustic source or calibration changed.

Two items of the package were **not** delivered and are carried forward:

- **Reference segment annotation.** Delivered as milestone 10 in its measurable
  half: the windows are annotated with inferred speed, confidence and the full
  metric set. **No listening comparison has been made against it**, and no
  measurement substitutes for one.
- **Cockpit-output export.** The cab is a Web Audio graph, and a second copy of
  it in Rust would drift from the one people hear. Cockpit comparison happens in
  the browser against the exported raw captures instead. Still open.

Phase 4 experiments can now be selected from measured deficits rather than
guessed at. Phases 4 and 5 can iterate together; phase 6 remains conditional.

## Second Implementation Package

The saturation stage, delivered as milestone 9. Taken before phase 4 on the
evidence rather than in plan order: three of the four milestone 8 deficits turned
out to be one defect in the listening stage, and source excitation cannot be
evaluated through a stage that is squashing whatever the source produces.

It also leaves phase 4 with better targets than it had. Passing the pulses intact
showed that the engine brake puts 55.2% of its energy above 300 Hz, which the
clipper had been reporting as 40.4% — the first honest measurement of that path,
and one that the band criteria, written against fuelled operation, have no
business judging yet.

It left the reference recording as the blocking item for three separate things:
the speed-dependent low-band tolerance at idle, the exhaust-over-block floor that
two operating points sit just under, and whether the brake's upper-band share is
a defect at all. Every one of those is a criterion waiting on characterisation
rather than a model waiting on a change. The next package measures it.

## Third Implementation Package

The reference characterisation, delivered as milestone 10. A firing-rate
estimator in `sim_core::analysis`, so a signal that does not report its own speed
can still be measured; `examples/reference_probe.rs`, which annotates any WAV
into operating-point segments with a confidence attached; and the benchmark
measured with it. No acoustic source, calibration or acceptance criterion
changed.

It answers the blocking item above in three different ways, which is the result:

| Blocked criterion | Outcome |
|---|---|
| Speed-dependent low-band tolerance at idle | **Evidence, not a verdict.** The reference reads 89-92% below 80 Hz at idle against the model's 40.8%. The criterion is left at 35%, failing |
| Exhaust over block by 6 dB | **Not blocked on this reference, and never was.** A stereo mix has no path separation; no analysis of it yields a per-path ratio |
| Whether the brake's upper-band share is a defect | **Still blocked, on material that does not exist here.** No engine-brake passage could be identified |

Two findings beyond the plan. The estimator read two of the model's six steady
captures an octave high before it was fixed — pointing a new instrument at
signals whose answer is already known caught it, which is the same method that
caught the modulation detector in milestone 8. And the benchmark's 14-minute test
drive is **unusable**: its loudest component sits at half the firing rate, over a
firing comb holding 18% of the energy.

The reference is a **sampled source played back by a game**, heard from its
interior camera — recorded audio pitch-shifted, looped and crossfaded, not a
rendering. That bounds every figure taken from it, explains the drive's
half-firing-rate component, and is the reason the idle result is evidence about a
convincing sound *design* rather than about an engine. Recorded in `README.md`
under **The acoustic reference, and what it is not**.

What phase 4 gets from this is narrower than hoped. The idle low-band question
has evidence behind it; the other two do not, and one of them cannot get any from
this source. A reference that could settle physical band balance would have to be
an unprocessed multi-microphone recording of a real truck, which this project does
not have and this plan does not assume.

Primary ownership:

| Area | Files/modules |
|---|---|
| Specification and results | `README.md` |
| Capture and measurement | `crates/sim-core/examples/audio_probe.rs`, `audio_capture.rs`, `reference_probe.rs`, `src/analysis.rs` and the acoustic tests |
| Physics and acoustic sources | `crates/sim-core/src/sim/`, validated engine configuration and provenance |
| Boundary and transport | `crates/sim-wasm`, `web/src/worker/`, `web/src/audio/exhaust-processor.js` |
| Listening and comparison | `web/src/lib/cabin.ts`, `web/src/lib/audioEngine.ts`, existing audio controls and browser tests |

Nothing delivered so far changes simulator behaviour or calibration. Milestone 10
adds measurement code, so it carries formatting, Clippy and the workspace tests;
it touches neither physics nor browser audio, so the web suite does not apply.
`README.md` records the commands run and their results.
