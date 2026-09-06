//! Per-cylinder phase and single-zone gas state.
//!
//! Cylinder phase is expressed as a *signed cycle angle* `psi`, measured from
//! that cylinder's firing top dead centre and covering `[-2*PI, 2*PI)`:
//!
//! ```text
//!   psi = -2*PI .. ivc     intake stroke and early compression (open to intake)
//!   psi =  ivc  .. evo     closed: compression, combustion, expansion
//!   psi =  evo  ..  2*PI   blowdown and exhaust stroke (open to exhaust)
//! ```
//!
//! `psi` increases monotonically through firing TDC (`psi = 0`), so the burn
//! integral never has to cope with a wrap in the middle of combustion. The only
//! discontinuity is at TDC overlap, inside the gas-exchange region.

use crate::config::validate::CYCLE_RAD;
use crate::sim::heat_release::Profile;
use crate::sim::injection::Event;

/// Which boundary condition currently applies to a cylinder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Open to the intake manifold.
    Intake,
    /// Valves closed: compression, combustion, expansion.
    Closed,
    /// Open to the exhaust manifold.
    Exhaust,
}

/// Single-zone state for one cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CylinderState {
    /// Cycle-angle offset of this cylinder relative to cylinder 1.
    pub phase_offset_rad: f64,
    pub phase: Phase,

    /// Trapped mass, kg: fresh charge plus residual plus injected fuel.
    pub mass_kg: f64,
    pub temperature_k: f64,
    pub pressure_pa: f64,

    /// Motored (no-combustion) pressure trace, needed by the Woschni
    /// combustion-velocity term and useful for comparison in the UI.
    pub motored_pressure_pa: f64,
    pub motored_temperature_k: f64,

    /// Fresh air trapped at intake valve closing, kg. The smoke limit uses this,
    /// not the total mass, so residual gas correctly reduces available oxygen.
    pub trapped_air_kg: f64,
    /// Burned gas carried over from the previous cycle, kg.
    pub residual_kg: f64,
    pub residual_temperature_k: f64,

    /// The injection event scheduled for the cycle in progress.
    pub injection: Event,
    /// The burn profile, built when injection actually starts.
    pub profile: Profile,
    pub profile_ready: bool,
    /// Cumulative burned fraction released so far this cycle, `0..=1`.
    pub burned_fraction: f64,
    /// Heat lost to the walls during the cycle in progress, joules.
    pub wall_heat_loss_j: f64,

    /// This cylinder's injector delivery multiplier, about 1.0.
    ///
    /// Drawn once at reset from the reset seed. Six bit-identical cylinders sum
    /// to a mathematically pure harmonic comb with no jitter and no amplitude
    /// scatter, and the ear hears that immediately as synthesised. Nothing in a
    /// real six is identical to anything else in it.
    pub fuel_trim: f64,
    /// This cylinder's exhaust port area multiplier, about 1.0.
    pub exhaust_area_trim: f64,
}

impl CylinderState {
    pub fn new(phase_offset_rad: f64, pressure_pa: f64, temperature_k: f64, mass_kg: f64) -> Self {
        Self {
            phase_offset_rad,
            phase: Phase::Intake,
            mass_kg,
            temperature_k,
            pressure_pa,
            motored_pressure_pa: pressure_pa,
            motored_temperature_k: temperature_k,
            trapped_air_kg: mass_kg,
            residual_kg: 0.0,
            residual_temperature_k: temperature_k,
            injection: Event::NONE,
            profile: Profile::NONE,
            profile_ready: false,
            burned_fraction: 0.0,
            wall_heat_loss_j: 0.0,
            // Neutral until a reset draws the real trims. A cylinder built
            // without them is an average cylinder, not a broken one.
            fuel_trim: 1.0,
            exhaust_area_trim: 1.0,
        }
    }

    /// Residual gas as a fraction of total trapped mass.
    pub fn residual_fraction(&self) -> f64 {
        let total = self.residual_kg + self.trapped_air_kg;
        if total > 0.0 {
            self.residual_kg / total
        } else {
            0.0
        }
    }
}

/// Signed cycle angle for a cylinder, in `[-2*PI, 2*PI)`.
///
/// `crank_angle_rad` is the global crank angle wrapped into `[0, 4*PI)`.
#[inline]
pub fn signed_cycle_angle(crank_angle_rad: f64, phase_offset_rad: f64) -> f64 {
    let mut phi = (crank_angle_rad - phase_offset_rad) % CYCLE_RAD;
    if phi < 0.0 {
        phi += CYCLE_RAD;
    }
    if phi >= CYCLE_RAD * 0.5 {
        phi - CYCLE_RAD
    } else {
        phi
    }
}

/// Which phase a signed cycle angle falls in.
#[inline]
pub fn phase_at(psi: f64, intake_valve_close_rad: f64, exhaust_valve_open_rad: f64) -> Phase {
    if psi <= intake_valve_close_rad {
        Phase::Intake
    } else if psi >= exhaust_valve_open_rad {
        Phase::Exhaust
    } else {
        Phase::Closed
    }
}
