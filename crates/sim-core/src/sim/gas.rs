//! Working-gas properties.
//!
//! Milestone 1 folded wall heat loss into fixed polytropic exponents. Milestone 2
//! models heat transfer explicitly, so the gas needs real specific heats instead.
//!
//! `cv` is a linear function of temperature, which is the cheapest useful stand-in
//! for variable specific heats: roughly 718 J/(kg K) for air near ambient rising
//! to about 1000 J/(kg K) for hot combustion products.

use crate::config::GasProperties;

/// Constant-volume specific heat at the given temperature.
#[inline]
pub fn cv_j_per_kg_k(gas: &GasProperties, temperature_k: f64) -> f64 {
    let cv = gas.cv_reference_j_per_kg_k
        + gas.cv_slope_j_per_kg_k2 * (temperature_k - gas.reference_temperature_k);
    // Never let the linear fit produce a non-physical specific heat at very low
    // temperatures; that would make the energy equation explode.
    cv.max(gas.cv_reference_j_per_kg_k * 0.5)
}

/// Ratio of specific heats, `gamma = 1 + R / cv(T)`.
#[inline]
pub fn gamma(gas: &GasProperties, temperature_k: f64) -> f64 {
    1.0 + gas.gas_constant_j_per_kg_k / cv_j_per_kg_k(gas, temperature_k)
}

/// Ideal-gas pressure.
#[inline]
pub fn pressure_pa(gas: &GasProperties, mass_kg: f64, temperature_k: f64, volume_m3: f64) -> f64 {
    mass_kg * gas.gas_constant_j_per_kg_k * temperature_k / volume_m3
}

/// Ideal-gas mass.
#[inline]
pub fn mass_kg(gas: &GasProperties, pressure_pa: f64, temperature_k: f64, volume_m3: f64) -> f64 {
    pressure_pa * volume_m3 / (gas.gas_constant_j_per_kg_k * temperature_k)
}
