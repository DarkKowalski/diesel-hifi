# Audio Realism — Analysis and Improvement Plan

Status: proposal, not implemented — re-audited against the tree at `da3e286` and confirmed
Scope: the exhaust/engine acoustic source in `sim-core`, and the listening stages downstream of it
Relates to: SPEC §6 (simulation model), SPEC §10 (acceptance criteria), Milestone 3

**Decisions taken.** The tailpipe duct belongs in `sim-core` — see §5.2. It is
simulation, not presentation, and is owned beside the manifold it is driven by.

This document records what is wrong with the sound the simulator currently produces,
why, and what to do about it. Nothing here has been implemented. No configuration,
solver, or web code has been changed.

**Nothing in this document is published data.** The source manual says nothing about
how this engine sounds. Every value proposed below is `calibrated` and must be declared
as such, with a purpose and a safe range, exactly like the five existing `audio.*`
entries.

---

## 1. Method

Three things were inspected end to end: the source (`crates/sim-core/src/sim/acoustics.rs`
and its call site in `crates/sim-core/src/sim/step.rs`), the transport
(`crates/sim-wasm/src/lib.rs:169`, `web/src/audio/exhaust-processor.js`), and the
listening stage (`web/src/lib/cabin.ts`, `web/src/lib/audioEngine.ts`).

The numbers below come from `cargo run --release -p sim-core --example audio_probe`,
run against the committed `mercedes-benz-om471-9-m3d-375kw.json` at `da3e286`. They
have since been re-run at the same commit and reproduce to the digit, including the
half-scale defect of §2.1.

---

## 2. What the measurement says

Band energy as a share of total, normalised (see §2.1 — the probe's own printout is
uniformly half these figures):

| Point | rpm | pedal | `<80 Hz` | `80–300 Hz` | `300 Hz–2 kHz` | `>2 kHz` | dBFS |
|---|---:|---:|---:|---:|---:|---:|---:|
| idle | 600 | 0.15 | 10% | **82%** | 4% | 4% | −33.8 |
| cruise | 1200 | 0.60 | 8% | **80%** | 12% | 0% | −12.9 |
| full | 1400 | 1.00 | 9% | **80%** | 11% | 0% | −10.5 |

Four fifths of the energy sits in one octave and a half around the low firing
harmonics. Above 2 kHz there is nothing at all at cruise and full load.

This is a drone. A heavy-duty diesel — heard from the cab or from outside — carries
substantial energy to 4 kHz and beyond, and it is that content, not the firing
fundamental, that makes the ear say *diesel* rather than *engine*.

### 2.1 A reporting defect in the probe

`crates/sim-core/examples/audio_probe.rs:115` computes `total` as the sum of squares of
the samples, but `band_energy` sums only positive-frequency bins (the `hi_bin` cap is
`n / 2`). By Parseval the negative bins hold the other half, so **every band reads
exactly half its true share** — which is why the four bands, covering 0–20 kHz and
therefore everything, sum to 50% rather than 100%. The probe's raw output confirms it
directly: the four bands sum to 50.1%, 50.2%, and 50.1% at idle, cruise, and full load
respectively.

Harmless to the physics, but the table is the instrument this whole effort will be
measured with, and an instrument that reads half scale should be fixed first. It is a
one-line change (halve `total`, or sum both halves), and once fixed the four bands
summing to 100% becomes a self-check on the probe itself.

---

## 3. Root cause

The single acoustic source in the model is, at `acoustics.rs:114`:

```text
source = Σ over cylinders of  A(ψ) · (p_cyl − p_exh) / p_amb
```

and its fastest deliberate feature is the exhaust valve ramp,
`valvetrain.exhaust_ramp_rad = 0.6109` rad (35°). Because that ramp is defined in crank
angle, its duration in seconds is set by engine speed:

| rpm | 35° takes | implied edge bandwidth |
|---:|---:|---:|
| 600 | 9.7 ms | ~50 Hz |
| 1400 | 4.2 ms | ~120 Hz |
| 2100 | 2.8 ms | ~180 Hz |

**The model contains no mechanism that can produce energy above roughly 500 Hz.** The
two-pole 6 kHz lowpass in `AudioCalibration` is filtering a signal that has nothing up
there to remove, and the 25 µs solver step — a 40 kHz sample rate, chosen precisely so
the acoustics would not need a synthesiser — is being spent on a signal with a few
hundred hertz of content.

Note that the ramp is *not* the thing to sharpen. `acoustics.rs:41` explains at length
why the ramp width was chosen, and it is right: a fast valve crack is what produces the
blowdown edge. Sharpening it further would push the fundamental structure of the pulse
around without adding any of the mechanisms that are actually missing, and it would be
sharpening a valve event past what a valve does.

The 4% above 2 kHz at idle and 0.0% at load is itself suspicious. A physical source does
not lose its entire top end when load is applied. The likeliest explanation is that the
idle figure is not signal at all but numerical residue from the explicit port transfer
near equilibrium — the two-sample limit cycle that `flow.rs` documents and clamps.

### 3.1 Settled: the idle top end is numerical, not signal

Step 0 added an octave dump to the probe, and it answers this conclusively. At idle the
spectrum decays monotonically from the firing octave down to −41 dB at 5–10 kHz, and
then **rises again to −11.6 dB in the top octave**:

| Octave | idle | cruise | full |
|---|---:|---:|---:|
| 630 Hz–1.25 kHz | 0.39% | 0.60% | 1.08% |
| 1.25–2.5 kHz | 0.06% | 0.04% | 0.11% |
| 2.5–5 kHz | 0.01% | 0.00% | 0.01% |
| 5–10 kHz | 0.00% | 0.00% | 0.00% |
| **10–20 kHz** | **3.73%** | 0.00% | 0.00% |

Of the 3.8% sitting above 2 kHz at idle, **3.73% is above 15 kHz** — within a whisker of
Nyquist. A physical source does not have a notch at 5–10 kHz and a peak at 18 kHz. This
is the two-sample limit cycle, it appears only at idle because that is where the port
sits nearest equilibrium, and it is arithmetic.

Two consequences, both of which change how the rest of the plan is measured:

- **The engine's real content above 2 kHz is 0.03% at idle and 0.02% at full load** —
  that is to say, none, everywhere. The §2 table overstated the top end; §4/P1 is
  understated, not overstated, by it.
- **The acceptance criteria of §9 must exclude the near-Nyquist residue**, or P1 could be
  "satisfied" at idle by 3.7% of energy that was already there and is not a sound. The
  probe now prints `>15kHz` separately for exactly this reason, and the criteria in §9
  have been restated against a 2–15 kHz band.

The residue is not worth fixing. It is inaudible where it sits, `flow.rs` already clamps
the mechanism that produces it, and P4 (§5.4) removes the `A · Δp` derivative that
amplifies it. It only needed to stop being counted as content.

---

## 4. Problem inventory

Ranked by audible payoff per unit of work.

### P1 — No combustion-excited structural noise

**Symptom.** The defining perceptual signature of a diesel is absent. The output has
the harmonic structure of an engine and none of the character of a compression-ignition
one.

**Physics.** In a real engine the premixed burn is a near-step pressure rise inside a
stiff iron box. That step excites bending and breathing modes of the block, head, and
covers, which ring for a few milliseconds and radiate directly off the engine's outer
surface — a path that never goes near the exhaust. This is *combustion noise*, and for a
heavy-duty diesel it dominates the spectrum from roughly 800 Hz to 4 kHz.

**Why the model cannot produce it.** The only radiating path implemented is gas leaving
the exhaust port. There is no structural path at all.

**Why it is cheap.** Everything needed is already computed. `p_cyl` exists per cylinder
at 40 kHz, and `combustion.premixed_duration_rad = 0.1745` rad (10°) puts the premixed
rise at 1.19 ms at 1400 rpm — about 48 solver steps, comfortably resolved.

### P2 — There is no exhaust system

**Symptom.** The sound has no pipe in it: no body, no resonance, no sense of length.

**Physics.** The acoustic path stops at a 0-D lumped manifold
(`air_path.exhaust_manifold_volume_m3 = 0.01`). Downstream of that a real truck has a
turbine, a downpipe, a DPF/SCR box of some tens of litres, and metres of tailpipe. An
exhaust note is very largely the standing-wave behaviour of that duct; the pulse train
is the excitation, not the sound.

Two specific absences:

- **No duct.** No delay, no reflection, therefore no resonance. Nothing gives the
  exhaust a pitch of its own independent of firing frequency.
- **No turbine.** Acoustically a turbine is a large silencer — order 10–20 dB of
  insertion loss, rising with frequency, plus substantial smearing of the pulse. Its
  absence is part of why the output reads as open-piped rather than as a turbodiesel.

What stands in for all of this today is `audio.lowpass_cutoff_hz`: two one-pole
sections. That is a tone control, not an exhaust system.

### P3 — Firing order is acoustically inert

**Symptom.** `geometry.firing_order` is `[1, 5, 3, 6, 2, 4]`, is validated as a
permutation, and drives the phase offsets at
`crates/sim-core/src/config/validate.rs:208` — yet **permuting it cannot change one
sample of audio output.**

**Why.** The six cylinders are evenly spaced by construction (`interval = CYCLE_RAD /
cylinders`), and they all discharge into a single lumped pressure node. A sum of six
identically-shaped pulses at even spacing is the same sum whichever cylinder is assigned
to which slot. The firing order is real in the config, validated in the config, and
invisible in the result.

**Physics.** A real inline-six has a log manifold with runner lengths spanning roughly
100 mm to 700 mm end to end. Pulses therefore arrive at the turbine at different times
and with different filtering, and *that* is what makes a straight six sound like a
straight six rather than like six identical pops. Firing order is audible in real
engines because the geometry makes it audible.

### P4 — The source is a Δp proxy where the real flow is already solved

**Symptom / defect.** `cylinder_source` uses `A(ψ) · Δp`. But `step.rs:487` already
calls `flow::exchange`, which solves the full compressible orifice relation with the
choked branch, the per-step mass clamp, and the equilibrium `settle` clamp.

So the model computes the correct mass flow through the exhaust port and then radiates
a different quantity. Real orifice flow goes as `√Δp` and saturates once the throat
reaches Mach 1; a linear-in-`Δp` proxy exaggerates the peak of the blowdown and changes
shape relative to the truth as the pulse moves through the choked-to-subsonic
transition. The pulse shape is the timbre.

**The provenance note is already wrong, today.** The entry for `audio.exhaust_gain`
describes the source as "a per-step difference of port flow". That is what the source
*should* be and is not what it is: the source is `A · Δp`, and the difference taken in
`push` is a difference of that, not of flow. This is not aspirational phrasing awaiting
P4 — it is a false description of the current model sitting in the one document whose
entire purpose is provenance discipline. It appears verbatim in both OM 471 documents
(`data/mercedes-benz-om471-9-m3d-375kw.json` and
`tests/fixtures/test-om471-m5v-brake.json`); the inline-four fixture escapes it only
because its note reads "invented test value", which is honest.

It is therefore worth correcting **now**, independently of whether P4 ever lands, and
not deferred to step 3. If P4 does land the note becomes true as written; if P4 is
dropped the note must still stop claiming a source the model does not have.

**Constraint on the fix.** The flow to radiate is the **clamped and settled** flow that
`exchange` actually applies, not a fresh unclamped call to `flow::mass_flow_kg_per_s`.
`flow.rs` documents that the unclamped explicit flow produces a two-sample limit cycle
at Nyquist which is inaudible in the pressure trace and glaring in the audio, "because
the radiation derivative amplifies with frequency". Radiating raw flow would walk
straight back into a defect that has already been paid for once.

### P5 — Zero cycle-to-cycle and cylinder-to-cylinder variation

**Symptom.** Six bit-identical cylinders produce a mathematically pure harmonic comb
with no jitter and no amplitude scatter. The ear is extremely good at hearing that, and
it hears it as *synthesised*.

**Physics.** Real engines have injector-to-injector delivery scatter, cylinder-to-cylinder
trapped-mass and residual spread, and genuine cycle-to-cycle combustion variability.
Nothing in a real six is identical to anything else in it.

**Compatibility.** SPEC §6 requires seeded randomness, not no randomness: *"Seed any
randomness explicitly."* A per-cylinder trim fixed once at reset from the existing
`ResetOptions.seed` is fully deterministic — the same seed gives the same engine, bit
for bit — and preserves `audio_is_bit_identical_across_batch_boundaries` and
`two_identical_runs_produce_identical_audio` unchanged, because it is not per-step noise.

### P6 — Consequential: the downstream chain is tuned for a signal that has no top end

These are harmless today and become real the moment P1 or P2 lands.

- **`cabin.ts:139` lowpasses at 1.7 kHz.** That figure was chosen when there was nothing
  above it to remove. It would destroy precisely the clatter P1 adds — and clatter is
  emphatically audible from a truck's driver seat, especially at idle. The value is
  defensible for glass and insulation and indefensible as a description of what a driver
  hears.
- **Linear-interpolation resampling** at 40 kHz → 48 kHz in the worklet is adequate for a
  signal that stops at 500 Hz. With genuine content at 2–4 kHz its imaging becomes
  audible, and the interpolator's own lowpass tilt starts removing content that was
  deliberately added.
- **`audio.soft_clip_knee = 0.85` with `exhaust_gain = 12000`** already puts full load at
  −10.5 dBFS with a 0.74 peak. Adding a second summed source will push the sum into the
  knee, and the knee is a distortion: it will generate its own harmonics and partially
  mask the ones being added on purpose. Gain staging must be revisited as part of P1,
  not after it.

---

## 5. Proposal

### 5.1 P1 — a structural radiation path

Add a second source term inside `Acoustics`, summed with the exhaust term *after* the
muffler filtering and *before* the soft clipper:

```text
exhaust    = lowpass²( highpass( d/dt  Σ A(ψ)·Δp ) )          existing path
structural = Σ over modes of  resonator_k( d/dt Σ p_cyl )     new path
sample     = softclip( g_exh · exhaust  +  g_str · structural )
```

Note that the exhaust term is written as `Σ A(ψ)·Δp` because that is what the code
actually radiates today — *not* `Σ port flow`, which is what it will radiate once P4
lands. Getting this the wrong way round is exactly the error the `exhaust_gain`
provenance note makes (§4/P4), so it is worth being pedantic about here: P1 is
implemented against the source as it is, and P4 later changes what feeds this same
slot without changing the topology.

The `lowpass²` term is what exists today and is likewise temporary: P2 substitutes the
duct into that same slot (§5.2), which is why the exhaust path is written as one
bracketed term rather than inlined. P1 must not assume the exhaust path's transfer
function.

The topology matters and is the main design decision here. The block radiates straight
to air; it does not go out of the tailpipe. Summing the structural term *before* the
muffler lowpass — or, later, before the duct — would filter it with a transfer function
that does not apply to it, and would attenuate exactly the band it exists to supply.
Getting this slot right when P1 lands is what makes P2 a substitution rather than a
rework.

**The resonators.** One two-pole modal filter per mode, in the standard form

```text
y[n] = 2·r·cos(θ)·y[n−1] − r²·y[n−2] + x[n]
θ = 2π·f_mode / f_s        r = 1 − θ / (2Q)
```

At `f_s = 40 kHz`, `f_mode = 1500 Hz`, `Q = 20`: `θ = 0.2356`, `r = 0.99411`, giving a
4.2 ms decay time constant and a ~29 ms T60. That is the right order for an iron
structure and it is unconditionally stable for `r < 1`. Cost is three multiplies per
mode per step — four modes at 40 kHz is negligible against the rest of the step.

Proposed starting bank, all `calibrated`, gains descending with frequency:

| Mode | Frequency | Q | Rationale |
|---|---:|---:|---|
| 1 | 900 Hz | 15 | Lower block/head bending; the "thud" under the clatter |
| 2 | 1600 Hz | 20 | Core of the combustion-noise band |
| 3 | 2600 Hz | 22 | The rattle proper |
| 4 | 3800 Hz | 25 | Top of the audible clatter; weakly driven, see below |

**Two properties worth stating, because they are what make this physics rather than an
effect.**

*No gating is needed, and none should be added.* Raw `dp/dt` is dominated in magnitude
by the compression and expansion swing — roughly 2 GPa/s for 20 MPa over 90° at
1400 rpm, against about 6.7 GPa/s for the premixed spike. Comparable numbers. But the
compression swing is a smooth 10 ms hump with essentially no energy in the 900–3800 Hz
passbands, while the 1.2 ms premixed rise has content well past 1 kHz. **The modal
filters do the discrimination themselves.** Anyone tempted to window the derivative
around the burn should not: the selectivity is already there and is the physically
correct mechanism.

*It couples to the existing combustion model for free.* Ignition delay already lengthens
when the cylinder is cold and lightly loaded (`ignition_delay.rs`), and a longer delay
already means a larger premixed fraction and a sharper rise
(`heat_release.rs`, `combustion.max_premixed_fraction = 0.85`). So the engine will
clatter hard when cold and at light load and mellow under load **because the combustion
model says so**, with no separate clatter schedule. The same mechanism gives the
decompression brake its crack: `brake::port_area_m2` opens a valve at the top of
compression, which is the fastest pressure event the model produces anywhere, and it
will drive the resonator bank hard without a single line of brake-specific audio code.

**One honest limitation.** The forcing is not white. A Wiebe rise with
`premixed_wiebe_shape = 2.0` starts with zero slope (`dx/dτ ∝ τ²`), so the model's burn
onset is *smoother* than a real one, and `dp/dt` energy falls off through the upper
modes. Mode 4 at 3800 Hz will be weakly driven and its gain will have to be large to
compensate. If the top end proves too quiet, the correct response is to reconsider the
premixed onset in `heat_release.rs` — where the smoothness actually originates — and not
to bolt a noise source onto the audio path.

#### 5.1.1 What building it changed — implemented at step 1

Four corrections, all found by tests rather than by reading.

**The resonator needs zeros, not just poles.** §5.1 specifies the bare all-pole form
`y[n] = 2r·cos(θ)·y[n−1] − r²·y[n−2] + x[n]`. That form has a DC gain of
`1/(1 − a1 − a2)`, which for these pole radii is about 50 — and a real mechanical mode
has *zero* response at DC, because a static pressure does not radiate. The consequence
was immediate and measurable: a stopped engine's trapped charge warms against the
cylinder walls at a bit over a pascal per step, and that genuine DC drift came through
the bank as a standing offset, breaking `a_reset_engine_is_silent` at 3.3 × 10⁻⁵ against
a 10⁻⁵ threshold. Adding zeros at `z = ±1` puts an exact null at DC and another at
Nyquist. The second null was unplanned and turned out to be worth as much as the first:
it dropped the near-Nyquist residue of §3.1 from 3.73% to 0.18% at idle, because the
bank now refuses to amplify the limit cycle.

**The peak response must be normalised.** Raw peak magnitude across this bank runs from
about 80 at 3800 Hz to 210 at 900 Hz, so an unnormalised `gain` is entangled with both
`q` and `frequency_hz` and retuning one mode silently retunes the others. Dividing the
exact `|H(e^{jθ})|` out at construction makes `gain` mean weight-at-resonance and
nothing else.

**Two seeded steps, not one.** The existing exhaust path seeds one predecessor because
it differences once. The structural path differences *twice* — once for `dp/dt`, once
more in the resonator zeros — so it needs two, and seeding only one leaves the thermal
drift arriving as an edge that the bank rings for tens of milliseconds. This is the same
argument `acoustics.rs` already makes about the reset click, applied one level deeper.

**The mode gains ascend with frequency, and §5.1's table is wrong to say otherwise.**
Shipped weights are 1.0, 4.0, 6.0, 11.0 for the four modes. §5.1 calls for weights
"descending with frequency" on radiation grounds and then separately predicts mode 4
"will have to be large to compensate" — those two statements cannot both be honoured,
and the measurement settles it: the drive falls off faster than the radiation weight
does, so the configured weights have to rise. With all four weights at 1.0 the 2–15 kHz
band sat at 0.88% against a 5% target; the ascending weights bring it to 9.72%. The
provenance entry says plainly that this compensates for a shortfall in the drive and is
not a claim that an engine radiates more at 3.8 kHz than at 900 Hz. The plan's own
prescription stands: this is evidence about `heat_release.rs`, and if the weights ever
need to be more extreme than this it should be pursued there.

**Gain staging was needed, exactly as §5.1 said it would be.** `exhaust_gain` is halved
from 12000 to 6000 and `structural_gain` set to 3.0; full load then peaks at 0.818
against the 0.85 knee. Left alone, the two summed sources put full load at 0.85 dead on
the knee and `the_output_keeps_headroom_across_the_operating_range` failed.

**One consequence deferred to step 5, and it is not small.** `cabin.ts` lowpasses at
1.7 kHz, so the cab stage — which is the *default* listening position — now discards
most of what P1 adds. It showed up as the wet path losing about 3 dB against the raw
path at both idle and load, breaking the end-to-end level-match assertion. Step 1 raises
`dynamics.makeupGain` from 1.5 to 2.1, which restores the level match and deliberately
does not address the real problem. Step 5 must raise that lowpass: as it stands, P1 is
audible on the raw stage and largely thrown away on the one a user hears first.

### 5.2 P2 — a one-dimensional exhaust duct

**Where it lives: `sim-core`. Decided.** A duct driven by real mass flow at the gas
temperature the model integrates is simulation, and it is owned beside the manifold that
drives it — a new `crates/sim-core/src/sim/exhaust.rs`, with a new `exhaust_system`
configuration section. It is not moved to the web layer, and specifically not moved
there to avoid writing provenance entries, which would be avoiding the wrong thing.

This draws the boundary for the whole audio path, and it is worth stating once:

| Concern | Owner | Why |
|---|---|---|
| Turbine insertion loss | `sim-core` | Gas dynamics through a turbine |
| Aftertreatment volume | `sim-core` | A compliance of known physical size |
| Tailpipe duct, reflection, radiation loss | `sim-core` | Wave propagation in a duct at a computed gas temperature |
| Per-cylinder runner delays (P3) | `sim-core` | Manifold geometry |
| Structural resonator bank (P1) | `sim-core` | Excited by cylinder pressure |
| Cab transfer path, reflections, room tail | `web/src/lib/cabin.ts` | Explicitly a listening choice, and says so |

The dividing line is not "physics versus filters" — the cab is filters and the duct is
filters. It is whether the thing being modelled is *the engine* or *where you are
standing*. Everything in the first five rows is the engine and does not change when the
listener moves.

Three consequences of `sim-core` ownership, all of which are constraints rather than
inconveniences:

- **The delay-line buffer is allocated at construction and never resized**, exactly as
  `Acoustics::new` allocates its ring (`sim/mod.rs:401`). SPEC §6 forbids allocation in
  the hot step loop, so the buffer must be sized for the longest duct the validated
  configuration permits, not for the current one.
- **The duct is state, so it must reset deterministically.** `Acoustics::reset` zeroes its
  samples as well as its cursors specifically so that two identically-reset simulations
  are bit-identical in every field; the duct owes the same. A duct that carries a stale
  standing wave across a reset would click, which is the defect
  `resetting_a_running_engine_does_not_click` exists to catch.
- **It must know nothing about the OM 471.** The inline-four fixture has to drive it too
  (§7). Anything whose only sensible value is OM 471-specific belongs in configuration,
  not in the module.

The model itself: a delay-line waveguide, with the source injected at the manifold end,
a one-way delay of `L / c` samples, a reflection at the open tip with `r ≈ −0.8` and a
one-pole lowpass on the reflected wave for radiation loss, and the listener taking the
outgoing wave at the tip. It replaces `audio.lowpass_cutoff_hz`, which is a tone control
standing in for an exhaust system.

With `c = √(γRT)`, `γ ≈ 1.33`, and turbine-outlet gas at 500–800 K, `c` is 440–560 m/s.
For `L = 3.5 m`:

| Quantity | Value |
|---|---|
| One-way delay | 6.3–8.0 ms — 250–320 samples at 40 kHz |
| First resonance `c/4L` | 31–40 Hz |
| Harmonics | odd multiples: ~31, 94, 156, 219 Hz … |

The duct should read its speed of sound from `state.exhaust.temperature_k`, which the
model already integrates. That makes the pipe resonance **shift with exhaust
temperature** — sharper when hot, flatter when cold — as an emergent consequence rather
than a scripted effect. It is the kind of result this codebase is built to produce, and
it costs one `sqrt` per step.

Two further elements belong in the same module and the same pass, sitting between the
manifold node and the duct inlet:

- **Turbine insertion loss.** A frequency-dependent attenuation. This is the term that
  makes the engine sound turbocharged rather than open-piped.
- **The aftertreatment box.** A lumped compliance shunted across the duct, which is what
  an expansion silencer of a few tens of litres is at these wavelengths.

Fractional delay will be needed — `L/c` is not an integer number of samples and `c`
moves with temperature. Linear interpolation on the delay tap is sufficient here and
avoids a zero-order hold whose step width varies, which would be broadband noise of
exactly the kind §3 is trying to add honestly.

**Configuration surface.** Because this lands in `sim-core`, the parameters are
configuration with provenance rather than constants in a module. The proposed
`exhaust_system` section, all `calibrated`, all needing a stated purpose and safe range:

| Path | Meaning |
|---|---|
| `exhaust_system.tailpipe_length_m` | Duct length; sets the resonance with `c` |
| `exhaust_system.open_end_reflection` | Reflection coefficient at the tip, negative, `\|r\| < 1` |
| `exhaust_system.radiation_cutoff_hz` | One-pole loss on the reflected wave |
| `exhaust_system.aftertreatment_volume_m3` | Shunt compliance of the DPF/SCR box |
| `exhaust_system.turbine_insertion_loss_db` | Broadband attenuation across the turbine |
| `exhaust_system.turbine_loss_cutoff_hz` | Where that attenuation starts rising |
| `exhaust_system.runner_length_min_m` | Shortest manifold runner (P3) |
| `exhaust_system.runner_length_max_m` | Longest manifold runner (P3) |

Runner lengths are a span rather than an enumerated list per cylinder, so the section
carries no cylinder count and the inline-four fixture can use it unchanged. Validation
must enforce `|open_end_reflection| < 1` for duct stability (§10) and a positive length
span, and must size the delay buffer from the maximum length the ranges permit.

`audio.lowpass_cutoff_hz` becomes dead once the duct lands. Remove it rather than leave
it in place doing nothing — a parameter that no longer affects the output but still
carries a provenance entry claiming it is the "muffler roll-off" is worse than no
parameter, and `paths.rs` requires the provenance list to match the path set exactly, so
the removal is mechanical and checked.

### 5.3 P3 — per-cylinder runner delays

Give each cylinder a short delay line between its port and the manifold summing
junction, with lengths derived from a configured runner-length span rather than
enumerated one by one. At `c ≈ 550 m/s`, a 100–700 mm spread is 0.18–1.27 ms, or 7–51
samples at 40 kHz — short, cheap, and enough to matter, since it is comparable to the
period of the upper firing harmonics.

This is the change that makes firing order audible, and it comes with the test that
proves it: **`1-5-3-6-2-4` and `1-2-3-4-5-6` must produce different waveforms.** That
assertion is impossible to satisfy today and is the cleanest possible statement that the
firing order has stopped being decorative.

Best done together with P2 — both are delay lines feeding the same junction, and doing
them separately means building the junction twice.

### 5.4 P4 — radiate the flow that was actually applied

Have `flow::exchange` return the net mass transferred alongside the resulting `Charge`,
and use that as the acoustic source in place of `A · Δp`. This is the clamped, settled
flow, which is the requirement from §4/P4.

**This is cheaper than it first appears, because the brake already goes through
`exchange` too.** There are two call sites, not one: `step.rs:487` is the exhaust
blowdown, and `step.rs:444` is the decompression brake's release lobe, which was
already routed through the same compressible orifice solver when the brake landed in
Milestone 4. So the brake does not need a parallel fix and there is no risk of the bark
and the pulse ending up on two different scales — a single return value from `exchange`,
threaded to both sites, covers them together. The `brake_area_m2` argument to
`cylinder_source` disappears rather than needing its own flow computation.

Consequences to expect and to handle:

- `audio.exhaust_gain` will need recalibrating; the source changes units and scale.
- The `exhaust_gain` provenance note becomes accurate — though per §4/P4 it should be
  corrected before this step regardless.
- **Four** unit tests in `acoustics.rs` call `cylinder_source` directly, not two:
  `a_closed_cylinder_contributes_nothing_however_high_its_pressure` (`:378`),
  `a_braked_cylinder_is_heard_even_though_it_is_closed` (`:384`),
  `the_brake_and_the_exhaust_event_never_open_the_port_twice` (`:414`), and
  `blowdown_produces_a_positive_source_and_backflow_a_negative_one` (`:440`). All four
  express invariants that should survive the change, but none of their call signatures
  will. The two brake-related ones need the most thought, since the brake area stops
  being an argument to the source function at all.

### 5.5 P5 — seeded per-cylinder trim

At reset, derive a fixed per-cylinder multiplier from `ResetOptions.seed` and apply it to
effective port area and delivered fuel. A ±1–2% spread is the right order — enough to
break the comb, far too little to affect any torque or pressure acceptance criterion.

Fix it once at reset. Not per step, not per cycle. That keeps determinism exact and
leaves the batch-invariance and repeatability tests untouched.

`cabin.ts` already has `xorshift32` for exactly this reason, with a comment explaining
why an unseeded room is unacceptable. The same argument applies here and the same
generator shape should be used on the Rust side.

#### 5.5.1 What building it changed — implemented at step 2

**Two spreads, not one.** §5.5 describes "a fixed per-cylinder multiplier" applied to
both port area and delivered fuel. Splitting it in two costs one extra path and buys
honesty: injector delivery scatter and port flow scatter are genuinely different
manufacturing facts with different magnitudes, and each lands in the section that
already owns the quantity it perturbs — `injection.cylinder_delivery_spread` at ±1.5%
and `valvetrain.exhaust_area_spread` at ±2%. One knob covering both would have needed a
new home and a name that described neither.

**`sim-core` already had the generator.** §5.5 proposes porting `cabin.ts`'s
`xorshift32` shape to Rust. Unnecessary: `rng.rs` has `Pcg32`, and `SimState` already
carries one seeded from `ResetOptions.seed` with a comment saying it exists so that
"cycle-to-cycle variation cannot later be added unseeded". This is that variation, and
it draws from the stream that was put there for it.

**Trims are drawn for every slot, not only the live ones.** The reset loop runs over
`MAX_CYLINDERS`, so drawing per slot keeps the value a given cylinder index receives
independent of the engine's cylinder count. Drawing only for live cylinders would have
made the inline-four fixture and the six share a stream position by accident.

**The smoke limit still applies after the trim.** A generous injector on a cylinder
short of air is held to the air it has, which is what stops the scatter from quietly
raising fuelling at the limit.

The calibration is unmoved, and that is now asserted rather than assumed:
`the_per_cylinder_build_scatter_does_not_move_the_calibration` sweeps the scattered
engine against one with both spreads zeroed and requires peak power and peak torque to
agree within 1%, plus the published 230 bar envelope to hold for the scattered engine at
every point rather than on average. The validator caps both spreads at 0.1 so a realism
knob cannot become a calibration change by being turned up.

### 5.6 P6 — retune the listening stages, last

Once there is content above 1 kHz, revisit in this order:

1. `CABIN_SPEC.filters` — the 1.7 kHz lowpass, and whether the −3 dB at 380 Hz is still
   the right boxiness correction against a spectrum that is no longer 80% low-mid.
2. Gain staging: `exhaust_gain`, the new `structural_gain`, and `soft_clip_knee`
   together, so full load approaches the knee without living on it.
3. `CABIN_SPEC.dynamics` — the compressor was calibrated against the current spectrum
   and its 6 ms attack will behave differently against transients with real high-frequency
   content.
4. Only if measurably needed: a better resampler in the worklet.

Doing any of this before P1 and P2 is retuning against a signal that is about to change.

---

## 6. Invariants that must not be broken

Every item below is currently asserted by a test, a SPEC clause, or a project rule.

| Invariant | Where it lives |
|---|---|
| Exactly one sample per solver step | `tests/acoustics.rs:149` |
| Bit-identical across batch boundaries | `tests/acoustics.rs:198` |
| Identical runs produce identical audio | `tests/acoustics.rs:228` |
| A reset engine is silent; reset does not click | `tests/acoustics.rs:166`, `:337` |
| Every sample finite and inside `[−1, 1]` | `tests/acoustics.rs:238` |
| The note sits at the firing frequency, for any cylinder count | `tests/acoustics.rs:257`, `:273` |
| No allocation, logging, blocking, or I/O in the hot loop | SPEC §6, CLAUDE.md |
| No branching on the OM 471 ID; nothing knows the cylinder count | SPEC §5, `acoustics.rs:27` |
| `sim-core` free of browser, DOM, audio-device, filesystem deps | SPEC §3 |
| Randomness seeded explicitly | SPEC §6 |
| Silence in, silence out; the stage adds nothing | `cabin.ts` module doc, `test/cabin.test.ts` |
| Nothing presented as OEM data | SPEC §5, §12 |

Two further constraints that shape the design:

- **Keep the WASM boundary as it is.** `drain_audio` returns one mono `Vec<f32>`
  (`crates/sim-wasm/src/lib.rs:169`) and the worklet consumes one mono stream. The
  structural path must be **mixed into that same stream inside `Acoustics::push`**, not
  exposed as a second channel. A second channel would touch the WASM adapter, the worker
  protocol, and the worklet, for no audible gain that a correctly weighted mono sum does
  not already give. SPEC §7 asks for a coarse boundary; this keeps it.
- **Ring buffer capacity is `solver.max_steps_per_batch`** (`sim/mod.rs:401`), currently
  20 000 samples ≈ 0.5 s. None of this changes the sample count per step, so no capacity
  change is needed.

---

## 7. The cost of a configuration field

Every new `audio.*` value touches six places, and validation fails loudly if any is
missed — `paths.rs` states that adding a field without provenance "is a validation
failure, not a silent omission". Budget for this; it is most of the mechanical work.

1. The struct field — `config/mod.rs:484` (`AudioCalibration`).
2. A `value_at` arm — `config/mod.rs:716`, or an explicit non-scalar exemption.
3. The path in `PARAMETER_PATHS` — `config/paths.rs:137`.
4. A provenance entry with `status: "calibrated"`, `value_note`, `purpose`, and
   `safe_range`, in **all three** configuration documents:
   - `crates/sim-core/data/mercedes-benz-om471-9-m3d-375kw.json`
   - `crates/sim-core/tests/fixtures/test-om471-m5v-brake.json`
   - `crates/sim-core/tests/fixtures/test-inline-four.json`
5. Range validation in `config/validate.rs`, plus whatever `tests/config_provenance.rs`
   requires of the new entries.
6. The `audio()` test helper at `sim/acoustics.rs:337`, which is the only place in the
   workspace that constructs an `AudioCalibration` as a struct literal. It is not
   covered by the provenance validator, so the compiler is what catches it — but it
   means every field addition and the `lowpass_cutoff_hz` removal (§5.2) break the unit
   tests in the same file being edited.

The inline-four fixture matters more than it looks: it is the test that proves the
acoustics know nothing about cylinder count, so it must carry every new field with
values that make sense for a different engine. Any new field whose sensible value is
OM 471-specific is a design smell — it means engine-specific calibration is leaking into
the solver, which SPEC §5 forbids.

**Scale of it.** With the duct in `sim-core` (§5.2), the plan adds roughly eight
`exhaust_system` paths plus the P1 mode parameters and the P5 trim spread — call it
fifteen new paths, each needing a provenance entry in all three configuration documents.
That is on the order of forty-five JSON entries, and it is the single largest block of
mechanical work in this plan. Two things follow. First, do not batch it: add each
section with its own step so a validation failure names one thing. Second, the effort is
the price of the provenance discipline working as designed — the same rule that makes it
impossible to smuggle in an unsourced value is what makes a new subsystem expensive to
declare, and that tradeoff was made deliberately in `paths.rs`.

One path is removed: `audio.lowpass_cutoff_hz` (§5.2).

---

## 8. Work order

Sequenced so each step is independently verifiable and independently revertable.

| Step | Change | Why here |
|---|---|---|
| 0 | Fix the `audio_probe` normalisation; add a spectrum dump to establish a baseline and settle whether the idle `>2 kHz` figure is signal or numerical residue. Separately, correct the `audio.exhaust_gain` `value_note` in both OM 471 documents so it describes `A · Δp` rather than a port-flow difference (§4/P4) | The instrument must read true before anything is measured with it — and the one provenance entry that is false today should not stay false through four more steps |
| 1 | **P1** — structural resonator bank | Largest audible gain; self-contained in `acoustics.rs` plus the summing loop in `step.rs`; no new subsystem |
| 2 | **P5** — seeded per-cylinder trim | Three lines, disproportionate realism, zero interaction with anything else |
| 3 | **P4** — radiate applied flow | Prerequisite for P2: a duct should be driven by flow, not by a proxy for it |
| 4 | **P2 + P3** — duct, turbine loss, aftertreatment volume, per-cylinder runners | One junction, built once |
| 5 | **P6** — retune `CABIN_SPEC`, gain staging, compressor | Last, when the signal has stopped moving |

Steps 1 and 2 together should be enough to change the answer to "does this sound like a
diesel". Step 4 changes the answer to "does this sound like *a truck*".

---

## 9. Acceptance criteria

Measurable, in the style of SPEC §10. The band targets are **calibration targets, not
published data**, and should be recorded as an assumption in the README's
"Assumptions not supported by the manual" table.

Spectral, from a corrected `audio_probe`. **All bands above 2 kHz are measured over
2–15 kHz, not 2 kHz–Nyquist**, so the near-Nyquist residue of §3.1 cannot satisfy a
criterion it is not signal for.

Baseline for each, from step 0, so the improvement is legible rather than asserted:

| Criterion | Target | Baseline at step 0 | After step 1 |
|---|---:|---:|---:|
| Idle, 500 Hz–15 kHz | ≥ 15% | 0.91% | **27.37%** |
| Idle, 2–15 kHz | ≥ 5% | 0.03% | **9.72%** |
| Full load, 500 Hz–15 kHz | ≥ 10% | 1.36% | **51.29%** |
| `80–300 Hz`, any point | ≤ ~55% | 82.0% (idle) | **39.2%** (idle) |
| `<80 Hz`, any point | ≤ 35% | 10.4% (idle) | **1.4%** (idle) |

All five hold after step 1, and full load peaks at 0.818 against the 0.85 knee, so the
improvement is not the clipper's doing — which §10 warns is the way to "succeed" here
dishonestly. The near-Nyquist residue of §3.1 also fell from 3.73% to 0.18% at idle,
because the modal bank's zeros sit at DC *and* at Nyquist and it therefore refuses to
amplify the limit cycle.

The idle `500 Hz–15 kHz` baseline is 0.91%, not the 4.6% a band running to Nyquist
would report, because 3.73% of that figure is residue. Only `<80 Hz` passes today, and
it passes comfortably — the low end is not the problem and no part of this plan should
make it one.

The gap is therefore much larger than §2's table suggested: P1 has to find a factor of
roughly sixteen in the 500 Hz–15 kHz band at idle, from a starting point of essentially
nothing.

The `<80 Hz` bound matters even though it is currently met: it is real energy but no
ordinary speaker reproduces it, and `cabin.ts` is right that spending headroom there is
worse than wasteful because the compressor ducks the audible band to make room.

Behavioural, as new tests:

- Clatter is load- and temperature-dependent: upper-band energy at light load and cold
  coolant measurably exceeds that at full load, with no clatter-specific schedule
  anywhere in the code.
- **Changing `geometry.firing_order` changes the output waveform.** Impossible today.
- The pipe resonance moves with exhaust temperature.
- ~~The engine brake's upper-band energy exceeds that of the fuelled engine at the same
  speed~~ — **corrected in step 1: this comparison is wrong and does not hold.** Measured
  at 1600 rpm, upper-band RMS is 0.079 braking against 0.112 for a fuelled engine at
  pedal 0.4. That is not a failure of the brake path; it is a badly chosen comparison. A
  fuelled engine is *burning*, and burning is loud, so the two cases differ by how much
  energy is in the system rather than by what the release lobe does. The comparison that
  isolates the lobe is against the **same engine coasting** — no fuel either way, brake
  the only difference — and there the brake wins decisively: 0.079 against 0.025, a
  factor of 3.2 in upper-band RMS and better than 4× in energy. That is the assertion
  now in the suite, and it still says what this criterion meant to say: the bark comes
  from the model, not from a layer.
- The inline-four fixture still sounds at its own firing frequency and still exercises
  every new parameter.

Non-regression:

- Every invariant in §6 still asserted and still passing.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `pnpm check`, `pnpm test`, `pnpm build` all run and pass.
  Per CLAUDE.md and SPEC §11, none of these may be reported as passing unless actually run.
- Torque, power, peak-pressure, and brake-anchor results unchanged within existing
  tolerances. P5 in particular must not move the calibration: a ±2% cylinder trim that
  shifts peak power is a trim that is too large.

---

## 10. Risks and open questions

**The soft clipper may end up doing the work.** With two summed sources and a 0.85 knee
already being approached at full load, the saturation will generate its own harmonics in
the same band P1 is adding. It is possible to "succeed" on the band-energy criteria
purely by clipping harder. The honest guard is to measure the bands *before* the clipper
as well as after, and to require that the improvement exists pre-clip.

**Mode gains have no published anchor whatsoever.** The four frequencies and Q values are
plausible for an engine of this size and are nothing more than that. They must be
declared `calibrated` with a stated purpose and safe range, and the README must say
plainly that the clatter is a listening choice fitted to a physical mechanism, not a
measurement of an OM 471. This is the same standard the existing five `audio.*` entries
already meet.

**The premixed onset may be too smooth to drive the top of the bank** (§5.1). If mode 4
needs an implausible gain to be heard, that is evidence about `heat_release.rs`, not about
the audio path, and should be pursued there.

**Duct stability.** A delay line with a temperature-varying fractional length and a
feedback reflection is the one genuinely new numerical risk in this plan. It needs
`|r| < 1` enforced, the fractional-delay interpolation checked for stability as `c`
moves, and a non-finite guard consistent with how the rest of the solver rejects bad
state.

**Settled: the duct lives in `sim-core`** (§5.2). What remains of the question is an
obligation rather than a choice. The tailpipe length is not published, and it will drive
an audible resonance — so it is precisely the kind of value SPEC §5 exists to govern. It
must carry a `calibrated` status, a stated purpose, and a safe range; the README's
"Assumptions not supported by the manual" table must list the duct length, the
reflection coefficient, and the turbine loss alongside the existing entries for firing
order and rotating inertia; and no part of the UI or documentation may imply the exhaust
geometry came from the manual. The risk of `sim-core` ownership is not that the physics
is wrong — it is that a calibrated number sitting in the physics crate reads as more
authoritative than one sitting in a file that announces itself as a listening choice.
The provenance entries are what answer that, so they have to be written properly rather
than filled in to satisfy the validator.

**Open: whether to model intake/compressor noise at all.** A truck radiates from its air
intake too. Deliberately excluded from this plan: at
`turbo.max_shaft_speed_rad_per_s = 15000` (≈143 000 rpm) the blade-pass frequency of a
twelve-blade wheel is ultrasonic, so the audible content would be low-order shaft
harmonics and diffuser noise — the least physically grounded and hardest to justify item
on the list. It should stay out until P1–P5 are done and it can be judged against a
sound that is otherwise right.
