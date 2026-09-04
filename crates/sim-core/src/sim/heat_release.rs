//! **Milestone 1 placeholder heat release.**
//!
//! A single smooth cosine burn profile. There is no ignition delay, no
//! premixed/diffusion split, no injector rate shaping, and no wall heat-transfer
//! model. It exists so that pedal demand reaches crank torque *through cylinder
//! pressure* rather than by writing a target torque into crank acceleration
//! (SPEC section 6).
//!
//! Milestone 2 replaces this module's body and its `calibrated` provenance
//! entries. Nothing outside this module encodes the burn law.

use std::f64::consts::PI;

use crate::config::Combustion;

/// Cumulative burned mass fraction at signed cycle angle `psi`.
///
/// `W(psi) = 0` before start of combustion, rises as
/// `(1 - cos(PI * u / duration)) / 2`, and saturates at 1. That is the exact
/// integral of a half-sine release rate over the burn window, so stepping it as
/// a difference of cumulative values is stable at any step size.
#[inline]
pub fn burned_fraction(combustion: &Combustion, psi: f64) -> f64 {
    let u = psi - combustion.start_of_combustion_rad;
    if u <= 0.0 {
        0.0
    } else if u >= combustion.burn_duration_rad {
        1.0
    } else {
        0.5 * (1.0 - (PI * u / combustion.burn_duration_rad).cos())
    }
}

/// Heat released between two signed cycle angles, in joules.
///
/// Returns zero when `psi` is not advancing (the cycle-angle discontinuity at
/// TDC overlap), so the burn integral can never be replayed.
#[inline]
pub fn heat_release_j(
    combustion: &Combustion,
    fuel_charge_kg: f64,
    fuel_lhv_j_per_kg: f64,
    burned_fraction_before: f64,
    psi_now: f64,
) -> (f64, f64) {
    let fraction_now = burned_fraction(combustion, psi_now).max(burned_fraction_before);
    let delta = fraction_now - burned_fraction_before;
    let energy = fuel_charge_kg * fuel_lhv_j_per_kg * combustion.combustion_efficiency * delta;
    (energy, fraction_now)
}
