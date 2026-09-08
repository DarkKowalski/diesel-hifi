# OM 471 Sound Improvement Plan

Status: phases 1-3 delivered 2026-09-07 as milestone 8. The first item of phase 5
— the shared saturation stage — delivered 2026-09-07 as milestone 9, ahead of
phase 4, because the measurements said the listening stage was flattening the
pulses the source already produced. Phase 2's carried-forward reference
annotation delivered 2026-09-07 as milestone 10, by measurement rather than by
ear. Phase 4's **prerequisite** — inspect the pressure and port-flow traces for
discontinuities before amplifying their high-frequency content — delivered
2026-09-07 as milestone 11. It found one, and it blocked the rest of phase 4 on a
fix.

**That fix is delivered 2026-09-07 as milestone 12: intake-valve orifice flow,
phase 6's fourth entry, taken out of order because the evidence moved it.** Phase
4 is unblocked. The remainder of phases 4 and 5, and phases 6 and 7, remain
proposed.

**The first listening report also arrived**, alongside milestone 12 and on the
build before it: too much high frequency, not enough low. It is one unrecorded
listen and it is not phase 7. It is recorded in `README.md` under **Results →
Where you are listening from → What listening has said so far**, because the
alternative to writing down weak evidence is having none.

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

**The prerequisite is met.** The trace inspection found a defect and milestone 12
fixed it: both gas-exchange assignments became orifice flow, the largest fell from
0.929 to 0.002, and the block path's content above 2 kHz went from 20.9% to 8.6%
at governed idle. The blind detector now finds no steps in the pressure forcing at
any operating point, and removing every valve event from it changes nothing. The
instruction to look before amplifying did its job: every experiment below would
have amplified an artefact worth a fifth of the idle block path.

Two of this phase's guidance notes are now settled rather than open:

- **The counterfactual predicted the fix.** It was recorded as an upper bound on
  the grounds that a real valve still makes a fast edge, and it landed within half
  a percentage point everywhere. The fast edge exists and is worth under half a
  point at a 35° ramp.
- **Cylinder-specific structural coupling loses its stated reason.** It was listed
  as the experiment most affected, because it would give each cylinder's
  *assignment* its own path to the bank. There are no assignments now. It remains
  a legitimate experiment on its own merits — six cylinders do not couple to a
  block identically — but the argument for taking it first is gone.

One new constraint on this phase, from `README.md` **Known deficits**: crest
factor fell 0.1 to 1.2 dB across the operating points, because part of what was
being counted as pulse dynamics was the artefact. Any experiment that reports
recovering crest should say whether it is recovering combustion or re-adding an
impulse train.

Run independently switchable experiments with cylinder-specific structural
coupling, bounded variation in combustion timing/burn shape, and crank-synchronized
mechanical excitation.

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
| ~~Intake-valve flow~~ | **Delivered as milestone 12.** Triggered by milestone 11's traces, taken ahead of this phase because it was blocking phase 4 |
| Pulsating manifold coupling | The half of that row still open, and deliberately so. Both ports flow for real; both manifolds are still mean-value speed-density, which is what the exhaust side had always done, so the two sides are symmetric rather than one being special. Closing it makes manifold pressure ripple at the firing rate — real induction and exhaust pulsation, and a plausible source of the load-dependent texture the first row of this table is about. Trigger: that texture identified as missing, or the boost limit cycle under **Known deficits** traced to the mean-value mismatch |
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

## Fourth Implementation Package

The trace inspection, delivered as milestone 11. The three forcings readable
where they enter the acoustic stage; a jump detector in `sim_core::analysis` that
asks whether a trace was integrated to or assigned to, tested against a ramp, a
tone, a fast pulse train and noise; and `examples/trace_probe.rs`, which
attributes every step in every forcing to the crank event that caused it and
measures what removing it would do. No acoustic source, calibration or acceptance
criterion changed.

It is phase 4's stated prerequisite and it came back with a finding rather than a
clean bill:

| Question | Outcome |
|---|---|
| Is the exhaust source clean? | **Yes.** No valve event moves it by more than 0.003. Its documented near-Nyquist residue is real, detectable at 0.8-1.7% of samples, and worth under 0.02 dB — asserted for three milestones, now measured |
| Is the torque forcing clean? | **Yes.** No steps at all; the largest valve-event excess anywhere is 0.0011, because the geometric factor goes to zero at the dead centres where the transitions happen |
| Is the pressure forcing clean? | **No.** Two of three gas-exchange transitions are assignments, and the block path is ringing them |

Three findings beyond the plan.

The **control** is the third transition. The exhaust valve opening is the one
modelled as orifice flow, and its excess is two to three orders of magnitude
smaller than the other two — same valve gear, same cylinder, same cycle. Without
it the measurement would be an interesting number with no way to tell whether it
was an artefact of how the excess is taken.

The **instrument caught itself**, for the third milestone running. A blind
detector finds the steps at two operating points and finds nothing at
`rated-1800`, where the largest assignment in the model sits: at 1800 rpm the
crank covers more angle per step, so the neighbouring differences grow and the
isolation ratio falls under the threshold while the step itself grows. Isolation
cannot see a discontinuity that lands on a steep enough slope. The probe reports
both the blind result and what the solver knows is a valve event, and prints the
disagreement rather than the flattering half.

And **idle is the worst case for a reason that is not the step size** — the steps
there are the smallest measured. It is that idle has the least combustion noise
to hide them behind: no boost, the gentlest premixed rise, a block path nearly
29 dB below rated. The artefact is roughly constant across the range and the signal it
competes with is not, which is the general shape of why an idle defect is heard
first.

The package deliberately stops at measuring. Interpolating the step away in the
solver would be a schedule bolted over a modelling boundary, and the boundary is
where the fix belongs.

## Fifth Implementation Package

The intake boundary, delivered as milestone 12. It is the first change to the
acoustic *source* since milestone 6 and the first since milestone 9 to move a
figure in **Results → Calibration**.

The intake side stopped being a boundary condition: the cylinder draws gas through
a port area under a pressure ratio, through the same `flow::exchange` the exhaust
valve and the brake lobe already used, so the trapped charge is what flowed rather
than a gas-law evaluation scaled by a calibrated volumetric efficiency. Trapping
efficiency became an outcome, which forced the recalibration — and the
recalibration is the part worth reading:

| Question | Outcome |
|---|---|
| How much does a finite-rate transition recover, against the counterfactual's perfect removal? | **Essentially all of it.** Within half a percentage point at every operating point. This was the question the plan said this package should open by settling, and the bound it was measured against held |
| Can the new port area carry the calibration? | **No, and that is physics rather than inconvenience.** A fixed area throttles more the faster the piston asks for gas, so area moves peak power and barely touches peak torque at 1000 rpm. Intake valve closing carried the fit, moving 20° later |
| Did the intake manifold's mass balance have to change with it? | **No — that estimate was wrong.** Both manifolds stay mean-value, which is what the exhaust side had always done. The scope note in milestone 11 overstated it |

Three findings beyond the plan.

**Two open deficits closed without being touched.** `80–300 Hz` at governed idle
went from 41.7% to 45.4% and now clears its floor; `rated-1800` went from 5.8 to
6.3 dB of exhaust over block and now clears the balance criterion. Both had been
listed as failing for two milestones and neither was ever a band or balance
problem — they were one defect in the source seen through two criteria. Which is
an argument for fixing mechanisms rather than adjusting the criteria that report
them.

**Cycle-to-cycle variation partly stopped needing to be faked.** With
`cycle_delivery_spread` at zero the model used to be a six-event loop on repeat.
It now varies 0.58% at idle on its own, because trapping through a real port
carries the residual forward into the next cycle's charge — roughly twice what the
shipped 0.02 contributes. The parameter is kept and is still a working lever, and
the test that asserted it was *the* mechanism now asserts both halves separately.

**A pre-existing air-path limit cycle surfaced, and nearly got absorbed.** A turbo
test asserting boost at one instant failed; the instinct is to settle longer.
Measuring the previous solver at the same operating point found the same
oscillation at the same amplitude — boost swinging 25 to 155 kPa near 1900 rpm
under load, which the full-load sweep had been reporting as `conv false` for
several milestones. Milestone 12 moved its phase, not its size. The test now
averages and the deficit is on the record instead of behind a lucky assertion.

The package stops short of one thing on purpose. The idle `<80 Hz` criterion now
has the measured reference and a listening report both saying the model wants more
low end, against a convention saying less. Changing an acceptance criterion in the
same breath as delivering a source change is how the previous set came to enforce
the defect it was meant to catch. It is the next decision, not this one's.

Primary ownership:

| Area | Files/modules |
|---|---|
| Specification and results | `README.md` |
| Capture and measurement | `crates/sim-core/examples/audio_probe.rs`, `audio_capture.rs`, `reference_probe.rs`, `trace_probe.rs`, `src/analysis.rs` and the acoustic tests |
| Physics and acoustic sources | `crates/sim-core/src/sim/`, validated engine configuration and provenance |
| Boundary and transport | `crates/sim-wasm`, `web/src/worker/`, `web/src/audio/exhaust-processor.js` |
| Listening and comparison | `web/src/lib/cabin.ts`, `web/src/lib/audioEngine.ts`, existing audio controls and browser tests |

Milestones 10 and 11 added measurement code only, so they carried formatting,
Clippy and the workspace tests and no more. **Milestone 12 is the first package
here to change simulator behaviour and calibration**, so it carries the sweeps and
both probes as well; `README.md` records every command and its result under
**Verification, milestone 12**, including why the web suite is stale rather than
passing — the physics moved, no TypeScript did, and that distinction should be
stated rather than assumed.

**The web chain is verified and the captures are fresh**, which is what a
listening comparison needs and what milestone 12 originally left stale. Running
it found a sixth thing: five browser tests failing with an engine reading
`stopped`, which was *not* the intake change — the previous solver failed six of
the same tests, and the native pull-away margin actually improved from a 256 rpm
dip to 269. The test helper had been loading the engine while it was still on the
starter. Recorded in `README.md` under **Verification, milestone 12**.

## Sixth Implementation Package

The driver's-seat balance, delivered as milestone 13, and the first thing in this
plan driven by a listening report rather than by an instrument. Each radiating
path gets a level of its own in the cab, applied cab-side so the raw stage stays
the solver's own output; the body goes up 1.5 dB, the block down 3, and about
2 dB more of the top end is taken out on the way to the driver.

**The request was for more than that, and a measurement refused it.** A +6 dB
body trim — which is what the report sounds like — put the cab 6 dB louder than
the raw stage at idle against −1.9 dB under load. Seven measured iterations are
recorded in `README.md`; every one of them failed at one end or the other, and
the physically better idea, moving weight out of the shared low shelf into the
path that actually carries it, failed at the loud end.

What that turned up is the finding, and it is a source deficit rather than a
listening one:

| Question | Outcome |
|---|---|
| Why can a cab trim not do this? | The body path is **43% of the idle mix and 5% of the loaded one**, so a constant trim is a large change at one end and nothing at the other |
| Why is it 5% under load? | The exhaust source is port mass flow, which grows with fuelling and boost. The body source is torque **normalised by rated torque**, which is bounded above by construction. One has a ceiling and the other does not |
| Is it the wrong way round? | Yes. Exhaust leads the body by 1.1 dB at idle and 12.8 dB at full load, so the roar recedes as the engine works harder |

This is phase 5's *"tune source-to-ear balance"* running into phase 4's
territory: the balance cannot be tuned at the ear because the deficit is in how
one source scales. The candidates — a body forcing not normalised by a fixed
rated torque, or one driven by torque *fluctuation* rather than torque level —
are source changes that move band shares, crest factors and the `<80 Hz` figure
that is already failing its criterion in the same direction. Recorded in
`README.md` under **Known deficits**, not attempted in the milestone that
measured it, for the reason this plan gives every time.

## Seventh Implementation Package

The cab's absorption and the bass measurement, delivered as milestone 14. Two
listener suggestions rather than a report, and both were right.

**The cabin is lined with soft things.** Porous absorbers take around 0.1 of the
energy at 125 Hz and 0.6 to 0.9 by 2 kHz, so a lined cab's reverberation time
falls steeply with frequency. The model had one decay constant for every
frequency — a hard box — and full-bandwidth early reflections. The tail now
decays four times faster above 900 Hz, the reflections are damped at 1.8 kHz, and
level-matching survived it. Two side findings came out of the calibration: the
tail was **11% quieter at 96 kHz than at 44.1 kHz** once the decay was
frequency-dependent, because white noise runs to Nyquist; and renormalising a
damped tail to unit energy makes the cab *louder* the more treble it absorbs,
which is a tone control rather than a furnishing.

**The bass has no harmonic series**, which is the larger finding and it is
recorded rather than fixed:

| Question | Outcome |
|---|---|
| Is the bass short of harmonics? | **Yes, decisively.** The torque forcing is 87-89% fundamental at every operating point, crest factor 4.0-4.9 dB against a sine's 3.0 |
| Is it the modal bank filtering them out? | **No.** The drive is a sine before it reaches the bank. Measured by dumping the forcings to WAV and running the reference probe on them |
| Is it a property of the measurement? | **No.** The exhaust drive over the same captures is 20-49% fundamental with crest to 11.5 dB — a real pulse train |
| Is there anywhere for harmonics to go? | **No.** The body bank ends at 220 Hz and the block bank's weights there are 0.25 and 0.4, so the structure-borne response has a trough over 220-750 Hz — exactly where the third to eighth firing orders sit |

This is the deficit behind milestone 13's balance request and it explains why
granting that request made the sound boomy rather than fuller: turning up a path
that carries one order gives more of that order. A fix needs both halves — a
drive with harmonics and a path that passes them — and it lands on the `<80 Hz`
criterion already failing plus the block bank's ascending weights that
`README.md` records as compensating for a different shortfall. Three coupled
calibrations, first measured this milestone. Recorded under **Known deficits**.

Worth noting for the plan's own method: the two most productive listening inputs
so far were not verdicts on the output but **hypotheses about the model**. "Too
much treble" agreed with two instruments and taught nothing. "The bass has no
harmonics" named a mechanism, took an afternoon to check, and found a defect
fourteen milestones of measurement had missed. Phase 7 should ask listeners for
mechanisms, not only for preferences.

## Eighth Implementation Package

The block modal bank reshaped, delivered as milestone 15. The weights ascended from 0.25 to 12.0 across the six modes, a 16.8 dB span, to compensate for the premixed Wiebe onset driving the upper modes too weakly. That compensation was audible: a listener reported too much high frequency from the block, and the same weights starved the 220-750 Hz band where a diesel growls — which was the open item from milestone 14's bass measurement. Both reports pointed at the same number.

The weights now span 6.7 dB with the peak at 750 Hz. Measured:

| | Before | After |
|---|---|---|
| Orders 1-4 at `cruise-1200` | 22.6% | **49.8%** |
| Block `>2 kHz` at `cruise-1200` | 8.1% | ~0.1% |
| Block `>2 kHz` at `full-1400` | 5.6% | ~0.1% |
| `light-900` exhaust over block | **5.2 dB** (failing) | **6.4 dB** (passing) |
| Loudest octave | varies, often >1 kHz | **315-630 Hz** everywhere |
| Q values at 750 and 1400 Hz | 11, 12 | **7, 8** |

The Q reduction at the mid-modes was not about shaping, and it should not need finding again: broadening those modes raised the crest factor from 8.8 to 9.3 at rated. The block *supplies* mix crest — its own is around 13 dB — so quieting it to hold the balance costs crest, and lower Q buys it back without moving the balance.

`structural_gain` halved from 0.40 to 0.20. The weights changed how much drive energy the bank captures, so the level had to come down to leave the exhaust-over-block balance where it was. The level says how loud the block is; the weights say what it sounds like; separating those two jobs is what made the reshape possible without moving the balance.

The shortfall in the drive is still real — the torque drive carries ~88% fundamental, and that affects what the bank can pass. The block path now has a harmonic series in the growl band at cruise and full load, but the body path does not. That is a source deficit and it is open.

Three acoustic tests had their claims deliberately moved, and the record of why is in each test's inline comment rather than here.

The next package is a choice between two remaining items:

- **The bass harmonic series**, measured in milestone 14 and now the clear
  favourite. It subsumes the body path's load scaling below — a path carrying one
  order has a level problem *and* a scaling problem, and both are symptoms of the
  drive being a sine. Two parts: give the torque drive harmonics, and open the
  220-750 Hz trough between the two modal banks so they have somewhere to go.
- **The body path's load scaling**, measured in milestone 13. Probably the same
  work as the item above rather than separate work.
- **The idle low-band criterion.** Now three pieces of evidence against it and
  none for it: the measured reference, and two listening reports. Cheapest, and a
  criterion decision rather than a model change — but the item above will push
  `<80 Hz` further past it, so deciding this first is what makes that one
  measurable.
- **A real listening comparison**, which is phase 7 and has never been done. The
  comparison tooling has existed since milestone 8 and nothing has been heard
  through it at matched loudness. Every figure in this document is a signal
  measurement.
- **The rest of phase 4**, now unblocked: cylinder-specific structural coupling,
  bounded combustion timing variation, crank-synchronised mechanical excitation.
  This is what the plan says comes next, and it is the one with the least evidence
  behind it — the deficits it would address are inferred rather than measured.

The middle one is the recommendation. The instruments have now caught three of
their own errors and closed two deficits without anyone hearing anything, and the
one report that did arrive agreed with them about the treble and pointed at a
criterion about the bass. What that suggests is not that more measurement is
wrong, but that a project with eleven milestones of measurement and one sentence
of listening has the cheaper information on the listening side.
