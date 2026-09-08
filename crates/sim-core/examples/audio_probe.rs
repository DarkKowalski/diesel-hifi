//! Calibration probe: what the engine sounds like, as numbers.
//!
//! `cargo run --release -p sim-core --example audio_probe`
//!
//! The sweep example checks that the *engine* is calibrated. This checks that
//! the audio is at a usable level, that its energy sits where a speaker can
//! reproduce it, that it is a train of firing events rather than a tone, and
//! which of the three radiating paths is in front. Those are separate questions,
//! and ones the test suite cannot answer on its own: a test can assert samples
//! are finite and bounded without noticing they are 50 dB below anything
//! audible, or that four fifths of them sit in one octave.
//!
//! Levels here are dBFS — decibels relative to full scale on the output signal —
//! not sound pressure. Nothing about how loud the real engine is is published or
//! claimed.
//!
//! Three things moved when the acoustic work restarted, and each shows in the
//! numbers below:
//!
//! - **The metrics live in `sim_core::analysis`** and are tested against signals
//!   whose answer is known. The modulation figure in particular is not the one
//!   earlier milestones printed; see the note where it is reported.
//! - **The operating points are `sim_core::scenario` runs**, so the probe, the
//!   capture tool and the acoustic tests measure the same conditions rather than
//!   three similar sets.
//! - **Idle is governed rather than pinned.** It used to be 600 rpm at 15% pedal
//!   with the crank re-pinned every 2.5 ms, which is a held speed with an
//!   idle-ish pedal. Governed idle is 560 rpm and the engine finds it itself.

use sim_core::analysis::{
    amplitude_modulated, crest_db, dbfs, firing_hz, modulation_depth, peak, pulse_train, rms, tone,
    Spectrum,
};
use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::scenario::{self, ScenarioRun};
use sim_core::{EngineConfig, ValidatedConfig};

/// Bands the acceptance criteria are written against. They partition the whole
/// spectrum, so their shares must sum to 100%.
const BANDS: [(&str, f64, f64); 4] = [
    ("<80Hz", 0.0, 80.0),
    ("80-300", 80.0, 300.0),
    ("300-2k", 300.0, 2_000.0),
    (">2kHz", 2_000.0, f64::INFINITY),
];

/// Octave bands, for shape. A spectrum that rolls off smoothly is signal; one
/// that sags and then rises again toward Nyquist is arithmetic.
const OCTAVES: [(&str, f64, f64); 10] = [
    ("20-40", 20.0, 40.0),
    ("40-80", 40.0, 80.0),
    ("80-160", 80.0, 160.0),
    ("160-315", 160.0, 315.0),
    ("315-630", 315.0, 630.0),
    ("630-1k25", 630.0, 1_250.0),
    ("1k25-2k5", 1_250.0, 2_500.0),
    ("2k5-5k", 2_500.0, 5_000.0),
    ("5k-10k", 5_000.0, 10_000.0),
    ("10k-20k", 10_000.0, f64::INFINITY),
];

const PATH_NAMES: [&str; 4] = ["exhaust", "block", "body", "starter"];

/// Report one scenario run.
fn report(run: &ScenarioRun, cylinders: usize) {
    let trace = run.summed();
    let rpm = run.captured_rpm();
    let f0 = firing_hz(rpm, cylinders);
    let spectrum = Spectrum::of(&trace, run.sample_rate_hz);

    println!(
        "{:>12} {:>7.0} rpm   {:>6.3} peak   {:>6.3} rms   {:>7.1} dBFS   {:.2} s captured",
        run.id,
        rpm,
        peak(&trace),
        rms(&trace),
        dbfs(rms(&trace)),
        run.frame_count() as f64 / run.sample_rate_hz,
    );

    let mut checksum = 0.0;
    for (name, lo, hi) in BANDS {
        let pct = spectrum.band_share(lo, hi);
        checksum += pct;
        println!("             {name:>8}: {pct:>5.1}%");
    }
    // The bands partition the spectrum, so this must read 100.0. It is the probe
    // checking its own normalisation rather than a claim about audio.
    println!(
        "             {:>8}: {checksum:>5.1}%  (must be 100.0)",
        "sum"
    );

    // Character, which is the part band shares are blind to. Two signals can
    // hold identical shares in every band above and be a diesel and a doorbell
    // respectively; these three say which.
    //
    // `modulation` is not comparable with the figure earlier milestones printed.
    // That detector rectified the signal and low-passed it, and rectification
    // puts a component at twice the carrier into the "envelope" — so it found
    // its own artefact and scored a steady 60 Hz sine at 0.42. This one takes
    // the analytic envelope, for which a tone is flat by construction, and
    // reports the fractional swing at f0 and 2·f0. A tone reads 0; a tone
    // modulated to depth d reads d.
    println!(
        "             {:>8}: {:>5.1} Hz   orders f0..4f0 {:>5.1}%  crest {:>5.1} dB  \
         modulation {:>5.2}",
        "firing",
        f0,
        spectrum.comb_share(f0, 4),
        crest_db(&trace),
        modulation_depth(&trace, run.sample_rate_hz, f0),
    );

    // The acceptance bands stop at 15 kHz rather than running to Nyquist. The
    // explicit port transfer leaves a two-sample limit cycle near equilibrium,
    // which lands within a whisker of Nyquist and is arithmetic rather than
    // sound. Measuring to Nyquist would let that residue satisfy a
    // high-frequency target it is not signal for; the `>15kHz` line reports it
    // separately so it stays visible without being counted. 150 Hz is where a
    // small speaker starts reproducing anything at all, so it is the boundary
    // the "can this be heard on ordinary hardware" criterion actually means.
    println!(
        "             {:>8}: {:>5.2}%  (acceptance band)",
        "150-15k",
        spectrum.band_share(150.0, 15_000.0)
    );
    println!(
        "             {:>8}: {:>5.2}%  (acceptance band)",
        "2k-15k",
        spectrum.band_share(2_000.0, 15_000.0)
    );

    // Octave shape, and the Nyquist question. A two-sample limit cycle from the
    // explicit port transfer would appear as energy piling up in the top octave
    // rather than as harmonics decaying away from the firing orders.
    let octaves: Vec<(&str, f64)> = OCTAVES
        .iter()
        .map(|(name, lo, hi)| (*name, spectrum.band_share(*lo, *hi)))
        .collect();
    let loudest = octaves.iter().map(|(_, p)| *p).fold(0.0f64, f64::max);
    println!("             spectrum, dB relative to the strongest octave:");
    for (name, pct) in &octaves {
        let db = 10.0 * (pct / loudest.max(1e-30)).max(1e-12).log10();
        let bars = ((60.0 + db) / 2.0).max(0.0).round() as usize;
        println!(
            "             {name:>8}: {db:>6.1} dB {:<30} {pct:>5.2}%",
            "#".repeat(bars)
        );
    }
    println!(
        "             {:>8}: {:>5.3}%  (near-Nyquist residue)",
        ">15kHz",
        spectrum.band_share(15_000.0, f64::INFINITY)
    );

    // Each path on its own, from the same frames the mix above came from.
    //
    // What this is for is the balance: the exhaust should carry the firing
    // orders, the body should sit under them and the modal bank above, and a
    // mixed spectrum cannot say which one moved when a band share changes.
    println!("             each path alone (dBFS, and its own band shares):");
    let mut levels_db: Vec<f64> = Vec::new();
    for (index, name) in PATH_NAMES.iter().enumerate() {
        let trace = run.path(index);
        // A path that is exactly silent is reported as silent rather than as
        // `-inf dBFS` beside a row of NaN band shares. The starter is silent at
        // every steady operating point by construction — its pinion is out —
        // and a share of nothing is not a small share, it is undefined.
        if trace.iter().all(|s| *s == 0.0) {
            levels_db.push(f64::NEG_INFINITY);
            println!("             {name:>8}: silent (not engaged at this operating point)");
            continue;
        }
        let spectrum = Spectrum::of(&trace, run.sample_rate_hz);
        let bands: Vec<String> = BANDS
            .iter()
            .map(|(label, lo, hi)| format!("{label} {:.1}%", spectrum.band_share(*lo, *hi)))
            .collect();
        let level_db = dbfs(rms(&trace));
        levels_db.push(level_db);
        println!(
            "             {name:>8}: {level_db:>6.1} dBFS   {}   orders {:>4.1}%",
            bands.join("  "),
            spectrum.comb_share(f0, 4),
        );
    }
    // The balance as a signed number rather than as lines to subtract in your
    // head. The exhaust carries the firing orders and the block carries the
    // clatter, so a block sitting in front of the exhaust is a small engine
    // however the bands come out.
    //
    // Indexed rather than destructured. This used to be `if let [exhaust,
    // block, body] = levels_db[..]`, which stops matching the moment a fourth
    // path exists — and stops printing the line without failing, which is the
    // worst way for a measurement to go missing.
    let named = |name: &str| {
        PATH_NAMES
            .iter()
            .position(|p| *p == name)
            .and_then(|i| levels_db.get(i).copied())
    };
    if let (Some(exhaust), Some(block), Some(body)) =
        (named("exhaust"), named("block"), named("body"))
    {
        println!(
            "             {:>8}: exhaust leads block by {:>5.1} dB, body by {:>5.1} dB",
            "balance",
            exhaust - block,
            exhaust - body
        );
    }
    println!();
}

/// Print what each metric reads for signals whose character is not in question.
///
/// The probe carries its own counterexamples because a metric is only worth the
/// number it gives for a signal it should *reject*. `tests/analysis.rs` asserts
/// these; this prints them, so anyone reading a run of the probe can see the
/// scale the engine figures sit on rather than taking it on trust.
fn calibrate_the_instruments(rate: f64) {
    const LEN: usize = 32_768;
    const F0: f64 = 60.0;

    println!("the instruments, against signals whose answer is known:");
    println!(
        "{:>22} {:>8} {:>12} {:>8}",
        "signal", "crest", "modulation", "orders"
    );
    let cases: [(&str, Vec<f32>); 4] = [
        ("60 Hz tone", tone(LEN, rate, F0)),
        ("800 Hz tone", tone(LEN, rate, 800.0)),
        (
            "800 Hz, 50% AM at 60 Hz",
            amplitude_modulated(LEN, rate, 800.0, F0, 0.5),
        ),
        ("60 Hz pulse train", pulse_train(LEN, rate, F0, 0.1)),
    ];
    for (label, signal) in cases {
        let spectrum = Spectrum::of(&signal, rate);
        println!(
            "{label:>22} {:>7.1} dB {:>12.2} {:>7.1}%",
            crest_db(&signal),
            modulation_depth(&signal, rate, F0),
            spectrum.comb_share(F0, 4),
        );
    }
    println!(
        "                       a tone must read ~0 modulated; the old detector scored the\n\
         \x20                      first of these 0.42 by measuring its own rectification\n"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: ValidatedConfig = EngineConfig::from_json(OM471_9_M3D_JSON)?.validate()?;
    let knee = config.config().audio.soft_clip_knee;
    let cylinders = config.config().geometry.cylinders;
    let rate = 1.0 / config.config().solver.fixed_step_s;

    calibrate_the_instruments(rate);

    println!("engine audio levels (dBFS, not sound pressure)\n");
    for scenario in scenario::all() {
        // Steady scenarios only. A spectrum of a run-down is a spectrum of every
        // speed it passed through, so the transient scenarios are captured by
        // `audio_capture` for listening and are not averaged here.
        if !scenario.steady {
            continue;
        }
        let run = scenario::run(&config, &scenario)?;
        assert_eq!(
            run.dropped_frames, 0,
            "the probe fell behind the ring and the capture has a hole in it"
        );
        report(&run, cylinders);
    }

    println!(
        "soft-clip knee {knee:.2}: full load should approach it, idle should stay \
         well below and still be audible"
    );
    Ok(())
}
