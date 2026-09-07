//! Calibration probe: report exhaust audio levels and spectrum at a few points.
//!
//! `cargo run --release -p sim-core --example audio_probe`
//!
//! The sweep example checks that the *engine* is calibrated. This checks that
//! the exhaust signal is at a usable level and that its energy sits where a
//! speaker can reproduce it, which is a separate question and one the test suite
//! cannot answer on its own: a test can assert samples are finite and bounded
//! without noticing that they are 50 dB below anything audible, or that four
//! fifths of them sit in a single octave.
//!
//! Levels here are dBFS — decibels relative to full scale on the output signal —
//! not sound pressure. Nothing about how loud the real engine is is published or
//! claimed.
//!
//! ## Why the band shares are folded
//!
//! A real signal's spectrum is symmetric: bin `k` and bin `N-k` carry the same
//! magnitude. Summing only the positive-frequency half and then dividing by the
//! signal's full mean square makes every band read exactly half its true share,
//! and makes a set of bands covering the whole spectrum sum to 50% instead of
//! 100%. `power_spectrum` folds the negative half back in, so the band table
//! sums to 100% and that sum is a standing self-check on this file.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

/// Steps between re-pinning the crank. Long batches let an unloaded engine
/// accelerate away from the speed being measured.
const PIN_CHUNK: u32 = 100;

/// The solver's step is 25 us, so the audio it produces is at 40 kHz.
const SAMPLE_RATE_HZ: f64 = 40_000.0;

/// Analysis window, a power of two so the transform can be radix-2.
///
/// 32768 samples at 40 kHz is 0.82 s, giving 1.22 Hz per bin — fine enough to
/// place a 28 Hz idle fundamental in a bin of its own, and long enough to
/// average over several dozen firing events.
const WINDOW: usize = 32_768;

/// In-place iterative radix-2 FFT, decimation in time.
///
/// Hand-written rather than pulled in as a dependency: this is an example, the
/// transform is textbook, and `sim-core` should not gain a crate for the sake of
/// one calibration printout.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two(), "radix-2 needs a power-of-two length");
    assert_eq!(im.len(), n);

    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Butterflies, stage by stage. Twiddles are evaluated directly rather than
    // by recurrence: a recurrence drifts over fifteen stages, and the trig calls
    // are nothing next to the simulation that produced the samples.
    let mut len = 2usize;
    while len <= n {
        let half = len / 2;
        let mut base = 0usize;
        while base < n {
            for k in 0..half {
                let angle = -std::f64::consts::TAU * k as f64 / len as f64;
                let (wi, wr) = angle.sin_cos();
                let lo = base + k;
                let hi = lo + half;
                let vr = re[hi] * wr - im[hi] * wi;
                let vi = re[hi] * wi + im[hi] * wr;
                let ur = re[lo];
                let ui = im[lo];
                re[lo] = ur + vr;
                im[lo] = ui + vi;
                re[hi] = ur - vr;
                im[hi] = ui - vi;
            }
            base += len;
        }
        len <<= 1;
    }
}

/// Power per bin from DC to Nyquist, normalised so the bins sum to the
/// mean-square energy of the windowed signal.
///
/// Bins other than DC and Nyquist carry their negative-frequency twin as well,
/// which is what makes a full-range set of bands sum to 100%.
fn power_spectrum(samples: &[f32]) -> Vec<f64> {
    let n = samples.len();
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];

    // Hann window. The firing period does not divide the window, so without a
    // window the discontinuity at the wrap leaks energy across every bin and
    // smears it upward — which would fabricate exactly the high-frequency
    // content this probe exists to measure.
    for (i, sample) in samples.iter().enumerate() {
        let w = 0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / n as f64).cos());
        re[i] = f64::from(*sample) * w;
    }

    fft(&mut re, &mut im);

    (0..=n / 2)
        .map(|k| {
            let fold = if k == 0 || k == n / 2 { 1.0 } else { 2.0 };
            fold * (re[k] * re[k] + im[k] * im[k]) / n as f64
        })
        .collect()
}

/// Energy in `[lo_hz, hi_hz)`, by bin centre.
fn band_energy(power: &[f64], lo_hz: f64, hi_hz: f64) -> f64 {
    let bin_hz = SAMPLE_RATE_HZ / ((power.len() - 1) * 2) as f64;
    power
        .iter()
        .enumerate()
        .filter(|(k, _)| {
            let f = *k as f64 * bin_hz;
            f >= lo_hz && f < hi_hz
        })
        .map(|(_, p)| p)
        .sum()
}

/// Half-width, in bins, of the window a spectral line is collected over.
///
/// The Hann window's main lobe is two bins either side of the line, so a
/// narrower window would report a fraction of a peak that is genuinely there and
/// a wider one would start counting the noise between the orders as if it were
/// an order.
const LINE_HALF_WIDTH_BINS: usize = 2;

/// Energy in the main lobe centred on `hz`.
fn line_energy(power: &[f64], hz: f64) -> f64 {
    let bin_hz = SAMPLE_RATE_HZ / ((power.len() - 1) * 2) as f64;
    let centre = (hz / bin_hz).round() as isize;
    let half = LINE_HALF_WIDTH_BINS as isize;
    (centre - half..=centre + half)
        .filter(|k| *k >= 0 && (*k as usize) < power.len())
        .map(|k| power[k as usize])
        .sum()
}

/// Firing frequency: an engine fires `cylinders` times per two revolutions.
fn firing_hz(rpm: f64, cylinders: usize) -> f64 {
    rpm / 60.0 * cylinders as f64 * 0.5
}

/// Share of the total energy sitting in the first `orders` firing orders.
///
/// This is the measurement band shares cannot make. A diesel puts its energy
/// into a comb at its firing frequency; an electric motor, a resonance being
/// rung, and a filtered noise floor all put it somewhere else. A spectrum can
/// satisfy every band target ever written and still have nothing at `f0` or its
/// first few multiples, which is exactly what "sounds like a motorbike rather
/// than a truck" turned out to mean.
fn comb_share(power: &[f64], f0_hz: f64, orders: usize) -> f64 {
    let total: f64 = power.iter().sum();
    let comb: f64 = (1..=orders)
        .map(|n| line_energy(power, f0_hz * n as f64))
        .sum();
    100.0 * comb / total.max(1e-30)
}

/// Peak over RMS, in decibels.
///
/// A sine is 3.0 dB and a pulse train is well into double figures, so this
/// separates "a tone at roughly the right pitch" from "a series of distinct
/// combustion events" without any reference to where the energy sits. Both can
/// hold identical band shares.
fn crest_db(samples: &[f32]) -> f64 {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let rms = rms_of(samples);
    if rms <= 1e-30 {
        return 0.0;
    }
    20.0 * (f64::from(peak) / rms).log10()
}

/// How deeply the signal's envelope is modulated at the firing rate.
///
/// Rectify, low-pass to an envelope, transform that envelope, and compare the
/// amplitude at `f0` and `2 f0` against the envelope's mean. A steady tone gives
/// nearly zero however loud it is; a train of distinct firing events gives a
/// large number. This is the other half of what a band share cannot see, and
/// between them they are the difference between an engine and a buzz.
///
/// The detector's corner is set at four times the firing rate so it follows the
/// pulses without following the carrier: too low and it smooths the modulation
/// being measured away, too high and it passes the waveform itself.
fn modulation_depth(samples: &[f32], f0_hz: f64) -> f64 {
    let corner_hz = (4.0 * f0_hz).clamp(20.0, 1_000.0);
    let dt = 1.0 / SAMPLE_RATE_HZ;
    let rc = 1.0 / (std::f64::consts::TAU * corner_hz);
    let alpha = dt / (rc + dt);

    let mut envelope = vec![0.0f32; samples.len()];
    let mut y = 0.0f64;
    for (slot, sample) in envelope.iter_mut().zip(samples) {
        y += (f64::from(sample.abs()) - y) * alpha;
        *slot = y as f32;
    }

    let power = power_spectrum(&envelope);
    // Bins 0..=2 are the window's main lobe at DC, which is the mean envelope -
    // the level the modulation is being measured against.
    let mean: f64 = power.iter().take(LINE_HALF_WIDTH_BINS + 1).sum();
    let modulated = line_energy(&power, f0_hz) + line_energy(&power, 2.0 * f0_hz);
    (modulated / mean.max(1e-30)).sqrt()
}

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

/// The shipped configuration with one `audio` gain zeroed, to hear one path.
///
/// The balance between the exhaust and the structure is the single thing that
/// decides whether this reads as a truck or as a generic motor, and it cannot be
/// judged from the mixed spectrum alone: a band share moves when *either* path
/// changes level, so the mixed table cannot say which one moved. Silencing one
/// gain and measuring the other is the same trick `tests/acoustics.rs` uses, and
/// it turns the balance into a number rather than an impression.
fn config_with_only(gain: &str) -> Result<ValidatedConfig, Box<dyn std::error::Error>> {
    let mut document: serde_json::Value = serde_json::from_str(OM471_9_M3D_JSON)?;
    for other in ["exhaust_gain", "structural_gain", "body_gain"] {
        if other != gain {
            document["audio"][other] = serde_json::json!(0.0);
        }
    }
    Ok(EngineConfig::from_json(&document.to_string())?.validate()?)
}

/// Settle a simulation at an operating point and return its produced samples.
fn trace_at(
    config: &ValidatedConfig,
    rpm: f64,
    pedal: f64,
    samples: usize,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut sim = Simulation::new(
        config.clone(),
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )?;
    sim.set_controls(Controls {
        pedal,
        load_torque_nm: 0.0,
        starter: false,
        ignition: true,
        egr_enabled: true,
        ..Controls::default()
    })?;

    let mut sink = vec![0.0f32; PIN_CHUNK as usize];
    // Settle, discarding the start-up transient.
    for _ in 0..2_000 {
        sim.advance(PIN_CHUNK)?;
        sim.pin_speed_rpm(rpm)?;
        sim.drain_audio(&mut sink);
    }

    let mut trace = vec![0.0f32; samples];
    let mut written = 0;
    while written < trace.len() {
        let batch = PIN_CHUNK.min((trace.len() - written) as u32);
        sim.advance(batch)?;
        sim.pin_speed_rpm(rpm)?;
        written += sim.drain_audio(&mut trace[written..]);
    }
    Ok(trace)
}

/// RMS of a trace, in dBFS.
fn rms_of(samples: &[f32]) -> f64 {
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len().max(1) as f64).sqrt()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: ValidatedConfig = EngineConfig::from_json(OM471_9_M3D_JSON)?.validate()?;
    let knee = config.config().audio.soft_clip_knee;
    let cylinders = config.config().geometry.cylinders;
    let exhaust_only = config_with_only("exhaust_gain")?;
    let structural_only = config_with_only("structural_gain")?;
    let body_only = config_with_only("body_gain")?;

    println!("exhaust audio levels (dBFS, not sound pressure)");
    println!(
        "{:>8} {:>7} {:>6} {:>9} {:>9} {:>8}",
        "point", "rpm", "pedal", "peak", "rms", "dBFS"
    );

    for (label, rpm, pedal) in [
        ("idle", 600.0, 0.15),
        ("cruise", 1_200.0, 0.60),
        ("full", 1_400.0, 1.00),
    ] {
        let mut sim = Simulation::new(
            config.clone(),
            ResetOptions {
                seed: 0,
                initial_rpm: rpm,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
        )?;
        sim.set_controls(Controls {
            pedal,
            load_torque_nm: 0.0,
            starter: false,
            ignition: true,
            egr_enabled: true,
            ..Controls::default()
        })?;

        let mut sink = vec![0.0f32; PIN_CHUNK as usize];

        // Settle, discarding the start-up transient.
        for _ in 0..2_000 {
            sim.advance(PIN_CHUNK)?;
            sim.pin_speed_rpm(rpm)?;
            sim.drain_audio(&mut sink);
        }

        let mut peak = 0.0f32;
        let mut sum_squares = 0.0f64;
        let mut count = 0usize;
        for _ in 0..400 {
            sim.advance(PIN_CHUNK)?;
            sim.pin_speed_rpm(rpm)?;
            let written = sim.drain_audio(&mut sink);
            for sample in &sink[..written] {
                peak = peak.max(sample.abs());
                sum_squares += f64::from(*sample) * f64::from(*sample);
                count += 1;
            }
        }

        let rms = (sum_squares / count as f64).sqrt();
        println!(
            "{label:>8} {rpm:>7.0} {pedal:>6.2} {peak:>9.4} {rms:>9.4} {:>8.1}",
            20.0 * rms.log10()
        );

        // Where the energy sits matters as much as how much there is: small
        // speakers reproduce almost nothing below about 150 Hz, and a
        // six-cylinder diesel at idle has a 28 Hz fundamental.
        let mut trace = vec![0.0f32; WINDOW];
        let mut written = 0;
        while written < trace.len() {
            let batch = PIN_CHUNK.min((trace.len() - written) as u32);
            sim.advance(batch)?;
            sim.pin_speed_rpm(rpm)?;
            written += sim.drain_audio(&mut trace[written..]);
        }

        let power = power_spectrum(&trace);
        let total: f64 = power.iter().sum();
        let share = |lo, hi| 100.0 * band_energy(&power, lo, hi) / total.max(1e-30);

        let mut checksum = 0.0;
        for (name, lo, hi) in BANDS {
            let pct = share(lo, hi);
            checksum += pct;
            println!("           {name:>8}: {pct:>5.1}%");
        }
        // The bands partition the spectrum, so this must read 100.0. It is the
        // probe checking its own normalisation rather than a claim about audio.
        println!("           {:>8}: {checksum:>5.1}%  (must be 100.0)", "sum");

        // Character, which is the part band shares are blind to. Two signals can
        // hold identical shares in every band above and be a diesel and a
        // doorbell respectively; these three say which.
        let f0 = firing_hz(rpm, cylinders);
        println!(
            "           {:>8}: {:>5.1} Hz   orders f0..4f0 {:>5.1}%  crest {:>5.1} dB  \
             modulation {:>5.2}",
            "firing",
            f0,
            comb_share(&power, f0, 4),
            crest_db(&trace),
            modulation_depth(&trace, f0),
        );

        // The acceptance bands stop at 15 kHz rather than running to Nyquist.
        // The explicit port transfer leaves a two-sample limit cycle near
        // equilibrium, which lands within a whisker of Nyquist and is arithmetic
        // rather than sound. Measuring to Nyquist would let that residue satisfy
        // a high-frequency target it is not signal for; the `>15kHz` line below
        // reports it separately so it stays visible without being counted.
        // 150 Hz is where a small speaker starts reproducing anything at all, so
        // it is the boundary the "can this be heard on ordinary hardware"
        // criterion actually means. The 500 Hz line is kept beside it because it
        // is the one the earlier milestones were written against, but it stopped
        // measuring that question once the block's bending mode landed at 480 Hz:
        // energy at 480 Hz plays perfectly well on a laptop and counts as failure
        // on that boundary alone.
        println!(
            "           {:>8}: {:>5.2}%  (acceptance band)",
            "150-15k",
            share(150.0, 15_000.0)
        );
        println!(
            "           {:>8}: {:>5.2}%",
            "500-15k",
            share(500.0, 15_000.0)
        );
        println!(
            "           {:>8}: {:>5.2}%  (acceptance band)",
            "2k-15k",
            share(2_000.0, 15_000.0)
        );

        // Octave shape, and the Nyquist question. A two-sample limit cycle from
        // the explicit port transfer would appear as energy piling up in the top
        // octave rather than as harmonics decaying away from the firing orders.
        let octaves: Vec<(&str, f64)> = OCTAVES
            .iter()
            .map(|(name, lo, hi)| (*name, share(*lo, *hi)))
            .collect();
        let loudest = octaves.iter().map(|(_, p)| *p).fold(0.0f64, f64::max);
        println!("           spectrum, dB relative to the strongest octave:");
        for (name, pct) in &octaves {
            let db = 10.0 * (pct / loudest.max(1e-30)).max(1e-12).log10();
            let bars = ((60.0 + db) / 2.0).max(0.0).round() as usize;
            println!(
                "           {name:>8}: {db:>6.1} dB {:<30} {pct:>5.2}%",
                "#".repeat(bars)
            );
        }
        println!(
            "           {:>8}: {:>5.3}%  (near-Nyquist residue)",
            ">15kHz",
            share(15_000.0, f64::INFINITY)
        );

        // The two paths on their own, at the same operating point. What this is
        // for is the balance: the exhaust should carry the low orders and the
        // modal bank should sit above them, and a mixed spectrum cannot say
        // which of the two moved when a band share changes.
        println!("           each path alone (dBFS, and its own band shares):");
        let mut levels_db: Vec<(&str, f64)> = Vec::new();
        for (path, path_config) in [
            ("exhaust", &exhaust_only),
            ("block", &structural_only),
            ("body", &body_only),
        ] {
            let trace = trace_at(path_config, rpm, pedal, WINDOW)?;
            let level = rms_of(&trace);
            let power = power_spectrum(&trace);
            let total: f64 = power.iter().sum();
            let bands: Vec<String> = BANDS
                .iter()
                .map(|(name, lo, hi)| {
                    let pct = 100.0 * band_energy(&power, *lo, *hi) / total.max(1e-30);
                    format!("{name} {pct:.1}%")
                })
                .collect();
            let level_db = 20.0 * level.max(1e-30).log10();
            levels_db.push((path, level_db));
            println!(
                "           {path:>8}: {level_db:>6.1} dBFS   {}   orders {:>4.1}%",
                bands.join("  "),
                comb_share(&power, f0, 4),
            );
        }
        // The balance as a signed number rather than as two lines to subtract in
        // your head. This is the single figure that decides whether the result
        // reads as a truck or as a generic motor: the exhaust carries the firing
        // orders and the block carries the clatter, so a block sitting in front
        // of the exhaust is a small engine however the bands come out.
        if let [(_, exhaust_db), (_, block_db), (_, body_db)] = levels_db[..] {
            println!(
                "           {:>8}: exhaust leads block by {:>5.1} dB, body by {:>5.1} dB",
                "balance",
                exhaust_db - block_db,
                exhaust_db - body_db
            );
        }
        println!();
    }

    println!(
        "soft-clip knee {knee:.2}: full load should approach it, idle should stay \
         well below and still be audible"
    );
    Ok(())
}
