//! Export named scenarios as WAV files, with a manifest of the conditions.
//!
//! ```bash
//! cargo run --release -p sim-core --example audio_capture
//! cargo run --release -p sim-core --example audio_capture -- --out captures --only idle,full-1400
//! ```
//!
//! This is the tool a listening comparison is made *from*. Without it, comparing
//! two versions of the sound means reproducing a pedal movement by hand twice
//! and trusting that they matched; with it, `idle` is a file, regenerated from a
//! seed and a script, and two of them differ only where the model does.
//!
//! Writing files is why this is an example rather than library code. `sim-core`
//! touches no filesystem API; [`sim_core::scenario`] runs the engine and hands
//! back samples, and everything below turns those into bytes.
//!
//! ## What gets written
//!
//! Per scenario, into `<out>/<id>/`:
//!
//! | File | What it is |
//! |---|---|
//! | `mix.wav` | the three paths added up: the raw listening stage |
//! | `exhaust.wav`, `block.wav`, `body.wav` | each radiating path on its own |
//! | `*-unsaturated.wav` | the same paths with the limiter's gain divided out |
//! | `manifest.json` (at the root) | conditions, and every metric, for every run |
//!
//! The unsaturated set is written only for scenarios where the limiter actually
//! engaged, and the manifest says how far it engaged. It exists because a pulse
//! shape is the timbre and a limiter changes pulse shapes: comparing sources
//! through a stage that is squashing them differently in the two runs compares
//! the squashing.
//!
//! **There is no cockpit-stage file.** The cab is a Web Audio graph in
//! `web/src/lib/cabin.ts`, and reimplementing it here to export it would be a
//! second copy of a listening stage that would immediately start drifting from
//! the one people actually hear. Cockpit comparison happens in the browser,
//! against these files, through the comparison controls in the sound panel.
//!
//! ## Float WAV, not 16-bit
//!
//! The samples are `f32` in `[-1, 1]` and are written as IEEE float, because
//! quantising to 16 bits would put a dither decision — or a truncation
//! artefact — between the solver and the comparison. Every ordinary audio tool
//! reads float WAV.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sim_core::analysis::{
    crest_db, dbfs, firing_hz, modulation_depth, peak, rms, saturation_gain, Spectrum,
};
use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::scenario::{self, Scenario, ScenarioRun};
use sim_core::{EngineConfig, ValidatedConfig};

/// Where captures land unless `--out` says otherwise.
///
/// Under `target/`, which is already ignored: a capture is a build artefact of a
/// particular working tree, not something to commit and compare by eye later.
const DEFAULT_OUT: &str = "target/audio-capture";

const PATH_NAMES: [&str; 3] = ["exhaust", "block", "body"];

/// Bands the acceptance criteria are written against, and which the manifest
/// records for every track so a comparison has them without re-analysing.
const BANDS: [(&str, f64, f64); 4] = [
    ("below80Hz", 0.0, 80.0),
    ("80to300Hz", 80.0, 300.0),
    ("300to2kHz", 300.0, 2_000.0),
    ("above2kHz", 2_000.0, f64::INFINITY),
];

/// Write mono 32-bit float WAV.
///
/// Hand-rolled because it is a 44-byte header and this crate takes no
/// dependencies beyond serialization.
fn write_wav(path: &Path, samples: &[f32], sample_rate_hz: f64) -> std::io::Result<()> {
    let rate = sample_rate_hz.round() as u32;
    let channels: u16 = 1;
    let bits: u16 = 32;
    let block_align = channels * bits / 8;
    let byte_rate = rate * u32::from(block_align);
    let data_bytes = (samples.len() * 4) as u32;

    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    // 3 is WAVE_FORMAT_IEEE_FLOAT. 1 would be PCM and a lie about what follows.
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }

    let mut file = fs::File::create(path)?;
    file.write_all(&out)
}

/// Every metric the comparison workflow reads, for one track.
///
/// `f0_hz` is `None` for a transient. The firing-rate metrics need a firing rate
/// that stands still for the length of the window, and a run-down passes through
/// every speed below idle: a comb share against the *mean* speed of a sweep is a
/// measurement of a frequency the engine was at for a fraction of the capture.
/// Reporting it as null is the honest answer; reporting a number invites it to
/// be compared with a steady one.
fn measure(samples: &[f32], sample_rate_hz: f64, f0_hz: Option<f64>) -> serde_json::Value {
    let spectrum = Spectrum::of(samples, sample_rate_hz);
    let mut bands = serde_json::Map::new();
    for (name, lo, hi) in BANDS {
        bands.insert(
            name.to_string(),
            serde_json::json!(round(spectrum.band_share(lo, hi), 2)),
        );
    }
    serde_json::json!({
        "peak": round(peak(samples), 4),
        "rms": round(rms(samples), 5),
        "dbfs": round(dbfs(rms(samples)), 2),
        "crestDb": round(crest_db(samples), 2),
        "modulationDepth": f0_hz
            .map(|f0| round(modulation_depth(samples, sample_rate_hz, f0), 3)),
        "firingOrdersPercent": f0_hz.map(|f0| round(spectrum.comb_share(f0, 4), 2)),
        "bandSharePercent": bands,
        "audibleBandPercent": round(spectrum.band_share(150.0, 15_000.0), 2),
        "aboveFifteenKilohertzPercent": round(spectrum.band_share(15_000.0, f64::INFINITY), 4),
        "loudestOctaveHz": round(spectrum.dominant_hz(20.0, 20_000.0), 1),
    })
}

/// Round for the manifest. Sixteen digits of a measurement that repeats to
/// three is noise in a diff.
fn round(value: f64, places: u32) -> f64 {
    let scale = 10f64.powi(places as i32);
    (value * scale).round() / scale
}

/// Divide the limiter's common gain back out of every path.
///
/// The clipper acts on the mix and is applied to the three paths as one gain, so
/// the mix — which is what was emitted — is enough to recover it. See
/// [`sim_core::analysis::saturation_gain`].
///
/// Returns the recovered tracks and the smallest gain seen, which is how far the
/// limiter engaged: 1.0 means it never did.
fn unsaturate(run: &ScenarioRun, knee: f64) -> (Vec<Vec<f32>>, f64) {
    let mut tracks = vec![Vec::with_capacity(run.frame_count()); run.paths];
    let mut smallest = 1.0f64;
    for frame in run.frames.chunks(run.paths) {
        let sum: f64 = frame.iter().map(|s| f64::from(*s)).sum();
        let gain = saturation_gain(sum, knee);
        smallest = smallest.min(gain);
        for (path, sample) in frame.iter().enumerate() {
            tracks[path].push((f64::from(*sample) / gain.max(1e-9)) as f32);
        }
    }
    (tracks, smallest)
}

fn capture(
    config: &ValidatedConfig,
    scenario: &Scenario,
    out: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let run = scenario::run(config, scenario)?;
    if run.dropped_frames > 0 {
        return Err(format!(
            "{}: the ring dropped {} frames, so this capture has a hole in it",
            scenario.id, run.dropped_frames
        )
        .into());
    }

    let dir = out.join(scenario.id);
    fs::create_dir_all(&dir)?;

    let cylinders = config.config().geometry.cylinders;
    let knee = config.config().audio.soft_clip_knee;
    let rate = run.sample_rate_hz;
    let f0 = scenario
        .steady
        .then(|| firing_hz(run.captured_rpm(), cylinders));

    let mix = run.summed();
    write_wav(&dir.join("mix.wav"), &mix, rate)?;

    let mut tracks = serde_json::Map::new();
    tracks.insert("mix".into(), measure(&mix, rate, f0));
    for (index, name) in PATH_NAMES.iter().enumerate().take(run.paths) {
        let samples = run.path(index);
        write_wav(&dir.join(format!("{name}.wav")), &samples, rate)?;
        tracks.insert((*name).to_string(), measure(&samples, rate, f0));
    }

    // Only when the limiter actually did something. A set of files identical to
    // the ones beside them is a set of files someone will later compare and draw
    // a conclusion from.
    let (recovered, smallest_gain) = unsaturate(&run, knee);
    let saturated = smallest_gain < 0.999;
    if saturated {
        for (index, name) in PATH_NAMES.iter().enumerate().take(run.paths) {
            write_wav(
                &dir.join(format!("{name}-unsaturated.wav")),
                &recovered[index],
                rate,
            )?;
        }
    }

    let phases: Vec<serde_json::Value> = run
        .phases
        .iter()
        .map(|p| {
            serde_json::json!({
                "label": p.label,
                "captured": p.captured,
                "heldSpeed": p.held,
                "frames": p.frames,
                "pedal": p.pedal,
                "loadTorqueNm": p.load_torque_nm,
                "brakeStage": p.brake_stage,
                "rpmStart": round(p.rpm_start, 1),
                "rpmEnd": round(p.rpm_end, 1),
                "rpmMin": round(p.rpm_min, 1),
                "rpmMax": round(p.rpm_max, 1),
                "rpmMean": round(p.rpm_mean, 1),
                "brakeTorqueNm": round(p.brake_torque_nm, 1),
            })
        })
        .collect();

    let character = match f0 {
        Some(f0) => format!(
            "modulation {:>5.2}  orders {:>5.1}%",
            modulation_depth(&mix, rate, f0),
            Spectrum::of(&mix, rate).comb_share(f0, 4),
        ),
        // A sweep has no single firing rate to measure against.
        None => format!("{:>27}", "transient, no steady f0"),
    };
    println!(
        "{:>12}  {:>6.0} rpm  {:>6.2} s  {:>7.1} dBFS  crest {:>5.1} dB  {character}{}",
        scenario.id,
        run.captured_rpm(),
        run.frame_count() as f64 / rate,
        dbfs(rms(&mix)),
        crest_db(&mix),
        if saturated {
            format!("  limiter to {smallest_gain:.3}")
        } else {
            String::new()
        }
    );

    Ok(serde_json::json!({
        "id": scenario.id,
        "summary": scenario.summary,
        "steady": scenario.steady,
        "configId": run.config_id,
        "seed": run.seed,
        "sampleRateHz": rate,
        "paths": PATH_NAMES[..run.paths].to_vec(),
        "chunkSteps": scenario::CHUNK_STEPS,
        "frames": run.frame_count(),
        "durationS": round(run.frame_count() as f64 / rate, 4),
        "capturedRpm": round(run.captured_rpm(), 1),
        "firingHz": f0.map(|f0| round(f0, 2)),
        "softClipKnee": knee,
        "limiterMinimumGain": round(smallest_gain, 4),
        "unsaturatedTracksWritten": saturated,
        "phases": phases,
        "tracks": tracks,
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out = PathBuf::from(DEFAULT_OUT);
    let mut only: Option<Vec<String>> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = PathBuf::from(args.next().ok_or("--out needs a directory")?);
            }
            "--only" => {
                only = Some(
                    args.next()
                        .ok_or("--only needs a comma-separated list of scenario IDs")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect(),
                );
            }
            "--list" => {
                for scenario in scenario::all() {
                    println!(
                        "{:>12}  {:>5}  {:>5.1} s  {}",
                        scenario.id,
                        if scenario.steady { "steady" } else { "trans" },
                        scenario.capture_s(),
                        scenario.summary
                    );
                }
                return Ok(());
            }
            other => return Err(format!("unrecognised argument {other}").into()),
        }
    }

    let config: ValidatedConfig = EngineConfig::from_json(OM471_9_M3D_JSON)?.validate()?;
    fs::create_dir_all(&out)?;

    let scenarios: Vec<Scenario> = scenario::all()
        .into_iter()
        .filter(|s| match &only {
            Some(ids) => ids.iter().any(|id| id == s.id),
            None => true,
        })
        .collect();
    if scenarios.is_empty() {
        return Err("no scenario matched --only; try --list".into());
    }

    println!(
        "capturing {} scenarios into {}",
        scenarios.len(),
        out.display()
    );
    let mut runs = Vec::new();
    for scenario in &scenarios {
        runs.push(capture(&config, scenario, &out)?);
    }

    // Sorted so the manifest diffs cleanly between runs.
    let mut manifest = BTreeMap::new();
    manifest.insert("configId", serde_json::json!(config.config().identity.id));
    manifest.insert("apiVersion", serde_json::json!(sim_core::API_VERSION));
    manifest.insert("scenarios", serde_json::json!(runs));
    let path = out.join("manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    println!("manifest written to {}", path.display());

    Ok(())
}
