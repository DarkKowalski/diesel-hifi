//! Exhaust acoustic output tests.
//!
//! The solver's 25 us step is a 40 kHz sample rate, so the exhaust sound is not
//! synthesised: one sample per step is taken from the computed blowdown through
//! the exhaust ports. These tests exist to prove that what comes out is really
//! the simulation — the right number of pulses, at the right frequency, derived
//! from configuration rather than hard-coded — and that producing it does not
//! break the determinism guarantees everything else depends on.

use std::path::Path;

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
    let mut sim = Simulation::new(
        config,
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )
    .expect("simulation builds");
    sim.set_controls(controls(0.4, false, true))
        .expect("controls accepted");

    // Settle, discarding the start-up transient.
    let mut sink = vec![0.0f32; PIN_CHUNK as usize];
    for _ in 0..1_200 {
        sim.advance(PIN_CHUNK).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        sim.drain_audio(&mut sink);
    }

    let mut out = vec![0.0f32; steps];
    let mut written = 0;
    while written < out.len() {
        let batch = PIN_CHUNK.min((out.len() - written) as u32);
        sim.advance(batch).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
        written += sim.drain_audio(&mut out[written..]);
    }
    out
}

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

/// Firing frequency of a four-stroke: one event per cylinder per two revolutions.
fn firing_hz(rpm: f64, cylinders: usize) -> f64 {
    rpm / 120.0 * cylinders as f64
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
fn exactly_one_sample_is_produced_per_step() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation builds");
    sim.set_controls(controls(0.0, true, true))
        .expect("controls");

    sim.advance(1_000).expect("advance");
    assert_eq!(sim.audio_available(), 1_000);

    let mut out = vec![0.0f32; 4_000];
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
    let mut out = vec![0.0f32; 20_000];
    let mut collected = 0;
    while collected < out.len() {
        let batch = 20_000u32.min((out.len() - collected) as u32);
        sim.advance(batch).expect("advance");
        collected += sim.drain_audio(&mut out[collected..]);
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
    let mut bulk = vec![0.0f32; 6_000];
    assert_eq!(one_batch.drain_audio(&mut bulk), 6_000);

    let mut stepwise = make();
    let mut single = vec![0.0f32; 6_000];
    for slot in single.iter_mut() {
        stepwise.advance(1).expect("advance");
        let mut one = [0.0f32; 1];
        assert_eq!(stepwise.drain_audio(&mut one), 1);
        *slot = one[0];
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

    let mut sink = vec![0.0f32; 4_000];
    for _ in 0..120 {
        sim.advance(4_000).expect("advance");
        sim.drain_audio(&mut sink);
    }

    sim.reset(ResetOptions::default()).expect("reset");

    // A reset engine is stopped, so the samples that follow should be near
    // silence rather than a full-scale spike.
    sim.advance(4_000).expect("advance");
    let mut out = vec![0.0f32; 4_000];
    let written = sim.drain_audio(&mut out);
    let peak = out[..written].iter().fold(0.0f32, |m, s| m.max(s.abs()));

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
    let mut sink = vec![0.0f32; 4_000];

    let mut total = 0usize;
    let mut pinned = 0usize;
    let mut peak = 0.0f32;
    let mut count = |sim: &mut Simulation, batches: usize, sink: &mut Vec<f32>| {
        for _ in 0..batches {
            sim.advance(4_000).expect("advance");
            let n = sim.drain_audio(sink);
            for sample in &sink[..n] {
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
    count(&mut sim, 160, &mut sink);
    sim.set_controls(controls(0.0, false, false))
        .expect("controls");
    count(&mut sim, 400, &mut sink);
    sim.set_controls(controls(0.6, true, true))
        .expect("controls");
    count(&mut sim, 60, &mut sink);

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
    let mut sink = vec![0.0f32; 4_000];

    // Run it hard, so the manifolds and cylinders are far from rest.
    sim.set_controls(controls(0.9, true, true))
        .expect("controls");
    for _ in 0..200 {
        sim.advance(4_000).expect("advance");
        sim.drain_audio(&mut sink);
    }

    // Cut fuelling and let it come to a complete stop.
    sim.set_controls(controls(0.0, false, false))
        .expect("controls");
    for _ in 0..600 {
        sim.advance(4_000).expect("advance");
        sim.drain_audio(&mut sink);
    }
    assert_eq!(sim.rpm(), 0.0, "the engine should have come to rest");

    let mut out = vec![0.0f32; 16_384];
    let mut written = 0;
    while written < out.len() {
        let batch = 4_000u32.min((out.len() - written) as u32);
        sim.advance(batch).expect("advance");
        written += sim.drain_audio(&mut out[written..]);
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

/// Idle must be audible and full load must not be at the ceiling: the usable
/// range has to sit between the two.
#[test]
fn the_output_keeps_headroom_across_the_operating_range() {
    let config = config();
    let knee = config.config().audio.soft_clip_knee as f32;

    let peak_at = |rpm: f64, pedal: f64| {
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
        let mut sink = vec![0.0f32; 100];
        for _ in 0..1_200 {
            sim.advance(100).expect("advance");
            sim.pin_speed_rpm(rpm).expect("pin");
            sim.drain_audio(&mut sink);
        }
        let mut peak = 0.0f32;
        for _ in 0..400 {
            sim.advance(100).expect("advance");
            sim.pin_speed_rpm(rpm).expect("pin");
            let n = sim.drain_audio(&mut sink);
            peak = peak.max(sink[..n].iter().fold(0.0f32, |m, s| m.max(s.abs())));
        }
        peak
    };

    let idle = peak_at(600.0, 0.15);
    let full = peak_at(1_400.0, 1.0);

    assert!(
        idle > 0.01,
        "idle at {idle} is too quiet to hear once a device volume is applied"
    );
    assert!(
        full < knee * 0.99,
        "full load at {full} is against the {knee} ceiling, leaving no headroom \
         for the louder start transient"
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
        let mut sink = vec![0.0f32; 4_000];
        sim.drain_audio(&mut sink);
    }
    let running = sim.audio_level_db();

    assert!(
        running > silent,
        "a running engine must be louder than a stopped one: {running} dB against \
         {silent} dB"
    );
    assert!(running.is_finite() && running > 0.0);
}
