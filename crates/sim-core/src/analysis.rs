//! Signal measurement for the acoustic work: transforms, band shares, and the
//! character metrics.
//!
//! This is the measuring instrument, and it is kept here rather than in the
//! probe that prints it for one reason: **an instrument that is never itself
//! measured will happily certify a defect.** The modulation detector below is
//! the case in point. Its predecessor lived in `examples/audio_probe.rs`, was
//! documented as reading "nearly zero however loud" for a steady tone, and read
//! 0.42 for a 60 Hz sine — a number large enough to have satisfied the
//! `> 0` acceptance criterion written against it, on a signal that is by
//! definition not a pulse train. Nothing caught that, because a metric that only
//! ever sees engine output has no counterexample to fail against.
//!
//! Everything here is therefore
//!
//! - **pure and platform independent**, so it stays inside this crate's rules;
//! - **defined against synthetic signals**, with `tests/analysis.rs` holding a
//!   tone, an amplitude-modulated tone, a pulse train, and noise, and asserting
//!   what each metric must read for each of them;
//! - **out of the hot loop**. These functions allocate freely. Nothing in
//!   `sim::step` calls them; the probe, the capture tool and the acoustic tests
//!   do.
//!
//! ## What each metric is for
//!
//! | Metric | Question it answers | Blind to |
//! |---|---|---|
//! | [`Spectrum::band_share`] | can ordinary hardware reproduce this | pulse shape, pitch |
//! | [`Spectrum::comb_share`] | is the energy at the firing rate and its orders | whether the orders arrive as pulses |
//! | [`crest_db`] | peaks over average: distinct events, or a steady buzz | where the energy sits |
//! | [`modulation_depth`] | does the *envelope* swing at the firing rate | absolute level |
//!
//! They are deliberately a set that cannot all be satisfied by pushing energy in
//! one direction. A tone can hold any band share you like and fails the last
//! two; noise passes the crest test at the wrong value and fails the comb.

use std::f64::consts::{PI, TAU};

/// In-place iterative radix-2 FFT, decimation in time.
///
/// Hand-written rather than pulled in as a dependency: the transform is
/// textbook, this crate takes only serialization dependencies, and the cost of
/// the trigonometry is nothing next to the simulation that produced the samples.
///
/// Twiddles are evaluated directly rather than by recurrence, which drifts over
/// fifteen stages.
///
/// # Panics
///
/// If `re.len()` is not a power of two, or `im` is a different length.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two(), "radix-2 needs a power-of-two length");
    assert_eq!(im.len(), n, "real and imaginary parts must match in length");

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

    let mut len = 2usize;
    while len <= n {
        let half = len / 2;
        let mut base = 0usize;
        while base < n {
            for k in 0..half {
                let angle = -TAU * k as f64 / len as f64;
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

/// Inverse of [`fft`], normalised so `ifft(fft(x)) == x`.
///
/// # Panics
///
/// If `re.len()` is not a power of two, or `im` is a different length.
pub fn ifft(re: &mut [f64], im: &mut [f64]) {
    // Conjugate, forward transform, conjugate, scale. Cheaper to read than a
    // second butterfly loop that differs only in the sign of the twiddle.
    for value in im.iter_mut() {
        *value = -*value;
    }
    fft(re, im);
    let scale = 1.0 / re.len() as f64;
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        *r *= scale;
        *i = -*i * scale;
    }
}

/// Largest power of two not exceeding `n`, and at least 1.
fn floor_pow2(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    1usize << (usize::BITS - 1 - n.leading_zeros()) as usize
}

/// Hann window of length `n`.
///
/// The firing period does not divide any fixed window, so without one the
/// discontinuity at the wrap leaks energy across every bin and smears it
/// upward — fabricating exactly the high-frequency content this module exists to
/// measure.
fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / n as f64).cos()))
        .collect()
}

/// Power per bin from DC to Nyquist, with the frequency axis that goes with it.
///
/// Bins other than DC and Nyquist carry their negative-frequency twin as well.
/// A real signal's spectrum is symmetric, so summing only the positive half
/// makes every band read half its true share and a full-range set of bands sum
/// to 50%. Folding is what makes [`Spectrum::band_share`] over a partition come
/// to 100%, and that sum is a standing self-check on this file.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    power: Vec<f64>,
    bin_hz: f64,
    sample_rate_hz: f64,
}

impl Spectrum {
    /// Transform `samples`, Hann-windowed, truncated to a power of two.
    ///
    /// Truncation rather than zero-padding: padding a windowed frame with zeros
    /// changes the effective window and the normalisation with it, and callers
    /// here always have more samples than they need.
    #[must_use]
    pub fn of(samples: &[f32], sample_rate_hz: f64) -> Self {
        let n = floor_pow2(samples.len());
        let window = hann(n);
        let mut re = vec![0.0f64; n];
        let mut im = vec![0.0f64; n];
        for i in 0..n {
            re[i] = f64::from(samples[i]) * window[i];
        }

        fft(&mut re, &mut im);

        let power = (0..=n / 2)
            .map(|k| {
                let fold = if k == 0 || k == n / 2 { 1.0 } else { 2.0 };
                fold * (re[k] * re[k] + im[k] * im[k]) / n as f64
            })
            .collect();

        Self {
            power,
            bin_hz: sample_rate_hz / n as f64,
            sample_rate_hz,
        }
    }

    /// Power per bin, DC first.
    #[must_use]
    pub fn power(&self) -> &[f64] {
        &self.power
    }

    /// Width of one bin, in hertz.
    #[must_use]
    pub fn bin_hz(&self) -> f64 {
        self.bin_hz
    }

    /// Rate the samples were taken at, in hertz.
    #[must_use]
    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }

    /// Total energy across every bin.
    #[must_use]
    pub fn total(&self) -> f64 {
        self.power.iter().sum()
    }

    /// Energy in `[lo_hz, hi_hz)`, by bin centre.
    #[must_use]
    pub fn band_energy(&self, lo_hz: f64, hi_hz: f64) -> f64 {
        self.power
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let f = *k as f64 * self.bin_hz;
                f >= lo_hz && f < hi_hz
            })
            .map(|(_, p)| p)
            .sum()
    }

    /// Percentage of the total energy in `[lo_hz, hi_hz)`.
    #[must_use]
    pub fn band_share(&self, lo_hz: f64, hi_hz: f64) -> f64 {
        100.0 * self.band_energy(lo_hz, hi_hz) / self.total().max(1e-30)
    }

    /// Energy in the window's main lobe centred on `hz`.
    ///
    /// A Hann window's main lobe is two bins either side of the line, so a
    /// narrower collection would report a fraction of a peak that is genuinely
    /// there and a wider one would start counting the noise between the orders
    /// as if it were an order.
    #[must_use]
    pub fn line_energy(&self, hz: f64) -> f64 {
        let centre = (hz / self.bin_hz).round() as isize;
        let half = LINE_HALF_WIDTH_BINS as isize;
        (centre - half..=centre + half)
            .filter(|k| *k >= 0 && (*k as usize) < self.power.len())
            .map(|k| self.power[k as usize])
            .sum()
    }

    /// Percentage of the total energy sitting in the first `orders` multiples of
    /// `f0_hz`.
    ///
    /// This is the measurement band shares cannot make. A diesel puts its energy
    /// into a comb at its firing frequency; an electric motor, a resonance being
    /// rung and a filtered noise floor all put it somewhere else. A spectrum can
    /// satisfy every band target ever written and still have nothing at `f0` or
    /// its first few multiples.
    #[must_use]
    pub fn comb_share(&self, f0_hz: f64, orders: usize) -> f64 {
        let comb: f64 = (1..=orders)
            .map(|n| self.line_energy(f0_hz * n as f64))
            .sum();
        100.0 * comb / self.total().max(1e-30)
    }

    /// Centre of the strongest bin in `[lo_hz, hi_hz)`, in hertz.
    ///
    /// Returns 0 when the range holds no bins.
    #[must_use]
    pub fn dominant_hz(&self, lo_hz: f64, hi_hz: f64) -> f64 {
        self.power
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let f = *k as f64 * self.bin_hz;
                f >= lo_hz && f < hi_hz
            })
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0.0, |(k, _)| k as f64 * self.bin_hz)
    }
}

/// Half-width, in bins, of the window a spectral line is collected over.
const LINE_HALF_WIDTH_BINS: usize = 2;

/// Root mean square of a trace.
#[must_use]
pub fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Largest absolute sample.
#[must_use]
pub fn peak(samples: &[f32]) -> f64 {
    f64::from(samples.iter().fold(0.0f32, |m, s| m.max(s.abs())))
}

/// Decibels relative to full scale, floored so digital silence reads as a
/// number rather than as negative infinity.
#[must_use]
pub fn dbfs(amplitude: f64) -> f64 {
    20.0 * amplitude.max(1e-12).log10()
}

/// Peak over RMS, in decibels.
///
/// A sine is 3.0 dB and a train of distinct combustion events is well into
/// double figures, so this separates "a tone at roughly the right pitch" from
/// "a series of firing events" without any reference to where the energy sits.
/// It also collapses long before saturation measures as distortion: during
/// calibration it read 3.5 dB with the gains too high, which is a signal squared
/// off into a buzz.
#[must_use]
pub fn crest_db(samples: &[f32]) -> f64 {
    let rms = rms(samples);
    if rms <= 1e-30 {
        return 0.0;
    }
    20.0 * (peak(samples) / rms).log10()
}

/// Magnitude of the analytic signal: the amplitude envelope.
///
/// Built by transforming, zeroing the negative-frequency half, doubling the
/// positive half and transforming back — the frequency-domain Hilbert
/// transform. The result is truncated to a power of two, like [`Spectrum::of`].
///
/// ## Why not rectify and low-pass
///
/// Because rectification *manufactures* the thing being measured. `|A cos(w t)|`
/// is not a constant: it is `2A/pi` plus a series of even harmonics of the
/// carrier, the first of which sits at `2w` with a third of the amplitude of the
/// mean. Point a detector at `f0` and `2 f0` and feed it a steady tone at any
/// frequency whose second harmonic lands near the search band, and it reports
/// deep modulation of a signal that has none. Measured on the detector this
/// replaces, a steady 60 Hz sine scored 0.42 — against documentation promising
/// "nearly zero however loud it is".
///
/// The analytic envelope has no such artefact. For `A cos(w t)` it is exactly
/// `A`, for every `w`, so a tone reads zero because it *is* unmodulated.
#[must_use]
pub fn envelope(samples: &[f32]) -> Vec<f64> {
    let n = floor_pow2(samples.len());
    let mut re: Vec<f64> = samples[..n].iter().map(|s| f64::from(*s)).collect();
    let mut im = vec![0.0f64; n];

    fft(&mut re, &mut im);

    // Keep DC and Nyquist, double the positive frequencies, discard the
    // negative ones. That is the analytic signal.
    for k in 1..n / 2 {
        re[k] *= 2.0;
        im[k] *= 2.0;
    }
    for k in (n / 2 + 1)..n {
        re[k] = 0.0;
        im[k] = 0.0;
    }

    ifft(&mut re, &mut im);

    re.iter()
        .zip(im.iter())
        .map(|(r, i)| (r * r + i * i).sqrt())
        .collect()
}

/// Amplitude of the component of `signal` at exactly `hz`, and its mean.
///
/// Correlated against a complex exponential at `hz` under a Hann window. The
/// correlation frequency is arbitrary rather than a bin centre, so a component
/// sitting exactly at `hz` is recovered exactly — there is no scalloping loss to
/// budget for — while the window keeps the large DC term from leaking into it.
fn windowed_amplitude(signal: &[f64], sample_rate_hz: f64, hz: f64) -> f64 {
    let n = signal.len();
    if n == 0 {
        return 0.0;
    }
    let window = hann(n);
    let weight: f64 = window.iter().sum();
    if weight <= 0.0 {
        return 0.0;
    }
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for i in 0..n {
        let angle = -TAU * hz * i as f64 / sample_rate_hz;
        let (sin, cos) = angle.sin_cos();
        let w = window[i] * signal[i];
        re += w * cos;
        im += w * sin;
    }
    2.0 * (re * re + im * im).sqrt() / weight
}

/// Mean of `signal` under the same Hann window the amplitudes use.
fn windowed_mean(signal: &[f64]) -> f64 {
    let window = hann(signal.len());
    let weight: f64 = window.iter().sum();
    if weight <= 0.0 {
        return 0.0;
    }
    window.iter().zip(signal).map(|(w, s)| w * s).sum::<f64>() / weight
}

/// How deeply the signal's envelope swings at the firing rate.
///
/// Take the analytic envelope, and report the fractional swing at `f0` and its
/// second harmonic:
///
/// ```text
/// depth = ( a(f0) + a(2 f0) ) / mean(envelope)
/// ```
///
/// where `a(f)` is the amplitude of the envelope's sinusoidal component at `f`.
/// The definition is chosen so it can be checked against signals whose answer is
/// known in advance, and `tests/analysis.rs` checks all four:
///
/// | Signal | Reads |
/// |---|---|
/// | steady tone, any frequency, any level | 0 |
/// | tone amplitude-modulated to depth `d` at `f0` | `d` |
/// | pulse train at `f0` | large — 1 and upward |
/// | white noise | small, and unrelated to `f0` |
///
/// The second harmonic is included because a firing comb's envelope is not a
/// sinusoid: a run of six pulses per cycle has envelope content at `f0` and its
/// multiples, and taking only the first would under-report a sharp pulse train
/// relative to a soft one.
///
/// Returns 0 for an empty or silent trace.
#[must_use]
pub fn modulation_depth(samples: &[f32], sample_rate_hz: f64, f0_hz: f64) -> f64 {
    if samples.is_empty() || f0_hz <= 0.0 {
        return 0.0;
    }
    let envelope = envelope(samples);
    let mean = windowed_mean(&envelope);
    if mean <= 1e-30 {
        return 0.0;
    }
    let first = windowed_amplitude(&envelope, sample_rate_hz, f0_hz);
    let second = windowed_amplitude(&envelope, sample_rate_hz, 2.0 * f0_hz);
    (first + second) / mean
}

/// Firing frequency: an engine fires `cylinders` times per two revolutions.
#[must_use]
pub fn firing_hz(rpm: f64, cylinders: usize) -> f64 {
    rpm / 60.0 * cylinders as f64 * 0.5
}

/// Undo the solver's soft clipper on a summed sample, returning the gain it
/// applied.
///
/// The clipper acts on the mix and is applied to the three paths as one common
/// gain `clipped / mix`, with `clipped = knee * tanh(mix / knee)`. That is
/// invertible: from the emitted sum — which *is* `clipped` — the pre-saturation
/// mix is `knee * atanh(clipped / knee)`, so dividing every path by this gain
/// recovers what the solver produced before the limiter touched it.
///
/// This is why the capture tool can export unsaturated source tracks without the
/// solver gaining a second output or an export mode. The recovery is exact
/// wherever the clipper is, which is everywhere except the outer `[-1, 1]` clamp
/// that measurement says never engages; a ratio at or past 1 is clamped just
/// short of it, where the inverse is finite, rather than returning infinity.
///
/// Returns 1.0 for a silent sample or a non-positive knee, i.e. no correction.
#[must_use]
pub fn saturation_gain(clipped_sum: f64, knee: f64) -> f64 {
    if knee <= 0.0 || clipped_sum == 0.0 {
        return 1.0;
    }
    let ratio = (clipped_sum / knee).clamp(-0.999_999, 0.999_999);
    let mix = knee * ratio.atanh();
    if mix.abs() <= f64::MIN_POSITIVE {
        return 1.0;
    }
    clipped_sum / mix
}

/// A synthetic train of raised-cosine pulses at `f0_hz`, for exercising the
/// metrics against a signal whose character is known.
///
/// Lives beside the metrics rather than in the test file because the capture
/// tool's self-check uses it too, and a reference signal that two callers define
/// separately is a reference signal that will eventually differ.
///
/// `duty` is the fraction of the pulse period the pulse occupies.
#[must_use]
pub fn pulse_train(len: usize, sample_rate_hz: f64, f0_hz: f64, duty: f64) -> Vec<f32> {
    let period = sample_rate_hz / f0_hz.max(1e-9);
    let width = (period * duty.clamp(1e-3, 1.0)).max(1.0);
    (0..len)
        .map(|i| {
            let phase = (i as f64) % period;
            if phase >= width {
                return 0.0;
            }
            (0.5 * (1.0 - (TAU * phase / width).cos())) as f32
        })
        .collect()
}

/// A tone of amplitude 1 at `hz`.
#[must_use]
pub fn tone(len: usize, sample_rate_hz: f64, hz: f64) -> Vec<f32> {
    (0..len)
        .map(|i| (TAU * hz * i as f64 / sample_rate_hz).sin() as f32)
        .collect()
}

/// A tone at `carrier_hz` whose amplitude swings by `depth` at `rate_hz`.
///
/// The signal [`modulation_depth`] must report `depth` for, which is what makes
/// that metric checkable rather than merely plausible.
#[must_use]
pub fn amplitude_modulated(
    len: usize,
    sample_rate_hz: f64,
    carrier_hz: f64,
    rate_hz: f64,
    depth: f64,
) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let t = i as f64 / sample_rate_hz;
            let envelope = 1.0 + depth * (TAU * rate_hz * t).cos();
            (envelope * (TAU * carrier_hz * t + PI * 0.25).sin()) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 40_000.0;
    const LEN: usize = 32_768;

    #[test]
    fn the_transform_round_trips() {
        let signal = tone(1_024, RATE, 700.0);
        let mut re: Vec<f64> = signal.iter().map(|s| f64::from(*s)).collect();
        let mut im = vec![0.0f64; re.len()];
        let original = re.clone();
        fft(&mut re, &mut im);
        ifft(&mut re, &mut im);
        for (a, b) in original.iter().zip(re.iter()) {
            assert!((a - b).abs() < 1e-9, "round trip drifted: {a} vs {b}");
        }
    }

    #[test]
    fn a_tone_lands_in_its_own_bin() {
        let spectrum = Spectrum::of(&tone(LEN, RATE, 1_000.0), RATE);
        assert!((spectrum.dominant_hz(0.0, RATE / 2.0) - 1_000.0).abs() < spectrum.bin_hz() * 2.0);
        assert!(
            spectrum.band_share(900.0, 1_100.0) > 99.0,
            "a tone should put essentially all of its energy in a band around itself"
        );
    }

    #[test]
    fn a_partition_of_bands_sums_to_one_hundred_percent() {
        // The folding self-check. Without it every band reads half its true
        // share and this sum comes to 50.
        let spectrum = Spectrum::of(&pulse_train(LEN, RATE, 60.0, 0.1), RATE);
        let sum = spectrum.band_share(0.0, 80.0)
            + spectrum.band_share(80.0, 300.0)
            + spectrum.band_share(300.0, 2_000.0)
            + spectrum.band_share(2_000.0, f64::INFINITY);
        assert!((sum - 100.0).abs() < 0.01, "bands summed to {sum}");
    }

    #[test]
    fn the_saturation_gain_inverts_the_soft_clipper() {
        let knee = 0.85f64;
        for mix in [-2.0f64, -0.9, -0.1, 0.05, 0.4, 1.2, 3.0] {
            let clipped = knee * (mix / knee).tanh();
            let gain = saturation_gain(clipped, knee);
            let recovered = clipped / gain;
            assert!(
                (recovered - mix).abs() < 1e-9,
                "recovering {mix} from {clipped} gave {recovered}"
            );
        }
        assert_eq!(
            saturation_gain(0.0, 0.85),
            1.0,
            "silence needs no correction"
        );
    }

    #[test]
    fn the_envelope_of_a_tone_is_its_amplitude() {
        let envelope = envelope(&tone(4_096, RATE, 500.0));
        // Skip the ends: a finite transform wraps, so the analytic signal is
        // only meaningful away from the edges.
        for value in &envelope[512..3_584] {
            assert!(
                (value - 1.0).abs() < 0.01,
                "a unit tone's envelope should be 1, got {value}"
            );
        }
    }
}
