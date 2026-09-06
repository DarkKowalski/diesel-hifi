//! The exhaust system downstream of the manifold: runners, turbine, box, duct.
//!
//! Before this module the acoustic path stopped at a 0-D lumped manifold, and
//! what stood in for everything after it was a two-pole lowpass called
//! `audio.lowpass_cutoff_hz`. That is a tone control, not an exhaust system. A
//! real truck has metres of pipe downstream of the turbine, and an exhaust note
//! is very largely the standing-wave behaviour of that pipe: the pulse train is
//! the excitation, not the sound.
//!
//! ```text
//! per cylinder:   q_i                     net port mass flow, kg/s
//! runners:        q = sum of delay(q_i, L_i / c)    P3, see below
//! turbine:        shelf(q)                 insertion loss, rising with frequency
//! duct:           waveguide of length L + V/S, substrate loss per traverse,
//!                 tip reflection, radiation loss
//! out:            the volume velocity at the pipe mouth, p+ - p-
//! ```
//!
//! What leaves at the end is the mouth's *flow*, not the pressure there. An open
//! end is a pressure node: the incident and reflected waves very nearly cancel,
//! so `p+ + p-` tends to zero at low frequency by construction and is not what
//! reaches a listener. See the comment on the return value.
//!
//! # Why this is simulation and not presentation
//!
//! Everything here is the *engine*, and none of it changes when the listener
//! moves. A duct driven by real mass flow, at a speed of sound taken from the
//! exhaust temperature the model already integrates, is gas dynamics. The cab
//! transfer path in `web/src/lib/cabin.ts` is the other thing — where you are
//! standing — and it lives there and says so.
//!
//! Nothing here is published. The source manual describes the aftertreatment
//! architecture and says nothing whatever about its acoustics, so every value
//! this module reads is `calibrated` and carries a purpose and a safe range.
//!
//! # P3: why the firing order became audible
//!
//! A real inline six has a log manifold whose runners span roughly 100 mm to
//! 700 mm end to end, so pulses reach the turbine at different times and with
//! different filtering. Before this module the six cylinders were evenly spaced
//! by construction and discharged into a single lumped node, and a sum of six
//! identically-shaped pulses at even spacing is the same sum whichever cylinder
//! is assigned to which slot — so `geometry.firing_order` was validated as a
//! permutation, used to derive phase offsets, and could not change one sample of
//! output. Runner length is assigned by *cylinder index*, because that is what
//! physical position on the head decides; the firing order then chooses the
//! order in which those different delays are exercised. Permuting it now changes
//! the waveform, which is what makes it real rather than decorative.
//!
//! # Numerical safety
//!
//! Both buffers are allocated at construction and never resized, so the hot loop
//! allocates nothing. They are sized for the coldest gas the duct could
//! plausibly carry rather than for the current temperature, because the delay
//! moves with the speed of sound and a buffer sized for hot gas would be too
//! short the moment the engine cooled. Reads are clamped to the buffer
//! regardless, and the feedback loop's gain is bounded by validation requiring
//! `|open_end_reflection| < 1`.

use crate::config::{ExhaustSystem as ExhaustConfig, GasProperties};

use super::gas;

/// Slowest speed of sound the delay buffers are sized to survive.
///
/// About 163 K for a diesel exhaust gas mixture — far colder than an exhaust
/// ever gets, which is the point: the buffer must be long enough for the longest
/// delay the duct can ever ask for, and the delay is longest when the gas is
/// coldest. Sizing for hot gas would leave the line too short on a cold start.
const MIN_SPEED_OF_SOUND_M_PER_S: f64 = 250.0;

/// Reflection back into the pipe at the manifold end.
///
/// A turbine outlet is not an open end. Acoustically it is much closer to a
/// closed one — a restriction that returns most of what arrives at it — so the
/// wave coming back up the pipe is reflected forwards again with the same sign,
/// which is what lets a standing wave establish itself at all. Slightly under
/// unity for the losses through the wheel.
///
/// Generic duct acoustics rather than anything about this engine, so it is a
/// constant here rather than a configured value. It is also half of the loop
/// gain: with `|open_end_reflection| < 1` enforced by validation, the round trip
/// can never reach unity.
const MANIFOLD_REFLECTION: f64 = 0.9;

/// A ring buffer read at a fractional delay.
///
/// Fractional because `L / c` is not a whole number of samples and `c` moves
/// with temperature. Rounding to the nearest sample would make the delay jump by
/// a whole step as the engine warmed, and a step in a delay line is a click;
/// worse, a zero-order hold whose step width varies is broadband noise, which is
/// exactly the kind of top end this whole effort is trying to add honestly
/// rather than to manufacture.
#[derive(Debug, Clone, PartialEq)]
struct DelayLine {
    buffer: Vec<f64>,
    write: usize,
}

impl DelayLine {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(4)],
            write: 0,
        }
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    /// Write one sample and read the one `delay` samples behind it.
    #[inline]
    fn tick(&mut self, x: f64, delay: f64) -> f64 {
        let n = self.buffer.len();
        self.buffer[self.write] = x;
        self.write = (self.write + 1) % n;

        // Clamped rather than trusted. Validation bounds the configured length
        // and the buffer is sized for gas colder than any exhaust, but the
        // temperature that sets `c` is solver state, and solver state is exactly
        // the thing that must not be able to index out of bounds.
        let delay = if delay.is_finite() {
            delay.clamp(0.0, (n - 2) as f64)
        } else {
            0.0
        };
        let whole = delay.floor();
        let fraction = delay - whole;
        let whole = whole as usize;

        let near = (self.write + n - 1 - whole) % n;
        let far = (near + n - 1) % n;
        self.buffer[near] * (1.0 - fraction) + self.buffer[far] * fraction
    }
}

/// One-pole lowpass coefficient for a cutoff at a given step.
#[inline]
fn pole(cutoff_hz: f64, dt: f64) -> f64 {
    let rc = 1.0 / (std::f64::consts::TAU * cutoff_hz);
    dt / (rc + dt)
}

/// Speed of sound in the exhaust gas, from the temperature the model integrates.
///
/// `c = sqrt(gamma R T)`, with `gamma` taken from the same temperature-dependent
/// specific heat the rest of the solver uses rather than assumed constant. This
/// is what makes the pipe resonance shift as the exhaust heats — sharper hot,
/// flatter cold — as a consequence of the model rather than as a scripted
/// effect. It costs one square root per step.
#[inline]
pub fn speed_of_sound_m_per_s(gas: &GasProperties, temperature_k: f64) -> f64 {
    let t = temperature_k.max(1.0);
    let cv = gas::cv_j_per_kg_k(gas, t);
    let gamma = (cv + gas.gas_constant_j_per_kg_k) / cv.max(1.0e-9);
    (gamma * gas.gas_constant_j_per_kg_k * t).sqrt()
}

/// The runners, the turbine, the aftertreatment box, and the tailpipe.
#[derive(Debug, Clone, PartialEq)]
pub struct ExhaustSystem {
    /// One delay line per cylinder slot, and the runner length each stands for.
    runners: Vec<DelayLine>,
    runner_length_m: Vec<f64>,

    /// Acoustic length of the duct: tailpipe plus the box's equivalent length.
    acoustic_length_m: f64,

    forward: DelayLine,
    backward: DelayLine,
    /// Wave arriving back at the manifold end.
    returning: f64,
    /// One-pole state for radiation loss on the reflected wave.
    radiation: f64,
    /// One-pole state for the turbine's insertion loss shelf.
    turbine: f64,
}

impl ExhaustSystem {
    /// Allocate for a configuration, sized for the longest delay it permits.
    pub fn new(config: &ExhaustConfig, cylinder_slots: usize, dt: f64) -> Self {
        let sample_rate_hz = if dt > 0.0 { 1.0 / dt } else { 40_000.0 };
        let samples_for = |length_m: f64| {
            (length_m / MIN_SPEED_OF_SOUND_M_PER_S * sample_rate_hz).ceil() as usize + 4
        };

        // A compliance on a duct behaves, at the wavelengths that set the first
        // few resonances, like an extra length of that duct. The DPF and SCR box
        // is a compliance of known physical size, so it lengthens the pipe
        // acoustically rather than being a filter bolted across it — which is
        // both the simpler model and the one that keeps the volume meaning a
        // volume. The approximation is a low-frequency one and stops describing
        // the box once a wavelength is comparable with its own dimensions.
        let equivalent_length_m = if config.duct_area_m2 > 0.0 {
            config.aftertreatment_volume_m3 / config.duct_area_m2
        } else {
            0.0
        };
        let acoustic_length_m = config.tailpipe_length_m + equivalent_length_m;

        // Runner length by cylinder index, spread evenly across the configured
        // span. A span rather than a list per cylinder, so this section carries
        // no cylinder count and a four-cylinder configuration uses it unchanged.
        let span = config.runner_length_max_m - config.runner_length_min_m;
        let runner_length_m: Vec<f64> = (0..cylinder_slots)
            .map(|index| {
                if cylinder_slots <= 1 {
                    config.runner_length_min_m
                } else {
                    config.runner_length_min_m + span * index as f64 / (cylinder_slots - 1) as f64
                }
            })
            .collect();

        let runner_capacity = samples_for(config.runner_length_max_m);
        let duct_capacity = samples_for(acoustic_length_m);

        Self {
            runners: (0..cylinder_slots)
                .map(|_| DelayLine::new(runner_capacity))
                .collect(),
            runner_length_m,
            acoustic_length_m,
            forward: DelayLine::new(duct_capacity),
            backward: DelayLine::new(duct_capacity),
            returning: 0.0,
            radiation: 0.0,
            turbine: 0.0,
        }
    }

    /// Empty every line and every filter.
    ///
    /// A duct carrying a stale standing wave across a reset would sound it out
    /// into a freshly reset engine, which is the click
    /// `resetting_a_running_engine_does_not_click` exists to catch. Two
    /// identically reset simulations must be bit-identical in every field, so
    /// the samples are cleared and not merely the cursors.
    pub fn reset(&mut self) {
        for runner in &mut self.runners {
            runner.clear();
        }
        self.forward.clear();
        self.backward.clear();
        self.returning = 0.0;
        self.radiation = 0.0;
        self.turbine = 0.0;
    }

    /// Acoustic length of the duct, tailpipe plus the box's equivalent length.
    pub fn acoustic_length_m(&self) -> f64 {
        self.acoustic_length_m
    }

    /// Advance one step and return the wave leaving the pipe mouth.
    ///
    /// `cylinder_flow_kg_per_s` is the net port flow per cylinder this step, the
    /// same quantity `flow::exchange` reported. `temperature_k` is the exhaust
    /// manifold temperature, which sets the speed of sound and therefore every
    /// delay in here.
    #[inline]
    pub fn advance(
        &mut self,
        config: &ExhaustConfig,
        gas: &GasProperties,
        cylinder_flow_kg_per_s: &[f64],
        temperature_k: f64,
        dt: f64,
    ) -> f64 {
        if dt <= 0.0 {
            return 0.0;
        }
        let sample_rate_hz = 1.0 / dt;
        let c = speed_of_sound_m_per_s(gas, temperature_k).max(1.0);

        // --- runners into the manifold junction (P3) ---
        let mut junction = 0.0;
        for (index, runner) in self.runners.iter_mut().enumerate() {
            let flow = cylinder_flow_kg_per_s.get(index).copied().unwrap_or(0.0);
            let delay = self.runner_length_m[index] / c * sample_rate_hz;
            junction += runner.tick(flow, delay);
        }

        // --- turbine insertion loss ---
        //
        // A turbine is a large silencer: order 10 to 20 dB, rising with
        // frequency, and it smears the pulse on the way through. A shelf is the
        // honest shape for that — everything below the corner passes, everything
        // above is attenuated by the configured loss — and it is the term that
        // makes this read as turbocharged rather than as open-piped.
        let turbine_pole = pole(config.turbine_loss_cutoff_hz.max(1.0e-6), dt);
        self.turbine += (junction - self.turbine) * turbine_pole;
        let high = junction - self.turbine;
        let high_gain = 10.0_f64.powf(-config.turbine_insertion_loss_db / 20.0);
        let drive = self.turbine + high * high_gain;

        // --- the duct ---
        //
        // A bidirectional waveguide. The wave the manifold end launches is the
        // fresh pulse plus whatever came back and was turned around again; it
        // travels `L / c` to the tip, reflects there with a negative coefficient
        // because an open end is a pressure node, and comes back.
        //
        // The substrate loss is taken on each traverse, because the box is
        // inside the length being traversed. The volume above already made the
        // box a compliance that lengthens the duct; this is the other half of
        // what a box is. A wall-flow filter pushes the gas through porous
        // ceramic, and a flow resistance is broadband, so it is a scalar rather
        // than another corner frequency.
        //
        // Without it the only damping in the whole duct is at the mouth, and a
        // pipe damped only at its mouth is an organ pipe: a loop gain of 0.72 per
        // round trip is a T60 near 360 ms, long enough that whichever firing
        // order lands on a resonance swallows the rest of the note and the engine
        // drones on one pitch instead of thrumming.
        let delay = self.acoustic_length_m / c * sample_rate_hz;
        let substrate = config.aftertreatment_transmission;
        let into_pipe = drive + self.returning * MANIFOLD_REFLECTION;
        let at_tip = self.forward.tick(into_pipe, delay) * substrate;

        // Radiation loss: a pipe mouth is a poor radiator at low frequency and a
        // good one high up, so what comes *back* is low-passed. This is why a
        // real pipe's resonances are sharp low down and damped higher.
        let radiation_pole = pole(config.radiation_cutoff_hz.max(1.0e-6), dt);
        self.radiation += (at_tip - self.radiation) * radiation_pole;
        let reflected = config.open_end_reflection * self.radiation;
        self.returning = self.backward.tick(reflected, delay) * substrate;

        // What radiates is the mouth's *volume velocity*, not the pressure there.
        //
        // This line carries pressure waves — that is the convention both
        // reflection coefficients are written in — and at an open end the two
        // travelling components very nearly cancel, because an open end is a
        // pressure node. So `p+ + p-` tends to zero at low frequency by
        // definition, and it is not what a listener hears. The mouth's volume
        // velocity is `S (p+ - p-) / (rho c)`, which is at a maximum exactly
        // where the pressure is at a minimum, and it is that flow which
        // `acoustics.rs` differentiates into far-field pressure.
        //
        // The constant `S / (rho c)` is absorbed into the calibrated exhaust
        // gain, as the radiation derivative's own constant already is.
        let radiated = at_tip - reflected;
        if radiated.is_finite() {
            radiated
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 2.5e-5;

    fn gas() -> GasProperties {
        GasProperties {
            gas_constant_j_per_kg_k: 287.0,
            cv_reference_j_per_kg_k: 830.0,
            cv_slope_j_per_kg_k2: 0.18,
            reference_temperature_k: 300.0,
        }
    }

    fn config() -> ExhaustConfig {
        ExhaustConfig {
            tailpipe_length_m: 3.5,
            duct_area_m2: 0.008,
            open_end_reflection: -0.8,
            radiation_cutoff_hz: 2_000.0,
            aftertreatment_volume_m3: 0.012,
            aftertreatment_transmission: 0.61,
            turbine_insertion_loss_db: 14.0,
            turbine_loss_cutoff_hz: 400.0,
            runner_length_min_m: 0.10,
            runner_length_max_m: 0.70,
        }
    }

    #[test]
    fn the_speed_of_sound_rises_with_temperature() {
        let g = gas();
        let cold = speed_of_sound_m_per_s(&g, 400.0);
        let hot = speed_of_sound_m_per_s(&g, 900.0);
        assert!(
            hot > cold,
            "hot gas must carry sound faster: {hot} against {cold}"
        );
        // Order of magnitude: a diesel exhaust at 700 K is around 500 m/s.
        let mid = speed_of_sound_m_per_s(&g, 700.0);
        assert!(
            (420.0..600.0).contains(&mid),
            "{mid} m/s is not plausible for exhaust gas at 700 K"
        );
    }

    #[test]
    fn the_box_lengthens_the_duct_by_its_equivalent_length() {
        let system = ExhaustSystem::new(&config(), 6, DT);
        // 0.012 m^3 over 0.008 m^2 is 1.5 m, on top of 3.5 m of tailpipe.
        assert!((system.acoustic_length_m() - 5.0).abs() < 1.0e-9);
    }

    #[test]
    fn silence_in_is_silence_out() {
        let mut system = ExhaustSystem::new(&config(), 6, DT);
        let flows = [0.0; 6];
        for _ in 0..10_000 {
            let out = system.advance(&config(), &gas(), &flows, 700.0, DT);
            assert_eq!(out, 0.0, "an unexcited duct must be silent");
        }
    }

    #[test]
    fn a_pulse_comes_back_around_and_decays() {
        // The duct is a feedback loop, so a single pulse must ring and then die.
        // Ringing for ever would be an unstable loop; not ringing at all would be
        // a pipe with no reflection, which is a pipe with no note.
        let mut system = ExhaustSystem::new(&config(), 6, DT);
        let mut flows = [0.0; 6];
        flows[0] = 1.0;
        let mut peak_early = 0.0f64;
        let mut peak_late = 0.0f64;
        for step in 0..40_000 {
            let out = system.advance(&config(), &gas(), &flows, 700.0, DT);
            flows[0] = 0.0;
            assert!(out.is_finite(), "the duct went non-finite at step {step}");
            if step < 8_000 {
                peak_early = peak_early.max(out.abs());
            } else if step > 30_000 {
                peak_late = peak_late.max(out.abs());
            }
        }
        assert!(peak_early > 0.0, "a pulse must reach the pipe mouth");
        assert!(
            peak_late < peak_early * 0.5,
            "the duct must decay: {peak_late} still present against an initial {peak_early}"
        );
    }

    #[test]
    fn a_hotter_duct_resonates_higher() {
        // The pipe resonance is c / 4L and c goes as the square root of
        // temperature, so a hot exhaust must ring sharper than a cold one. This
        // is the emergent behaviour the module is for: nothing schedules it.
        let period = |temperature_k: f64| {
            let mut system = ExhaustSystem::new(&config(), 6, DT);
            let mut flows = [0.0; 6];
            flows[0] = 1.0;
            // Time between the first two returns of the pulse is the round trip.
            let mut first = None;
            let mut second = None;
            for step in 0..40_000 {
                let out = system.advance(&config(), &gas(), &flows, temperature_k, DT);
                flows[0] = 0.0;
                if out.abs() > 0.01 {
                    match (first, second) {
                        (None, _) => first = Some(step),
                        (Some(arrived), None) if step > arrived + 100 => {
                            second = Some(step);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            (first, second)
        };

        let (cold_first, cold_second) = period(400.0);
        let (hot_first, hot_second) = period(900.0);
        let cold = cold_second.expect("cold duct returns") - cold_first.expect("cold duct arrives");
        let hot = hot_second.expect("hot duct returns") - hot_first.expect("hot duct arrives");
        assert!(
            hot < cold,
            "a hot duct must have a shorter round trip: {hot} samples against {cold}"
        );
    }

    #[test]
    fn a_longer_duct_has_a_longer_round_trip() {
        // The pipe's note is c / 4L, so length is what sets its pitch. Measured
        // as the round trip a pulse takes to come back, which is the same thing
        // seen in the time domain and does not need a transform.
        let round_trip = |tailpipe_length_m: f64| {
            let cfg = ExhaustConfig {
                tailpipe_length_m,
                ..config()
            };
            let mut system = ExhaustSystem::new(&cfg, 1, DT);
            let mut drive = [1.0];
            let (mut first, mut second) = (None, None);
            for step in 0..80_000 {
                let out = system.advance(&cfg, &gas(), &drive, 700.0, DT);
                drive[0] = 0.0;
                if out.abs() > 0.01 {
                    match (first, second) {
                        (None, _) => first = Some(step),
                        (Some(arrived), None) if step > arrived + 50 => {
                            second = Some(step);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            second.expect("the pulse returns") - first.expect("the pulse arrives")
        };

        let short = round_trip(1.5);
        let long = round_trip(8.0);
        assert!(
            long > short * 2,
            "8 m of pipe should take much longer to ring round than 1.5 m: \
             {long} samples against {short}"
        );
    }

    #[test]
    fn the_runners_give_each_cylinder_its_own_arrival_time() {
        let system = ExhaustSystem::new(&config(), 6, DT);
        assert_eq!(system.runner_length_m.len(), 6);
        assert!((system.runner_length_m[0] - 0.10).abs() < 1.0e-9);
        assert!((system.runner_length_m[5] - 0.70).abs() < 1.0e-9);
        for pair in system.runner_length_m.windows(2) {
            assert!(pair[1] > pair[0], "runner lengths must be distinct");
        }
    }

    #[test]
    fn a_single_cylinder_configuration_still_works() {
        // The span has to degrade sensibly rather than divide by zero.
        let system = ExhaustSystem::new(&config(), 1, DT);
        assert_eq!(system.runner_length_m.len(), 1);
        assert!(system.runner_length_m[0].is_finite());
    }

    #[test]
    fn reset_returns_it_to_a_freshly_built_state() {
        let mut system = ExhaustSystem::new(&config(), 6, DT);
        let mut flows = [0.0; 6];
        flows[2] = 5.0;
        for _ in 0..2_000 {
            system.advance(&config(), &gas(), &flows, 700.0, DT);
            flows[2] = 0.0;
        }
        system.reset();
        assert_eq!(system, ExhaustSystem::new(&config(), 6, DT));
    }

    #[test]
    fn a_lossy_substrate_shortens_the_ring() {
        // The box's volume makes it a compliance that lengthens the duct. Its
        // substrate makes it a resistance, and that is where a real exhaust
        // system's damping lives: a duct damped only at its mouth is an organ
        // pipe, and an organ pipe droning on whichever firing order lands on a
        // resonance is not what a truck sounds like.
        //
        // Measured as ring-down against the direct pulse, which is the thing the
        // ear actually reacts to. The windows are in round trips: at 700 K the
        // duct's is about 770 samples, so the first covers the pulse and its
        // first couple of returns and the second covers the tail.
        let tail = |transmission: f64| {
            let cfg = ExhaustConfig {
                aftertreatment_transmission: transmission,
                ..config()
            };
            let mut system = ExhaustSystem::new(&cfg, 6, DT);
            let mut flows = [0.0; 6];
            flows[0] = 1.0;
            let (mut early, mut late) = (0.0f64, 0.0f64);
            for step in 0..6_000 {
                let out = system.advance(&cfg, &gas(), &flows, 700.0, DT);
                flows[0] = 0.0;
                if step < 2_000 {
                    early += out * out;
                } else {
                    late += out * out;
                }
            }
            late / early.max(1.0e-30)
        };

        let lossless = tail(1.0);
        let lossy = tail(0.61);
        assert!(
            lossy < lossless * 0.25,
            "the substrate must damp the duct: a lossless box left {lossless:.4} of its \
             energy in the tail and a lossy one {lossy:.4}, which is not enough of a \
             difference to be the mechanism this parameter claims to be"
        );
    }

    #[test]
    fn a_hard_open_end_does_not_silence_the_low_end() {
        // The mistake this catches cost the model every bit of its bottom.
        //
        // An open end is a pressure node: the incident and reflected waves
        // cancel there, so `p+ + p-` tends to zero at low frequency however hard
        // the engine is driving the pipe. That quantity is *not* what radiates.
        // The mouth's volume velocity is `p+ - p-`, which is at a maximum
        // precisely where the pressure is at a minimum, and radiating the sum
        // instead of the difference put a factor of `(1 + R) / (1 - R)` on the
        // whole exhaust path below the radiation corner - about -19 dB at the
        // shipped -0.8, which is most of what a truck sounds like.
        //
        // Measured against a nearly anechoic termination at the same frequency,
        // so the duct's own response, the turbine shelf and the runner delay all
        // cancel in the ratio - the trick the turbine test below uses for the
        // same reason. The pipe is short and the frequency low, so this sits far
        // below the first quarter-wave resonance and is not measuring one.
        let peak_at = |open_end_reflection: f64| {
            let cfg = ExhaustConfig {
                tailpipe_length_m: 0.35,
                aftertreatment_volume_m3: 0.0,
                open_end_reflection,
                ..config()
            };
            let mut system = ExhaustSystem::new(&cfg, 1, DT);
            let mut peak = 0.0f64;
            for step in 0..40_000 {
                let t = step as f64 * DT;
                let drive = (std::f64::consts::TAU * 40.0 * t).sin();
                let out = system.advance(&cfg, &gas(), &[drive], 700.0, DT);
                if step > 24_000 {
                    peak = peak.max(out.abs());
                }
            }
            peak
        };

        // Quasi-statically the loop gives `(1 - R) / (1 - 0.9 R)`, so a hard end
        // is very slightly *louder* at the mouth than an open one, not quieter.
        // Radiating the sum gives `(1 + R) / (1 - 0.9 R)` instead, which lands
        // near 0.12 here.
        let ratio = peak_at(-0.8) / peak_at(-0.01);
        assert!(
            ratio > 0.5,
            "a pressure-release end must not stop the pipe radiating at 40 Hz: a hard \
             end came out {ratio:.3} of an anechoic one, which means the mouth's \
             pressure is being radiated rather than its flow"
        );
    }

    #[test]
    fn the_turbine_loss_takes_the_top_off_and_leaves_the_bottom() {
        // Comparing two frequencies through the whole system would measure the
        // duct, not the shelf: at 5 kHz the radiation loss has killed the
        // reflection and the pipe is nearly transparent, while at 60 Hz it sits
        // between a resonance and a null. So hold the frequency and vary the
        // loss instead. The duct's own response then cancels in the ratio and
        // what is left is the parameter's effect alone.
        let run = |loss_db: f64, frequency_hz: f64| {
            let cfg = ExhaustConfig {
                turbine_insertion_loss_db: loss_db,
                ..config()
            };
            let mut system = ExhaustSystem::new(&cfg, 1, DT);
            let mut peak = 0.0f64;
            for step in 0..20_000 {
                let t = step as f64 * DT;
                let drive = (std::f64::consts::TAU * frequency_hz * t).sin();
                let out = system.advance(&cfg, &gas(), &[drive], 700.0, DT);
                if step > 12_000 {
                    peak = peak.max(out.abs());
                }
            }
            peak
        };

        // Well above the 400 Hz corner: the loss should bite hard.
        let high_ratio = run(0.0, 5_000.0) / run(24.0, 5_000.0);
        // Well below it: the loss should barely register.
        let low_ratio = run(0.0, 40.0) / run(24.0, 40.0);

        assert!(
            high_ratio > 4.0,
            "24 dB of turbine loss should be plainly audible at 5 kHz, got a ratio of \
             {high_ratio:.2}"
        );
        assert!(
            low_ratio < 1.5,
            "the same loss should barely touch 40 Hz, got a ratio of {low_ratio:.2}"
        );
        assert!(
            high_ratio > low_ratio * 3.0,
            "the loss must be frequency-selective: {high_ratio:.2} at 5 kHz against \
             {low_ratio:.2} at 40 Hz"
        );
    }
}
