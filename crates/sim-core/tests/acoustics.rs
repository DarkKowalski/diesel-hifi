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

    let with_share = high_band_share(&with, 500.0);
    let without_share = high_band_share(&without, 500.0);
    // The margin is 2x rather than the 4x it was when the modal bank landed,
    // and the reason is a real improvement rather than a regression: radiating
    // the applied port flow instead of an `area * pressure difference` proxy
    // sharpened the blowdown edge, so the exhaust path now carries genuine top
    // end of its own. The structural path is still the majority of what is up
    // there, which is what this test is for.
    assert!(
        with_share > without_share * 2.0,
        "the modal bank should dominate above 500 Hz: {with_share:.4} with it \
         against {without_share:.4} without, which is not a large enough difference \
         to be the mechanism this path claims to be"
    );
}

#[test]
fn the_modal_bank_only_rings_where_it_is_told_to() {
    // Move every mode down an octave and the energy must follow. If it does not,
    // the top end is coming from something other than the configured bank —
    // clipping, say, or the exhaust path's own edges — and the configuration is
    // decorative.
    let high = samples_at(config(), 1_000.0, 0.4, 293.15, 40_000);
    let low = samples_at(
        config_with_audio(|audio| {
            for mode in audio["structural_modes"]
                .as_array_mut()
                .expect("modes are an array")
            {
                let hz = mode["frequency_hz"].as_f64().expect("frequency");
                mode["frequency_hz"] = serde_json::json!(hz * 0.5);
            }
        }),
        1_000.0,
        0.4,
        293.15,
        40_000,
    );

    let high_share = high_band_share(&high, 2_000.0);
    let low_share = high_band_share(&low, 2_000.0);
    assert!(
        high_share > low_share * 1.5,
        "halving every mode frequency should move energy out of the 2 kHz band, \
         but the share went {high_share:.4} -> {low_share:.4}"
    );
}

#[test]
fn the_clatter_is_load_dependent_without_a_clatter_schedule() {
    // Ignition delay lengthens when the cylinder is cold and lightly loaded, a
    // longer delay means a larger premixed fraction, and a larger premixed
    // fraction means a sharper pressure rise. So a lightly loaded engine must
    // clatter harder *in proportion* than a hard-working one, and it must do so
    // because the combustion model says so: there is no load term anywhere in
    // the audio path to arrange it.
    let speed = 1_200.0;
    let light = samples_at(config(), speed, 0.08, 293.15, 40_000);
    let heavy = samples_at(config(), speed, 1.0, 293.15, 40_000);

    let light_share = high_band_share(&light, 1_500.0);
    let heavy_share = high_band_share(&heavy, 1_500.0);
    assert!(
        light_share > heavy_share,
        "at {speed:.0} rpm a lightly loaded engine should be proportionally \
         rattlier than a fully loaded one, got {light_share:.4} light against \
         {heavy_share:.4} heavy"
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

        let mut sink = vec![0.0f32; PIN_CHUNK as usize];
        for _ in 0..1_200 {
            sim.advance(PIN_CHUNK).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            sim.drain_audio(&mut sink);
        }
        let mut out = vec![0.0f32; 40_000];
        let mut written = 0;
        while written < out.len() {
            let batch = PIN_CHUNK.min((out.len() - written) as u32);
            sim.advance(batch).expect("advance");
            sim.pin_speed_rpm(speed).expect("pin");
            written += sim.drain_audio(&mut out[written..]);
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
