//! Explicit crankshaft torque terms.
//!
//! Gas and pumping torque come from cylinder pressure through slider-crank
//! geometry: `T = (p_cyl - p_crankcase) * dV/dtheta`. No term writes a target
//! torque into crank acceleration.

use crate::config::{Friction, Load};
use crate::geometry::SliderCrank;

/// Torque contribution of one cylinder at its current angle.
///
/// `dv_dtheta` is `dV/dtheta` for that cylinder; the sign convention follows
/// the geometry, so a descending piston under pressure produces positive torque.
#[inline]
pub fn cylinder_torque_nm(pressure_pa: f64, crankcase_pressure_pa: f64, dv_dtheta: f64) -> f64 {
    (pressure_pa - crankcase_pressure_pa) * dv_dtheta
}

/// Chen-Flynn style friction mean effective pressure, in pascals.
#[inline]
pub fn fmep_pa(
    friction: &Friction,
    peak_cylinder_pressure_pa: f64,
    piston_speed_m_per_s: f64,
) -> f64 {
    friction.fmep_constant_pa
        + friction.fmep_peak_pressure_coeff * peak_cylinder_pressure_pa
        + friction.fmep_piston_speed_coeff_pa_s_per_m * piston_speed_m_per_s
        + friction.fmep_piston_speed_sq_coeff * piston_speed_m_per_s * piston_speed_m_per_s
}

/// Friction torque, always resisting. `total_displacement_m3` is swept volume
/// for the whole engine; one four-stroke cycle spans `4 * PI` radians.
#[inline]
pub fn friction_torque_nm(fmep_pa: f64, total_displacement_m3: f64) -> f64 {
    fmep_pa * total_displacement_m3 / (4.0 * std::f64::consts::PI)
}

/// Accessory drive torque, always resisting.
#[inline]
pub fn accessory_torque_nm(load: &Load, omega_rad_per_s: f64) -> f64 {
    load.accessory_torque_constant_nm + load.accessory_torque_per_rad_s * omega_rad_per_s.abs()
}

/// Starter torque, tapering linearly to zero at the cut-out speed.
#[inline]
pub fn starter_torque_nm(load: &Load, engaged: bool, rpm: f64) -> f64 {
    if !engaged {
        return 0.0;
    }
    let factor = (1.0 - rpm / load.starter_cutout_rpm).clamp(0.0, 1.0);
    load.starter_torque_nm * factor
}

/// Mean piston speed for the given crank speed.
#[inline]
pub fn piston_speed_m_per_s(slider: &SliderCrank, omega_rad_per_s: f64) -> f64 {
    slider.mean_piston_speed_m_per_s(omega_rad_per_s)
}
