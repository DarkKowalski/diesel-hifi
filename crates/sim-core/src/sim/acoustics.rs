//! Exhaust acoustic source.
//!
//! The solver's fixed step is 25 us, which is a sample rate of 40 kHz. That is
//! already an audio rate, so the exhaust sound does not need a synthesiser: one
//! sample per step, taken from the blowdown through the exhaust ports, *is* the
//! signal. What you hear is the same cylinder pressure that drives the crank.
//!
//! ```text
//! source  = sum over cylinders of  a(psi) * (p_cyl - p_exhaust) / p_ambient
//! r       = d(source)/dt          an open pipe radiates the RATE of flow
//! x       = highpass(r)           remove any residual offset
//! y       = lowpass(lowpass(x))   muffler roll-off, two poles
//! sample  = softclip(gain * y)    bounded in [-1, 1]
//! ```
//!
//! The derivative is not a brightness trick. A pipe mouth is an acoustic
//! monopole, and a monopole radiates the time derivative of the volume flow
//! through it. Radiating the flow itself instead loses 6 dB per octave, which
//! pushes almost all the energy below the range an ordinary speaker can
//! reproduce at all — the signal measures fine and is inaudible.
//!
//! Nothing here is published. The manual says nothing about how the engine
//! sounds, so every coefficient is calibrated.
//!
//! Firing frequency falls out of the geometry rather than being programmed: an
//! inline six fires six times per `4*PI`, so at 1500 rpm the fundamental is
//! 75 Hz. Nothing in this module knows the cylinder count.

use crate::config::validate::CYCLE_RAD;
use crate::config::AudioCalibration;

use super::cylinder::Phase;

/// Smooth 0 to 1 ramp, continuous at both ends.
#[inline]
fn ramp(t: f64) -> f64 {
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

/// Contribution of one cylinder to the acoustic source, in square metres.
///
/// Open port area times the pressure difference driving gas through it, scaled
/// by ambient so the result does not depend on the absolute pressure unit. A
/// larger port makes a louder pulse, which is why the effective area is a real
/// input rather than a normalised shape.
///
/// Zero unless the cylinder is actually open to the exhaust; a closed cylinder
/// makes no sound at the tailpipe however high its pressure climbs.
#[inline]
pub fn cylinder_source(
    phase: Phase,
    psi: f64,
    cylinder_pressure_pa: f64,
    exhaust_pressure_pa: f64,
    ambient_pressure_pa: f64,
    port: &Port,
) -> f64 {
    if phase != Phase::Exhaust || ambient_pressure_pa <= 0.0 {
        return 0.0;
    }
    port.area_m2(psi) * (cylinder_pressure_pa - exhaust_pressure_pa) / ambient_pressure_pa
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

/// Filter state and the ring buffer of produced samples.
///
/// The buffer is allocated once when the simulation is constructed and never
/// resized, so producing audio allocates nothing in the hot loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Acoustics {
    /// Whether `previous_source` holds a real predecessor yet.
    primed: bool,
    /// Previous raw source, for the radiation derivative.
    previous_source: f64,
    highpass_previous_input: f64,
    highpass_previous_output: f64,
    lowpass_stage_one: f64,
    lowpass_stage_two: f64,

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
    /// Allocate for a given capacity in samples.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            primed: false,
            previous_source: 0.0,
            highpass_previous_input: 0.0,
            highpass_previous_output: 0.0,
            lowpass_stage_one: 0.0,
            lowpass_stage_two: 0.0,
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
        self.primed = false;
        self.previous_source = 0.0;
        self.highpass_previous_input = 0.0;
        self.highpass_previous_output = 0.0;
        self.lowpass_stage_one = 0.0;
        self.lowpass_stage_two = 0.0;
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

    /// Filter one raw source value and push the resulting sample.
    #[inline]
    pub fn push(&mut self, audio: &AudioCalibration, source: f64, dt: f64) {
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
        if !self.primed {
            self.previous_source = source;
            self.primed = true;
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

        let sample = soft_clip(
            audio.exhaust_gain * self.lowpass_stage_two,
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

    const PORT: Port = Port {
        effective_area_m2: 2.5e-3,
        valve_open_rad: 2.4,
        ramp_rad: RAMP,
    };
    use crate::config::validate::CYCLE_RAD;

    fn audio() -> AudioCalibration {
        AudioCalibration {
            reference_spl_db: 90.0,
            exhaust_gain: 0.35,
            highpass_cutoff_hz: 25.0,
            lowpass_cutoff_hz: 4_000.0,
            soft_clip_knee: 0.9,
        }
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
    fn a_closed_cylinder_contributes_nothing_however_high_its_pressure() {
        let s = cylinder_source(Phase::Closed, 0.0, 20.0e6, 200_000.0, 101_325.0, &PORT);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn blowdown_produces_a_positive_source_and_backflow_a_negative_one() {
        let mid = (2.4 + CYCLE_RAD * 0.5) * 0.5;
        let out = cylinder_source(Phase::Exhaust, mid, 600_000.0, 200_000.0, 101_325.0, &PORT);
        let back = cylinder_source(Phase::Exhaust, mid, 150_000.0, 200_000.0, 101_325.0, &PORT);
        assert!(out > 0.0);
        assert!(back < 0.0);
    }

    #[test]
    fn samples_stay_bounded_however_violent_the_source() {
        let a = audio();
        let mut ac = Acoustics::new(1024);
        for i in 0..1024 {
            let source = if i % 2 == 0 { 1.0e6 } else { -1.0e6 };
            ac.push(&a, source, 2.5e-5);
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
        let mut ac = Acoustics::new(200_000);
        // A steady source is a DC offset: it must decay away, not sit there.
        for _ in 0..200_000 {
            ac.push(&a, 5.0, 2.5e-5);
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
        let mut ac = Acoustics::new(64);
        for i in 0..10 {
            ac.push(&a, f64::from(i), 2.5e-5);
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
        let mut ac = Acoustics::new(8);
        for i in 0..20 {
            ac.push(&a, f64::from(i), 2.5e-5);
        }
        assert_eq!(ac.available(), 8, "capacity is the ceiling");
        assert_eq!(ac.dropped(), 12);
    }

    #[test]
    fn reset_clears_both_the_filters_and_the_buffer() {
        let a = audio();
        let mut ac = Acoustics::new(32);
        for _ in 0..32 {
            ac.push(&a, 100.0, 2.5e-5);
        }
        ac.reset();
        assert_eq!(ac.available(), 0);
        assert_eq!(ac.dropped(), 0);
        assert_eq!(ac, Acoustics::new(32));
    }
}
