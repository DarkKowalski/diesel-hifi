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
    /// Trapped mass (air plus any burned products), kg.
    pub mass_kg: f64,
    pub temperature_k: f64,
    pub pressure_pa: f64,
    /// Air mass trapped at intake valve closing, kg.
    pub trapped_air_kg: f64,
    /// Fuel latched for the cycle in progress, kg.
    pub fuel_charge_kg: f64,
    /// Cumulative burned fraction already released this cycle, `0..=1`.
    pub burned_fraction: f64,
}

impl CylinderState {
    pub fn new(phase_offset_rad: f64, pressure_pa: f64, temperature_k: f64, mass_kg: f64) -> Self {
        Self {
            phase_offset_rad,
            phase: Phase::Intake,
            mass_kg,
            temperature_k,
            pressure_pa,
            trapped_air_kg: mass_kg,
            fuel_charge_kg: 0.0,
            burned_fraction: 0.0,
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
