//! Ignition delay, Hardenberg-Hase correlation.
//!
//! The correlation returns delay directly in *crank-angle degrees*, so it is
//! already speed-aware: at a fixed physical delay, more crank angle passes at
//! higher engine speed.
//!
//! ```text
//! E_A    = 618840 / (CN + 25)                            J/mol
//! tau_id = (0.36 + 0.22 * Sp) *
//!          exp[ E_A * (1/(R_u * T) - 1/17190) * (21.2 / (p_bar - 12.4))^0.63 ]
//! ```
//!
//! `Sp` is mean piston speed in m/s, `T` the charge temperature in K at start of
//! injection, and `p_bar` the cylinder pressure in bar at start of injection.
//!
//! Reference: Hardenberg and Hase (1979), as presented in Heywood,
//! *Internal Combustion Engine Fundamentals*. This is a published empirical
//! correlation, not OEM data for this engine.

use std::f64::consts::PI;

/// Universal gas constant, J/(mol K).
const R_UNIVERSAL: f64 = 8.3143;

/// Pressure offset in the correlation, bar. Cylinder pressure below this leaves
/// the correlation's valid range, so it is clamped.
const PRESSURE_OFFSET_BAR: f64 = 12.4;

/// Smallest pressure margin above the offset we will evaluate at, bar.
const MIN_PRESSURE_MARGIN_BAR: f64 = 1.0;

/// Delay is clamped to this range in crank-angle degrees. The upper bound stops
/// a cold, low-pressure cranking cylinder from producing an absurd delay.
const MIN_DELAY_DEG: f64 = 0.1;
const MAX_DELAY_DEG: f64 = 40.0;

/// Activation energy for the given cetane number, J/mol.
#[inline]
pub fn activation_energy_j_per_mol(cetane_number: f64) -> f64 {
    618_840.0 / (cetane_number + 25.0)
}

/// Ignition delay in crank-angle **radians**.
///
/// `pressure_pa` and `temperature_k` are the cylinder conditions at start of
/// injection; `mean_piston_speed_m_per_s` follows the crank speed.
#[inline]
pub fn ignition_delay_rad(
    cetane_number: f64,
    pressure_pa: f64,
    temperature_k: f64,
    mean_piston_speed_m_per_s: f64,
) -> f64 {
    let activation = activation_energy_j_per_mol(cetane_number);
    let pressure_bar = (pressure_pa / 1.0e5).max(PRESSURE_OFFSET_BAR + MIN_PRESSURE_MARGIN_BAR);
    let temperature = temperature_k.max(1.0);

    let thermal = 1.0 / (R_UNIVERSAL * temperature) - 1.0 / 17_190.0;
    let pressure_term = (21.2 / (pressure_bar - PRESSURE_OFFSET_BAR)).powf(0.63);
    let exponent = activation * thermal * pressure_term;

    let degrees = (0.36 + 0.22 * mean_piston_speed_m_per_s.abs()) * exponent.exp();
    let clamped = if degrees.is_finite() {
        degrees.clamp(MIN_DELAY_DEG, MAX_DELAY_DEG)
    } else {
        MAX_DELAY_DEG
    };
    clamped * PI / 180.0
}
