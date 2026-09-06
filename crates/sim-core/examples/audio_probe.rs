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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: ValidatedConfig = EngineConfig::from_json(OM471_9_M3D_JSON)?.validate()?;
    let knee = config.config().audio.soft_clip_knee;

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

        // The acceptance bands stop at 15 kHz rather than running to Nyquist.
        // The explicit port transfer leaves a two-sample limit cycle near
        // equilibrium, which lands within a whisker of Nyquist and is arithmetic
        // rather than sound. Measuring to Nyquist would let that residue satisfy
        // a high-frequency target it is not signal for; the `>15kHz` line below
        // reports it separately so it stays visible without being counted.
        println!(
            "           {:>8}: {:>5.2}%  (acceptance band)",
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
        println!();
    }

    println!(
        "soft-clip knee {knee:.2}: full load should approach it, idle should stay \
         well below and still be audible"
    );
    Ok(())
}
