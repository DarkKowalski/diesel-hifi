//! Cooled high-pressure exhaust gas recirculation.
//!
//! The manual (`GF14.20-W-3000H`) describes the loop: exhaust is branched off
//! ahead of the turbine, cooled against the coolant circuit, passed through a
//! throttle valve driven by positioner Y621, and mixed with charge air in the
//! charge-air pipe before reaching the cylinders. Recirculated exhaust displaces
//! oxygen, which lowers combustion temperature and so lowers NOx.
//!
//! The one published pair here is the cooler duty: exhaust enters at about
//! 650 C and leaves at about 170 C. The effectiveness the model uses is derived
//! from those two temperatures. The recirculation *rate* is not published, and
//! is calibrated.
//!
//! The rate is bounded on both sides for a reason the manual states directly:
//! too much recirculated exhaust spoils combustion and raises soot, CO and HC,
//! while too little raises NOx.

use crate::config::{Egr, GasProperties};

use super::flow;

/// Cooler effectiveness implied by a published inlet/outlet temperature pair.
///
/// `effectiveness = (T_in - T_out) / (T_in - T_coolant)`
///
/// Used by the configuration generator to derive the calibration value from the
/// two published temperatures rather than inventing a third number.
#[inline]
pub fn effectiveness_from_published(inlet_k: f64, outlet_k: f64, coolant_k: f64) -> Option<f64> {
    let span = inlet_k - coolant_k;
    if span <= 0.0 {
        return None;
    }
    Some(((inlet_k - outlet_k) / span).clamp(0.0, 1.0))
}

/// Gas temperature leaving the EGR cooler.
#[inline]
pub fn cooler_outlet_temperature_k(egr: &Egr, inlet_k: f64, coolant_k: f64) -> f64 {
    coolant_k + (1.0 - egr.cooler_effectiveness) * (inlet_k - coolant_k)
}

/// Recirculated mass flow, kg/s.
///
/// Flow needs the exhaust manifold to sit above the intake manifold. On a
/// wastegated engine that is normally true, because the turbine restricts the
/// exhaust; when it is not, the loop simply stops flowing rather than reversing.
#[inline]
pub fn mass_flow_kg_per_s(
    egr: &Egr,
    g: &GasProperties,
    valve_position: f64,
    exhaust_pressure_pa: f64,
    cooled_temperature_k: f64,
    intake_pressure_pa: f64,
) -> f64 {
    let area =
        egr.valve_max_area_m2 * egr.valve_discharge_coefficient * valve_position.clamp(0.0, 1.0);
    flow::mass_flow_kg_per_s(
        g,
        area,
        exhaust_pressure_pa,
        cooled_temperature_k,
        intake_pressure_pa,
    )
}

/// Recirculation rate: recirculated mass over total mass reaching the cylinders.
#[inline]
pub fn rate(egr_flow_kg_per_s: f64, fresh_flow_kg_per_s: f64) -> f64 {
    let total = egr_flow_kg_per_s + fresh_flow_kg_per_s;
    if total <= 0.0 {
        return 0.0;
    }
    (egr_flow_kg_per_s / total).clamp(0.0, 1.0)
}

/// Advance the EGR valve toward the scheduled rate.
///
/// With EGR commanded off the valve is driven shut and the integral term is
/// released, so re-enabling starts from rest rather than from a wound-up state.
#[inline]
pub fn update_valve(
    egr: &Egr,
    position: f64,
    integral: &mut f64,
    measured_rate: f64,
    target_rate: f64,
    enabled: bool,
    dt: f64,
) -> f64 {
    if !enabled {
        *integral = 0.0;
        // Shut at the same rate the controller could otherwise open it.
        return (position - dt * 4.0).max(0.0);
    }

    let error = target_rate - measured_rate;

    let candidate = *integral + error * egr.rate_i_gain_per_s * dt;
    let unsaturated = (position > 0.0 || error > 0.0) && (position < 1.0 || error < 0.0);
    if unsaturated {
        *integral = candidate.clamp(-1.0, 1.0);
    }

    let demand = (error * egr.rate_p_gain + *integral).clamp(0.0, 1.0);
    let max_move = 4.0 * dt;
    position + (demand - position).clamp(-max_move, max_move)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Schedule;

    fn gas_props() -> GasProperties {
        GasProperties {
            gas_constant_j_per_kg_k: 287.0,
            cv_reference_j_per_kg_k: 718.0,
            cv_slope_j_per_kg_k2: 0.18,
            reference_temperature_k: 300.0,
        }
    }

    fn egr() -> Egr {
        let coolant = 358.15;
        let inlet = 923.15;
        let outlet = 443.15;
        Egr {
            cooler_inlet_temperature_k: inlet,
            cooler_outlet_temperature_k: outlet,
            cooler_effectiveness: effectiveness_from_published(inlet, outlet, coolant).unwrap(),
            rate_schedule: Schedule {
                breakpoints: vec![600.0, 2100.0],
                values: vec![0.20, 0.10],
            },
            valve_max_area_m2: 8.0e-4,
            valve_discharge_coefficient: 0.7,
            rate_p_gain: 6.0,
            rate_i_gain_per_s: 60.0,
            max_rate: 0.35,
        }
    }

    #[test]
    fn the_cooler_reproduces_the_published_outlet_temperature() {
        // The whole point of deriving effectiveness from the published pair:
        // feed the published inlet, get the published outlet back.
        let e = egr();
        let out = cooler_outlet_temperature_k(&e, 923.15, 358.15);
        assert!(
            (out - 443.15).abs() < 1.0e-9,
            "expected the published 443.15 K outlet, got {out}"
        );
    }

    #[test]
    fn effectiveness_needs_a_real_temperature_span() {
        assert!(effectiveness_from_published(400.0, 350.0, 400.0).is_none());
        assert!(effectiveness_from_published(400.0, 350.0, 500.0).is_none());
    }

    #[test]
    fn no_recirculation_without_a_favourable_pressure_difference() {
        let (e, g) = (egr(), gas_props());
        // Intake above exhaust: the loop cannot flow.
        let flow = mass_flow_kg_per_s(&e, &g, 1.0, 200_000.0, 443.15, 250_000.0);
        assert_eq!(flow, 0.0);
    }

    #[test]
    fn a_shut_valve_passes_nothing() {
        let (e, g) = (egr(), gas_props());
        assert_eq!(
            mass_flow_kg_per_s(&e, &g, 0.0, 300_000.0, 443.15, 200_000.0),
            0.0
        );
    }

    #[test]
    fn opening_the_valve_increases_recirculated_flow() {
        let (e, g) = (egr(), gas_props());
        let part = mass_flow_kg_per_s(&e, &g, 0.3, 320_000.0, 443.15, 240_000.0);
        let full = mass_flow_kg_per_s(&e, &g, 1.0, 320_000.0, 443.15, 240_000.0);
        assert!(full > part && part > 0.0);
    }

    #[test]
    fn the_controller_converges_on_its_target_rate() {
        let e = egr();
        let mut integral = 0.0;
        let mut position = 0.0;
        // Stand-in plant: rate responds proportionally to valve position.
        let mut measured = 0.0;
        for _ in 0..5000 {
            position = update_valve(&e, position, &mut integral, measured, 0.20, true, 1.0e-3);
            measured += (position * 0.30 - measured) * 0.05;
        }
        assert!(
            (measured - 0.20).abs() < 0.01,
            "controller settled at {measured}, wanted 0.20"
        );
    }

    #[test]
    fn disabling_egr_shuts_the_valve_and_clears_the_integral() {
        let e = egr();
        let mut integral = 0.5;
        let mut position = 1.0;
        for _ in 0..1000 {
            position = update_valve(&e, position, &mut integral, 0.0, 0.20, false, 1.0e-3);
        }
        assert_eq!(position, 0.0);
        assert_eq!(integral, 0.0);
    }

    #[test]
    fn rate_is_a_fraction_of_the_total_charge() {
        assert_eq!(rate(0.0, 0.5), 0.0);
        assert_eq!(rate(0.0, 0.0), 0.0);
        assert!((rate(0.1, 0.9) - 0.1).abs() < 1.0e-12);
    }
}
