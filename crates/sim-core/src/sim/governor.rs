//! Min-max fuelling governor.
//!
//! Pedal position and the idle governor each request a fuel quantity; the
//! larger wins, as in a conventional diesel min-max governor. An overspeed
//! taper removes fuel between the taper start and cut-out speeds.
//!
//! The governor produces a *fuel* request. It never produces torque.

use crate::config::{Governor, Injection};

/// Radians per second for a given engine speed in rpm.
#[inline]
pub fn rpm_to_rad_per_s(rpm: f64) -> f64 {
    rpm * std::f64::consts::PI / 30.0
}

/// Engine speed in rpm for a given angular velocity.
#[inline]
pub fn rad_per_s_to_rpm(omega: f64) -> f64 {
    omega * 30.0 / std::f64::consts::PI
}

/// Idle governor integrator state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GovernorState {
    pub integral_mg: f64,
}

impl GovernorState {
    pub fn reset(&mut self) {
        self.integral_mg = 0.0;
    }

    /// Advance the integrator by one fixed step and return the idle fuel request
    /// in milligrams per cylinder per cycle.
    #[inline]
    pub fn update(&mut self, governor: &Governor, omega_rad_per_s: f64, dt_s: f64) -> f64 {
        let target = rpm_to_rad_per_s(governor.idle_target_rpm);
        let error = target - omega_rad_per_s;
        self.integral_mg += governor.idle_i_gain_mg_per_rad * error * dt_s;
        self.integral_mg = self.integral_mg.clamp(0.0, governor.idle_integral_limit_mg);
        governor.idle_p_gain_mg_per_rad_s * error + self.integral_mg
    }
}

/// Fraction of demanded fuel permitted at the current speed.
#[inline]
pub fn overspeed_factor(governor: &Governor, rpm: f64) -> f64 {
    let span = governor.overspeed_cutoff_rpm - governor.overspeed_taper_start_rpm;
    ((governor.overspeed_cutoff_rpm - rpm) / span).clamp(0.0, 1.0)
}

/// Combine pedal and idle requests into a fuel demand, in milligrams per
/// cylinder per cycle.
#[inline]
pub fn fuel_demand_mg(
    injection: &Injection,
    governor: &Governor,
    pedal: f64,
    idle_request_mg: f64,
    ignition_on: bool,
    rpm: f64,
) -> f64 {
    if !ignition_on {
        return 0.0;
    }
    let pedal_request = pedal.clamp(0.0, 1.0) * injection.max_fuel_mg_per_cycle;
    let demand = pedal_request.max(idle_request_mg.max(0.0));
    (demand * overspeed_factor(governor, rpm)).clamp(0.0, injection.max_fuel_mg_per_cycle)
}
