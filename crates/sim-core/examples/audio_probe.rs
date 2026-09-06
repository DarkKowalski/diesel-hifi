//! Calibration probe: report exhaust audio levels at a few operating points.
//!
//! `cargo run --release -p sim-core --example audio_probe`
//!
//! The sweep example checks that the *engine* is calibrated. This checks that
//! the exhaust signal is at a usable level, which is a separate question and one
//! the test suite cannot answer on its own: a test can assert samples are finite
//! and bounded without noticing that they are 50 dB below anything audible.
//!
//! Levels here are dBFS — decibels relative to full scale on the output signal —
//! not sound pressure. Nothing about how loud the real engine is is published or
//! claimed.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

/// Steps between re-pinning the crank. Long batches let an unloaded engine
/// accelerate away from the speed being measured.
const PIN_CHUNK: u32 = 100;

/// Energy in a frequency band, by direct Goertzel-style summation over bins.
fn band_energy(samples: &[f32], sample_rate_hz: f64, lo_hz: f64, hi_hz: f64) -> f64 {
    let n = samples.len();
    let bin_hz = sample_rate_hz / n as f64;
    let lo_bin = (lo_hz / bin_hz).ceil() as usize;
    let hi_bin = ((hi_hz / bin_hz).floor() as usize).min(n / 2);

    let mut energy = 0.0;
    for k in lo_bin..=hi_bin {
        let mut re = 0.0;
        let mut im = 0.0;
        let w = std::f64::consts::TAU * k as f64 / n as f64;
        for (i, sample) in samples.iter().enumerate() {
            let angle = w * i as f64;
            re += f64::from(*sample) * angle.cos();
            im -= f64::from(*sample) * angle.sin();
        }
        energy += (re * re + im * im) / n as f64;
    }
    energy
}

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
        let mut trace = vec![0.0f32; 40_000];
        let mut written = 0;
        while written < trace.len() {
            let batch = PIN_CHUNK.min((trace.len() - written) as u32);
            sim.advance(batch)?;
            sim.pin_speed_rpm(rpm)?;
            written += sim.drain_audio(&mut trace[written..]);
        }
        let total: f64 = trace.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        for (name, lo, hi) in [
            ("<80Hz", 0.0, 80.0),
            ("80-300", 80.0, 300.0),
            ("300-2k", 300.0, 2000.0),
            (">2kHz", 2000.0, 20_000.0),
        ] {
            let energy = band_energy(&trace, 40_000.0, lo, hi);
            println!(
                "           {name:>8}: {:>5.1}%",
                100.0 * energy / total.max(1e-30)
            );
        }
    }

    println!(
        "\nsoft-clip knee {knee:.2}: full load should approach it, idle should stay \
         well below and still be audible"
    );
    Ok(())
}
