//! Export repeatable scenarios as float WAVs with configuration and event metadata.
//! Each scenario contains mix.wav, four-channel paths.wav and non-silent mono paths.
//! When limiting occurs, gain-recovered tracks are also exported.
//! `pnpm audio:render <capture directory>` renders the raw/cockpit comparison
//! through the shipped browser graph at a common output rate.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sim_core::analysis::{crest_db, dbfs, firing_hz, modulation_depth, peak, rms, Spectrum};
use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::scenario::{self, Scenario, ScenarioRun};
use sim_core::{EngineConfig, ValidatedConfig};

/// Where captures land unless `--out` says otherwise.
///
/// Under `target/`, which is already ignored: a capture is a build artefact of a
/// particular working tree, not something to commit and compare by eye later.
const DEFAULT_OUT: &str = "target/audio-capture";

const PATH_NAMES: [&str; 4] = ["exhaust", "block", "body", "starter"];

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
    write_multichannel_wav(path, samples, sample_rate_hz, 1)
}

fn write_multichannel_wav(
    path: &Path,
    samples: &[f32],
    sample_rate_hz: f64,
    channels: u16,
) -> std::io::Result<()> {
    let rate = sample_rate_hz.round() as u32;
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

/// What the limiter did to a run, and the signal it did it to.
struct Unsaturated {
    /// Each radiating path with the limiter's common gain divided back out.
    tracks: Vec<Vec<f32>>,
    /// Those paths summed: the mix as the solver produced it, pre-limiter.
    mix: Vec<f32>,
    /// The smallest gain seen. 1.0 means the limiter never engaged.
    smallest_gain: f64,
}

/// Divide the limiter's common gain back out of every path.
///
/// The limiter decides one gain from the mix and largest individual path and applies it to all four paths,
/// so dividing each sample by the gain its frame carries recovers exactly what
/// the solver produced before the limiter engaged. The gains come from the run
/// rather than from arithmetic on the output: the follower has a release time,
/// so its gain depends on the whole passage and not on the sample it scaled.
///
/// The recovered mix is returned as well as the recovered paths, because *how
/// far* the limiter engaged does not say what it cost. A gain of 0.247 that
/// moves slowly is a level reduction and preserves the waveform; the same figure
/// applied per sample is a waveshaper. The difference shows up as crest factor,
/// and only measuring both signals tells them apart.
fn unsaturate(run: &ScenarioRun) -> Unsaturated {
    let frames = run.frame_count();
    let mut tracks = vec![Vec::with_capacity(frames); run.paths];
    let mut mix = Vec::with_capacity(frames);
    let mut smallest = 1.0f64;
    for (index, frame) in run.frames.chunks(run.paths).enumerate() {
        let gain = run
            .limiter_gains
            .get(index)
            .map_or(1.0, |g| f64::from(*g))
            .max(1e-9);
        smallest = smallest.min(gain);
        let mut recovered_sum = 0.0f64;
        for (path, sample) in frame.iter().enumerate() {
            let value = f64::from(*sample) / gain;
            recovered_sum += value;
            tracks[path].push(value as f32);
        }
        mix.push(recovered_sum as f32);
    }
    Unsaturated {
        tracks,
        mix,
        smallest_gain: smallest,
    }
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
    write_multichannel_wav(&dir.join("paths.wav"), &run.frames, rate, run.paths as u16)?;

    let mut tracks = serde_json::Map::new();
    tracks.insert("mix".into(), measure(&mix, rate, f0));
    let mut silent: Vec<&str> = Vec::new();
    for (index, name) in PATH_NAMES.iter().enumerate().take(run.paths) {
        let samples = run.path(index);
        // No file for a path that is exactly silent, on the same grounds the
        // unsaturated set is conditional below: a `starter.wav` full of zeros in
        // every steady capture directory is a file someone will open and draw a
        // conclusion from. The manifest says which paths were silent instead,
        // which is the information without the artefact.
        if samples.iter().all(|s| *s == 0.0) {
            silent.push(name);
            continue;
        }
        write_wav(&dir.join(format!("{name}.wav")), &samples, rate)?;
        tracks.insert((*name).to_string(), measure(&samples, rate, f0));
    }

    // Only when the limiter actually did something. A set of files identical to
    // the ones beside them is a set of files someone will later compare and draw
    // a conclusion from.
    let recovered = unsaturate(&run);
    let smallest_gain = recovered.smallest_gain;
    let saturated = smallest_gain < 0.999;
    if saturated {
        for (index, name) in PATH_NAMES.iter().enumerate().take(run.paths) {
            if silent.contains(name) {
                continue;
            }
            write_wav(
                &dir.join(format!("{name}-unsaturated.wav")),
                &recovered.tracks[index],
                rate,
            )?;
        }
        write_wav(&dir.join("mix-unsaturated.wav"), &recovered.mix, rate)?;
        tracks.insert("mix-unsaturated".into(), measure(&recovered.mix, rate, f0));
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
                "ignition": p.ignition,
                "starterRequested": p.starter_requested,
                "starterCurrentMeanA": round(p.starter_current_mean_a, 1),
                "starterCurrentMaxA": round(p.starter_current_max_a, 1),
                "starterTerminalMinV": round(p.starter_terminal_min_v, 2),
                "starterRotorMaxRadPerS": round(p.starter_rotor_max_rad_per_s, 2),
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
            format!(
                "  limiter to {smallest_gain:.3}, crest was {:.1} dB",
                crest_db(&recovered.mix)
            )
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
        // Which paths had nothing in them at all, so a missing WAV is a
        // statement rather than an omission.
        "silentPaths": silent,
        "chunkSteps": scenario::CHUNK_STEPS,
        "frames": run.frame_count(),
        "durationS": round(run.frame_count() as f64 / rate, 4),
        "capturedRpm": round(run.captured_rpm(), 1),
        "firingHz": f0.map(|f0| round(f0, 2)),
        "softClipKnee": knee,
        "limiterMinimumGain": round(smallest_gain, 4),
        "unsaturatedTracksWritten": saturated,
        "phases": phases,
        "events": run.events,
        "eventResolutionS": { "first-combustion": config.config().solver.fixed_step_s, "starter": scenario::CHUNK_STEPS as f64 / rate },
        "tracks": tracks,
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out = PathBuf::from(DEFAULT_OUT);
    let mut only: Option<Vec<String>> = None;
    let mut config_json = OM471_9_M3D_JSON.to_string();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = PathBuf::from(args.next().ok_or("--out needs a directory")?);
            }
            "--config" => {
                config_json = fs::read_to_string(args.next().ok_or("--config needs a JSON path")?)?;
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

    let config: ValidatedConfig = EngineConfig::from_json(&config_json)?.validate()?;
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
    manifest.insert(
        "configSchemaVersion",
        serde_json::json!(config.config().schema_version),
    );
    // Explicit stable checksum of the exact configuration bytes, not a security hash.
    let checksum = config_json
        .bytes()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    manifest.insert(
        "configFnv1a64",
        serde_json::json!(format!("{checksum:016x}")),
    );
    fs::write(out.join("config.json"), &config_json)?;
    manifest.insert("scenarios", serde_json::json!(runs));
    let path = out.join("manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    println!("manifest written to {}", path.display());

    Ok(())
}
