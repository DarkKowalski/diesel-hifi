//! Injection scheduling and nozzle flow.
//!
//! The source manual publishes the *structure* of the APCRS system: fuel is
//! "injected, according to the injection variant, with or without additional
//! pressure amplification", injection pressure reaches up to 2100 bar against a
//! 900 bar rail, and "the injection quantity, the injection timing point and the
//! relevant injection variants are determined by the MCM depending on the
//! operating condition of the engine"
//! (`GF47.00-W-3013H`, printed page 100).
//!
//! It publishes no timing, quantity, nozzle geometry, or rate shape, so every
//! magnitude here is calibrated. What the published structure buys us is that
//! the two rail pressures are live model inputs rather than inert metadata:
//! they set the injection rate, which sets the injection duration, which decides
//! how much fuel is present when ignition occurs, which sets the premixed
//! fraction of the burn.

use std::f64::consts::PI;

use crate::config::Injection;

/// Which APCRS injection variant the MCM selects for this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Rail pressure straight to the nozzle.
    Standard,
    /// Local pressure amplification at the injector.
    Amplified,
}

impl Variant {
    pub const fn as_str(self) -> &'static str {
        match self {
            Variant::Standard => "standard",
            Variant::Amplified => "amplified",
        }
    }
}

/// A scheduled injection event for one cylinder cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub variant: Variant,
    /// Injection pressure at the nozzle, Pa.
    pub injection_pressure_pa: f64,
    /// Signed crank angle from firing TDC at which injection starts.
    pub start_of_injection_rad: f64,
    /// Crank angle the injection occupies.
    pub duration_rad: f64,
    /// Fuel mass delivered, kg.
    pub fuel_kg: f64,
}

impl Event {
    /// An event that delivers nothing, used when fuelling is cut.
    pub const NONE: Self = Self {
        variant: Variant::Standard,
        injection_pressure_pa: 0.0,
        start_of_injection_rad: 0.0,
        duration_rad: 0.0,
        fuel_kg: 0.0,
    };

    /// Fuel mass delivered by the time `psi` is reached, kg.
    ///
    /// Rate is treated as constant over the event, which is the simplification
    /// the milestone accepts: no rate shaping, no needle dynamics.
    #[inline]
    pub fn delivered_kg(&self, psi_rad: f64) -> f64 {
        if self.duration_rad <= 0.0 {
            return 0.0;
        }
        let elapsed = psi_rad - self.start_of_injection_rad;
        if elapsed <= 0.0 {
            0.0
        } else if elapsed >= self.duration_rad {
            self.fuel_kg
        } else {
            self.fuel_kg * (elapsed / self.duration_rad)
        }
    }
}

/// Total nozzle discharge area, m^2.
#[inline]
pub fn nozzle_area_m2(injection: &Injection) -> f64 {
    let hole = PI * 0.25 * injection.nozzle_hole_diameter_m * injection.nozzle_hole_diameter_m;
    hole * f64::from(injection.nozzle_hole_count)
}

/// Which variant the MCM selects for a given fuel quantity.
#[inline]
pub fn select_variant(injection: &Injection, fuel_mg: f64) -> Variant {
    if fuel_mg >= injection.amplified_variant_min_fuel_mg {
        Variant::Amplified
    } else {
        Variant::Standard
    }
}

/// Injection pressure for a variant, Pa.
#[inline]
pub fn injection_pressure_pa(injection: &Injection, variant: Variant) -> f64 {
    match variant {
        Variant::Standard => injection.rail_pressure_max_pa,
        Variant::Amplified => injection.amplified_pressure_max_pa,
    }
}

/// Mass flow through the nozzle, kg/s.
///
/// `mdot = n * Cd * A_hole * sqrt(2 * rho_fuel * dp)`
#[inline]
pub fn mass_flow_kg_per_s(
    injection: &Injection,
    injection_pressure_pa: f64,
    cylinder_pressure_pa: f64,
) -> f64 {
    let delta_p = (injection_pressure_pa - cylinder_pressure_pa).max(0.0);
    if delta_p <= 0.0 {
        return 0.0;
    }
    injection.discharge_coefficient
        * nozzle_area_m2(injection)
        * (2.0 * injection.fuel_density_kg_m3 * delta_p).sqrt()
}

/// Base start of injection for an operating point, signed radians from firing TDC.
///
/// The schedule sets the speed dependence; the load term retards injection as
/// fuelling rises, which is what keeps peak cylinder pressure inside the
/// published envelope at high load.
#[inline]
pub fn start_of_injection_rad(injection: &Injection, rpm: f64, fuel_mg: f64) -> f64 {
    injection.soi_schedule.lookup(rpm) + injection.soi_load_retard_rad_per_mg * fuel_mg
}

/// Schedule the injection event for one cylinder cycle.
///
/// `cylinder_pressure_pa` is the pressure at scheduling time, used for the
/// nozzle pressure drop. `omega_rad_per_s` converts the physical injection
/// duration into crank angle.
pub fn schedule(
    injection: &Injection,
    fuel_kg: f64,
    rpm: f64,
    omega_rad_per_s: f64,
    cylinder_pressure_pa: f64,
) -> Event {
    let fuel_mg = fuel_kg * 1.0e6;
    if fuel_kg <= 0.0 || omega_rad_per_s <= 0.0 {
        return Event::NONE;
    }

    let variant = select_variant(injection, fuel_mg);
    let pressure = injection_pressure_pa(injection, variant);
    let mass_flow = mass_flow_kg_per_s(injection, pressure, cylinder_pressure_pa);
    if mass_flow <= 0.0 {
        return Event::NONE;
    }

    let duration_s = fuel_kg / mass_flow;
    Event {
        variant,
        injection_pressure_pa: pressure,
        start_of_injection_rad: start_of_injection_rad(injection, rpm, fuel_mg),
        duration_rad: duration_s * omega_rad_per_s,
        fuel_kg,
    }
}
