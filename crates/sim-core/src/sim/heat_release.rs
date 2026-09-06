//! Double-Wiebe heat release with an emergent premixed fraction.
//!
//! This replaces the Milestone 1 placeholder (a single cosine bump with no
//! ignition delay and no phase split).
//!
//! ```text
//! x_b(theta) = beta   * [1 - exp(-a_p * ((theta - soc) / dur_p)^(m_p + 1))]
//!            + (1-beta) * [1 - exp(-a_d * ((theta - soc) / dur_d)^(m_d + 1))]
//! ```
//!
//! The premixed fraction `beta` is not a free parameter: it is the fuel actually
//! delivered by the injector during the ignition delay, divided by the total for
//! the cycle, capped by `max_premixed_fraction`. Long delays at light load give a
//! large premixed spike; short delays at high load and high boost give a
//! diffusion-dominated burn. That is the correct qualitative behaviour and it
//! falls out of the injection model rather than being asserted.
//!
//! Wiebe is naturally cumulative, so the solver steps it as a difference of
//! cumulative burned fractions. That is unconditionally stable at any step size
//! and cannot double-count heat.
//!
//! Reference: Wiebe, as presented in Heywood, *Internal Combustion Engine
//! Fundamentals*. A published functional form, not OEM data for this engine.

use crate::config::Combustion;
use crate::sim::injection::Event;

/// The phasing and shape of one cylinder's burn for one cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// Signed crank angle from firing TDC where heat release begins.
    pub start_of_combustion_rad: f64,
    /// Fraction of the charge burned in the premixed phase.
    pub premixed_fraction: f64,
    pub premixed_duration_rad: f64,
    pub diffusion_duration_rad: f64,
    /// Total chemical energy available, joules.
    pub total_heat_j: f64,
}

impl Profile {
    pub const NONE: Self = Self {
        start_of_combustion_rad: 0.0,
        premixed_fraction: 0.0,
        premixed_duration_rad: 1.0,
        diffusion_duration_rad: 1.0,
        total_heat_j: 0.0,
    };
}

/// Build the burn profile for a scheduled injection event.
///
/// `ignition_delay_rad` comes from [`crate::sim::ignition_delay`].
pub fn profile(
    combustion: &Combustion,
    event: &Event,
    ignition_delay_rad: f64,
    fuel_lhv_j_per_kg: f64,
) -> Profile {
    if event.fuel_kg <= 0.0 {
        return Profile::NONE;
    }

    let start_of_combustion_rad = event.start_of_injection_rad + ignition_delay_rad;

    // Fuel physically in the chamber when ignition occurs sets the premixed
    // fraction. This is the whole reason the injection model exists.
    let premixed_kg = event.delivered_kg(start_of_combustion_rad);
    let premixed_fraction =
        (premixed_kg / event.fuel_kg).clamp(0.0, combustion.max_premixed_fraction);

    let fuel_mg = event.fuel_kg * 1.0e6;
    let diffusion_duration_rad = (combustion.diffusion_duration_rad_per_mg * fuel_mg)
        .max(combustion.diffusion_duration_min_rad);

    Profile {
        start_of_combustion_rad,
        premixed_fraction,
        premixed_duration_rad: combustion.premixed_duration_rad,
        diffusion_duration_rad,
        total_heat_j: event.fuel_kg * fuel_lhv_j_per_kg * combustion.combustion_efficiency,
    }
}

/// One Wiebe term's cumulative burned fraction.
#[inline]
fn wiebe(normalised: f64, efficiency: f64, shape: f64) -> f64 {
    if normalised <= 0.0 {
        return 0.0;
    }
    if normalised >= 1.0 {
        // Saturate rather than letting the exponential creep asymptotically.
        return 1.0;
    }
    1.0 - (-efficiency * normalised.powf(shape + 1.0)).exp()
}

/// Cumulative burned mass fraction at signed cycle angle `psi`.
#[inline]
pub fn burned_fraction(combustion: &Combustion, profile: &Profile, psi_rad: f64) -> f64 {
    let elapsed = psi_rad - profile.start_of_combustion_rad;
    if elapsed <= 0.0 {
        return 0.0;
    }
    let premixed = wiebe(
        elapsed / profile.premixed_duration_rad,
        combustion.premixed_wiebe_efficiency,
        combustion.premixed_wiebe_shape,
    );
    let diffusion = wiebe(
        elapsed / profile.diffusion_duration_rad,
        combustion.diffusion_wiebe_efficiency,
        combustion.diffusion_wiebe_shape,
    );
    (profile.premixed_fraction * premixed + (1.0 - profile.premixed_fraction) * diffusion)
        .clamp(0.0, 1.0)
}

/// Heat released since the last step, joules, plus the new cumulative fraction.
///
/// Returns zero heat when the cycle angle is not advancing, which is what
/// happens at the gas-exchange discontinuity in the signed cycle angle. The burn
/// integral therefore can never be replayed.
#[inline]
pub fn heat_release_j(
    combustion: &Combustion,
    profile: &Profile,
    burned_fraction_before: f64,
    psi_rad: f64,
) -> (f64, f64) {
    let fraction_now = burned_fraction(combustion, profile, psi_rad).max(burned_fraction_before);
    let delta = fraction_now - burned_fraction_before;
    (profile.total_heat_j * delta, fraction_now)
}
