//! Measure a recording with the instruments the model is measured with.
//!
//! ```bash
//! cargo run --release -p sim-core --example reference_probe -- \
//!     --file datasheet/audio-references/max2712-om471/interior-engine-01m33s-02m49s.wav
//! ```
//!
//! Milestone 8 built every instrument needed to compare the model against a
//! recording — named scenarios, exported captures, a metric library tested
//! against known signals, a level-matched A/B in the browser — and then pointed
//! all of them at the model. The recording was never measured. This points the
//! same metrics at the other side, so that for the first time the two sit on one
//! scale.
//!
//! ## What it does that the model's probe cannot
//!
//! **A recording does not come with an rpm reading.** `audio_probe` asks the
//! solver what speed the crank is turning at and derives `f0` from it; here the
//! firing rate has to be recovered from the signal, by
//! [`sim_core::analysis::estimate_firing_hz`], before any of the firing-rate
//! metrics can say anything at all. Every rpm this tool prints is therefore
//! **inferred**, and is printed beside the comb share that is its confidence.
//!
//! That confidence is not decoration. The material this was written for is a
//! game mix that also carries a road-and-wind noise mod, a gearbox and air-brake
//! chapter, and a horn; a window of tyre roar still has a best-scoring
//! candidate frequency, and would otherwise be annotated as an operating point.
//! Windows below `--min-comb` are reported and excluded from segments.
//!
//! ## What it is measuring
//!
//! Whatever WAV it is given, which is deliberate. Pointed at a reference
//! recording it produces the annotation phase 2 of the improvement plan asked
//! for; pointed at a `mix.wav` from `audio_capture` it re-derives the model's own
//! operating point from the audio alone, which is a check on the tool as much as
//! on the capture.
//!
//! It does **not** compare them. A reference figure and a model figure are not
//! measured at the same point in the chain — the model's are its raw pre-cab mix
//! and a recording has already been through a cab, a mix and a codec — and the
//! comparison belongs in the README with those limits stated, not inside a tool
//! that would have to state them again every time it printed a row.
//!
//! ## Filesystem
//!
//! Reading files is why this is an example rather than library code, exactly as
//! for `audio_capture`. `sim-core` touches no filesystem API;
//! [`sim_core::analysis`] measures slices of `f32` and knows nothing about where
//! they came from.
//!
//! The reference recordings live under the gitignored `datasheet/`, so for
//! anyone without them this tool has no input. A missing file is reported with
//! the command that would produce it rather than as a panic.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sim_core::analysis::{
    crest_db, dbfs, estimate_firing_hz, modulation_depth, peak, rms, rpm_from_firing_hz, Spectrum,
};

/// Where the annotation lands unless `--json` says otherwise.
///
/// Under `target/`, which is already ignored, for the same reason
/// `audio_capture` writes there: an annotation is a measurement of a particular
/// file with a particular tool, regenerable in one command.
const DEFAULT_JSON: &str = "target/reference-annotation/annotation.json";

/// Analysis window, in samples. 65536 is 1.365 s at 48 kHz.
///
/// A power of two, and stated in samples rather than seconds, because
/// [`Spectrum::of`] truncates to a power of two: asking for "one second" at
/// 48 kHz would quietly measure the first 0.68 s of it and report the answer as
/// though it covered the window.
const DEFAULT_WINDOW: usize = 65_536;

/// Slowest and fastest the search will consider, in rpm.
///
/// An OM 471 idles near 560 and is governed near 2000. The range is wider than
/// that at both ends so a real reading never sits against a bound — a value
/// pinned to the edge of its own search range is not a measurement.
const DEFAULT_RPM_MIN: f64 = 400.0;
const DEFAULT_RPM_MAX: f64 = 2_400.0;

/// Comb share below which a window is not treated as engine-dominated.
///
/// Set from what the metric library says about the wrong signals rather than
/// from the recording: `tests/analysis.rs` has white noise reading under 2% and
/// a pulse train well over 30%. Ten per cent is comfortably clear of noise and
/// well under anything with a firing comb in it, and it is a flag rather than a
/// filter — excluded windows are still printed, with their share, so the
/// exclusion can be argued with.
const DEFAULT_MIN_COMB: f64 = 10.0;

/// Share of the window's energy the **second** order must hold before the
/// window counts as engine-dominated.
///
/// A comb share on its own is not enough, and this reference is what showed it.
/// Its first three seconds are the transition out of the previous chapter: a
/// lone line near 108 Hz with nothing above it, which scored 17 to 26% comb —
/// clear of any noise floor — and was duly reported as a confident 2170 rpm. A
/// single spectral line satisfies a comb criterion by itself, because the
/// criterion cannot tell one line from four.
///
/// **A firing comb has more than one order in it.** So the second order has to
/// be there too, and the threshold is set where the two populations actually
/// separate: those transition windows hold at most 0.5% of their energy at the
/// second order, and every window with an engine in it holds at least 1.4%.
/// A factor of two either side of 1% is not a fine distinction.
///
/// It is a discriminator tuned on material, which is worth saying plainly, and
/// it is a flag rather than a filter for exactly that reason: excluded windows
/// are printed with their order shares so the call can be argued with.
const DEFAULT_MIN_SECOND_ORDER: f64 = 1.0;

/// How far two neighbouring windows' inferred speeds may differ and still be
/// called the same steady segment, in rpm.
///
/// A truck under way is never exactly steady, and the estimator has its own
/// resolution. 40 rpm is about 3% at idle and 2% at cruise.
const DEFAULT_RPM_TOLERANCE: f64 = 40.0;

/// The same four bands the acceptance criteria are written against, so a
/// reference row and a model row can be read down the same column.
const BANDS: [(&str, f64, f64); 4] = [
    ("<80Hz", 0.0, 80.0),
    ("80-300", 80.0, 300.0),
    ("300-2k", 300.0, 2_000.0),
    (">2kHz", 2_000.0, f64::INFINITY),
];

/// The model probe's octave set, for shape.
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

// --- reading a WAV ----------------------------------------------------------

/// A decoded file: interleaved samples, and what they are.
struct Wav {
    sample_rate_hz: f64,
    channels: usize,
    interleaved: Vec<f32>,
}

impl Wav {
    fn frames(&self) -> usize {
        self.interleaved.len() / self.channels.max(1)
    }

    fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.sample_rate_hz
    }

    /// The mono signal everything is measured on: the mean of the channels.
    fn mono(&self) -> Vec<f32> {
        if self.channels == 1 {
            return self.interleaved.clone();
        }
        let scale = 1.0 / self.channels as f32;
        self.interleaved
            .chunks(self.channels)
            .map(|frame| frame.iter().sum::<f32>() * scale)
            .collect()
    }

    /// Correlation between the first two channels, or 1.0 for mono.
    ///
    /// Reported because the downmix above is a sum, and a sum can cancel. A
    /// stereo file whose channels are largely out of phase would arrive here as
    /// a quiet, oddly-filtered mono signal that measures perfectly well and is
    /// not what anybody heard. A figure near 1 says the downmix is safe; a
    /// negative one says stop and measure the channels separately.
    fn channel_correlation(&self) -> f64 {
        if self.channels < 2 {
            return 1.0;
        }
        let (mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64);
        for frame in self.interleaved.chunks(self.channels) {
            let l = f64::from(frame[0]);
            let r = f64::from(frame[1]);
            ll += l * l;
            rr += r * r;
            lr += l * r;
        }
        let denominator = (ll * rr).sqrt();
        if denominator <= 1e-30 {
            return 1.0;
        }
        lr / denominator
    }
}

/// Read a 16-, 24- or 32-bit WAV, PCM or IEEE float, any channel count.
///
/// Hand-rolled, like `write_wav` in `audio_capture`, and for the same reason:
/// this crate takes no dependencies beyond serialization. It walks the chunk
/// list rather than assuming the canonical 44-byte header, because ffmpeg emits
/// a `LIST` chunk between `fmt ` and `data` and a fixed offset would read the
/// metadata as audio.
fn read_wav(path: &Path) -> Result<Wav, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{} is not a RIFF/WAVE file", path.display()).into());
    }

    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

    let mut format: Option<(u16, usize, f64, u16)> = None;
    let mut data: Option<(usize, usize)> = None;

    let mut cursor = 12usize;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32_at(cursor + 4) as usize;
        let body = cursor + 8;
        let end = body.saturating_add(size).min(bytes.len());
        if id == b"fmt " && size >= 16 {
            let mut tag = u16_at(body);
            let channels = u16_at(body + 2) as usize;
            let rate = f64::from(u32_at(body + 4));
            let bits = u16_at(body + 14);
            // WAVE_FORMAT_EXTENSIBLE carries the real tag in the first two bytes
            // of its subformat GUID.
            if tag == 0xFFFE && size >= 26 {
                tag = u16_at(body + 24);
            }
            format = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            data = Some((body, end));
        }
        // Chunks are word-aligned, and an odd size carries a pad byte.
        cursor = body + size + (size & 1);
    }

    let (tag, channels, sample_rate_hz, bits) =
        format.ok_or_else(|| format!("{}: no fmt chunk", path.display()))?;
    let (start, end) = data.ok_or_else(|| format!("{}: no data chunk", path.display()))?;
    if channels == 0 {
        return Err(format!("{}: zero channels", path.display()).into());
    }

    let body = &bytes[start..end];
    let interleaved: Vec<f32> = match (tag, bits) {
        // IEEE float, already in [-1, 1].
        (3, 32) => body
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        (1, 16) => body
            .chunks_exact(2)
            .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
            .collect(),
        (1, 24) => body
            .chunks_exact(3)
            .map(|b| {
                // Sign-extend by placing the three bytes in the top of an i32.
                let value = i32::from_le_bytes([0, b[0], b[1], b[2]]);
                value as f32 / 2_147_483_648.0
            })
            .collect(),
        (1, 32) => body
            .chunks_exact(4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0)
            .collect(),
        _ => {
            return Err(format!(
                "{}: unsupported format tag {tag} at {bits} bits. Convert it first: \
                 ffmpeg -i <in> -c:a pcm_f32le <out>.wav",
                path.display()
            )
            .into())
        }
    };

    Ok(Wav {
        sample_rate_hz,
        channels,
        interleaved,
    })
}

// --- measuring a window -----------------------------------------------------

/// One analysis window's worth of measurement.
struct Window {
    start_s: f64,
    dbfs: f64,
    /// `None` when the window holds no recoverable firing rate at all.
    f0_hz: Option<f64>,
    rpm: Option<f64>,
    /// Comb share at the estimate: the confidence in `rpm`.
    comb_share: f64,
    /// Share of the total energy at each of the first four orders separately.
    ///
    /// The aggregate comb share cannot distinguish a comb standing on its own
    /// fundamental from one whose energy is all in the third and fourth orders,
    /// and that distinction is exactly how a harmonic lock looks: an estimate
    /// with a hollow first order is an estimate that has taken some higher order
    /// for the firing rate. Printed rather than merely used, because the answer
    /// on real material is sometimes "the fundamental really is missing".
    order_shares: [f64; 4],
    /// Whether `comb_share` cleared the floor.
    engine_like: bool,
    crest_db: f64,
    modulation: Option<f64>,
    bands: [f64; 4],
    audible: f64,
    loudest_octave: &'static str,
    /// The strongest spectral lines below [`PEAK_CEILING_HZ`], for `--peaks`.
    peaks: Vec<(f64, f64)>,
}

/// Highest frequency `--peaks` looks at.
///
/// The question peaks are for is which low order the estimator locked to, and
/// on a six that argument is settled well below 500 Hz.
const PEAK_CEILING_HZ: f64 = 500.0;

/// Strongest local maxima in the spectrum below [`PEAK_CEILING_HZ`], with their
/// share of the total energy.
///
/// Local maxima rather than the strongest bins outright: a single peak occupies
/// several bins, so the strongest *bins* in a spectrum are usually five
/// neighbours of one line.
fn spectral_peaks(spectrum: &Spectrum, count: usize) -> Vec<(f64, f64)> {
    let power = spectrum.power();
    let bin = spectrum.bin_hz();
    let total = spectrum.total().max(1e-30);
    let last = ((PEAK_CEILING_HZ / bin).floor() as usize).min(power.len().saturating_sub(2));
    let mut peaks: Vec<(f64, f64)> = (1..last)
        .filter(|k| power[*k] > power[k - 1] && power[*k] >= power[k + 1])
        .map(|k| (k as f64 * bin, 100.0 * power[k] / total))
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks.truncate(count);
    peaks
}

struct Settings {
    window: usize,
    hop: usize,
    rpm_min: f64,
    rpm_max: f64,
    cylinders: usize,
    min_comb: f64,
    min_second_order: f64,
    rpm_tolerance: f64,
    orders: usize,
}

fn measure_window(samples: &[f32], rate: f64, start_s: f64, settings: &Settings) -> Window {
    let spectrum = Spectrum::of(samples, rate);
    let lo = sim_core::analysis::firing_hz(settings.rpm_min, settings.cylinders);
    let hi = sim_core::analysis::firing_hz(settings.rpm_max, settings.cylinders);
    let estimate = estimate_firing_hz(samples, rate, lo, hi, settings.orders);

    let mut bands = [0.0; 4];
    for (index, (_, lo, hi)) in BANDS.iter().enumerate() {
        bands[index] = spectrum.band_share(*lo, *hi);
    }

    let loudest_octave = OCTAVES
        .iter()
        .max_by(|a, b| {
            spectrum
                .band_energy(a.1, a.2)
                .total_cmp(&spectrum.band_energy(b.1, b.2))
        })
        .map_or("—", |o| o.0);

    let comb_share = estimate.map_or(0.0, |e| e.comb_share);

    let total = spectrum.total().max(1e-30);
    let mut order_shares = [0.0; 4];
    if let Some(estimate) = estimate {
        for (index, share) in order_shares.iter_mut().enumerate() {
            *share = 100.0 * spectrum.line_energy(estimate.f0_hz * (index + 1) as f64) / total;
        }
    }

    // Both conditions, because either alone passes a signal that is not an
    // engine: a broad noise floor has no comb, and a lone tone has no orders.
    let engine_like =
        comb_share >= settings.min_comb && order_shares[1] >= settings.min_second_order;

    Window {
        start_s,
        dbfs: dbfs(rms(samples)),
        f0_hz: estimate.map(|e| e.f0_hz),
        rpm: estimate.map(|e| rpm_from_firing_hz(e.f0_hz, settings.cylinders)),
        comb_share,
        order_shares,
        engine_like,
        crest_db: crest_db(samples),
        // Against the window's own inferred rate, and absent when there is no
        // rate to measure against. A modulation depth taken at the wrong `f0` is
        // a measurement of nothing, and a zero printed there would be read as
        // "no modulation" rather than as "no question asked".
        modulation: estimate.map(|e| modulation_depth(samples, rate, e.f0_hz)),
        bands,
        audible: spectrum.band_share(150.0, 15_000.0),
        loudest_octave,
        peaks: spectral_peaks(&spectrum, 6),
    }
}

// --- merging windows into segments ------------------------------------------

/// A run of neighbouring windows at what appears to be one operating point.
///
/// This is the annotation. A recording is not a set of scenarios, so the closest
/// thing to `full-1400` it can offer is "from here to here, the engine appears
/// to be at this speed, and here is how confident that is". Everything the
/// improvement plan asked annotation to carry — timestamps, steady or otherwise,
/// inferred rpm, a confidence — is a column below.
struct Segment {
    start_s: f64,
    end_s: f64,
    windows: usize,
    rpm_mean: f64,
    rpm_min: f64,
    rpm_max: f64,
    comb_share_mean: f64,
    dbfs_mean: f64,
    crest_mean: f64,
    modulation_mean: f64,
    bands: [f64; 4],
    audible_mean: f64,
    loudest_octave: &'static str,
}

/// Group consecutive engine-like windows whose inferred speed agrees.
///
/// The rpm tolerance is against the segment's *running mean* rather than against
/// the previous window, so a slow sweep cannot walk a segment across the whole
/// range one tolerable step at a time and be reported as steady at its average.
fn segments(windows: &[Window], settings: &Settings, window_s: f64) -> Vec<Segment> {
    let mut out: Vec<Vec<&Window>> = Vec::new();
    let mut current: Vec<&Window> = Vec::new();

    for window in windows {
        let Some(rpm) = window.rpm.filter(|_| window.engine_like) else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        };
        let mean = if current.is_empty() {
            rpm
        } else {
            current.iter().filter_map(|w| w.rpm).sum::<f64>() / current.len() as f64
        };
        if !current.is_empty() && (rpm - mean).abs() > settings.rpm_tolerance {
            out.push(std::mem::take(&mut current));
        }
        current.push(window);
    }
    if !current.is_empty() {
        out.push(current);
    }

    out.into_iter()
        .map(|group| {
            let n = group.len() as f64;
            let rpms: Vec<f64> = group.iter().filter_map(|w| w.rpm).collect();
            let mut bands = [0.0; 4];
            for (index, band) in bands.iter_mut().enumerate() {
                *band = group.iter().map(|w| w.bands[index]).sum::<f64>() / n;
            }
            // The whole span the group covers, which is the last window's start
            // plus a window, not plus a hop.
            let start_s = group[0].start_s;
            let end_s = group[group.len() - 1].start_s + window_s;
            Segment {
                start_s,
                end_s,
                windows: group.len(),
                rpm_mean: rpms.iter().sum::<f64>() / rpms.len().max(1) as f64,
                rpm_min: rpms.iter().cloned().fold(f64::INFINITY, f64::min),
                rpm_max: rpms.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                comb_share_mean: group.iter().map(|w| w.comb_share).sum::<f64>() / n,
                dbfs_mean: group.iter().map(|w| w.dbfs).sum::<f64>() / n,
                crest_mean: group.iter().map(|w| w.crest_db).sum::<f64>() / n,
                modulation_mean: group.iter().filter_map(|w| w.modulation).sum::<f64>() / n,
                bands,
                audible_mean: group.iter().map(|w| w.audible).sum::<f64>() / n,
                loudest_octave: group[group.len() / 2].loudest_octave,
            }
        })
        .collect()
}

// --- reporting --------------------------------------------------------------

fn round(value: f64, places: u32) -> f64 {
    let scale = 10f64.powi(places as i32);
    (value * scale).round() / scale
}

fn report(
    path: &Path,
    settings: &Settings,
    show_windows: bool,
    show_peaks: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let wav = read_wav(path)?;
    let mono = wav.mono();
    let rate = wav.sample_rate_hz;
    let correlation = wav.channel_correlation();

    println!();
    println!("{}", path.display());
    println!(
        "  {:.2} s, {} ch at {:.0} Hz, {:.1} dBFS overall, peak {:.3}, channel correlation {:+.2}",
        wav.duration_s(),
        wav.channels,
        rate,
        dbfs(rms(&mono)),
        peak(&mono),
        correlation,
    );
    if correlation < 0.2 {
        println!(
            "  the channels are barely correlated, so the mono downmix may be cancelling: \
             measure them separately before believing anything below"
        );
    }

    let window_s = settings.window as f64 / rate;
    let hop_s = settings.hop as f64 / rate;
    let mut windows = Vec::new();
    let mut start = 0usize;
    while start + settings.window <= mono.len() {
        windows.push(measure_window(
            &mono[start..start + settings.window],
            rate,
            start as f64 / rate,
            settings,
        ));
        start += settings.hop;
    }
    if windows.is_empty() {
        return Err(format!(
            "{}: {:.2} s is shorter than one {} sample window",
            path.display(),
            wav.duration_s(),
            settings.window
        )
        .into());
    }

    if show_windows {
        println!(
            "  {:>7} {:>7} {:>7} {:>6} {:>23} {:>6} {:>6}  {:>6} {:>6} {:>6} {:>6}  {:>8}",
            "t/s",
            "dBFS",
            "rpm",
            "comb%",
            "orders 1..4, % each",
            "crest",
            "mod",
            "<80",
            "80-300",
            "300-2k",
            ">2k",
            "octave"
        );
        for window in &windows {
            println!(
                "  {:>7.2} {:>7.1} {:>7} {:>6.1} {:>5.1}{:>6.1}{:>6.1}{:>6.1} {:>6.1} {:>6}  {:>6.1} {:>6.1} {:>6.1} {:>6.1}  {:>8}{}",
                window.start_s,
                window.dbfs,
                window
                    .rpm
                    .map_or_else(|| "—".to_string(), |rpm| format!("{rpm:.0}")),
                window.comb_share,
                window.order_shares[0],
                window.order_shares[1],
                window.order_shares[2],
                window.order_shares[3],
                window.crest_db,
                window
                    .modulation
                    .map_or_else(|| "—".to_string(), |m| format!("{m:.2}")),
                window.bands[0],
                window.bands[1],
                window.bands[2],
                window.bands[3],
                window.loudest_octave,
                if window.engine_like {
                    ""
                } else {
                    "  not engine-dominated"
                },
            );
            if show_peaks {
                let lines: Vec<String> = window
                    .peaks
                    .iter()
                    .map(|(hz, share)| format!("{hz:.0} Hz {share:.1}%"))
                    .collect();
                println!("          peaks: {}", lines.join(", "));
            }
        }
    }

    let segments = segments(&windows, settings, window_s);
    let engine_like = windows.iter().filter(|w| w.engine_like).count();
    println!(
        "  {} windows of {:.3} s at a {:.3} s hop; {} engine-dominated at {:.0}% comb \
         with {:.1}% at the second order",
        windows.len(),
        window_s,
        hop_s,
        engine_like,
        settings.min_comb,
        settings.min_second_order,
    );

    println!(
        "  {:>13} {:>7} {:>6} {:>13} {:>7} {:>6} {:>6}  {:>6} {:>6} {:>6} {:>6}  {:>8}",
        "from-to / s",
        "rpm",
        "±",
        "comb% / dBFS",
        "crest",
        "mod",
        "audib",
        "<80",
        "80-300",
        "300-2k",
        ">2k",
        "octave"
    );
    for segment in &segments {
        println!(
            "  {:>5.1}-{:>6.1} {:>7.0} {:>6.0} {:>6.1} / {:>4.0} {:>7.1} {:>6.2} {:>6.1}  {:>6.1} {:>6.1} {:>6.1} {:>6.1}  {:>8}",
            segment.start_s,
            segment.end_s,
            segment.rpm_mean,
            (segment.rpm_max - segment.rpm_min) / 2.0,
            segment.comb_share_mean,
            segment.dbfs_mean,
            segment.crest_mean,
            segment.modulation_mean,
            segment.audible_mean,
            segment.bands[0],
            segment.bands[1],
            segment.bands[2],
            segment.bands[3],
            segment.loudest_octave,
        );
    }
    if segments.is_empty() {
        println!("  no segment cleared the confidence floor: nothing here is engine-dominated");
    }

    Ok(serde_json::json!({
        "file": path.display().to_string(),
        "durationS": round(wav.duration_s(), 3),
        "channels": wav.channels,
        "sampleRateHz": rate,
        "channelCorrelation": round(correlation, 4),
        "overallDbfs": round(dbfs(rms(&mono)), 2),
        "peak": round(peak(&mono), 4),
        "windowSamples": settings.window,
        "hopSamples": settings.hop,
        "windowS": round(window_s, 4),
        "engineLikeWindows": engine_like,
        "windows": windows.iter().map(|w| serde_json::json!({
            "startS": round(w.start_s, 3),
            "dbfs": round(w.dbfs, 2),
            "firingHz": w.f0_hz.map(|f| round(f, 3)),
            "inferredRpm": w.rpm.map(|r| round(r, 1)),
            "combSharePercent": round(w.comb_share, 2),
            "orderSharePercent": w.order_shares.iter().map(|s| round(*s, 3)).collect::<Vec<_>>(),
            "engineDominated": w.engine_like,
            "crestDb": round(w.crest_db, 2),
            "modulationDepth": w.modulation.map(|m| round(m, 3)),
            "bandSharePercent": {
                "below80Hz": round(w.bands[0], 2),
                "80to300Hz": round(w.bands[1], 2),
                "300to2kHz": round(w.bands[2], 2),
                "above2kHz": round(w.bands[3], 2),
            },
            "audibleBandPercent": round(w.audible, 2),
            "loudestOctave": w.loudest_octave,
            "strongestLines": w.peaks.iter()
                .map(|(hz, share)| serde_json::json!([round(*hz, 1), round(*share, 3)]))
                .collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "segments": segments.iter().map(|s| serde_json::json!({
            "startS": round(s.start_s, 2),
            "endS": round(s.end_s, 2),
            "windows": s.windows,
            "inferredRpmMean": round(s.rpm_mean, 0),
            "inferredRpmMin": round(s.rpm_min, 0),
            "inferredRpmMax": round(s.rpm_max, 0),
            "combSharePercentMean": round(s.comb_share_mean, 2),
            "dbfsMean": round(s.dbfs_mean, 2),
            "crestDbMean": round(s.crest_mean, 2),
            "modulationDepthMean": round(s.modulation_mean, 3),
            "bandSharePercentMean": {
                "below80Hz": round(s.bands[0], 2),
                "80to300Hz": round(s.bands[1], 2),
                "300to2kHz": round(s.bands[2], 2),
                "above2kHz": round(s.bands[3], 2),
            },
            "audibleBandPercentMean": round(s.audible_mean, 2),
            "loudestOctave": s.loudest_octave,
        })).collect::<Vec<_>>(),
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut json = PathBuf::from(DEFAULT_JSON);
    let mut show_windows = false;
    let mut show_peaks = false;
    let mut settings = Settings {
        window: DEFAULT_WINDOW,
        hop: DEFAULT_WINDOW / 2,
        rpm_min: DEFAULT_RPM_MIN,
        rpm_max: DEFAULT_RPM_MAX,
        cylinders: 6,
        min_comb: DEFAULT_MIN_COMB,
        min_second_order: DEFAULT_MIN_SECOND_ORDER,
        rpm_tolerance: DEFAULT_RPM_TOLERANCE,
        orders: 4,
    };
    let mut hop_set = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut next = |what: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("{arg} needs {what}"))
        };
        match arg.as_str() {
            "--file" => files.push(PathBuf::from(next("a path")?)),
            "--json" => json = PathBuf::from(next("a path")?),
            "--window" => settings.window = next("a sample count")?.parse()?,
            "--hop" => {
                settings.hop = next("a sample count")?.parse()?;
                hop_set = true;
            }
            "--rpm-min" => settings.rpm_min = next("an engine speed")?.parse()?,
            "--rpm-max" => settings.rpm_max = next("an engine speed")?.parse()?,
            "--cylinders" => settings.cylinders = next("a cylinder count")?.parse()?,
            "--min-comb" => settings.min_comb = next("a percentage")?.parse()?,
            "--min-second-order" => {
                settings.min_second_order = next("a percentage")?.parse()?;
            }
            "--rpm-tolerance" => settings.rpm_tolerance = next("an rpm span")?.parse()?,
            "--windows" => show_windows = true,
            "--peaks" => {
                show_windows = true;
                show_peaks = true;
            }
            other => return Err(format!("unrecognised argument {other}").into()),
        }
    }

    if !settings.window.is_power_of_two() {
        return Err(format!(
            "--window {} is not a power of two, and the transform truncates to one: \
             the measurement would cover less of the window than it claimed to",
            settings.window
        )
        .into());
    }
    if !hop_set {
        settings.hop = settings.window / 2;
    }
    if settings.hop == 0 {
        return Err("--hop must be at least one sample".into());
    }
    if settings.rpm_max <= settings.rpm_min {
        return Err("--rpm-max must be above --rpm-min".into());
    }
    if files.is_empty() {
        return Err(
            "no --file given. The reference recordings are gitignored; see the README's \
             \"Characterising the reference\" for the ffmpeg lines that extract them"
                .into(),
        );
    }
    for file in &files {
        if !file.exists() {
            return Err(format!(
                "{} does not exist. The reference recordings are gitignored; see the \
                 README's \"Characterising the reference\" for how to extract them",
                file.display()
            )
            .into());
        }
    }

    println!(
        "searching {:.0}-{:.0} rpm on {} cylinders ({:.1}-{:.1} Hz firing), {} orders, \
         confidence floor {:.0}% comb",
        settings.rpm_min,
        settings.rpm_max,
        settings.cylinders,
        sim_core::analysis::firing_hz(settings.rpm_min, settings.cylinders),
        sim_core::analysis::firing_hz(settings.rpm_max, settings.cylinders),
        settings.orders,
        settings.min_comb,
    );

    let mut reports = Vec::new();
    for file in &files {
        reports.push(report(file, &settings, show_windows, show_peaks)?);
    }

    if let Some(parent) = json.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut annotation = BTreeMap::new();
    annotation.insert("apiVersion", serde_json::json!(sim_core::API_VERSION));
    annotation.insert("cylinders", serde_json::json!(settings.cylinders));
    annotation.insert("rpmSearchMin", serde_json::json!(settings.rpm_min));
    annotation.insert("rpmSearchMax", serde_json::json!(settings.rpm_max));
    annotation.insert("minCombSharePercent", serde_json::json!(settings.min_comb));
    annotation.insert(
        "minSecondOrderPercent",
        serde_json::json!(settings.min_second_order),
    );
    annotation.insert("rpmTolerance", serde_json::json!(settings.rpm_tolerance));
    annotation.insert("files", serde_json::json!(reports));
    fs::write(&json, serde_json::to_string_pretty(&annotation)?)?;
    println!();
    println!("annotation written to {}", json.display());

    Ok(())
}
