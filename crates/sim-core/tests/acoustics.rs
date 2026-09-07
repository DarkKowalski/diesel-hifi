//! Exhaust acoustic output tests.
//!
//! The solver's 25 us step is a 40 kHz sample rate, so the exhaust sound is not
//! synthesised: one sample per step is taken from the computed blowdown through
//! the exhaust ports. These tests exist to prove that what comes out is really
//! the simulation — the right number of pulses, at the right frequency, derived
//! from configuration rather than hard-coded — and that producing it does not
//! break the determinism guarantees everything else depends on.

use std::path::Path;

// Crest factor, RMS and the firing frequency come from `sim_core::analysis`
// rather than being defined again here. They used to be local copies, which is
// how a test and the probe it is meant to agree with come to disagree: one of
// them gets a correction and the other does not. `analysis` is also the module
// that is itself tested against known signals, in `tests/analysis.rs`.
use sim_core::analysis::{crest_db, firing_hz, rms as rms_of};
use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::{
    Catalog, Controls, Engine, EngineConfig, ResetOptions, Simulation, ValidatedConfig,
};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

fn fixture_config() -> ValidatedConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-inline-four.json");
    let text = std::fs::read_to_string(path).expect("fixture readable");
    EngineConfig::from_json(&text)
        .expect("fixture parses")
        .validate()
        .expect("fixture validates")
}

fn controls(pedal: f64, starter: bool, ignition: bool) -> Controls {
    Controls {
        pedal,
        load_torque_nm: 0.0,
        starter,
        ignition,
        egr_enabled: true,
        ..Controls::default()
    }
}

/// Run at a held speed and return the samples produced over `steps`.
///
/// The crank is re-pinned in short chunks. Pinning once per large batch is not
/// good enough: an unloaded engine at part pedal accelerates by a couple of
/// hundred rpm inside a 2000-step batch, so the *average* speed over the batch
/// would sit well above the speed being pinned — and the exhaust note would
/// come out correspondingly sharp.
const PIN_CHUNK: u32 = 100;

fn samples_at_speed(config: ValidatedConfig, rpm: f64, steps: usize) -> Vec<f32> {
    samples_at(config, rpm, 0.4, 293.15, steps)
}

fn samples_at(
    config: ValidatedConfig,
    rpm: f64,
    pedal: f64,
    coolant_temp_k: f64,
    steps: usize,
) -> Vec<f32> {
    summed(&frames_at(config, rpm, pedal, coolant_temp_k, steps))
}

/// Radiating paths interleaved into each frame: exhaust, block, body.
const PATHS: usize = 3;

/// Run at a held speed and return `steps` interleaved frames.
///
/// The solver emits the three paths separately so the listening stage can filter
/// each by its own route. Tests that care about *the output* sum them with
/// [`summed`]; tests that care about one path take it with [`path_of`], which is
/// one run rather than the three it used to take.
fn frames_at(
    config: ValidatedConfig,
    rpm: f64,
    pedal: f64,
    coolant_temp_k: f64,
    steps: usize,
) -> Vec<f32> {
    let mut sim = Simulation::new(
        config,
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k,
        },
    )
    .expect("simulation builds");
    sim.set_controls(controls(pedal, false, true))
        .expect("controls accepted");

    // Settle, discarding the start-up transient.
    let mut sink = vec![0.0f32; PIN_CHUNK as usize * PATHS];
    for _ in 0..1_200 {
        sim.advance(PIN_CHUNK).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        sim.drain_audio(&mut sink);
    }

    let mut out = vec![0.0f32; steps * PATHS];
    let mut frames = 0;
    while frames < steps {
        let batch = PIN_CHUNK.min((steps - frames) as u32);
        sim.advance(batch).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        frames += sim.drain_audio(&mut out[frames * PATHS..]);
    }
    out
}

/// What a listener hears: the three paths of each frame added up.
fn summed(frames: &[f32]) -> Vec<f32> {
    frames.chunks(PATHS).map(|f| f.iter().sum()).collect()
}

/// Drain at most `frames` frames and return them as interleaved floats.
fn drain_frames(sim: &mut Simulation, frames: usize) -> Vec<f32> {
    let mut interleaved = vec![0.0f32; frames * PATHS];
    let written = sim.drain_audio(&mut interleaved);
    interleaved.truncate(written * PATHS);
    interleaved
}

/// Drain at most `frames` frames, summed down to what a listener hears.
fn drain_summed(sim: &mut Simulation, frames: usize) -> Vec<f32> {
    summed(&drain_frames(sim, frames))
}

/// Drain and discard, for settling loops that only need the buffer emptied.
fn discard_audio(sim: &mut Simulation, frames: usize) {
    let mut sink = vec![0.0f32; frames * PATHS];
    sim.drain_audio(&mut sink);
}

/// One path on its own, taken from interleaved frames.
fn path_of(frames: &[f32], path: usize) -> Vec<f32> {
    frames.chunks(PATHS).map(|f| f[path]).collect()
}

/// Path indices, matching `sim::acoustics`.
const EXHAUST: usize = 0;
const BLOCK: usize = 1;
const BODY: usize = 2;

/// Fundamental frequency of a signal, by autocorrelation.
///
/// Counting pulses by threshold over-counts here: a blowdown is not a single
/// bump. The valve cracks open against a large pressure ratio, then the piston
/// sweeps the rest out, so one exhaust event has structure. Autocorrelation asks
/// the question that actually matters — what does the signal repeat at — and is
/// indifferent to the shape of what repeats.
fn fundamental_hz(samples: &[f32], sample_rate_hz: f64, min_hz: f64, max_hz: f64) -> f64 {
    let min_lag = (sample_rate_hz / max_hz).round() as usize;
    let max_lag = ((sample_rate_hz / min_hz).round() as usize).min(samples.len() / 2);
    assert!(min_lag > 0 && max_lag > min_lag, "invalid search range");

    let mean = samples.iter().map(|s| f64::from(*s)).sum::<f64>() / samples.len() as f64;
    let centred: Vec<f64> = samples.iter().map(|s| f64::from(*s) - mean).collect();

    let scores: Vec<f64> = (min_lag..=max_lag)
        .map(|lag| {
            let overlap = centred.len() - lag;
            (0..overlap)
                .map(|i| centred[i] * centred[i + lag])
                .sum::<f64>()
                / overlap as f64
        })
        .collect();

    let best = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    // Take the shortest *peak* that scores nearly as well as the best, not the
    // best outright. A signal that repeats every N samples also repeats every
    // 2N, and with any cylinder-to-cylinder variation the double lag can score
    // higher — which would report the note an octave low. Requiring a local
    // maximum rather than merely a high score keeps this off the slope leading
    // up to the true peak.
    let index = (1..scores.len() - 1)
        .find(|i| {
            scores[*i] >= best * 0.85 && scores[*i] > scores[i - 1] && scores[*i] >= scores[i + 1]
        })
        .unwrap_or_else(|| {
            scores
                .iter()
                .position(|score| *score == best)
                .expect("the best score is its own match")
        });
    sample_rate_hz / (min_lag + index) as f64
}

#[test]
fn the_sample_rate_is_the_reciprocal_of_the_solver_step() {
    let config = config();
    let dt = config.config().solver.fixed_step_s;
    let sim = Simulation::new(config, ResetOptions::default()).expect("simulation builds");
    assert!((sim.audio_sample_rate_hz() - 1.0 / dt).abs() < 1.0e-9);
    assert!(
        (sim.audio_sample_rate_hz() - 40_000.0).abs() < 1.0e-6,
        "a 25 us step is 40 kHz"
    );
}

#[test]
fn exactly_one_frame_is_produced_per_step() {
    // A frame, not a sample: the three radiating paths cross the boundary
    // interleaved, and `drain_audio` counts frames so that this stays one per
    // step. Counting floats would make it three, and the invariant worth having
    // is the one about steps.
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");
    sim.set_controls(controls(0.0, true, true))
        .expect("controls");

    sim.advance(1_000).expect("advance");
    assert_eq!(sim.audio_available(), 1_000);

    let mut out = vec![0.0f32; 4_000 * PATHS];
    assert_eq!(sim.drain_audio(&mut out), 1_000);
    assert_eq!(sim.audio_available(), 0);

    sim.advance(2_500).expect("advance");
    assert_eq!(sim.audio_available(), 2_500);
}

#[test]
fn a_reset_engine_is_silent_and_a_reset_clears_buffered_audio() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");

    // Nothing turns and nothing burns, so a reset engine makes no sound at all —
    // not merely a quiet one. It has no settling transient either: its manifolds
    // start at ambient, which is the only pressure a stopped engine can hold.
    let mut out: Vec<f32> = Vec::new();
    while out.len() < 20_000 {
        let batch = 20_000u32.min((20_000 - out.len()) as u32);
        sim.advance(batch).expect("advance");
        out.extend(drain_summed(&mut sim, 20_000));
    }

    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak < 1.0e-5,
        "a stationary engine must be silent, peaked at {peak}"
    );

    sim.set_controls(controls(0.0, true, true))
        .expect("controls");
    sim.advance(4_000).expect("advance");
    sim.reset(ResetOptions::default()).expect("reset");
    assert_eq!(sim.audio_available(), 0);
    assert_eq!(sim.audio_dropped(), 0);
}

/// Producing audio must not disturb the batching invariance the rest of the
/// simulation relies on: the samples are a pure function of state, so one batch
/// of `n` steps and `n` batches of one step must yield the same signal.
#[test]
fn audio_is_bit_identical_across_batch_boundaries() {
    let make = || {
        let mut sim =
            Simulation::new(config(), ResetOptions::default()).expect("simulation builds");
        sim.set_controls(controls(0.5, true, true))
            .expect("controls");
        sim
    };

    let mut one_batch = make();
    one_batch.advance(6_000).expect("advance");
    let mut bulk = vec![0.0f32; 6_000 * PATHS];
    assert_eq!(one_batch.drain_audio(&mut bulk), 6_000);

    // Compared interleaved rather than summed, so this holds for every path
    // rather than for a total that could hide two paths trading places.
    let mut stepwise = make();
    let mut single = vec![0.0f32; 6_000 * PATHS];
    for frame in single.chunks_mut(PATHS) {
        stepwise.advance(1).expect("advance");
        let mut one = [0.0f32; PATHS];
        assert_eq!(stepwise.drain_audio(&mut one), 1);
        frame.copy_from_slice(&one);
    }

    assert_eq!(
        bulk, single,
        "audio must be bit-identical however the steps are batched"
    );
}

#[test]
fn two_identical_runs_produce_identical_audio() {
    let run = || samples_at_speed(config(), 1_200.0, 8_000);
    assert_eq!(
        run(),
        run(),
        "audio must be reproducible for identical input"
    );
}

#[test]
fn every_sample_is_finite_and_inside_the_output_range() {
    let samples = samples_at_speed(config(), 1_400.0, 20_000);
    for (index, sample) in samples.iter().enumerate() {
        assert!(
            sample.is_finite() && sample.abs() <= 1.0,
            "sample {index} is {sample}, outside the range an audio device accepts"
        );
    }
    assert!(
        samples.iter().any(|s| s.abs() > 1.0e-4),
        "a running engine should not be silent"
    );
}

/// An inline six fires six times per `4*PI`, so its exhaust note has a
/// fundamental at `rpm/120 * 6` — 60 Hz at 1200 rpm. Nothing in the acoustic
/// code knows the cylinder count: this comes out of the firing order and the
/// phase offsets derived from it.
#[test]
fn the_exhaust_note_sits_at_the_firing_frequency() {
    let rpm = 1_200.0;
    let samples = samples_at_speed(config(), rpm, 24_000);
    let measured = fundamental_hz(&samples, 40_000.0, 20.0, 200.0);
    let expected = firing_hz(rpm, 6);

    assert!(
        (measured - expected).abs() / expected < 0.05,
        "expected a fundamental near {expected:.1} Hz at {rpm} rpm, measured {measured:.1} Hz"
    );
}

/// The same code, a different configuration: the four-cylinder fixture must
/// sound at four events per cycle, not six. This is what proves the note comes
/// from the configuration rather than from a constant somewhere in the solver.
#[test]
fn the_four_cylinder_fixture_sounds_at_its_own_firing_frequency() {
    let rpm = 1_200.0;
    let config = fixture_config();
    let cylinders = config.config().geometry.cylinders;
    assert_eq!(cylinders, 4, "the fixture is an inline four");

    let sample_rate = 1.0 / config.config().solver.fixed_step_s;
    let samples = samples_at_speed(config, rpm, 24_000);
    let measured = fundamental_hz(&samples, sample_rate, 20.0, 200.0);
    let expected = firing_hz(rpm, cylinders);

    assert!(
        (measured - expected).abs() / expected < 0.05,
        "the four-cylinder fixture should sound near {expected:.1} Hz, measured \
         {measured:.1} Hz"
    );

    // And it must be audibly different from the six at the same speed.
    let six = firing_hz(rpm, 6);
    assert!(
        (measured - six).abs() / six > 0.1,
        "a four and a six must not sound the same"
    );
}

/// Double the engine speed, double the firing frequency.
#[test]
fn the_exhaust_note_rises_with_engine_speed() {
    let low = fundamental_hz(
        &samples_at_speed(config(), 700.0, 24_000),
        40_000.0,
        15.0,
        200.0,
    );
    let high = fundamental_hz(
        &samples_at_speed(config(), 1_400.0, 24_000),
        40_000.0,
        15.0,
        200.0,
    );

    assert!(
        (low - firing_hz(700.0, 6)).abs() / firing_hz(700.0, 6) < 0.06,
        "700 rpm should sound near 35 Hz, measured {low:.1} Hz"
    );
    assert!(
        (high - firing_hz(1_400.0, 6)).abs() / firing_hz(1_400.0, 6) < 0.06,
        "1400 rpm should sound near 70 Hz, measured {high:.1} Hz"
    );
    assert!(
        (high / low - 2.0).abs() < 0.15,
        "doubling engine speed must double the note: {low:.1} Hz then {high:.1} Hz"
    );
}

/// Resetting a running engine must not click.
///
/// The radiated signal is a difference between consecutive source values. After
/// a reset there is no previous value, and treating the missing one as zero
/// turns the standing pressure across an already-open exhaust port into a
/// one-sample impulse. An impulse is broadband, so the muffler filter rings at
/// its cutoff and it is heard as a sharp crack — right at the moment the user
/// restarts the engine.
#[test]
fn resetting_a_running_engine_does_not_click() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");
    sim.set_controls(controls(0.6, true, true))
        .expect("controls");

    for _ in 0..120 {
        sim.advance(4_000).expect("advance");
        discard_audio(&mut sim, 4_000);
    }

    sim.reset(ResetOptions::default()).expect("reset");

    // A reset engine is stopped, so the samples that follow should be near
    // silence rather than a full-scale spike.
    sim.advance(4_000).expect("advance");
    let out = drain_summed(&mut sim, 4_000);
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));

    assert!(
        peak < 0.05,
        "reset produced a click: first samples peaked at {peak}, and a stopped \
         engine should be silent"
    );
}

/// The signal must never sit pinned against the clipping ceiling.
///
/// This is the defect behind the crack heard on starting: not a discontinuity,
/// but sustained saturation. Driven far enough past the knee, the soft clipper
/// stops being soft — `tanh` goes flat and consecutive samples come out at
/// exactly the ceiling. A flat-topped waveform is rich in high harmonics, so it
/// is heard as a buzz near the filter cutoff rather than as a louder engine.
///
/// Deliberately *not* asserted as a per-sample jump. The muffler passes content
/// to 6 kHz, and at a 40 kHz sample rate a legitimate 6 kHz component at full
/// amplitude moves most of the way from peak to trough between one sample and
/// the next, so a large single-sample step is ordinary signal.
///
/// The sequence covers the loudest thing the model does: cranking from cold,
/// running, coasting to rest, and starting again. Starting is louder than the
/// governed rev limit, so calibrating gain on steady running alone misses it.
#[test]
fn the_signal_never_sits_pinned_at_the_clipping_ceiling() {
    let config = config();
    let knee = config.config().audio.soft_clip_knee as f32;
    let mut sim = Simulation::new(config, ResetOptions::default()).expect("simulation builds");

    let mut total = 0usize;
    let mut pinned = 0usize;
    let mut peak = 0.0f32;
    // Measured on the summed frame. The knee bounds the *mix* — that is what the
    // saturation gain is derived from — so the sum is the quantity this
    // assertion is about, and a single path is always well inside it.
    let mut count = |sim: &mut Simulation, batches: usize| {
        for _ in 0..batches {
            sim.advance(4_000).expect("advance");
            for sample in drain_summed(sim, 4_000) {
                total += 1;
                peak = peak.max(sample.abs());
                if sample.abs() >= knee * 0.995 {
                    pinned += 1;
                }
            }
        }
    };

    sim.set_controls(controls(0.6, true, true))
        .expect("controls");
    count(&mut sim, 160);
    sim.set_controls(controls(0.0, false, false))
        .expect("controls");
    count(&mut sim, 400);
    sim.set_controls(controls(0.6, true, true))
        .expect("controls");
    count(&mut sim, 60);

    assert!(
        peak > 0.05,
        "the sequence should produce real output, got {peak}"
    );
    let fraction = pinned as f64 / total as f64;
    assert!(
        fraction < 0.005,
        "{:.2}% of samples sat at the clipping ceiling; the gain is driving the \
         soft clipper into hard saturation, which buzzes",
        fraction * 100.0
    );
}

/// An engine that has run and then stopped must fall genuinely silent.
///
/// The failure this guards is not loudness but *frequency*. Explicit integration
/// of the exhaust port overshoots equilibrium once the pressures are nearly
/// equal, flipping the flow every step: a two-sample limit cycle, which is a
/// tone at the Nyquist frequency. It is invisible in the pressure trace, sits at
/// a level far below the running engine, and is still plainly audible — because
/// once combustion stops it is the only thing playing, and the radiation
/// derivative amplifies with frequency.
///
/// So this asserts on high-frequency content specifically, not just on level. A
/// plain amplitude check passes at -63 dBFS while the output is 100% ultrasonic
/// hiss.
#[test]
fn a_stopped_engine_emits_no_high_frequency_hiss() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");

    // Run it hard, so the manifolds and cylinders are far from rest.
    sim.set_controls(controls(0.9, true, true))
        .expect("controls");
    for _ in 0..200 {
        sim.advance(4_000).expect("advance");
        discard_audio(&mut sim, 4_000);
    }

    // Cut fuelling and let it come to a complete stop.
    sim.set_controls(controls(0.0, false, false))
        .expect("controls");
    for _ in 0..600 {
        sim.advance(4_000).expect("advance");
        discard_audio(&mut sim, 4_000);
    }
    assert_eq!(sim.rpm(), 0.0, "the engine should have come to rest");

    let mut out: Vec<f32> = Vec::new();
    while out.len() < 16_384 {
        let batch = 4_000u32.min((16_384 - out.len()) as u32);
        sim.advance(batch).expect("advance");
        out.extend(drain_summed(&mut sim, 4_000));
    }

    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak < 1.0e-4,
        "a stopped engine should be silent, peaked at {peak}"
    );

    // Alternating samples are the signature of a Nyquist limit cycle, so
    // compare neighbouring samples directly: a real signal is correlated
    // between neighbours, a step-rate oscillation is anti-correlated.
    let alternation: f64 = out
        .windows(2)
        .map(|w| f64::from(w[1] - w[0]).abs())
        .sum::<f64>()
        / out.len() as f64;
    assert!(
        alternation < 1.0e-4,
        "output alternates by {alternation} per sample, which is a tone at the \
         step rate rather than engine sound"
    );
}

/// Idle must be audible, and the loud end must reach the ceiling without being
/// squared off against it.
///
/// **The criterion changed with the mechanism, and this records why.** It used to
/// require full load to sit at 99% of the knee or below, on the grounds that the
/// start transient is louder than full load and needed room underneath the
/// ceiling to arrive in. That was a true statement about a *static* gain feeding
/// a memoryless soft clipper: nothing else stopped the transient flat-topping, so
/// unused headroom was the only defence, and the cost was that every loud passage
/// was reshaped rather than turned down.
///
/// The limiter now follows the level, so the ceiling is reached by reducing gain
/// and the transient needs no room reserved for it. Full load reaching the knee
/// is therefore expected rather than a defect — and the question the old
/// threshold was really asking, *is the loud end squared off*, is not answered by
/// how close the peak is to the ceiling at all. It is answered by crest factor,
/// which is what this asserts instead: a peak sitting exactly on the knee with 13
/// dB of crest is a pulse train that has been turned down, and the same peak with
/// 3 dB of crest is a buzz. The old form could not tell those apart, and the
/// mechanism it was written for produced the second one.
#[test]
fn the_output_reaches_the_ceiling_without_being_squared_off() {
    let config = config();
    let knee = config.config().audio.soft_clip_knee as f32;

    let measure_at = |rpm: f64, pedal: f64| {
        let mut sim = Simulation::new(
            config.clone(),
            ResetOptions {
                seed: 0,
                initial_rpm: rpm,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
        )
        .expect("simulation builds");
        sim.set_controls(controls(pedal, false, true))
            .expect("controls");
        for _ in 0..1_200 {
            sim.advance(100).expect("advance");
            sim.pin_speed_rpm(rpm).expect("pin");
            discard_audio(&mut sim, 100);
        }
        let mut out: Vec<f32> = Vec::new();
        for _ in 0..400 {
            sim.advance(100).expect("advance");
            sim.pin_speed_rpm(rpm).expect("pin");
            out.extend(drain_summed(&mut sim, 100));
        }
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        (peak, crest_db(&out))
    };

    let (idle, idle_crest) = measure_at(600.0, 0.15);
    let (full, full_crest) = measure_at(1_400.0, 1.0);

    assert!(
        idle > 0.01,
        "idle at {idle} is too quiet to hear once a device volume is applied"
    );
    assert!(
        full <= knee * 1.000_01,
        "full load at {full} is past the {knee} ceiling the limiter is meant to \
         hold it to"
    );
    // The acceptance figure for a train of distinct combustion events. A steady
    // tone is 3 dB and the memoryless clipper this replaced pulled full load down
    // to 10.4 dB while it was pinning the peak just under the knee, so this is
    // the assertion that would have caught it.
    assert!(
        full_crest > 9.0,
        "full load peaks at {full} with only {full_crest:.1} dB of crest: the \
         loud end is being squared off against the ceiling rather than turned \
         down to it"
    );
    assert!(
        idle_crest > 9.0,
        "idle has only {idle_crest:.1} dB of crest, which is a hum rather than \
         an idling diesel"
    );
    assert!(full > idle * 2.0, "the range should have real dynamics");
}

#[test]
fn a_consumer_that_never_drains_loses_the_oldest_samples_and_says_so() {
    let mut engine = Engine::from_catalog(
        Catalog::builtin().expect("catalog"),
        ResetOptions::default(),
    )
    .expect("engine builds");
    engine
        .set_controls(controls(0.5, true, true))
        .expect("controls");

    // The buffer holds one full batch. Advancing several without draining must
    // drop, not grow, and must report the loss rather than hide it.
    for _ in 0..4 {
        engine.advance(20_000).expect("advance");
    }
    let sim = engine.simulation();
    assert_eq!(sim.audio_available(), 20_000, "capacity is the ceiling");
    assert!(
        sim.audio_dropped() > 0,
        "samples lost to a slow consumer must be counted"
    );
}

#[test]
fn the_level_meter_rises_once_the_engine_is_running() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");
    sim.advance(4_000).expect("advance");
    let silent = sim.audio_level_db();

    sim.set_controls(controls(0.6, true, true))
        .expect("controls");
    for _ in 0..200 {
        sim.advance(4_000).expect("advance");
        discard_audio(&mut sim, 4_000);
    }
    let running = sim.audio_level_db();

    assert!(
        running > silent,
        "a running engine must be louder than a stopped one: {running} dB against \
         {silent} dB"
    );
    assert!(running.is_finite() && running > 0.0);
}

// --- the structural radiation path ---------------------------------------
//
// The exhaust is not the only thing an engine radiates from. The premixed burn
// is a near-step pressure rise inside a stiff iron box, and the box rings: that
// is combustion noise, it dominates a heavy-duty diesel from roughly 800 Hz to
// 4 kHz, and it reaches the ear straight off the engine's skin without ever
// going near the exhaust. These tests exist to prove that path is driven by the
// combustion model rather than scheduled alongside it.

/// Fraction of a signal's energy surviving a two-pole high pass.
///
/// The `audio_probe` example uses a transform for this because it reports
/// absolute band shares. A test only needs to compare two signals, and a filter
/// answers that in one pass.
fn high_band_share(samples: &[f32], cutoff_hz: f64) -> f64 {
    let dt = 1.0 / 40_000.0;
    let rc = 1.0 / (std::f64::consts::TAU * cutoff_hz);
    let alpha = rc / (rc + dt);

    let (mut previous_input, mut stage_one, mut previous_stage_one, mut stage_two) =
        (0.0, 0.0, 0.0, 0.0);
    let (mut total, mut high) = (0.0, 0.0);
    for sample in samples {
        let x = f64::from(*sample);
        stage_one = alpha * (stage_one + x - previous_input);
        previous_input = x;
        stage_two = alpha * (stage_two + stage_one - previous_stage_one);
        previous_stage_one = stage_one;
        total += x * x;
        high += stage_two * stage_two;
    }
    if total <= 0.0 {
        0.0
    } else {
        high / total
    }
}

/// Absolute energy above a cutoff, rather than its share of the total.
///
/// The distinction matters once there are two radiating paths and a duct: a
/// share moves when either path changes level, so a claim about how much energy
/// is up there has to be measured as energy.
fn high_band_energy(samples: &[f32], cutoff_hz: f64) -> f64 {
    high_band_share(samples, cutoff_hz)
        * samples
            .iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum::<f64>()
}

/// Absolute energy in a narrow band around one frequency.
///
/// A one-pole cascade cannot answer a question about *where* a resonance sits.
/// Its skirts are 6 dB per octave per pole, so a loud band on the wrong side of
/// the corner leaks through in quantity — and when the thing under test moves,
/// that leak moves with it in the opposite direction and cancels the effect
/// being measured. A high pass at 5 kHz reads a bank at 3.8 kHz as almost the
/// same as a bank at 5.7 kHz, which is the reverse of the truth.
///
/// So this is a resonator, the same two-pole two-zero form the solver's own modal
/// bank uses, run as a bandpass. Zeros at DC and Nyquist, poles at the frequency
/// of interest. It is selective enough to say which side of it a mode is on.
fn tone_energy(samples: &[f32], centre_hz: f64, q: f64) -> f64 {
    let theta = std::f64::consts::TAU * centre_hz / 40_000.0;
    let r = 1.0 - theta / (2.0 * q);
    let a1 = 2.0 * r * theta.cos();
    let a2 = -(r * r);

    let (mut y1, mut y2, mut x1, mut x2) = (0.0, 0.0, 0.0, 0.0);
    let mut energy = 0.0;
    for sample in samples {
        let x = f64::from(*sample);
        let y = a1 * y1 + a2 * y2 + (x - x2);
        x2 = x1;
        x1 = x;
        y2 = y1;
        y1 = y;
        energy += y * y;
    }
    energy
}

/// The shipped configuration with its `audio` section altered.
fn config_with_audio(mutate: impl FnOnce(&mut serde_json::Value)) -> ValidatedConfig {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    mutate(&mut document["audio"]);
    EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect("mutated config validates")
}

#[test]
fn the_structural_path_is_what_puts_energy_above_the_firing_harmonics() {
    // Silence the modal bank and the top end goes with it. This is the whole
    // claim of the structural path stated as a difference rather than as an
    // absolute number, so it does not depend on how the bank happens to be
    // calibrated today.
    let with = samples_at(config(), 1_000.0, 0.4, 293.15, 40_000);
    let without = samples_at(
        config_with_audio(|audio| audio["structural_gain"] = serde_json::json!(0.0)),
        1_000.0,
        0.4,
        293.15,
        40_000,
    );

    // Energy rather than share. A share is a ratio against the total, and the
    // total collapses when the modal bank is silenced, so the exhaust path's
    // *share* of its own much smaller output can look large while the energy up
    // there is negligible. The claim being made is about how much is up there.
    //
    // Measured above 1 kHz rather than above 500 Hz. This boundary was chosen
    // when the bank's lowest mode was at 480 Hz, so "above 500 Hz" meant "where
    // only the block is". The bank now starts at 210 Hz, to give the engine the
    // size a 12.8 litre iron structure ought to have, and the exhaust path
    // legitimately carries content through the same region — so 500 Hz stopped
    // separating the two paths and started straddling both. A kilohertz is
    // where the block is still the only thing radiating.
    let with_energy = high_band_energy(&with, 1_000.0);
    let without_energy = high_band_energy(&without, 1_000.0);
    assert!(
        with_energy > without_energy * 4.0,
        "the modal bank should dominate above 1 kHz: {with_energy:.4e} with it \
         against {without_energy:.4e} without, which is not a large enough difference \
         to be the mechanism this path claims to be"
    );
}

#[test]
fn the_modal_bank_only_rings_where_it_is_told_to() {
    // Move every mode up by half and the energy must follow it up. If it does
    // not, the top end is coming from something other than the configured bank —
    // clipping, say, or the exhaust path's own edges — and the configuration is
    // decorative.
    //
    // *Up*, not down, which is the direction that makes this test honest. The
    // drive falls steeply with frequency, so a bank moved down is driven harder
    // and gets louder; a bank moved up is driven more weakly and has to prove
    // itself against a headwind. Testing downward lets the drive supply the
    // difference the bank is supposed to.
    let shipped_bank = samples_at(config(), 1_000.0, 0.4, 293.15, 40_000);
    let raised_bank = samples_at(
        config_with_audio(|audio| {
            for mode in audio["structural_modes"]
                .as_array_mut()
                .expect("modes are an array")
            {
                let hz = mode["frequency_hz"].as_f64().expect("frequency");
                mode["frequency_hz"] = serde_json::json!(hz * 1.5);
            }
        }),
        1_000.0,
        0.4,
        293.15,
        40_000,
    );

    // Measured at 5.7 kHz, where the raised bank's top mode lands and the shipped
    // one has nothing, with a resonator rather than a high pass.
    //
    // This test used to compare the *share* above 2 kHz after halving the bank,
    // and both halves of that were wrong once the exhaust path was radiating
    // properly. A share is a ratio against a total the exhaust now dominates, so
    // moving the bank moved the denominator with the numerator and the reading
    // barely twitched — the same share-versus-energy distinction
    // `the_structural_path_is_what_puts_energy_above_the_firing_harmonics` draws
    // above. And a one-pole corner is far too gentle to say which side of it a
    // mode is on: with a high pass at 5 kHz, a bank at 3.8 kHz and a bank at
    // 5.7 kHz read within a factor of two of each other, because the 2.5-5 kHz
    // band leaks through in quantity and moves the opposite way. A resonator
    // sees the 31x difference that is actually there.
    let shipped = tone_energy(&shipped_bank, 5_700.0, 8.0);
    let raised = tone_energy(&raised_bank, 5_700.0, 8.0);
    assert!(
        raised > shipped * 4.0,
        "moving every mode up by half should put the top one at 5.7 kHz, but the \
         energy there went {shipped:.4e} -> {raised:.4e}"
    );
}

#[test]
fn the_clatter_tracks_the_premixed_burn_and_not_the_fuel() {
    // Ignition delay lengthens when the cylinder is lightly loaded, a longer
    // delay means a larger premixed fraction, and a larger premixed fraction
    // means a sharper pressure rise. So a lightly loaded engine must be rattly
    // out of proportion to how little fuel it is burning - and it must be so
    // because the combustion model says so, since there is no load term anywhere
    // in the audio path to arrange it.
    //
    // Stated per milligram of fuel, deliberately. In absolute terms a fully
    // loaded engine clatters *more*, which is both what this model produces and
    // what a real one does: the premixed fraction collapses under load but the
    // premixed *mass* barely changes, and it burns into a much denser charge. The
    // plan's original phrasing - upper-band energy at light load exceeding that
    // at full load - is not true here and should not be.
    let speed = 1_200.0;

    // The exhaust path is silenced for this measurement and only this one. It has
    // a strong load dependence of its own, more fuel being more gas through the
    // port, which has nothing to do with combustion noise. Nothing is added to
    // the structural path to make this pass.
    let structural_only = config_with_audio(|audio| audio["exhaust_gain"] = serde_json::json!(0.0));

    let measure = |pedal: f64| {
        let mut sim = Simulation::new(
            structural_only.clone(),
            ResetOptions {
                seed: 0,
                initial_rpm: speed,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
        )
        .expect("simulation builds");
        sim.set_controls(controls(pedal, false, true))
            .expect("controls accepted");
        for _ in 0..1_200 {
            sim.advance(PIN_CHUNK).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            discard_audio(&mut sim, PIN_CHUNK as usize);
        }
        let snapshot = sim.snapshot();
        let mut out: Vec<f32> = Vec::new();
        while out.len() < 40_000 {
            let batch = PIN_CHUNK.min((40_000 - out.len()) as u32);
            sim.advance(batch).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            out.extend(drain_summed(&mut sim, PIN_CHUNK as usize));
        }
        (
            snapshot.premixed_fraction,
            snapshot.fuel_per_cycle_mg,
            high_band_energy(&out, 1_500.0),
        )
    };

    let (light_premixed, light_fuel, light_energy) = measure(0.08);
    let (heavy_premixed, heavy_fuel, heavy_energy) = measure(1.0);

    // The mechanism itself, named rather than inferred.
    assert!(
        light_premixed > heavy_premixed * 3.0,
        "the premixed fraction should collapse under load: {light_premixed:.3} light \
         against {heavy_premixed:.3} heavy"
    );
    assert!(
        light_fuel < heavy_fuel * 0.25,
        "the light case should be burning far less fuel: {light_fuel:.1} mg against \
         {heavy_fuel:.1} mg"
    );

    // And its consequence: clatter per milligram, not clatter per se.
    let light_per_mg = light_energy / light_fuel;
    let heavy_per_mg = heavy_energy / heavy_fuel;
    assert!(
        light_per_mg > heavy_per_mg * 2.0,
        "a lightly loaded engine should rattle out of proportion to its fuel: \
         {light_per_mg:.4} per mg light against {heavy_per_mg:.4} per mg heavy"
    );
}

#[test]
fn the_engine_brake_barks_harder_than_the_same_engine_coasting() {
    // The release lobe cracks a valve at the top of compression, which is the
    // fastest pressure event the model produces anywhere. It drives the modal
    // bank harder than anything else the engine does, and it does so without a
    // line of brake-specific code in the audio path.
    //
    // The comparison that means something is against the *same* engine coasting:
    // no fuel either way, brake the only difference. Comparing a braking engine
    // to a fuelled one instead would be comparing two different amounts of
    // energy in the system and would say nothing about the release lobe — a
    // fuelled engine is burning, and burning is loud.
    let speed = 1_600.0;

    let run = |stage: u8| {
        let mut sim = Simulation::new(
            config(),
            ResetOptions {
                seed: 0,
                initial_rpm: speed,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
        )
        .expect("simulation builds");
        sim.set_controls(Controls {
            pedal: 0.0,
            ignition: false,
            brake_stage: stage,
            ..Controls::default()
        })
        .expect("controls accepted");

        for _ in 0..1_200 {
            sim.advance(PIN_CHUNK).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            discard_audio(&mut sim, PIN_CHUNK as usize);
        }
        let mut out: Vec<f32> = Vec::new();
        while out.len() < 40_000 {
            let batch = PIN_CHUNK.min((40_000 - out.len()) as u32);
            sim.advance(batch).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            out.extend(drain_summed(&mut sim, PIN_CHUNK as usize));
        }
        out
    };

    let coasting = run(0);
    let braking = run(3);

    // Absolute upper-band energy, not a share: the brake makes the engine
    // louder up there, and a share would hide that behind the low end rising
    // with it.
    let energy = |samples: &[f32]| {
        high_band_share(samples, 1_500.0)
            * samples
                .iter()
                .map(|s| f64::from(*s) * f64::from(*s))
                .sum::<f64>()
    };
    let braking_energy = energy(&braking);
    let coasting_energy = energy(&coasting);
    assert!(
        braking_energy > coasting_energy * 4.0,
        "the release lobe should dominate the upper band: {braking_energy:.4e} \
         braking against {coasting_energy:.4e} coasting"
    );
}

#[test]
fn the_four_cylinder_fixture_drives_its_own_modal_bank() {
    // The fixture carries a different bank — three modes rather than four, and
    // higher, because a smaller block rings higher. If the solver knew anything
    // about the OM 471's bank this would not hold.
    let fixture = fixture_config();
    let modes = &fixture.config().audio.structural_modes;
    assert_eq!(
        modes.len(),
        3,
        "the fixture defines its own number of modes"
    );

    let with = samples_at(fixture.clone(), 1_000.0, 0.4, 293.15, 40_000);
    let share = high_band_share(&with, 500.0);
    assert!(
        share > 0.05,
        "the fixture's own bank must actually ring, got a share of {share:.4}"
    );
    for sample in &with {
        assert!(
            sample.is_finite() && sample.abs() <= 1.0,
            "fixture sample out of range: {sample}"
        );
    }
}

#[test]
fn an_unstable_modal_bank_is_rejected_rather_than_run() {
    // A two-pole resonator is stable only while `r = 1 - pi f dt / Q` stays
    // inside (0, 1). A high frequency with a low Q pushes it out, and an
    // unstable pole in the hot loop grows without bound where nothing can
    // recover it. It has to be refused at the door.
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["audio"]["structural_modes"] = serde_json::json!([
        { "frequency_hz": 18_000.0, "q": 0.5, "gain": 1.0 }
    ]);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("an unstable mode must be rejected");
    assert!(
        error.message.contains("stable resonator"),
        "the rejection should name the stability condition, got: {}",
        error.message
    );
}

#[test]
fn a_modal_bank_above_nyquist_is_rejected() {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["audio"]["structural_modes"] = serde_json::json!([
        { "frequency_hz": 25_000.0, "q": 40.0, "gain": 1.0 }
    ]);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("a mode above Nyquist must be rejected");
    assert!(
        error.message.contains("Nyquist"),
        "the rejection should name Nyquist, got: {}",
        error.message
    );
}

// --- seeded cylinder-to-cylinder variation --------------------------------
//
// Six bit-identical cylinders sum to a mathematically pure harmonic comb with
// no jitter and no amplitude scatter, and the ear is extremely good at hearing
// that: it hears it as synthesised. Real engines have injector delivery scatter
// and port-to-port flow scatter, and nothing in a real six is identical to
// anything else in it. The trims are drawn once at reset from the reset seed,
// which is what makes this compatible with determinism rather than a threat to
// it.

/// The shipped configuration with both build-scatter spreads set to zero.
fn perfect_engine() -> ValidatedConfig {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["valvetrain"]["exhaust_area_spread"] = serde_json::json!(0.0);
    document["injection"]["cylinder_delivery_spread"] = serde_json::json!(0.0);
    // The per-cycle spread too, or the seed still has something to vary and
    // "perfect" would mean "perfectly built and still running unevenly".
    document["injection"]["cycle_delivery_spread"] = serde_json::json!(0.0);
    EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect("mutated config validates")
}

fn samples_with_seed(config: ValidatedConfig, seed: u64, rpm: f64, steps: usize) -> Vec<f32> {
    let mut sim = Simulation::new(
        config,
        ResetOptions {
            seed,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )
    .expect("simulation builds");
    sim.set_controls(controls(0.4, false, true))
        .expect("controls accepted");

    for _ in 0..1_200 {
        sim.advance(PIN_CHUNK).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        discard_audio(&mut sim, PIN_CHUNK as usize);
    }
    let mut out: Vec<f32> = Vec::new();
    while out.len() < steps {
        let batch = PIN_CHUNK.min((steps - out.len()) as u32);
        sim.advance(batch).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        out.extend(drain_summed(&mut sim, PIN_CHUNK as usize));
    }
    out
}

#[test]
fn a_different_seed_builds_a_different_engine() {
    let a = samples_with_seed(config(), 1, 1_000.0, 20_000);
    let b = samples_with_seed(config(), 2, 1_000.0, 20_000);
    assert_ne!(
        a, b,
        "two seeds must build two engines; if the trims were not applied these \
         would be identical"
    );

    // Different, but not wildly so: this is a build tolerance, not a different
    // engine family. Compare energies rather than samples.
    let energy = |s: &[f32]| s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>();
    let (ea, eb) = (energy(&a), energy(&b));
    let ratio = ea.max(eb) / ea.min(eb);
    assert!(
        ratio < 1.5,
        "a 2% build tolerance should not change the output energy by {ratio:.2}x"
    );
}

#[test]
fn the_same_seed_builds_the_same_engine_bit_for_bit() {
    // The whole reason the trims are drawn at reset rather than per step. This
    // is the guarantee that keeps batch invariance and repeatability intact.
    let a = samples_with_seed(config(), 7, 1_000.0, 20_000);
    let b = samples_with_seed(config(), 7, 1_000.0, 20_000);
    assert_eq!(a, b, "one seed must give one engine, bit for bit");
}

#[test]
fn zeroing_the_spreads_restores_six_identical_cylinders() {
    // Proves the variation comes from the configured spreads and not from
    // somewhere incidental: with both set to zero the seed stops mattering.
    let a = samples_with_seed(perfect_engine(), 1, 1_000.0, 20_000);
    let b = samples_with_seed(perfect_engine(), 2, 1_000.0, 20_000);
    assert_eq!(
        a, b,
        "with no build scatter the seed must have nothing left to vary"
    );

    // And a perfect engine really is a different signal from a scattered one.
    let scattered = samples_with_seed(config(), 1, 1_000.0, 20_000);
    assert_ne!(a, scattered, "the spreads must actually reach the solver");
}

#[test]
fn the_trim_breaks_the_harmonic_comb_without_moving_the_note() {
    // Scatter is supposed to disturb the *evenness* of the pulse train, not its
    // period. If it moved the firing frequency it would be a defect rather than
    // realism.
    let cylinders = config().config().geometry.cylinders;
    for rpm in [800.0, 1_400.0] {
        let scattered = samples_with_seed(config(), 3, rpm, 40_000);
        let measured = fundamental_hz(&scattered, 40_000.0, 20.0, 400.0);
        let expected = firing_hz(rpm, cylinders);
        assert!(
            (measured - expected).abs() < expected * 0.05,
            "at {rpm:.0} rpm the note should stay at {expected:.1} Hz, got {measured:.1} Hz"
        );
    }
}

#[test]
fn an_implausibly_wide_build_tolerance_is_rejected() {
    // The cap is what stops a realism knob from becoming a calibration change.
    for (section, field) in [
        ("valvetrain", "exhaust_area_spread"),
        ("injection", "cylinder_delivery_spread"),
    ] {
        let mut document: serde_json::Value =
            serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
        document[section][field] = serde_json::json!(0.5);
        let error = EngineConfig::from_json(&document.to_string())
            .expect("mutated config parses")
            .validate()
            .expect_err("a 50% build tolerance must be rejected");
        assert!(
            error.message.contains(field),
            "the rejection should name `{section}.{field}`, got: {}",
            error.message
        );
    }

    // And a negative spread is meaningless rather than merely too large.
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["valvetrain"]["exhaust_area_spread"] = serde_json::json!(-0.01);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("a negative build tolerance must be rejected");
    assert!(
        error.message.contains("exhaust_area_spread"),
        "the rejection should name the field, got: {}",
        error.message
    );
}

// --- the exhaust system: duct, turbine, box, runners ----------------------

/// The shipped configuration with its `exhaust_system` section altered, and the
/// structural path silenced.
///
/// These are tests of the exhaust path, and the modal bank is loud: at full load
/// it carries most of the output, so a change to the duct or the turbine is a
/// few percent of the total and any assertion about it would really be an
/// assertion about the bank. Silencing the bank measures the thing being named.
fn config_with_exhaust(mutate: impl FnOnce(&mut serde_json::Value)) -> ValidatedConfig {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["audio"]["structural_gain"] = serde_json::json!(0.0);
    mutate(&mut document["exhaust_system"]);
    EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect("mutated config validates")
}

#[test]
fn changing_the_firing_order_changes_the_waveform() {
    // The assertion this whole step exists to make possible.
    //
    // `geometry.firing_order` has always been in the configuration, always been
    // validated as a permutation, and always derived the cylinder phase offsets
    // — and until the runners landed it could not change one sample of output.
    // Six evenly spaced cylinders discharging into a single lumped node sum to
    // the same signal whichever cylinder is assigned to which slot. Runner
    // lengths differ by cylinder position, so the order in which the cylinders
    // fire is now the order in which different delays are exercised.
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    assert_eq!(
        document["geometry"]["firing_order"],
        serde_json::json!([1, 5, 3, 6, 2, 4]),
        "this test is written against the shipped firing order"
    );
    document["geometry"]["firing_order"] = serde_json::json!([1, 2, 3, 4, 5, 6]);
    let reordered = EngineConfig::from_json(&document.to_string())
        .expect("reordered config parses")
        .validate()
        .expect("reordered config validates");

    let shipped = samples_at(config(), 1_200.0, 0.5, 293.15, 40_000);
    let sequential = samples_at(reordered, 1_200.0, 0.5, 293.15, 40_000);

    assert_ne!(
        shipped, sequential,
        "1-5-3-6-2-4 and 1-2-3-4-5-6 must not produce the same waveform; if they \
         do, the firing order is decorative again"
    );

    // Different, and audibly so rather than in the last bit.
    let difference: f64 = shipped
        .iter()
        .zip(&sequential)
        .map(|(a, b)| {
            let d = f64::from(*a) - f64::from(*b);
            d * d
        })
        .sum();
    let energy: f64 = shipped.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    assert!(
        difference > energy * 0.01,
        "the two firing orders differ by only {:.4}% of the signal energy, which is \
         not a difference anyone could hear",
        100.0 * difference / energy
    );
}

#[test]
fn collapsing_the_runner_span_makes_the_firing_order_inert_again() {
    // The converse, and the proof that the runners are the mechanism rather than
    // something incidental: with every runner nearly the same length, permuting
    // the firing order should stop mattering nearly as much.
    // The build scatter has to be zeroed as well, and finding that out was the
    // point of writing this test. P5's per-cylinder trims are indexed by
    // cylinder, so permuting the firing order already changed which trim fired
    // when — meaning the firing order became audible one step earlier than the
    // plan expected, by a mechanism the plan did not intend. With the trims off,
    // the runners are the only thing left that can tell one cylinder from
    // another.
    let narrow = |order: serde_json::Value| {
        let mut document: serde_json::Value =
            serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
        document["geometry"]["firing_order"] = order;
        document["valvetrain"]["exhaust_area_spread"] = serde_json::json!(0.0);
        document["injection"]["cylinder_delivery_spread"] = serde_json::json!(0.0);
        document["exhaust_system"]["runner_length_min_m"] = serde_json::json!(0.30);
        document["exhaust_system"]["runner_length_max_m"] = serde_json::json!(0.3001);
        EngineConfig::from_json(&document.to_string())
            .expect("config parses")
            .validate()
            .expect("config validates")
    };

    let a = samples_at(
        narrow(serde_json::json!([1, 5, 3, 6, 2, 4])),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );
    let b = samples_at(
        narrow(serde_json::json!([1, 2, 3, 4, 5, 6])),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );

    let difference: f64 = a
        .iter()
        .zip(&b)
        .map(|(x, y)| {
            let d = f64::from(*x) - f64::from(*y);
            d * d
        })
        .sum();
    let energy: f64 = a.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    assert!(
        difference < energy * 0.01,
        "with all runners the same length the firing order should barely matter, \
         but the two differ by {:.2}% of the signal energy",
        100.0 * difference / energy
    );
}

#[test]
fn the_pipe_resonance_moves_with_exhaust_temperature() {
    // `c = sqrt(gamma R T)`, so a hot duct rings higher than a cold one. Nothing
    // schedules this: the duct reads the exhaust temperature the solver
    // integrates, and the resonance follows.
    //
    // Measured through the duct alone rather than through a running engine,
    // because an engine hot enough to shift the resonance is also an engine
    // making a different sound for a dozen other reasons.
    use sim_core::sim::exhaust::speed_of_sound_m_per_s;
    let gas = config().config().gas;
    let cold = speed_of_sound_m_per_s(&gas, 450.0);
    let hot = speed_of_sound_m_per_s(&gas, 900.0);
    assert!(
        hot > cold * 1.3,
        "doubling the exhaust temperature should raise the speed of sound by about \
         40%: {hot:.0} m/s against {cold:.0} m/s"
    );
}

#[test]
fn the_tailpipe_length_reaches_the_output() {
    // The duct has a pitch of its own, independent of firing frequency, and
    // length is what sets it. If length did not reach the output the pipe would
    // be a tone control again.
    //
    // Where the resonance *lands* is asserted in `exhaust.rs`, against the round
    // trip, because that is where it can be measured without a transform. Note
    // that the obvious integration-level test - a longer pipe having a larger
    // share of low-frequency energy - is false here, and instructively so: the
    // radiation loss is taken once per round trip, so a short pipe reflects far
    // more often per second and accumulates far more of it. A long pipe rings
    // lower *and* brighter. That is a property of this loss model rather than a
    // robust fact about pipes, so it is recorded here and not asserted.
    let short = samples_at(
        config_with_exhaust(|ex| ex["tailpipe_length_m"] = serde_json::json!(1.5)),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );
    let long = samples_at(
        config_with_exhaust(|ex| ex["tailpipe_length_m"] = serde_json::json!(8.0)),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );
    assert_ne!(short, long, "the duct length must reach the output");

    let difference: f64 = short
        .iter()
        .zip(&long)
        .map(|(a, b)| {
            let d = f64::from(*a) - f64::from(*b);
            d * d
        })
        .sum();
    let energy: f64 = short.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    assert!(
        difference > energy * 0.1,
        "1.5 m and 8 m of pipe should sound plainly different, but differ by only \
         {:.2}% of the signal energy",
        100.0 * difference / energy
    );
}

#[test]
fn the_turbine_insertion_loss_reaches_the_output() {
    // The term that makes the engine read as turbocharged rather than open-piped.
    let muted = samples_at(
        config_with_exhaust(|ex| ex["turbine_insertion_loss_db"] = serde_json::json!(30.0)),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );
    let open = samples_at(
        config_with_exhaust(|ex| ex["turbine_insertion_loss_db"] = serde_json::json!(0.0)),
        1_200.0,
        0.5,
        293.15,
        40_000,
    );
    assert!(
        high_band_energy(&open, 500.0) > high_band_energy(&muted, 500.0) * 2.0,
        "30 dB of turbine loss should plainly quieten the top of the exhaust note: \
         {:.4e} open against {:.4e} muted",
        high_band_energy(&open, 500.0),
        high_band_energy(&muted, 500.0)
    );
}

#[test]
fn an_unstable_duct_is_rejected_rather_than_run() {
    // The waveguide is a feedback loop and this is the coefficient that bounds
    // its gain. A magnitude of one or more grows without bound in the hot loop,
    // where nothing can recover it.
    for bad in [-1.0, -1.5, 0.8, 0.0] {
        let mut document: serde_json::Value =
            serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
        document["exhaust_system"]["open_end_reflection"] = serde_json::json!(bad);
        let error = EngineConfig::from_json(&document.to_string())
            .expect("mutated config parses")
            .validate()
            .expect_err("an unstable or wrong-signed reflection must be rejected");
        assert!(
            error.message.contains("open_end_reflection"),
            "the rejection should name the coefficient, got: {}",
            error.message
        );
    }
}

#[test]
fn a_manifold_with_no_runner_spread_is_rejected() {
    // A span of zero is a manifold that cannot make the firing order audible,
    // which is a configuration mistake rather than a valid choice.
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["exhaust_system"]["runner_length_max_m"] = serde_json::json!(0.10);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("a zero runner span must be rejected");
    assert!(
        error.message.contains("runner_length_max_m"),
        "the rejection should name the field, got: {}",
        error.message
    );
}

// --- what a heavy truck sounds like -----------------------------------------
//
// The tests above prove the audio is finite, bounded, deterministic and pitched
// at the firing frequency. A signal can satisfy every one of them and still
// sound like a motorbike, and for a while this one did: the loudest octave at
// cruise was 315-630 Hz, the clatter path ran 5 dB in front of the exhaust, and
// only a fifth of the energy sat in the firing orders. Band shares did not catch
// it, because a band share cannot tell a pulse train from a tone.
//
// These are the assertions that would have. They are about *character* rather
// than about level or bandwidth.

/// Power at one frequency, by direct correlation against a windowed sinusoid.
///
/// A resonator cannot answer this: its skirts are 6 dB per octave and the
/// firing orders here are 30 Hz apart, so a neighbouring order leaks into any
/// filter narrow enough to be interesting. Correlating against the exact
/// frequency does not have skirts at all.
fn line_power(samples: &[f32], hz: f64) -> f64 {
    let n = samples.len();
    let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
    for (i, sample) in samples.iter().enumerate() {
        let w = 0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / n as f64).cos());
        let phase = std::f64::consts::TAU * hz * i as f64 / 40_000.0;
        let v = f64::from(*sample) * w;
        re += v * phase.cos();
        im += v * phase.sin();
        norm += w * w;
    }
    (re * re + im * im) / norm.max(1.0e-30)
}

#[test]
fn the_energy_sits_in_the_firing_orders_and_not_between_them() {
    // What a diesel is, spectrally: a comb at the firing frequency. What it is
    // not: a resonance being rung, which puts its energy wherever the resonance
    // happens to be and pays no attention to engine speed.
    //
    // Asserted as on-order against off-order rather than as a share of the
    // total, so it needs no normalisation and no window-bandwidth bookkeeping.
    // The half-order points are where a comb has nothing and a rung resonance
    // has just as much as anywhere else.
    let rpm = 1_400.0;
    let cylinders = config().config().geometry.cylinders;
    let f0 = firing_hz(rpm, cylinders);
    let samples = samples_at(config(), rpm, 1.0, 293.15, 32_768);

    let on: f64 = (1..=4).map(|n| line_power(&samples, f0 * n as f64)).sum();
    let off: f64 = (1..=4)
        .map(|n| line_power(&samples, f0 * (n as f64 + 0.5)))
        .sum();

    assert!(
        on > off * 20.0,
        "the firing orders should tower over the gaps between them: {on:.3e} on the \
         orders against {off:.3e} halfway between. A signal that scores near even here \
         is a resonance being rung rather than an engine firing"
    );
}

#[test]
fn the_exhaust_path_leads_the_clatter_path() {
    // The balance that decides whether this reads as a truck or as a motorbike,
    // and the single number that was wrong. The exhaust carries the firing
    // orders; the block carries the clatter on top of them. A block sitting in
    // front of the exhaust is a small engine however the band shares come out —
    // and for a while this one led by 5 dB in the wrong direction.
    //
    // Six decibels rather than "louder": a margin small enough to be an accident
    // is not a balance, and the clatter still has to be plainly audible, so this
    // is a floor and not a target.
    //
    // One run per operating point rather than two. The paths now cross the
    // boundary separately, so a path is a slice of the output instead of a
    // configuration with the other gains zeroed and the whole simulation run
    // again — and it is the *same* run, so nothing can drift between them.
    for (rpm, pedal) in [(600.0, 0.15), (1_200.0, 0.60), (1_400.0, 1.0)] {
        let frames = frames_at(config(), rpm, pedal, 293.15, 32_768);
        let exhaust = rms_of(&path_of(&frames, EXHAUST));
        let block = rms_of(&path_of(&frames, BLOCK));
        let lead_db = 20.0 * (exhaust / block.max(1.0e-30)).log10();
        assert!(
            lead_db > 6.0,
            "at {rpm:.0} rpm and {pedal:.2} pedal the exhaust leads the block by only \
             {lead_db:.1} dB; below 6 dB the clatter starts carrying the engine and a \
             12.8 litre six stops sounding like one"
        );
    }
}

#[test]
fn the_output_is_a_pulse_train_and_not_a_tone() {
    // The measurement band shares are blind to. A sine and a train of distinct
    // combustion events can hold identical energy in every band; a sine has a
    // crest factor of 3.0 dB and a pulse train is well into double figures.
    //
    // This also catches the soft clipper being driven into: saturation flattens
    // the peaks and the crest factor collapses long before the output measures
    // as distorted. During calibration this read 3.5 dB with the gains too high,
    // which is a signal that has been squared off into a buzz.
    for (rpm, pedal) in [(600.0, 0.15), (1_200.0, 0.60), (1_400.0, 1.0)] {
        let samples = samples_at(config(), rpm, pedal, 293.15, 32_768);
        let crest = crest_db(&samples);
        assert!(
            crest > 9.0,
            "at {rpm:.0} rpm and {pedal:.2} pedal the crest factor is {crest:.1} dB; \
             a diesel is a series of distinct events, and a figure approaching a sine's \
             3 dB means the pulses have been flattened into a tone"
        );
    }
}

#[test]
fn the_body_path_is_what_supplies_the_low_orders() {
    // The third path's whole claim, stated as a property of the path rather than
    // as a level: torque-driven radiation belongs below a few hundred hertz.
    // Both other gains are silenced, so what is measured is this path alone.
    let frames = frames_at(config(), 1_400.0, 1.0, 293.15, 32_768);
    let samples = path_of(&frames, BODY);
    let f0 = firing_hz(1_400.0, config().config().geometry.cylinders);

    let low: f64 = (1..=3).map(|n| line_power(&samples, f0 * n as f64)).sum();
    let high: f64 = [1_000.0, 2_000.0, 3_000.0]
        .iter()
        .map(|hz| line_power(&samples, *hz))
        .sum();

    assert!(
        low > high * 100.0,
        "the body path must live in the low orders: {low:.3e} in the first three \
         against {high:.3e} in the kilohertz. A body path with content up there is \
         not modelling a frame"
    );
}

#[test]
fn silencing_the_body_path_changes_the_output() {
    // Guards against the path being wired up but never reaching the sample — the
    // failure mode where every other test above still passes and the parameter
    // is decoration. Compare against the shipped configuration rather than
    // against a threshold, so it stays true whatever the gain is calibrated to.
    let with = samples_at(config(), 1_400.0, 1.0, 293.15, 8_000);
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["audio"]["body_gain"] = serde_json::json!(0.0);
    let without = samples_at(
        EngineConfig::from_json(&document.to_string())
            .expect("mutated config parses")
            .validate()
            .expect("mutated config validates"),
        1_400.0,
        1.0,
        293.15,
        8_000,
    );
    assert_ne!(with, without, "body_gain must actually reach the solver");
}

/// Coefficient of variation of a cylinder's pulse height against its own history.
///
/// Chops the trace into firing windows, then walks every `cylinders`-th window
/// so each series is one cylinder meeting itself one engine cycle later. Build
/// scatter makes cylinders differ from *each other* and would inflate a figure
/// taken across all of them; this asks the different question - does a cylinder
/// differ from its own last cycle - which is the one the per-cycle spread is
/// about.
fn per_cylinder_pulse_variation(samples: &[f32], rpm: f64, cylinders: usize) -> f64 {
    let window = (40_000.0 / firing_hz(rpm, cylinders)).round() as usize;
    let peaks: Vec<f64> = samples
        .chunks(window)
        .filter(|chunk| chunk.len() == window)
        .map(|chunk| f64::from(chunk.iter().fold(0.0f32, |m, x| m.max(x.abs()))))
        .collect();

    let mut total = 0.0;
    for offset in 0..cylinders {
        let series: Vec<f64> = peaks
            .iter()
            .skip(offset)
            .step_by(cylinders)
            .copied()
            .collect();
        let mean = series.iter().sum::<f64>() / series.len().max(1) as f64;
        let variance =
            series.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / series.len().max(1) as f64;
        total += variance.sqrt() / mean.max(1.0e-30);
    }
    total / cylinders as f64
}

#[test]
fn the_per_cycle_spread_makes_a_cylinder_differ_from_its_own_last_cycle() {
    // The defect this parameter exists for: with build scatter alone the six
    // cylinders differ from each other but every cylinder is a bit-identical
    // copy of its own previous cycle, so the engine is a six-event loop played
    // on repeat. That is audible, and what it is audible as is a synthesiser.
    //
    // Measured at idle, which is not a convenience. At full pedal the smoke
    // limit sets the fuel - the commanded quantity is clamped to the air the
    // cylinder trapped - so the trim is clipped away and this spread does
    // nothing at all, measuring 1.00x there. That is the right behaviour rather
    // than a gap: published cycle-to-cycle variation for heavy-duty diesels runs
    // a few percent at light load and under one percent at high load, and the
    // model reproduces the load dependence without being told to, because the
    // limiter that erases the variation is already there for another reason.
    let cylinders = config().config().geometry.cylinders;
    let rpm = 600.0;

    let variation = |spread: f64| {
        let mut document: serde_json::Value =
            serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
        document["injection"]["cycle_delivery_spread"] = serde_json::json!(spread);
        let cfg = EngineConfig::from_json(&document.to_string())
            .expect("mutated config parses")
            .validate()
            .expect("mutated config validates");
        per_cylinder_pulse_variation(&samples_at(cfg, rpm, 0.15, 293.15, 120_000), rpm, cylinders)
    };

    let identical = variation(0.0);
    let varying = variation(0.02);
    assert!(
        varying > identical * 1.3,
        "the per-cycle spread must make a cylinder differ from its own last cycle: \
         pulse-height variation {varying:.5} with it against {identical:.5} without. \
         An engine whose every cycle is a copy of its last is a loop, not a machine"
    );
}

#[test]
fn a_negative_per_cycle_spread_is_rejected() {
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["injection"]["cycle_delivery_spread"] = serde_json::json!(-0.01);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("a negative spread must be rejected");
    assert!(
        error.message.contains("cycle_delivery_spread"),
        "the rejection should name the field, got: {}",
        error.message
    );
}

#[test]
fn an_unstable_body_mode_is_rejected() {
    // The body bank runs Q in the single figures, which is far closer to the
    // stability boundary than the structural bank's tens. A high frequency
    // paired with a low Q drives the pole radius negative and the mode grows
    // without bound in the hot loop, so validation has to cover this bank too
    // rather than only the one it was originally written for.
    let mut document: serde_json::Value =
        serde_json::from_str(OM471_9_M3D_JSON).expect("config parses as JSON");
    document["audio"]["body_modes"] = serde_json::json!([
        { "frequency_hz": 18_000.0, "q": 0.5, "gain": 1.0 }
    ]);
    let error = EngineConfig::from_json(&document.to_string())
        .expect("mutated config parses")
        .validate()
        .expect_err("an unstable body mode must be rejected");
    assert!(
        error.message.contains("body_modes"),
        "the rejection should name the bank, got: {}",
        error.message
    );
}

// --- the three paths at the boundary ----------------------------------------

#[test]
fn the_summed_frame_stays_inside_the_soft_clip_knee() {
    // The property the shared saturation gain exists to buy, and the reason the
    // clipper stayed in the solver rather than moving to the browser with the
    // mix. The knee bounds what a listener hears, so it has to bound the *sum*
    // of the three paths — not each of them separately, which would be a
    // different and much weaker statement, and not nothing at all, which is
    // what emitting the paths raw would have left.
    let config = config();
    let knee = config.config().audio.soft_clip_knee as f32;

    for (rpm, pedal) in [(600.0, 0.15), (1_200.0, 0.60), (1_400.0, 1.0)] {
        let out = samples_at(config.clone(), rpm, pedal, 293.15, 32_768);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak <= knee * 1.000_01,
            "at {rpm:.0} rpm and {pedal:.2} pedal the summed frame peaked at {peak}, \
             past the {knee} knee; the saturation gain is meant to make that \
             impossible"
        );
    }
}

#[test]
fn the_per_channel_clamp_never_has_to_engage() {
    // The clamp on each path is a guarantee, not a mechanism. `factor` bounds
    // the sum, and three signed terms summing inside the knee does not by
    // itself bound each one: two paths in opposition could in principle each
    // exceed it. This says that never actually happens, so the clamp is not
    // quietly shaping the sound while claiming to be a safety net.
    //
    // If this ever fails it is information rather than a nuisance — it means the
    // paths have started cancelling hard enough that the mix is a poor proxy for
    // any of them, and the saturation would need rethinking rather than the
    // threshold relaxing.
    let config = config();

    for (rpm, pedal) in [(600.0, 0.15), (1_200.0, 0.60), (1_400.0, 1.0)] {
        let frames = frames_at(config.clone(), rpm, pedal, 293.15, 32_768);
        let peak = frames.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak < 1.0,
            "at {rpm:.0} rpm and {pedal:.2} pedal a single path reached {peak}, so the \
             clamp engaged and is no longer only a safety net"
        );
    }
}

#[test]
fn every_path_is_finite_and_inside_the_output_range() {
    // The boundary contract, restated per channel. Each path is played in its
    // own right by the graph at the far end, so "finite and inside [-1, 1]" has
    // to hold of each rather than of their sum.
    let frames = frames_at(config(), 1_400.0, 1.0, 293.15, 40_000);
    for sample in &frames {
        assert!(
            sample.is_finite() && sample.abs() <= 1.0,
            "sample out of range: {sample}"
        );
    }
}

#[test]
fn the_paths_are_emitted_in_the_documented_order() {
    // The interleave order is a contract with `web/src/lib/cabin.ts`, which
    // gives each path a different transfer and cannot tell them apart by
    // content. Asserted by their signatures rather than by reading the
    // constants back, which would only restate them: the body path is almost
    // entirely below 80 Hz and the block path almost entirely above 300 Hz, so
    // if the two were transposed the cab would filter each with the other's
    // route and this would catch it.
    let frames = frames_at(config(), 1_400.0, 1.0, 293.15, 32_768);
    let f0 = firing_hz(1_400.0, config().config().geometry.cylinders);

    let low_heavy = |path: usize| {
        let samples = path_of(&frames, path);
        let low: f64 = (1..=2).map(|n| line_power(&samples, f0 * n as f64)).sum();
        let high: f64 = [1_500.0, 3_000.0]
            .iter()
            .map(|hz| line_power(&samples, *hz))
            .sum();
        low / high.max(1.0e-30)
    };

    assert!(
        low_heavy(BODY) > low_heavy(BLOCK) * 100.0,
        "the body path must be the low one and the block path the high one; \
         body/block low-to-high ratios were {:.3e} and {:.3e}",
        low_heavy(BODY),
        low_heavy(BLOCK)
    );
    assert!(
        low_heavy(EXHAUST) > low_heavy(BLOCK),
        "the exhaust path carries the firing orders, so it must be lower-weighted \
         than the clatter: {:.3e} against {:.3e}",
        low_heavy(EXHAUST),
        low_heavy(BLOCK)
    );
}
