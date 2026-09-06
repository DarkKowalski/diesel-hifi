//! Convective wall heat transfer, Woschni correlation.
//!
//! ```text
//! w   = C1 * Sp + C2 * (Vd * T_ivc) / (p_ivc * V_ivc) * (p - p_motored)
//! h_c = 3.26 * B^-0.2 * p_kPa^0.8 * T^-0.55 * w^0.8            W/(m^2 K)
//! Q   = h_c * A(theta) * (T - T_wall)
//! ```
//!
//! The second velocity term is the combustion-driven contribution: it is the
//! difference between the fired and motored pressure traces, which is why the
//! solver carries a parallel motored pressure per cylinder.
//!
//! Reference: Woschni (1967), as presented in Heywood, *Internal Combustion
//! Engine Fundamentals*. A published empirical correlation, not OEM data.

use std::f64::consts::PI;

use crate::config::HeatTransfer;
use crate::geometry::SliderCrank;

/// Leading coefficient of the Woschni correlation in SI-with-kPa form.
const WOSCHNI_COEFFICIENT: f64 = 3.26;

/// Characteristic gas velocity, m/s.
///
/// `swept_volume_m3`, `ivc_pressure_pa`, `ivc_temperature_k` and `ivc_volume_m3`
/// describe the trapped state at intake valve closing.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn gas_velocity_m_per_s(
    heat_transfer: &HeatTransfer,
    closed: bool,
    mean_piston_speed_m_per_s: f64,
    pressure_pa: f64,
    motored_pressure_pa: f64,
    swept_volume_m3: f64,
    ivc_pressure_pa: f64,
    ivc_temperature_k: f64,
    ivc_volume_m3: f64,
) -> f64 {
    let c1 = if closed {
        heat_transfer.woschni_c1_closed
    } else {
        heat_transfer.woschni_c1_gas_exchange
    };
    let mut w = c1 * mean_piston_speed_m_per_s.abs();

    // The combustion term only applies once the cylinder is closed and firing.
    if closed && ivc_pressure_pa > 0.0 && ivc_volume_m3 > 0.0 {
        let rise = (pressure_pa - motored_pressure_pa).max(0.0);
        w += heat_transfer.woschni_c2 * (swept_volume_m3 * ivc_temperature_k)
            / (ivc_pressure_pa * ivc_volume_m3)
            * rise;
    }
    w
}

/// Convective heat transfer coefficient, W/(m^2 K).
#[inline]
pub fn coefficient_w_per_m2_k(
    bore_m: f64,
    pressure_pa: f64,
    temperature_k: f64,
    gas_velocity_m_per_s: f64,
) -> f64 {
    if gas_velocity_m_per_s <= 0.0 || pressure_pa <= 0.0 || temperature_k <= 0.0 {
        return 0.0;
    }
    let pressure_kpa = pressure_pa / 1000.0;
    WOSCHNI_COEFFICIENT
        * bore_m.powf(-0.2)
        * pressure_kpa.powf(0.8)
        * temperature_k.powf(-0.55)
        * gas_velocity_m_per_s.powf(0.8)
}

/// Instantaneous heat-transfer area: cylinder head, piston crown, and the
/// exposed liner, m^2.
#[inline]
pub fn chamber_area_m2(slider: &SliderCrank, bore_m: f64, psi_rad: f64) -> f64 {
    let crown = PI * 0.25 * bore_m * bore_m;
    let liner = PI * bore_m * slider.piston_displacement_m(psi_rad).max(0.0);
    // Head and crown are both flat discs of bore area in this simplified chamber.
    2.0 * crown + liner
}

/// Heat lost to the walls over a crank-angle step, joules.
///
/// Positive means heat leaves the gas. Returns zero when heat transfer is
/// disabled, which exists so tests can prove the term is actually wired in.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn heat_loss_j(
    heat_transfer: &HeatTransfer,
    slider: &SliderCrank,
    bore_m: f64,
    psi_rad: f64,
    pressure_pa: f64,
    temperature_k: f64,
    gas_velocity_m_per_s: f64,
    omega_rad_per_s: f64,
    d_theta_rad: f64,
) -> f64 {
    if !heat_transfer.enabled || d_theta_rad <= 0.0 || omega_rad_per_s <= 0.0 {
        return 0.0;
    }
    let h = coefficient_w_per_m2_k(bore_m, pressure_pa, temperature_k, gas_velocity_m_per_s);
    let area = chamber_area_m2(slider, bore_m, psi_rad);
    let dt_s = d_theta_rad / omega_rad_per_s;
    h * area * (temperature_k - heat_transfer.wall_temperature_k) * dt_s
}
