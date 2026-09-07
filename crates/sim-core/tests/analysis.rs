//! The measuring instruments, measured.
//!
//! Every metric the acoustic work is steered by is checked here against signals
//! whose answer is known before the code runs: a steady tone, a tone modulated
//! to a stated depth, a train of pulses, and noise. This file exists because the
//! previous modulation detector was documented as reading "nearly zero however
//! loud it is" for a tone and read 0.42 — a number that would have satisfied the
//! `> 0` acceptance criterion written against it, on a signal that is by
//! definition not a pulse train. A metric only ever pointed at engine output has
//! nothing to fail against.
//!
//! The rule these tests encode: **a metric must give the wrong-looking answer
//! for the wrong signal.** Passing on engine audio proves nothing on its own.

use sim_core::analysis::{
    amplitude_modulated, crest_db, dbfs, estimate_firing_hz, firing_hz, modulation_depth, peak,
    pulse_train, rms, rpm_from_firing_hz, tone, Spectrum,
};

/// The solver's own rate, so these read against the same numbers the probe does.
const RATE: f64 = 40_000.0;

/// 0.82 s at 40 kHz — the probe's analysis window.
const LEN: usize = 32_768;

/// A six-cylinder firing rate at 1200 rpm.
const F0: f64 = 60.0;

/// Deterministic white noise. A seeded xorshift rather than a dependency, and
/// seeded because a test that is only usually right is not a test.
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            // Top 24 bits to a float in [-1, 1).
            let unit = (state >> 40) as f64 / f64::from(1u32 << 24);
            (unit * 2.0 - 1.0) as f32
        })
        .collect()
}

// --- modulation depth -------------------------------------------------------

#[test]
fn a_steady_tone_is_not_modulated() {
    // The counterexample the old detector failed. A sine has a constant
    // amplitude envelope by definition, so no detector may report it swinging,
    // at any carrier frequency and at any level.
    //
    // The old detector rectified and low-passed, and rectification puts a
    // component at twice the carrier into the "envelope" with a third of the
    // mean's amplitude. Pointed at f0 and 2 f0 it then found its own artefact.
    for carrier in [30.0, 60.0, 120.0, 480.0, 2_000.0] {
        for level in [0.01f32, 0.2, 1.0] {
            let signal: Vec<f32> = tone(LEN, RATE, carrier).iter().map(|s| s * level).collect();
            let depth = modulation_depth(&signal, RATE, F0);
            assert!(
                depth < 0.05,
                "a steady {carrier} Hz tone at level {level} read as {depth:.3} modulated"
            );
        }
    }
}

#[test]
fn an_amplitude_modulated_tone_reads_its_own_depth() {
    // The other half: a detector that reports zero for everything would pass the
    // test above. Feed it a swing of a stated size and it must report that size.
    for depth in [0.1, 0.25, 0.5, 0.9] {
        let signal = amplitude_modulated(LEN, RATE, 800.0, F0, depth);
        let measured = modulation_depth(&signal, RATE, F0);
        assert!(
            (measured - depth).abs() < 0.05,
            "a tone modulated to {depth} read as {measured:.3}"
        );
    }
}

#[test]
fn modulation_is_measured_at_the_firing_rate_and_not_anywhere() {
    // A signal modulated at some other rate is not a firing comb, and must not
    // read as one. This is what stops the metric being satisfied by any wobble.
    let signal = amplitude_modulated(LEN, RATE, 800.0, 17.0, 0.9);
    let measured = modulation_depth(&signal, RATE, F0);
    assert!(
        measured < 0.05,
        "modulation at 17 Hz read as {measured:.3} at a 60 Hz firing rate"
    );
}

#[test]
fn a_pulse_train_is_deeply_modulated() {
    // What engine audio is supposed to look like. A train of distinct events has
    // an envelope that goes to nothing between them, so the swing is at least as
    // large as the mean.
    let signal = pulse_train(LEN, RATE, F0, 0.12);
    let measured = modulation_depth(&signal, RATE, F0);
    assert!(
        measured > 1.0,
        "a train of pulses at the firing rate read as only {measured:.3} modulated"
    );
}

#[test]
fn noise_is_not_modulated_at_the_firing_rate() {
    // Noise has a fluctuating envelope, but not one that swings *at f0*. A
    // detector that cannot tell the two apart would let a hiss pass as an
    // engine.
    let measured = modulation_depth(&noise(LEN, 0x51ed), RATE, F0);
    assert!(
        measured < 0.15,
        "white noise read as {measured:.3} modulated at the firing rate"
    );
}

// --- crest factor -----------------------------------------------------------

#[test]
fn the_crest_factor_of_a_sine_is_three_decibels() {
    // The textbook anchor: peak over RMS for a sinusoid is sqrt(2).
    let measured = crest_db(&tone(LEN, RATE, 500.0));
    assert!(
        (measured - 3.01).abs() < 0.1,
        "a sine should read 3.01 dB, got {measured:.2}"
    );
}

#[test]
fn a_pulse_train_has_a_high_crest_factor_and_noise_a_middling_one() {
    let pulses = crest_db(&pulse_train(LEN, RATE, F0, 0.08));
    assert!(
        pulses > 9.0,
        "a train of distinct events should be well into double figures, got {pulses:.1} dB"
    );

    // Uniform white noise sits near 4.8 dB: the peak is the full scale and the
    // RMS is 1/sqrt(3) of it. Well below a pulse train, well above a sine, and
    // the reason crest factor alone cannot certify an engine.
    let hiss = crest_db(&noise(LEN, 0xa17c));
    assert!(
        (3.0..7.0).contains(&hiss),
        "white noise should read middling, got {hiss:.1} dB"
    );
}

#[test]
fn squaring_a_signal_off_collapses_its_crest_factor() {
    // The failure mode this metric was introduced for: gains set too high, the
    // clipper flattening every peak, and a signal that measures fine in every
    // band while having become a buzz.
    //
    // Two statements, because how far the crest falls depends on how much of the
    // time the signal is up against the ceiling. On a sparse train it falls; on
    // a dense one it falls through the criterion.
    let sparse = pulse_train(LEN, RATE, F0, 0.08);
    let squared: Vec<f32> = sparse.iter().map(|s| (s * 12.0).min(1.0)).collect();
    let (before, after) = (crest_db(&sparse), crest_db(&squared));
    assert!(
        after < before - 3.0,
        "clipping should cost crest factor: {before:.1} dB became {after:.1} dB"
    );

    let dense: Vec<f32> = pulse_train(LEN, RATE, F0, 0.45)
        .iter()
        .map(|s| (s * 12.0).min(1.0))
        .collect();
    let measured = crest_db(&dense);
    assert!(
        measured < 9.0,
        "a signal held against the ceiling should fail the crest criterion, got {measured:.1} dB"
    );
}

// --- comb share -------------------------------------------------------------

#[test]
fn a_comb_at_the_firing_rate_reads_as_orders_and_a_tone_between_them_does_not() {
    // Four orders of a comb, all present.
    let comb: Vec<f32> = (0..LEN)
        .map(|i| {
            let t = i as f64 / RATE;
            (1..=4)
                .map(|n| (std::f64::consts::TAU * F0 * f64::from(n) * t).sin() * 0.25)
                .sum::<f64>() as f32
        })
        .collect();
    let share = Spectrum::of(&comb, RATE).comb_share(F0, 4);
    assert!(
        share > 99.0,
        "a pure four-order comb should read as essentially all orders, got {share:.1}%"
    );

    // The same energy placed between the orders must read as none of them. This
    // is the distinction a band share cannot make: both signals sit in the same
    // octave.
    let between = tone(LEN, RATE, F0 * 2.5);
    let share = Spectrum::of(&between, RATE).comb_share(F0, 4);
    assert!(
        share < 1.0,
        "a tone between the orders read as {share:.1}% in the orders"
    );
}

#[test]
fn noise_holds_almost_nothing_in_the_orders() {
    // A filtered noise floor can satisfy any band target; this is what says it
    // is not an engine. Four narrow lines out of 16384 bins is a vanishing
    // fraction of a flat spectrum.
    let share = Spectrum::of(&noise(LEN, 0x9e37), RATE).comb_share(F0, 4);
    assert!(
        share < 1.0,
        "white noise read as {share:.1}% of its energy in the firing orders"
    );
}

// --- levels and bands -------------------------------------------------------

#[test]
fn levels_are_what_they_claim_to_be() {
    let half: Vec<f32> = tone(LEN, RATE, 500.0).iter().map(|s| s * 0.5).collect();
    assert!((peak(&half) - 0.5).abs() < 1e-3);
    assert!(
        (rms(&half) - 0.5 / std::f64::consts::SQRT_2).abs() < 1e-3,
        "a sine's RMS is its amplitude over root two"
    );
    assert!(
        (dbfs(0.5) + 6.02).abs() < 0.01,
        "half scale is −6 dBFS by definition"
    );
}

#[test]
fn a_band_share_answers_only_where_the_energy_is() {
    // Two signals with identical shares in every band, one a tone and one a
    // pulse train filtered to the same place. The band table cannot separate
    // them; that is what the two metrics above are for, and this test states the
    // limitation rather than leaving it implied.
    let spectrum = Spectrum::of(&tone(LEN, RATE, 200.0), RATE);
    assert!(spectrum.band_share(80.0, 300.0) > 99.0);
    assert!(spectrum.band_share(300.0, 2_000.0) < 1.0);
    // And it says nothing about character: the same table for a tone reports a
    // crest factor and a modulation depth that fail every criterion.
    assert!(crest_db(&tone(LEN, RATE, 200.0)) < 9.0);
    assert!(modulation_depth(&tone(LEN, RATE, 200.0), RATE, F0) < 0.05);
}

#[test]
fn the_firing_frequency_comes_from_the_cylinder_count() {
    assert!((firing_hz(1_200.0, 6) - 60.0).abs() < 1e-9);
    assert!((firing_hz(1_200.0, 4) - 40.0).abs() < 1e-9);
    assert!((firing_hz(560.0, 6) - 28.0).abs() < 1e-9);
}

// --- the firing rate, recovered from the signal -----------------------------
//
// The estimator is the one metric here that has to find its own reference
// frequency, and it is the one with a failure mode that looks like an answer: an
// engine reported at half or double its speed gives a plausible rpm, a plausible
// comb share and a plausible modulation depth. Every test below puts a signal at
// one of those two traps.

/// The speed range the reference probe searches: 400 to 2400 rpm on a six.
const SEARCH_LO: f64 = 20.0;
const SEARCH_HI: f64 = 120.0;

#[test]
fn the_firing_rate_is_recovered_from_a_pulse_train() {
    let estimate = estimate_firing_hz(
        &pulse_train(LEN, RATE, F0, 0.1),
        RATE,
        SEARCH_LO,
        SEARCH_HI,
        4,
    )
    .expect("a pulse train has a firing rate");
    assert!(
        (estimate.f0_hz - F0).abs() < 0.5,
        "a 60 Hz pulse train read as {:.2} Hz",
        estimate.f0_hz
    );
    assert!(
        estimate.comb_share > 30.0,
        "a pulse train's own comb should account for a large share, got {:.1}%",
        estimate.comb_share
    );
}

#[test]
fn the_estimate_does_not_fall_to_the_subharmonic() {
    // The trap: a candidate at f0/2 lands on every real line as well, just with
    // empty bins between them. Only the 1/n weighting separates them, and this
    // asserts it does. 30 Hz is inside the search range, so nothing but the
    // score is keeping the estimator out of it.
    let estimate = estimate_firing_hz(
        &pulse_train(LEN, RATE, F0, 0.1),
        RATE,
        SEARCH_LO,
        SEARCH_HI,
        4,
    )
    .expect("a pulse train has a firing rate");
    assert!(
        (estimate.f0_hz - F0 / 2.0).abs() > 5.0,
        "the estimator halved the firing rate: {:.2} Hz",
        estimate.f0_hz
    );
}

#[test]
fn the_estimate_does_not_fall_to_the_octave() {
    // The other trap, and the one an unweighted harmonic sum cannot escape: for
    // a lone tone at 120 Hz the candidates 60 and 120 each collect exactly one
    // real line and score identically. The answer is then decided by rounding.
    let estimate = estimate_firing_hz(&tone(LEN, RATE, 120.0), RATE, SEARCH_LO, SEARCH_HI, 4)
        .expect("a tone in range has a best candidate");
    assert!(
        (estimate.f0_hz - 120.0).abs() < 1.0,
        "a 120 Hz tone read as {:.2} Hz, which is its subharmonic",
        estimate.f0_hz
    );
}

#[test]
fn a_comb_with_a_weak_fundamental_is_not_reported_an_octave_high() {
    // The counterexample the 1/n weighting alone gets wrong, and it was found by
    // pointing the estimator at the model's own governed idle, whose speed is
    // known: at 551 rpm the firing fundamental at 27.5 Hz held 5.4% of the
    // energy and its second order at 55 Hz held 19.8%. The weighted sum chose
    // 55 Hz and reported 1100 rpm — a plausible-looking number, and twice the
    // truth.
    //
    // Reproduced here as a comb with those proportions, so the fix is checked
    // against a signal whose answer is known rather than against the capture
    // that exposed it. The amplitudes are the square roots of the measured
    // energy shares 5.4, 19.8, 4.0, 3.8 and 5.9%, normalised to the loudest.
    let weights = [0.52, 1.0, 0.45, 0.44, 0.55];
    let comb: Vec<f32> = (0..LEN)
        .map(|i| {
            let t = i as f64 / RATE;
            weights
                .iter()
                .enumerate()
                .map(|(index, w)| (std::f64::consts::TAU * 27.5 * (index + 1) as f64 * t).sin() * w)
                .sum::<f64>() as f32
        })
        .collect();

    let estimate =
        estimate_firing_hz(&comb, RATE, SEARCH_LO, SEARCH_HI, 4).expect("a comb has a firing rate");
    assert!(
        (estimate.f0_hz - 27.5).abs() < 1.0,
        "a comb spaced 27.5 Hz whose second order is the loudest read as {:.2} Hz",
        estimate.f0_hz
    );
}

#[test]
fn the_subharmonic_check_does_not_halve_a_comb_that_has_nothing_below_it() {
    // The other side of the previous test, and the one that keeps the fix from
    // being a licence to report half of everything. A clean comb at 60 Hz has
    // nothing at 30, so nothing may move it there.
    let comb: Vec<f32> = (0..LEN)
        .map(|i| {
            let t = i as f64 / RATE;
            (1..=4)
                .map(|n| (std::f64::consts::TAU * F0 * f64::from(n) * t).sin() / f64::from(n))
                .sum::<f64>() as f32
        })
        .collect();
    let estimate =
        estimate_firing_hz(&comb, RATE, SEARCH_LO, SEARCH_HI, 4).expect("a comb has a firing rate");
    assert!(
        (estimate.f0_hz - F0).abs() < 0.5,
        "a clean 60 Hz comb read as {:.2} Hz",
        estimate.f0_hz
    );
}

#[test]
fn the_refinement_beats_the_bin_it_started_in() {
    // 90.37 Hz is deliberately not a bin centre: at this window a bin is 1.22 Hz
    // and the coarse scan can only ever land within half of one. Anything closer
    // than that is the time-domain refinement working, and without it an rpm
    // figure would carry ±12 rpm of quantisation it did not admit to.
    let f0 = 90.37;
    let estimate = estimate_firing_hz(
        &pulse_train(LEN, RATE, f0, 0.1),
        RATE,
        SEARCH_LO,
        SEARCH_HI,
        4,
    )
    .expect("a pulse train has a firing rate");
    assert!(
        (estimate.f0_hz - f0).abs() < 0.2,
        "expected {f0} Hz to within a fifth of a hertz, got {:.3} Hz",
        estimate.f0_hz
    );
}

#[test]
fn noise_produces_an_estimate_with_no_confidence_in_it() {
    // Noise still has a best-scoring candidate — every signal does. What it must
    // not have is a comb share, because that is the number the probe uses to
    // decide whether a window is an engine at all. A window of road noise that
    // reported 1500 rpm with authority would be annotated as an operating point.
    let estimate = estimate_firing_hz(&noise(LEN, 0x51ed), RATE, SEARCH_LO, SEARCH_HI, 4)
        .expect("noise has a best candidate");
    assert!(
        estimate.comb_share < 2.0,
        "white noise claimed {:.1}% of its energy in the comb at {:.1} Hz",
        estimate.comb_share,
        estimate.f0_hz
    );
}

#[test]
fn silence_has_no_firing_rate_rather_than_a_default_one() {
    assert!(estimate_firing_hz(&[0.0; 4_096], RATE, SEARCH_LO, SEARCH_HI, 4).is_none());
    assert!(estimate_firing_hz(&[], RATE, SEARCH_LO, SEARCH_HI, 4).is_none());
    // An inverted range is a caller error, not a range to scan backwards.
    let signal = pulse_train(LEN, RATE, F0, 0.1);
    assert!(estimate_firing_hz(&signal, RATE, SEARCH_HI, SEARCH_LO, 4).is_none());
}

#[test]
fn the_speed_and_the_firing_rate_convert_both_ways() {
    for (rpm, cylinders) in [(1_200.0, 6), (560.0, 6), (1_800.0, 4), (2_100.0, 8)] {
        let round_trip = rpm_from_firing_hz(firing_hz(rpm, cylinders), cylinders);
        assert!(
            (round_trip - rpm).abs() < 1e-9,
            "{rpm} rpm on {cylinders} cylinders round-tripped to {round_trip}"
        );
    }
    assert!((rpm_from_firing_hz(60.0, 6) - 1_200.0).abs() < 1e-9);
}
