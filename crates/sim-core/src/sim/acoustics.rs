//! Engine acoustic sources: the exhaust, and the engine's own structure.
//!
//! The solver's fixed step is 25 us, which is a sample rate of 40 kHz. That is
//! already an audio rate, so the sound does not need a synthesiser: one sample
//! per step, taken from quantities the solver already integrates, *is* the
//! signal. What you hear is the same cylinder pressure that drives the crank.
//!
//! Two paths radiate, and they are summed at the end rather than in series:
//!
//! ```text
//! source     = sum over cylinders of  net port mass flow, kg/s
//! exhaust    = lowpass(lowpass(highpass(d(source)/dt)))    out of the pipe
//! forcing    = d( sum over cylinders of p_cyl / p_ambient )/dt
//! structural = sum over modes of  gain_k * resonator_k(forcing)   off the iron
//! sample     = softclip(g_exh * exhaust + g_str * structural)
//! ```
//!
//! The derivative on the exhaust path is not a brightness trick. A pipe mouth is
//! an acoustic monopole, and a monopole radiates the time derivative of the
//! volume flow through it. Radiating the flow itself instead loses 6 dB per
//! octave, which pushes almost all the energy below the range an ordinary
//! speaker can reproduce at all — the signal measures fine and is inaudible.
//!
//! ## What the exhaust source is, and what it used to be
//!
//! The source is the mass flow `flow::exchange` actually passed through the
//! port, summed over cylinders. It is not computed here: `step.rs` accumulates
//! it from the transfers it already performs, so the model radiates the quantity
//! it solved rather than a second quantity resembling it.
//!
//! It used to be `area(psi) * (p_cyl - p_exhaust) / p_ambient`, computed beside
//! the exchange call. That proxy is linear in the pressure difference, while
//! real orifice flow goes as its square root and saturates once the throat
//! chokes — so the proxy exaggerated the peak of the blowdown and changed shape
//! against the truth right through the choked-to-subsonic transition. The pulse
//! shape is the timbre, so that was audible rather than academic.
//!
//! Two properties of the flow matter and both come from `flow.rs` rather than
//! from here. It is the **clamped and settled** flow, not a fresh unclamped
//! orifice call: the unclamped explicit flow carries a two-sample limit cycle at
//! Nyquist which is invisible in the pressure trace and glaring in the audio,
//! because the radiation derivative amplifies with frequency. And it is signed,
//! so backflow into a cylinder is a negative contribution without any special
//! case.
//!
//! A cylinder exchanging no gas contributes nothing, however high its pressure
//! climbs — and *two* things can open its port. The normal exhaust event is one.
//! The other is the decompression brake, whose release lobe cracks a valve at the
//! top of compression, when the cylinder-to-manifold pressure difference is larger
//! than ordinary blowdown ever sees. Both go through `flow::exchange`, so the
//! brake's bark is on the same scale as the exhaust pulse by construction rather
//! than by calibration, and the two can never both open the port in one step
//! because a cylinder is in exactly one phase.
//!
//! ## Why the structural path is summed last
//!
//! The block radiates straight to air. It does not go out of the tailpipe.
//! Putting the structural term through the muffler lowpass would filter it with
//! a transfer function that does not apply to it, and would attenuate precisely
//! the band it exists to supply. So the two paths are filtered separately and
//! summed after, before the clipper.
//!
//! ## Why no gate around the burn
//!
//! Raw `d(sum p_cyl)/dt` is dominated in magnitude by the compression and
//! expansion swing, not by combustion: 20 MPa over 90 degrees at 1400 rpm is
//! about 2 GPa/s, against about 6.7 GPa/s for the premixed spike. Comparable
//! numbers. But the compression swing is a smooth 10 ms hump with essentially no
//! energy in the modal passbands, while the 1.2 ms premixed rise has content
//! well past 1 kHz. The modal filters do the discrimination themselves, so
//! windowing the derivative around the burn would be adding a schedule to
//! replace selectivity that is already there and is physically correct.
//!
//! For the same reason the clatter needs no schedule of its own. Ignition delay
//! already lengthens when the cylinder is cold and lightly loaded, a longer
//! delay already means a larger premixed fraction and a sharper rise, so the
//! engine rattles cold and mellows under load because the combustion model says
//! so. The decompression brake gets its bark the same way: its release lobe
//! cracks a valve at the top of compression, the fastest pressure event the
//! model produces anywhere, and it drives this bank hard without a line of
//! brake-specific audio code.
//!
//! Nothing here is published. The manual says nothing about how the engine
//! sounds, so every coefficient is calibrated.
//!
//! Firing frequency falls out of the geometry rather than being programmed: an
//! inline six fires six times per `4*PI`, so at 1500 rpm the fundamental is
//! 75 Hz. Nothing in this module knows the cylinder count.

use crate::config::validate::CYCLE_RAD;
use crate::config::{AudioCalibration, StructuralMode};

/// Smooth 0 to 1 ramp, continuous at both ends.
#[inline]
pub fn ramp(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    0.5 * (1.0 - (std::f64::consts::PI * t).cos())
}

/// Normalised exhaust port open area at a signed cycle angle.
///
/// A trapezoid: the valve ramps open over `ramp_rad` of crank, holds near full
/// lift for the exhaust stroke, then ramps shut into TDC overlap.
///
/// The ramp width matters far more than it looks. Spreading the opening across
/// the whole exhaust window — as a single raised cosine over the full span does
/// — leaves the port almost shut at the exact moment the cylinder-to-manifold
/// pressure difference is largest. The product is then a slow, smooth hump with
/// no harmonic content above a few tens of hertz, which is both wrong and
/// literally inaudible on any ordinary speaker. A real valve cracks open
/// quickly, and that sharp blowdown edge is what an exhaust actually sounds
/// like.
#[inline]
pub fn port_area_fraction(
    psi: f64,
    exhaust_valve_open_rad: f64,
    ramp_rad: f64,
    cycle_rad: f64,
) -> f64 {
    let end = cycle_rad * 0.5;
    if psi < exhaust_valve_open_rad || psi >= end {
        return 0.0;
    }
    let span = end - exhaust_valve_open_rad;
    if span <= 0.0 {
        return 0.0;
    }
    // A ramp cannot take more than half the window, or the valve would never
    // reach full lift.
    let width = ramp_rad.clamp(1.0e-6, span * 0.5);
    let opening = ramp((psi - exhaust_valve_open_rad) / width);
    let closing = ramp((end - psi) / width);
    opening.min(closing)
}

/// Exhaust port geometry, grouped because these three always travel together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Port {
    /// Flow area at full lift.
    pub effective_area_m2: f64,
    /// Signed cycle angle at which the valve starts to open.
    pub valve_open_rad: f64,
    /// Crank angle taken to ramp between shut and full lift.
    pub ramp_rad: f64,
}

impl Port {
    /// Open area at a signed cycle angle, in square metres.
    #[inline]
    pub fn area_m2(&self, psi: f64) -> f64 {
        self.effective_area_m2
            * port_area_fraction(psi, self.valve_open_rad, self.ramp_rad, CYCLE_RAD)
    }
}

/// Soft clipper: linear for small signals, saturating at `knee`.
#[inline]
fn soft_clip(x: f64, knee: f64) -> f64 {
    if knee <= 0.0 {
        return 0.0;
    }
    (knee * (x / knee).tanh()).clamp(-1.0, 1.0)
}

/// One-pole coefficient for a cutoff frequency at a given step.
#[inline]
fn pole(cutoff_hz: f64, dt: f64) -> f64 {
    let rc = 1.0 / (std::f64::consts::TAU * cutoff_hz);
    dt / (rc + dt)
}

/// One resonant mode of the engine structure: two poles and two zeros.
///
/// ```text
/// y[n] = 2 r cos(theta) y[n-1] - r^2 y[n-2] + (x[n] - x[n-2])
/// theta = 2 pi f / f_s        r = 1 - theta / (2 Q)
/// ```
///
/// ## Why the zeros are not optional
///
/// The bare all-pole resonator — the same recursion without the `- x[n-2]` — has
/// a DC gain of `1 / (1 - a1 - a2)`, which for these pole radii is in the tens.
/// A real mechanical mode has *zero* response at DC: a static pressure does not
/// radiate. Leaving the zeros out is therefore both wrong and expensive. A
/// stopped engine still drifts a fraction of a pascal per step as its trapped
/// charge equilibrates with the walls, and a constant drift through a DC gain of
/// fifty is a constant offset on the output — audible as a click on every state
/// change, and enough on its own to break the invariant that a reset engine is
/// silent.
///
/// Zeros at `z = +/-1` put an exact null at DC and another at Nyquist. The
/// second one is worth having for its own sake: the explicit port transfer
/// leaves a two-sample limit cycle near equilibrium, which lives within a
/// whisker of Nyquist, and this bank now refuses to amplify it.
///
/// ## Why the response is normalised
///
/// Peak magnitude is divided out at construction, so a mode's configured `gain`
/// is its weight at resonance and nothing else. Without that, `gain` would be
/// entangled with `q` and `frequency_hz` — the raw peak response runs from about
/// 80 to 210 across this bank — and calibrating one mode would silently
/// recalibrate the others.
///
/// Coefficients are computed once at construction, not per step: the solver step
/// is fixed, and trigonometry per mode per sample would be a hot-loop cost for
/// an answer that never changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resonator {
    /// `2 r cos(theta)`.
    a1: f64,
    /// `-r^2`.
    a2: f64,
    /// Configured weight divided by the peak magnitude, applied on output.
    scale: f64,
    y1: f64,
    y2: f64,
    x1: f64,
    x2: f64,
}

impl Resonator {
    fn new(mode: &StructuralMode, dt: f64) -> Self {
        let theta = std::f64::consts::TAU * mode.frequency_hz * dt;
        let r = 1.0 - theta / (2.0 * mode.q);
        // Validation rejects any configuration that lands outside this, because
        // an unstable pole grows without bound and nothing downstream can
        // recover it. Assert rather than clamp: silently moving a pole would
        // hide a configuration error behind a sound that is merely different.
        debug_assert!(
            r > 0.0 && r < 1.0,
            "unstable resonator pole radius {r}; validation should have rejected this"
        );
        let a1 = 2.0 * r * theta.cos();
        let a2 = -(r * r);

        // |H| at the resonant frequency, evaluated exactly rather than
        // approximated, so the normalisation holds for any Q the configuration
        // is allowed to name.
        let (sin1, cos1) = theta.sin_cos();
        let (sin2, cos2) = (2.0 * theta).sin_cos();
        let numerator = ((1.0 - cos2).powi(2) + sin2 * sin2).sqrt();
        let real = 1.0 - a1 * cos1 - a2 * cos2;
        let imaginary = a1 * sin1 + a2 * sin2;
        let denominator = (real * real + imaginary * imaginary).sqrt();
        let peak = if denominator > 0.0 {
            numerator / denominator
        } else {
            1.0
        };

        Self {
            a1,
            a2,
            scale: if peak > 0.0 { mode.gain / peak } else { 0.0 },
            y1: 0.0,
            y2: 0.0,
            x1: 0.0,
            x2: 0.0,
        }
    }

    /// Advance one sample, returning this mode's weighted contribution.
    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.a1 * self.y1 + self.a2 * self.y2 + (x - self.x2);
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        self.scale * y
    }

    #[inline]
    fn clear(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
        self.x1 = 0.0;
        self.x2 = 0.0;
    }

    /// Seed the input history so a standing forcing produces no step.
    ///
    /// The zeros difference the input, so an input history of zero against a
    /// first sample of `x` is a step of `x` — and a step rings. Seeding both
    /// taps makes the first difference exactly zero, which is the same argument
    /// that seeds the pressure history one level up.
    #[inline]
    fn seed_input(&mut self, x: f64) {
        self.x1 = x;
        self.x2 = x;
    }
}

/// Filter state and the ring buffer of produced samples.
///
/// The buffer is allocated once when the simulation is constructed and never
/// resized, so producing audio allocates nothing in the hot loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Acoustics {
    /// How many pushes have been absorbed to seed the difference histories.
    ///
    /// One is not enough. The exhaust path differences once, but the structural
    /// path differences *twice* — once to turn cylinder pressure into its rate
    /// of change, and again inside each resonator's DC-blocking zeros — so it
    /// needs two predecessors before its output means anything.
    ///
    /// The engine this matters for is a stopped one. Its trapped charge is not
    /// quite in equilibrium with the cylinder walls, so it warms at a bit over a
    /// pascal per step: a real, physical, utterly inaudible drift at DC. With
    /// only one seeded step the bank sees that drift arrive as a step from
    /// nothing and rings it, which is a bell struck by the simulation starting
    /// rather than by anything the engine did.
    primed_steps: u8,
    /// Previous raw source, for the radiation derivative.
    previous_source: f64,
    highpass_previous_input: f64,
    highpass_previous_output: f64,
    lowpass_stage_one: f64,
    lowpass_stage_two: f64,

    /// Previous normalised cylinder-pressure sum, for the structural forcing.
    previous_pressure_sum: f64,
    /// Modal bank, sized and tuned once at construction.
    resonators: Vec<Resonator>,

    buffer: Vec<f32>,
    write: usize,
    read: usize,
    len: usize,
    /// Samples discarded because the consumer did not drain in time.
    dropped: u64,

    /// Running mean square of produced samples, for the level meter.
    mean_square: f64,
}

impl Acoustics {
    /// Allocate for a given capacity in samples and tune the modal bank.
    ///
    /// Both the sample ring and the resonator bank are allocated here and never
    /// resized, so producing audio allocates nothing in the hot loop.
    pub fn new(capacity: usize, modes: &[StructuralMode], dt: f64) -> Self {
        let capacity = capacity.max(1);
        Self {
            primed_steps: 0,
            previous_source: 0.0,
            highpass_previous_input: 0.0,
            highpass_previous_output: 0.0,
            lowpass_stage_one: 0.0,
            lowpass_stage_two: 0.0,
            previous_pressure_sum: 0.0,
            resonators: modes.iter().map(|m| Resonator::new(m, dt)).collect(),
            buffer: vec![0.0; capacity],
            write: 0,
            read: 0,
            len: 0,
            dropped: 0,
            mean_square: 0.0,
        }
    }

    /// Clear filters and discard buffered samples.
    pub fn reset(&mut self) {
        self.primed_steps = 0;
        self.previous_source = 0.0;
        self.highpass_previous_input = 0.0;
        self.highpass_previous_output = 0.0;
        self.lowpass_stage_one = 0.0;
        self.lowpass_stage_two = 0.0;
        self.previous_pressure_sum = 0.0;
        // The modes keep their tuning but lose their ringing. A bank still
        // carrying the last burn across a reset would sound it out into a
        // freshly reset engine, which is the click this method exists to avoid.
        for resonator in &mut self.resonators {
            resonator.clear();
        }
        // Zero the samples too, not just the cursors. Stale bytes would be
        // unreachable through `drain`, but leaving them means two simulations
        // reset identically would not be bit-identical in every field, and this
        // is a reset rather than the hot loop.
        self.buffer.fill(0.0);
        self.write = 0;
        self.read = 0;
        self.len = 0;
        self.dropped = 0;
        self.mean_square = 0.0;
    }

    /// Samples waiting to be drained.
    #[inline]
    pub fn available(&self) -> usize {
        self.len
    }

    /// Samples dropped since the last reset because the consumer fell behind.
    #[inline]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Level in decibels relative to the configured reference.
    pub fn level_db(&self, reference_spl_db: f64) -> f64 {
        let rms = self.mean_square.max(0.0).sqrt();
        if rms <= 1.0e-9 {
            return 0.0;
        }
        reference_spl_db + 20.0 * rms.log10()
    }

    /// Filter one step's worth of both sources and push the resulting sample.
    ///
    /// `source` is the summed exhaust-port term; `pressure_sum` is the summed
    /// cylinder pressure normalised by ambient, which drives the structural
    /// path. Both are dimensionless and both are differenced here rather than in
    /// the caller, so the whole radiation model stays in one place.
    #[inline]
    pub fn push(&mut self, audio: &AudioCalibration, source: f64, pressure_sum: f64, dt: f64) {
        // What an open pipe radiates into the far field is proportional to the
        // *rate of change* of the flow leaving it, not to the flow itself: it is
        // an acoustic monopole, and a monopole radiates `d(volume flow)/dt`.
        //
        // Feeding the flow through directly is a real modelling error, and an
        // audible one. It costs 6 dB per octave across the whole spectrum, which
        // buries everything above a couple of hundred hertz — leaving a signal
        // whose energy sits almost entirely below the point where an ordinary
        // speaker can reproduce anything at all.
        //
        // Differencing rather than dividing by `dt` keeps the numbers in a sane
        // range; the constant that would come out of the division is absorbed
        // into the calibrated gain.
        //
        // The first sample after a reset has no predecessor to difference
        // against. Treating the missing one as zero would turn the standing
        // pressure difference across an already-open exhaust port into a
        // one-sample impulse — and an impulse is white, so after the muffler
        // filter it rings at the cutoff and comes out as an audible crack every
        // time the simulation is reset. Seed the history instead and emit
        // silence for that one step.
        //
        // The structural forcing needs the same treatment for the same reason,
        // and needs it more: the cylinder-pressure sum sits at six atmospheres
        // even on a dead engine, so differencing against an assumed zero would
        // hand the resonator bank a step of six and it would ring that step for
        // tens of milliseconds. That is a bell struck on every reset.
        let seeding_first_step = self.primed_steps == 0;
        if seeding_first_step {
            self.previous_source = source;
            self.previous_pressure_sum = pressure_sum;
            self.primed_steps = 1;
        }
        let radiated = source - self.previous_source;
        self.previous_source = source;

        // One-pole high pass, differencing form. Removes any residual offset the
        // derivative leaves behind.
        let hp_alpha = 1.0 - pole(audio.highpass_cutoff_hz, dt);
        let highpassed =
            hp_alpha * (self.highpass_previous_output + radiated - self.highpass_previous_input);
        self.highpass_previous_input = radiated;
        self.highpass_previous_output = highpassed;

        // Two cascaded one-pole low passes.
        let lp_alpha = pole(audio.lowpass_cutoff_hz, dt);
        self.lowpass_stage_one += (highpassed - self.lowpass_stage_one) * lp_alpha;
        self.lowpass_stage_two += (self.lowpass_stage_one - self.lowpass_stage_two) * lp_alpha;

        // --- structural path ---
        //
        // The premixed burn is a near-step pressure rise inside a stiff iron
        // box. Its derivative is what shakes the block, and the modal bank is
        // what the block does about it. This never sees the muffler filtering
        // above: iron radiates to air, not down the pipe.
        let forcing = pressure_sum - self.previous_pressure_sum;
        self.previous_pressure_sum = pressure_sum;
        if !seeding_first_step && self.primed_steps == 1 {
            // The second seeded step. On the first, `forcing` is zero by
            // construction and seeding with it would achieve nothing; this is
            // the first value the bank will ever see, so it becomes its own
            // predecessor and a standing drift enters as a level, not an edge.
            for resonator in &mut self.resonators {
                resonator.seed_input(forcing);
            }
            self.primed_steps = 2;
        }
        let mut structural = 0.0;
        for resonator in &mut self.resonators {
            structural += resonator.tick(forcing);
        }

        let sample = soft_clip(
            audio.exhaust_gain * self.lowpass_stage_two + audio.structural_gain * structural,
            audio.soft_clip_knee,
        );

        // A non-finite sample would poison the audio device; drop it and keep
        // the simulation's own fault latching responsible for reporting it.
        let sample = if sample.is_finite() {
            sample as f32
        } else {
            0.0
        };

        // Slow decay toward the current level, roughly a 50 ms window at 40 kHz.
        self.mean_square += (f64::from(sample) * f64::from(sample) - self.mean_square) * 0.0005;

        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
        if self.len == self.buffer.len() {
            // Full: the oldest sample is overwritten and the read cursor follows.
            self.read = (self.read + 1) % self.buffer.len();
            self.dropped += 1;
        } else {
            self.len += 1;
        }
    }

    /// Copy buffered samples into `out`, returning how many were written.
    pub fn drain(&mut self, out: &mut [f32]) -> usize {
        let count = self.len.min(out.len());
        for slot in out.iter_mut().take(count) {
            *slot = self.buffer[self.read];
            self.read = (self.read + 1) % self.buffer.len();
        }
        self.len -= count;
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 35 degrees, the calibrated ramp width.
    const RAMP: f64 = 0.6109;

    use crate::config::validate::CYCLE_RAD;

    /// The solver step these tests assume, so 40 kHz.
    const DT: f64 = 2.5e-5;

    fn modes() -> Vec<StructuralMode> {
        vec![
            StructuralMode {
                frequency_hz: 900.0,
                q: 15.0,
                gain: 1.0,
            },
            StructuralMode {
                frequency_hz: 2_600.0,
                q: 22.0,
                gain: 1.0,
            },
        ]
    }

    fn audio() -> AudioCalibration {
        AudioCalibration {
            reference_spl_db: 90.0,
            exhaust_gain: 0.35,
            highpass_cutoff_hz: 25.0,
            lowpass_cutoff_hz: 4_000.0,
            soft_clip_knee: 0.9,
            structural_gain: 0.02,
            structural_modes: modes(),
        }
    }

    /// An `Acoustics` with the bank these tests use.
    fn bank(capacity: usize) -> Acoustics {
        Acoustics::new(capacity, &modes(), DT)
    }

    /// A quiet engine: no exhaust source, no cylinder pressure change.
    fn silent(ac: &mut Acoustics, audio: &AudioCalibration, steps: usize) {
        for _ in 0..steps {
            ac.push(audio, 0.0, 6.0, DT);
        }
    }

    #[test]
    fn a_standing_cylinder_pressure_rings_nothing() {
        // The modal bank has an exact zero at DC, so an engine holding a
        // constant pressure must produce silence rather than a settling offset.
        // An all-pole resonator would have a DC gain in the tens here, and that
        // offset is what a reset engine would come out sounding like.
        let a = audio();
        let mut ac = bank(4_000);
        silent(&mut ac, &a, 4_000);
        let mut out = vec![0.0f32; 4_000];
        ac.drain(&mut out);
        for sample in &out {
            assert_eq!(*sample, 0.0, "a standing pressure must not ring the bank");
        }
    }

    #[test]
    fn a_standing_pressure_drift_does_not_strike_the_bank() {
        // A stopped engine's trapped charge is not quite in equilibrium with the
        // cylinder walls, so the summed pressure climbs by a small constant
        // amount every step. That is a real drift and it sits at DC, so it must
        // not be heard. With only one seeded step the bank takes its onset as an
        // edge and rings it for tens of milliseconds, which is why the structural
        // path seeds two.
        let a = audio();
        let mut ac = bank(4_000);
        let mut sum = 6.0;
        for _ in 0..4_000 {
            ac.push(&a, 0.0, sum, DT);
            sum += 1.35e-5;
        }
        let mut out = vec![0.0f32; 4_000];
        ac.drain(&mut out);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak < 1.0e-9,
            "a constant drift is DC and must not ring the bank, peaked at {peak}"
        );
    }

    #[test]
    fn the_port_is_shut_outside_the_exhaust_window() {
        let evo = 2.4;
        assert_eq!(port_area_fraction(0.0, evo, RAMP, CYCLE_RAD), 0.0);
        assert_eq!(port_area_fraction(evo - 0.01, evo, RAMP, CYCLE_RAD), 0.0);
        assert_eq!(
            port_area_fraction(CYCLE_RAD * 0.5, evo, RAMP, CYCLE_RAD),
            0.0
        );
    }

    #[test]
    fn the_port_opens_and_shuts_smoothly_across_the_window() {
        let evo = 2.4;
        let end = CYCLE_RAD * 0.5;
        let mid = (evo + end) * 0.5;
        let peak = port_area_fraction(mid, evo, RAMP, CYCLE_RAD);
        assert!(
            (peak - 1.0).abs() < 1.0e-9,
            "should be fully open at mid-window"
        );

        // Shut at both ends, never negative, never above one.
        for i in 0..=200 {
            let psi = evo + (end - evo) * f64::from(i) / 200.0;
            let a = port_area_fraction(psi, evo, RAMP, CYCLE_RAD);
            assert!((0.0..=1.0).contains(&a), "area {a} at psi {psi}");
        }
    }

    #[test]
    fn samples_stay_bounded_however_violent_the_source() {
        let a = audio();
        let mut ac = bank(1024);
        for i in 0..1024 {
            let source = if i % 2 == 0 { 1.0e6 } else { -1.0e6 };
            ac.push(&a, source, 6.0, DT);
        }
        let mut out = vec![0.0f32; 1024];
        let n = ac.drain(&mut out);
        assert_eq!(n, 1024);
        for s in &out {
            assert!(s.is_finite() && s.abs() <= 1.0, "sample out of range: {s}");
        }
    }

    #[test]
    fn the_high_pass_removes_a_constant_offset() {
        let a = audio();
        let mut ac = bank(200_000);
        // A steady source is a DC offset: it must decay away, not sit there.
        for _ in 0..200_000 {
            ac.push(&a, 5.0, 6.0, DT);
        }
        let mut out = vec![0.0f32; 200_000];
        ac.drain(&mut out);
        let last = out[199_999];
        assert!(
            last.abs() < 1.0e-3,
            "a constant source should decay to silence, got {last}"
        );
    }

    #[test]
    fn draining_returns_samples_in_order_and_empties_the_buffer() {
        let a = audio();
        let mut ac = bank(64);
        for i in 0..10 {
            ac.push(&a, f64::from(i), 6.0, DT);
        }
        assert_eq!(ac.available(), 10);

        let mut first = vec![0.0f32; 4];
        assert_eq!(ac.drain(&mut first), 4);
        assert_eq!(ac.available(), 6);

        let mut rest = vec![0.0f32; 32];
        assert_eq!(ac.drain(&mut rest), 6);
        assert_eq!(ac.available(), 0);
        assert_eq!(ac.drain(&mut rest), 0, "an empty buffer yields nothing");
    }

    #[test]
    fn overrunning_the_buffer_drops_the_oldest_samples_and_counts_them() {
        let a = audio();
        let mut ac = bank(8);
        for i in 0..20 {
            ac.push(&a, f64::from(i), 6.0, DT);
        }
        assert_eq!(ac.available(), 8, "capacity is the ceiling");
        assert_eq!(ac.dropped(), 12);
    }

    #[test]
    fn reset_clears_both_the_filters_and_the_buffer() {
        let a = audio();
        let mut ac = bank(32);
        for _ in 0..32 {
            ac.push(&a, 100.0, 6.0, DT);
        }
        ac.reset();
        assert_eq!(ac.available(), 0);
        assert_eq!(ac.dropped(), 0);
        assert_eq!(ac, bank(32));
    }
}
