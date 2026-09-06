//! Compressible flow through a restriction.
//!
//! One function serves the turbine, the wastegate, and the EGR valve. All three
//! are the same physics — gas crossing an effective area under a pressure ratio —
//! and writing it once keeps the three subsystems from drifting apart.
//!
//! This is the standard isentropic nozzle relation with the choked branch:
//!
//! ```text
//! m_dot = Cd * A * p_up / sqrt(R * T_up) * Phi(p_dn / p_up)
//! ```
//!
//! where `Phi` follows the subsonic form until the pressure ratio falls below
//! the critical value, then holds flat because the throat has reached Mach 1 and
//! lowering downstream pressure further cannot pull more mass through.
//!
//! Milestone 2's injector used the incompressible Bernoulli form, which is right
//! for liquid fuel and wrong for gas at these pressure ratios.

use crate::config::GasProperties;

use super::gas;

/// Critical pressure ratio at which the throat chokes.
#[inline]
fn critical_ratio(gamma: f64) -> f64 {
    (2.0 / (gamma + 1.0)).powf(gamma / (gamma - 1.0))
}

/// The dimensionless flow function `Phi`.
///
/// `ratio` is downstream over upstream pressure, and is clamped into `[0, 1]`:
/// callers handle reverse flow by swapping the ends, not by passing a ratio
/// above one.
#[inline]
pub fn flow_function(gamma: f64, ratio: f64) -> f64 {
    let ratio = ratio.clamp(0.0, 1.0);
    let critical = critical_ratio(gamma);

    if ratio <= critical {
        // Choked: fixed at the critical value.
        gamma.sqrt() * (2.0 / (gamma + 1.0)).powf((gamma + 1.0) / (2.0 * (gamma - 1.0)))
    } else {
        let term = ratio.powf(2.0 / gamma) - ratio.powf((gamma + 1.0) / gamma);
        // Guard the square root: `term` can go very slightly negative from
        // rounding as `ratio` approaches one.
        (2.0 * gamma / (gamma - 1.0) * term.max(0.0)).sqrt()
    }
}

/// Mass flow through an effective area, kg/s.
///
/// Returns zero when the area is non-positive or the pressure difference does
/// not drive flow in the forward direction. Never returns a negative value:
/// reverse flow is the caller's business to model, because what a reversed flow
/// *means* differs between a turbine and an EGR valve.
#[inline]
pub fn mass_flow_kg_per_s(
    gas: &GasProperties,
    effective_area_m2: f64,
    upstream_pressure_pa: f64,
    upstream_temperature_k: f64,
    downstream_pressure_pa: f64,
) -> f64 {
    if effective_area_m2 <= 0.0
        || upstream_pressure_pa <= 0.0
        || upstream_temperature_k <= 0.0
        || downstream_pressure_pa >= upstream_pressure_pa
    {
        return 0.0;
    }

    let gamma = gas::gamma(gas, upstream_temperature_k);
    let phi = flow_function(gamma, downstream_pressure_pa / upstream_pressure_pa);

    effective_area_m2 * upstream_pressure_pa * phi
        / (gas.gas_constant_j_per_kg_k * upstream_temperature_k).sqrt()
}

/// The gas held in one cylinder, as far as a port transfer is concerned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Charge {
    pub mass_kg: f64,
    pub temperature_k: f64,
    pub pressure_pa: f64,
}

/// The manifold on the other side of a port, and how far that port is open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortState {
    pub area_m2: f64,
    pub pressure_pa: f64,
    pub temperature_k: f64,
}

/// Transfer gas between a cylinder and a manifold through an open port.
///
/// One step of explicit exchange: mass crosses the port in whichever direction
/// the pressure difference drives it, enthalpy travels with it, and the piston
/// does `p dV` on what remains. Pass `dv_m3 = 0.0` when the caller has already
/// accounted for the piston work this step, which is what the engine brake does:
/// its lobes open during the closed period, whose thermodynamics have run first.
///
/// Two clamps keep an explicit step honest, and both were paid for in Milestone 3:
///
/// The first bounds how much of the charge can leave in one step, because an
/// explicit step cannot follow a transfer faster than itself.
///
/// The second is subtler and was the cause of an audible defect. An explicit step
/// moves a whole step's worth of mass at the rate it saw at the *start* of the
/// step, so near equilibrium it overshoots, the pressure difference flips sign,
/// and the next step pushes back — a two-sample limit cycle sitting at the
/// Nyquist frequency for as long as the port is open. It is invisible in the
/// pressure trace and glaring in the audio, because the radiation derivative
/// amplifies with frequency: a stopped engine ends up emitting a steady
/// ultrasonic buzz that is the loudest thing left once combustion stops.
///
/// Flow through an orifice stops when the ends equalise; it does not reverse
/// within one step. Clamping the transfer to the mass that reaches equilibrium
/// says exactly that.
pub fn exchange(
    gas: &GasProperties,
    charge: Charge,
    port: &PortState,
    volume_m3: f64,
    dv_m3: f64,
    dt: f64,
) -> Charge {
    let cv = gas::cv_j_per_kg_k(gas, charge.temperature_k);
    let cp = cv + gas.gas_constant_j_per_kg_k;

    let out_kg = mass_flow_kg_per_s(
        gas,
        port.area_m2,
        charge.pressure_pa,
        charge.temperature_k,
        port.pressure_pa,
    ) * dt;
    let in_kg = mass_flow_kg_per_s(
        gas,
        port.area_m2,
        port.pressure_pa,
        port.temperature_k,
        charge.pressure_pa,
    ) * dt;

    let out_kg = out_kg.min(charge.mass_kg * MAX_FRACTION_PER_STEP).max(0.0);

    let equilibrium_kg = gas::mass_kg(gas, port.pressure_pa, charge.temperature_k, volume_m3);
    let net_kg = in_kg - out_kg;
    let headroom_kg = equilibrium_kg - charge.mass_kg;
    let settle = if net_kg == 0.0 {
        1.0
    } else {
        (headroom_kg / net_kg).clamp(0.0, 1.0)
    };
    let out_kg = out_kg * settle;
    let in_kg = in_kg * settle;

    let energy_j = charge.mass_kg * cv * charge.temperature_k;
    let new_mass_kg = (charge.mass_kg - out_kg + in_kg).max(1.0e-12);
    // Enthalpy leaves with the gas that leaves and arrives with any that flows
    // back; the piston does `p dV` on the rest.
    let new_energy_j = energy_j - out_kg * cp * charge.temperature_k
        + in_kg * cp * port.temperature_k
        - charge.pressure_pa * dv_m3;

    let temperature_k = (new_energy_j / (new_mass_kg * cv)).max(1.0);
    Charge {
        mass_kg: new_mass_kg,
        temperature_k,
        pressure_pa: gas::pressure_pa(gas, new_mass_kg, temperature_k, volume_m3),
    }
}

/// Most of the charge that may cross a port in a single step.
const MAX_FRACTION_PER_STEP: f64 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    fn gas() -> GasProperties {
        GasProperties {
            gas_constant_j_per_kg_k: 287.0,
            cv_reference_j_per_kg_k: 718.0,
            cv_slope_j_per_kg_k2: 0.18,
            reference_temperature_k: 300.0,
        }
    }

    #[test]
    fn no_flow_without_a_pressure_difference() {
        let g = gas();
        assert_eq!(
            mass_flow_kg_per_s(&g, 1.0e-3, 200_000.0, 800.0, 200_000.0),
            0.0
        );
        assert_eq!(
            mass_flow_kg_per_s(&g, 1.0e-3, 200_000.0, 800.0, 300_000.0),
            0.0
        );
    }

    #[test]
    fn no_flow_through_a_shut_valve() {
        let g = gas();
        assert_eq!(
            mass_flow_kg_per_s(&g, 0.0, 300_000.0, 800.0, 100_000.0),
            0.0
        );
    }

    #[test]
    fn flow_rises_with_upstream_pressure() {
        let g = gas();
        let low = mass_flow_kg_per_s(&g, 1.0e-3, 150_000.0, 800.0, 100_000.0);
        let high = mass_flow_kg_per_s(&g, 1.0e-3, 400_000.0, 800.0, 100_000.0);
        assert!(high > low, "{high} should exceed {low}");
    }

    #[test]
    fn the_flow_function_peaks_at_the_critical_ratio_and_then_holds() {
        let gamma = 1.4;
        let critical = critical_ratio(gamma);
        let at_critical = flow_function(gamma, critical);

        // Below the critical ratio the throat is choked, so Phi stops growing.
        for ratio in [critical * 0.5, critical * 0.1, 0.0] {
            let phi = flow_function(gamma, ratio);
            assert!(
                (phi - at_critical).abs() < 1.0e-9,
                "Phi({ratio}) = {phi} should stay at the choked value {at_critical}"
            );
        }

        // Above it, Phi falls away to nothing as the ends equalise.
        assert!(flow_function(gamma, 0.9) < at_critical);
        assert!(flow_function(gamma, 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn the_flow_function_is_finite_across_the_whole_ratio_range() {
        for i in 0..=1000 {
            let ratio = f64::from(i) / 1000.0;
            let phi = flow_function(1.35, ratio);
            assert!(phi.is_finite() && phi >= 0.0, "Phi({ratio}) = {phi}");
        }
    }
}
