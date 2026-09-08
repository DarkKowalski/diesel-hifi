//! The starter motor: a series-wound DC machine geared to the flywheel.
//!
//! Modelled on a Bosch HEF109-M 24 V, 7.8 kW unit with a 12-tooth pinion. What
//! is published about it is a voltage, a power and a tooth count; everything
//! electrical below is calibrated, and `starter_sweep` is what it is calibrated
//! against.
//!
//! ## Why a circuit and not a torque curve
//!
//! This replaces two fields — a stall torque written straight at the crank and a
//! speed for it to taper to. That shape is not wrong about the *shape*: a series
//! machine's torque really does fall very nearly linearly with speed, so a line
//! from stall to no-load is a fair description of one. Three things it cannot do:
//!
//! - **Gear.** Torque applied directly at the crank has no pinion and no ring
//!   gear, so the tooth-mesh rate — which is the frequency a starter whines at,
//!   and the only frequency it whines at — does not exist anywhere in the model.
//! - **Sag.** A starter dragging a 12.8 litre six pulls on the order of a
//!   thousand amps through a battery with real internal resistance, and the
//!   terminals drop several volts answering it. As a cylinder comes up on
//!   compression the crank slows, back-EMF falls with it, current rises, the
//!   terminals sag further and the torque with them. That is the labouring in a
//!   heavy diesel start. It is a property of the circuit, and a torque-speed
//!   line has no circuit to have it.
//! - **Be a real machine.** The line it replaces peaks at 750 N m at 150 rpm,
//!   which is 11.8 kW of mechanical output at the crank. The largest starter in
//!   this family is rated 9.2 kW.
//!
//! ## The circuit solves in closed form
//!
//! Armature inductance is neglected — see the note at the bottom — which leaves
//! the electrical side algebraic rather than another state variable. A series
//! machine's field winding carries the armature current, so its flux *rises*
//! with that current and saturates:
//!
//! ```text
//! phi(I) = I I_sat / (I_sat + I)   flux: proportional to I, saturating at I_sat
//! T_e    = k phi(I) I              so torque goes as I^2 until the iron gives up
//! E      = k phi(I) w_m            back-EMF through the same constant
//! V      = I R + E                 one loop, one resistance
//! ```
//!
//! Substituting `E` and multiplying out by `(I_sat + I)` gives a quadratic in
//! `I`:
//!
//! ```text
//! R I^2 + (I_sat (R + k w_m) - V) I - V I_sat = 0
//! ```
//!
//! The leading coefficient is positive and the constant term is negative, so it
//! has **exactly one positive root** at every speed and the current falls
//! smoothly to zero as speed rises. No iteration, no history, no allocation, and
//! no branch to choose between roots.
//!
//! That last property is not a convenience, it is the reason for this flux law
//! rather than a simpler one. A field that *falls* with current — `phi = I_sat /
//! (I_sat + I)`, which is the obvious way to write "saturating" and is wrong for
//! a series machine — makes the back-EMF fall with current too, and then the loop
//! equation folds: two roots below one speed, none above it, and the torque steps
//! to zero at the fold. Measured, it stepped 260 N m in one solver step at
//! 340 crank rpm. It also made torque very nearly independent of speed, which is
//! a shunt machine and not this one.
//!
//! The labouring survives the algebra intact. What makes the current swing is
//! `w_m` moving under compression, which it does at the firing rate, and
//! neglecting inductance means the current follows that *immediately* rather than
//! not at all.
//!
//! ## Why the same constant appears in both equations
//!
//! `k` multiplies flux in the torque equation and in the back-EMF equation, and
//! it has to be the same number in both or the machine generates or destroys
//! energy. `T_e w_m` and `E I` are the same product written twice; using two
//! constants would make a starter that is not a motor.
//!
//! ## Why there is no finite no-load speed without the drag term
//!
//! A series machine's torque goes to zero only as its current does, and its
//! current goes to zero only as its speed goes to infinity. That is the classic
//! series-motor runaway, and it is real: an unloaded series motor destroys
//! itself. `internal_drag_nm_per_rad_s` is what makes the no-load speed finite
//! here, so the cranking speed is where two curves cross rather than a number
//! anybody chose.
//!
//! ## The overrun clutch is why the engine catching does not destroy it
//!
//! Output torque is clamped at zero. A meshed pinion turning slower than the
//! ring gear would otherwise be driven *by* the engine, which at idle would spin
//! this machine to several times its no-load speed. Real starters put a one-way
//! clutch in the drive for exactly that, and the clamp is that clutch. It is also
//! why the relay dropping out at `disengage_rpm` is a tidiness rather than a
//! rescue.
//!
//! ## What is not modelled
//!
//! Armature inductance, so the first few milliseconds of current rise are
//! instantaneous rather than taking an `L/R` of some milliseconds. Battery state
//! of charge and temperature, so `circuit_resistance_ohm` is one number standing
//! for a warm battery on short cables. Commutator and brush noise, which is a
//! separate source from the mesh and a much quieter one. And the solenoid's
//! slight pre-turn of the pinion before the main contacts close: the pinion here
//! seats, and then current arrives.

use crate::config::Starter;
use crate::sim::acoustics::{ramp, valve_area_fraction};

/// How far through engaging or retracting the pinion is.
///
/// Two raw ramp phases in `[0, 1]`, shaped through [`ramp`] on the way out so
/// that neither the torque nor the acoustic drive has a corner in it, let alone
/// a step. The old two-field starter went from nothing to 1500 N m in a single
/// solver step; milestones 11 and 12 were about what a step in a radiated
/// quantity does, and this is the same defect on the torque side.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StarterState {
    /// Mechanical seating of the pinion in the ring gear.
    seat_phase: f64,
    /// Main current, released once the pinion has seated.
    power_phase: f64,
    /// Seconds the solenoid has been energised, for the two-stage sequence.
    energised_s: f64,
    /// Whether the relay has dropped out and is latched out until the key is
    /// released.
    ///
    /// A real start relay latches: once the engine has caught you cannot
    /// re-engage without letting go of the key and turning it again. Modelling
    /// it as a bare speed comparison instead makes the relay *chatter*, because
    /// crank speed ripples by tens of rpm over a firing cycle and the drop-out
    /// threshold sits inside that ripple. Measured, it flipped several times
    /// while the engine accelerated through 420 rpm, and each flip reversed the
    /// direction of the engagement ramp — which put a 1.52 step into the mesh
    /// drive, an edge of exactly the kind this module is careful not to make.
    released: bool,
}

/// What the starter did this step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StarterOutput {
    /// Torque delivered at the crankshaft. Never negative.
    pub crank_torque_nm: f64,
    /// Armature current.
    pub current_a: f64,
    /// Terminal voltage after the supply's own droop.
    pub terminal_voltage_v: f64,
    /// How far the pinion is into mesh, zero to one.
    pub engagement: f64,
    /// Dimensionless tooth-mesh force, driving the starter acoustic path.
    ///
    /// Exactly zero whenever the pinion is out, which is what keeps every steady
    /// operating point bit-identical to the model that had no starter path.
    pub mesh_forcing: f64,
}

impl StarterState {
    /// Clear to a retracted pinion.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether the pinion is in mesh at all.
    ///
    /// The run state uses this to say `cranking` rather than reading a speed
    /// threshold, so the answer comes from where the pinion is instead of from a
    /// number that also had to serve as a torque taper.
    #[inline]
    pub fn is_engaged(&self) -> bool {
        self.seat_phase > 0.0
    }

    /// Advance the starter one step.
    ///
    /// `requested` is the driver holding the key; the relay itself drops out
    /// above `disengage_rpm`, so a driver who never lets go still gets a normal
    /// start. `crank_angle_rad` is the mesh phase's own clock: the ring gear is
    /// bolted to the flywheel, so tooth contacts arrive on crank angle and the
    /// whine tracks cranking speed with nothing scheduling it.
    pub fn advance(
        &mut self,
        cfg: &Starter,
        requested: bool,
        rpm: f64,
        omega_crank_rad_per_s: f64,
        crank_angle_rad: f64,
        dt: f64,
    ) -> StarterOutput {
        // The relay, not the driver. A real start relay is released by the
        // engine catching, and until now this was done in the Svelte UI because
        // the model would not do it.
        //
        // Latching, and it has to be: crank speed ripples by tens of rpm over a
        // firing cycle, so a bare `rpm < disengage_rpm` chatters as the engine
        // accelerates through the threshold. Letting go of the key rearms it,
        // which is what a driver does and what a key switch does.
        if !requested {
            self.released = false;
        } else if rpm >= cfg.disengage_rpm {
            self.released = true;
        }
        let holding = requested && !self.released;

        let seat_rate = dt / cfg.mesh_contact_time_s;
        let direction = if holding { 1.0 } else { -1.0 };
        self.seat_phase = (self.seat_phase + direction * seat_rate).clamp(0.0, 1.0);

        if holding {
            self.energised_s += dt;
        } else {
            self.energised_s = 0.0;
        }

        // Two stages: the pinion seats, and only then do the main contacts
        // close. Doing it the other way round is what wears ring gears out, and
        // it is the sequence this machine's own literature describes.
        let powering = holding && self.energised_s >= cfg.pre_engage_time_s;
        let power_direction = if powering { 1.0 } else { -1.0 };
        self.power_phase = (self.power_phase + power_direction * seat_rate).clamp(0.0, 1.0);

        let engagement = ramp(self.seat_phase);
        let current_factor = ramp(self.power_phase);

        // A retracted pinion is not a quiet starter, it is no starter. Return
        // exact zeros rather than something small: the acoustic path's silence
        // when disengaged is what every steady acceptance figure rests on, and
        // "small" is not "zero".
        if self.seat_phase <= 0.0 && self.power_phase <= 0.0 {
            return StarterOutput {
                crank_torque_nm: 0.0,
                current_a: 0.0,
                terminal_voltage_v: cfg.system_voltage_v,
                engagement: 0.0,
                mesh_forcing: 0.0,
            };
        }

        let ratio = cfg.gear_ratio();
        let omega_motor = omega_crank_rad_per_s * ratio;

        let current_a = armature_current_a(cfg, omega_motor) * current_factor;
        let electromagnetic_nm = electromagnetic_torque_nm(cfg, current_a);

        // Windage and brush drag, and then the one-way clutch. The clamp is the
        // clutch: a pinion cannot be driven by the ring gear.
        let shaft_nm = (electromagnetic_nm - cfg.internal_drag_nm_per_rad_s * omega_motor).max(0.0);
        let crank_torque_nm = shaft_nm * ratio * cfg.mesh_efficiency * engagement;

        StarterOutput {
            crank_torque_nm,
            current_a,
            terminal_voltage_v: cfg.system_voltage_v - current_a * cfg.circuit_resistance_ohm,
            engagement,
            mesh_forcing: self.mesh_forcing(cfg, crank_torque_nm, crank_angle_rad),
        }
    }

    /// The dimensionless force at the tooth mesh.
    ///
    /// Two terms, both contact events rather than tones:
    ///
    /// - **Tooth contact**, a pulse train on crank angle. Its rate is the ring
    ///   gear passing the pinion and its amplitude is the torque being
    ///   transmitted, so the whine rises in pitch as the engine speeds up and
    ///   deepens as each cylinder comes up on compression, with nothing
    ///   scheduling either. The pulse is narrow for the same reason a valve's
    ///   ramp is: spread contact over the whole pitch and what is left is a
    ///   smooth hum with no harmonic content, and a starter whine is buzzy.
    /// - **Seating**, one shaped bump as the pinion goes into mesh and another
    ///   as it comes out. This is the clunk. It is a lumped stand-in for an
    ///   axial contact force, of order one over `mesh_contact_time_s`, in the
    ///   same spirit as the body path being a lumped vehicle response — the
    ///   alternative is a contact-mechanics model of a pinion, and the manual
    ///   publishes nothing whatever about the flywheel housing.
    ///
    /// Torque is normalised by the machine's own stall torque, so
    /// `audio.starter_gain` is a level and not a unit conversion — the same
    /// reason ambient pressure normalises the structural forcing.
    #[inline]
    fn mesh_forcing(&self, cfg: &Starter, crank_torque_nm: f64, crank_angle_rad: f64) -> f64 {
        let stall_nm = stall_crank_torque_nm(cfg);
        let load = if stall_nm > 0.0 {
            crank_torque_nm / stall_nm
        } else {
            0.0
        };

        let teeth = f64::from(cfg.ring_gear_teeth);
        let pitch_phase = (teeth * crank_angle_rad / std::f64::consts::TAU).rem_euclid(1.0);
        let width = cfg.mesh_contact_fraction;
        let contact = valve_area_fraction(pitch_phase, 0.0, 2.0 * width, width);

        // `d(ramp)/dt * mesh_contact_time_s`, evaluated rather than differenced:
        // no history to keep, and it is exactly zero at both ends of the ramp, so
        // the clunk starts and finishes without an edge of its own.
        //
        // Unsigned, deliberately. Going in and coming out are both contact
        // events and both make a noise, so there is nothing for a sign to mean —
        // and a signed version can invert if the ramp ever reverses part-way,
        // which puts a step of up to `pi` into the drive. The latching relay
        // above stops the ramp reversing at all; this makes it unrepresentable.
        //
        // Scaled by `seating_impulse`, and that parameter exists because the two
        // terms are normalised against different things. `load` is transmitted
        // torque over *stall* torque, and stall is a locked-rotor figure the
        // machine never reaches in service — cranking transmits about an eighth
        // of it — while this term is the derivative of a ramp and arrives at
        // `pi/2` regardless. Unscaled, the engagement is thirteen times the
        // whine, and a listener hears a short bark with no starter behind it.
        let seating = cfg.seating_impulse
            * 0.5
            * std::f64::consts::PI
            * (std::f64::consts::PI * self.seat_phase).sin();

        load * contact + seating
    }
}

/// Field flux, in amperes of effective field current.
///
/// Rises with armature current, because in a series machine the field winding
/// *is* in the armature circuit, and saturates towards `field_saturation_a` as
/// the iron gives up.
#[inline]
fn flux_a(cfg: &Starter, current_a: f64) -> f64 {
    let sat = cfg.field_saturation_a;
    let i = current_a.max(0.0);
    i * sat / (sat + i)
}

/// Electromagnetic torque at the armature, before windage and the clutch.
#[inline]
fn electromagnetic_torque_nm(cfg: &Starter, current_a: f64) -> f64 {
    cfg.torque_constant_nm_per_a2 * flux_a(cfg, current_a) * current_a.max(0.0)
}

/// The positive root of the loop equation, in amperes.
///
/// `R I^2 + (I_sat (R + k w_m) - V) I - V I_sat = 0`, from the module header.
/// Positive leading coefficient, negative constant term, so there is exactly one
/// positive root at every speed and no branch to choose.
#[inline]
fn armature_current_a(cfg: &Starter, omega_motor_rad_per_s: f64) -> f64 {
    let r = cfg.circuit_resistance_ohm;
    if r <= 0.0 {
        return 0.0;
    }
    let v = cfg.system_voltage_v;
    let sat = cfg.field_saturation_a;
    let omega = omega_motor_rad_per_s.max(0.0);

    let b = sat * (r + cfg.torque_constant_nm_per_a2 * omega) - v;
    let c = -v * sat;
    let discriminant = b * b - 4.0 * r * c;
    if discriminant <= 0.0 {
        return 0.0;
    }
    ((-b + discriminant.sqrt()) / (2.0 * r)).max(0.0)
}

/// Torque at the pinion shaft: electromagnetic, less windage, through the clutch.
#[inline]
fn pinion_torque_nm(cfg: &Starter, omega_motor_rad_per_s: f64, current_a: f64) -> f64 {
    (electromagnetic_torque_nm(cfg, current_a)
        - cfg.internal_drag_nm_per_rad_s * omega_motor_rad_per_s)
        .max(0.0)
}

/// Crank torque at stall, used to normalise the acoustic forcing.
///
/// Not a calibrated value: it falls out of the circuit at zero speed, where
/// there is no back-EMF and the current is simply `V / R`.
#[inline]
pub fn stall_crank_torque_nm(cfg: &Starter) -> f64 {
    let stall_a = armature_current_a(cfg, 0.0);
    electromagnetic_torque_nm(cfg, stall_a) * cfg.gear_ratio() * cfg.mesh_efficiency
}

/// Mechanical output at the pinion shaft, for the calibration sweep.
///
/// The published `rated_power_w` is a *target* for this function to reach, and
/// nothing in the stepping path reads it.
pub fn shaft_power_w(cfg: &Starter, omega_motor_rad_per_s: f64) -> f64 {
    shaft_torque_nm(cfg, omega_motor_rad_per_s) * omega_motor_rad_per_s
}

/// Torque at the pinion shaft, for the calibration sweep.
pub fn shaft_torque_nm(cfg: &Starter, omega_motor_rad_per_s: f64) -> f64 {
    let current_a = armature_current_a(cfg, omega_motor_rad_per_s);
    pinion_torque_nm(cfg, omega_motor_rad_per_s, current_a)
}

/// Current drawn at a given motor speed, for the calibration sweep.
pub fn sweep_current_a(cfg: &Starter, omega_motor_rad_per_s: f64) -> f64 {
    armature_current_a(cfg, omega_motor_rad_per_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped machine, so the numbers below are the ones that ship.
    fn hef109m() -> Starter {
        Starter {
            system_voltage_v: 24.0,
            rated_power_w: 7_800.0,
            pinion_teeth: 12,
            ring_gear_teeth: 145,
            circuit_resistance_ohm: 0.01263,
            torque_constant_nm_per_a2: 9.9752e-5,
            field_saturation_a: 620.0,
            internal_drag_nm_per_rad_s: 0.063,
            mesh_efficiency: 0.9,
            mesh_contact_time_s: 0.008,
            pre_engage_time_s: 0.06,
            disengage_rpm: 420.0,
            mesh_contact_fraction: 0.15,
            seating_impulse: 0.12,
        }
    }

    const DT: f64 = 2.5e-5;

    #[test]
    fn stall_current_is_the_supply_divided_by_the_circuit() {
        // At zero speed there is no back-EMF, so the loop is a resistor. This is
        // the one point where the quadratic has an answer that can be checked
        // without solving it, which is why it is the anchor for the rest.
        let cfg = hef109m();
        let expected = cfg.system_voltage_v / cfg.circuit_resistance_ohm;
        let solved = armature_current_a(&cfg, 0.0);
        assert!(
            (solved - expected).abs() < 1.0e-6,
            "stall current solved as {solved:.1} A where the loop says {expected:.1} A"
        );
    }

    #[test]
    fn the_terminals_sag_under_a_cranking_load() {
        // The whole reason for a circuit rather than a torque curve. A starter
        // pulling several hundred amps has to pull its own supply down with it.
        let cfg = hef109m();
        let cranking_motor_rad_per_s = 200.0 / 60.0 * std::f64::consts::TAU * cfg.gear_ratio();
        let current = armature_current_a(&cfg, cranking_motor_rad_per_s);
        let terminal = cfg.system_voltage_v - current * cfg.circuit_resistance_ohm;
        assert!(
            current > 400.0,
            "cranking a 12.8 litre six should draw hundreds of amps, drew {current:.0} A"
        );
        assert!(
            terminal < cfg.system_voltage_v - 3.0,
            "the terminals should sag several volts under that, sat at {terminal:.1} V"
        );
    }

    #[test]
    fn torque_falls_with_speed_and_reaches_zero_at_a_finite_no_load_speed() {
        // A series machine with no internal drag has no finite no-load speed at
        // all, so this is really a test that the drag term is doing its job.
        let cfg = hef109m();
        let mut previous = f64::INFINITY;
        let mut no_load_rpm = None;
        for step in 0..400 {
            let crank_rpm = f64::from(step);
            let omega = crank_rpm / 60.0 * std::f64::consts::TAU * cfg.gear_ratio();
            let torque = shaft_torque_nm(&cfg, omega);
            assert!(
                torque <= previous + 1.0e-9,
                "torque rose with speed at {crank_rpm} rpm"
            );
            previous = torque;
            if torque <= 0.0 && no_load_rpm.is_none() {
                no_load_rpm = Some(crank_rpm);
            }
        }
        let no_load = no_load_rpm.expect("torque must reach zero below 400 crank rpm");
        assert!(
            (150.0..400.0).contains(&no_load),
            "no-load speed landed at {no_load} crank rpm"
        );
    }

    #[test]
    fn a_retracted_pinion_produces_exactly_nothing() {
        // Assert exact zeros, not smallness. Every steady acceptance figure in
        // README rests on the starter path being silent rather than quiet, and a
        // tolerance here would let a denormal through into the mix.
        let cfg = hef109m();
        let mut state = StarterState::default();
        for _ in 0..10_000 {
            let out = state.advance(&cfg, false, 0.0, 0.0, 1.234, DT);
            assert_eq!(out.crank_torque_nm, 0.0);
            assert_eq!(out.mesh_forcing, 0.0);
            assert_eq!(out.current_a, 0.0);
            assert_eq!(out.engagement, 0.0);
        }
        assert!(!state.is_engaged());
    }

    #[test]
    fn engaging_steps_neither_the_torque_nor_the_mesh_drive() {
        // The defect the old two-field starter had: it went from zero to
        // 1500 N m in one step. A step in a radiated quantity is an impulse and
        // an impulse is white, which is what milestones 11 and 12 were about.
        let cfg = hef109m();
        let mut state = StarterState::default();
        let mut previous_torque = 0.0;
        let mut previous_forcing = 0.0;
        let mut worst_torque: f64 = 0.0;
        let mut worst_forcing: f64 = 0.0;

        // Held at rest, so the only thing moving is the engagement itself.
        for _ in 0..8_000 {
            let out = state.advance(&cfg, true, 0.0, 0.0, 0.0, DT);
            worst_torque = worst_torque.max((out.crank_torque_nm - previous_torque).abs());
            worst_forcing = worst_forcing.max((out.mesh_forcing - previous_forcing).abs());
            previous_torque = out.crank_torque_nm;
            previous_forcing = out.mesh_forcing;
        }

        // A step would be the whole of stall torque in one 25 us sample. This
        // bounds the per-step move to a small fraction of it.
        let stall = stall_crank_torque_nm(&cfg);
        assert!(
            worst_torque < stall * 0.01,
            "torque moved {worst_torque:.1} N m in one step against a stall of \
             {stall:.0} N m, which is an edge rather than a ramp"
        );
        assert!(
            worst_forcing < 0.05,
            "the mesh drive moved {worst_forcing:.4} in one step"
        );
    }

    #[test]
    fn the_relay_drops_out_once_the_engine_catches() {
        // A driver holding the key past idle must not leave the pinion in mesh.
        let cfg = hef109m();
        let mut state = StarterState::default();
        for _ in 0..8_000 {
            state.advance(&cfg, true, 150.0, 15.7, 0.0, DT);
        }
        assert!(state.is_engaged(), "it should be cranking at 150 rpm");

        // Still holding the key, but the engine has caught.
        for _ in 0..8_000 {
            state.advance(&cfg, true, 560.0, 58.6, 0.0, DT);
        }
        let out = state.advance(&cfg, true, 560.0, 58.6, 0.0, DT);
        assert!(!state.is_engaged(), "the relay must release above idle");
        assert_eq!(out.crank_torque_nm, 0.0);
        assert_eq!(out.mesh_forcing, 0.0);
    }

    #[test]
    fn the_main_current_waits_for_the_pinion_to_seat() {
        // The two-stage sequence. Current before mesh is what chews ring gears.
        let cfg = hef109m();
        let mut state = StarterState::default();
        let seated_at = (cfg.mesh_contact_time_s / DT).ceil() as usize;

        let mut out = StarterOutput::default();
        for _ in 0..seated_at {
            out = state.advance(&cfg, true, 0.0, 0.0, 0.0, DT);
        }
        assert!(
            (out.engagement - 1.0).abs() < 1.0e-9,
            "the pinion should be seated after {seated_at} steps"
        );
        assert_eq!(
            out.current_a, 0.0,
            "no main current until the pre-engage delay has run"
        );

        let powered_at = ((cfg.pre_engage_time_s + cfg.mesh_contact_time_s) / DT).ceil() as usize;
        for _ in seated_at..=powered_at {
            out = state.advance(&cfg, true, 0.0, 0.0, 0.0, DT);
        }
        assert!(
            out.current_a > 1_000.0,
            "the main contacts should be closed by now, drew {:.0} A",
            out.current_a
        );
    }

    #[test]
    fn the_overrun_clutch_stops_the_engine_driving_the_starter() {
        // Above no-load speed a meshed pinion would be driven by the ring gear.
        // Real starters put a one-way clutch there; the clamp is that clutch.
        let cfg = hef109m();
        let fast = 2_000.0 / 60.0 * std::f64::consts::TAU * cfg.gear_ratio();
        assert_eq!(shaft_torque_nm(&cfg, fast), 0.0);
        assert_eq!(shaft_power_w(&cfg, fast), 0.0);
    }

    #[test]
    fn the_mesh_drive_arrives_at_the_ring_gear_tooth_rate() {
        // The frequency comes from the tooth count and the crank angle, so this
        // counts contacts over one revolution and expects one per tooth.
        let cfg = hef109m();
        let mut state = StarterState::default();

        // Seat and power up first, at rest, so only the sweep below moves.
        for _ in 0..12_000 {
            state.advance(&cfg, true, 0.0, 0.0, 0.0, DT);
        }

        // One crank revolution, swept by hand so the count is exact.
        const SAMPLES: usize = 200_000;
        let mut contacts = 0;
        let mut was_open = false;
        for i in 0..SAMPLES {
            let angle = std::f64::consts::TAU * i as f64 / SAMPLES as f64;
            let out = state.advance(&cfg, true, 100.0, 10.5, angle, DT);
            let open = out.mesh_forcing.abs() > 1.0e-12;
            if open && !was_open {
                contacts += 1;
            }
            was_open = open;
        }
        assert_eq!(
            contacts, cfg.ring_gear_teeth as usize,
            "one revolution should give one contact per ring gear tooth"
        );
    }

    #[test]
    fn a_bigger_ring_gear_raises_the_mesh_rate_and_the_reduction_together() {
        // The two are the same number seen twice, which is the point of deriving
        // the ratio from the tooth counts rather than configuring it.
        let small = hef109m();
        let mut large = hef109m();
        large.ring_gear_teeth = 174;
        assert!(large.gear_ratio() > small.gear_ratio());
        assert!(stall_crank_torque_nm(&large) > stall_crank_torque_nm(&small));
    }
}
