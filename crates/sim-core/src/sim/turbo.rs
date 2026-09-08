//! Wastegate turbocharger.
//!
//! A mean-value model: compressor, turbine, a shaft with inertia, and a
//! wastegate that bypasses exhaust around the turbine. Milestone 2 prescribed
//! manifold pressure from a schedule; here boost is what comes *out* of the
//! shaft power balance, and the schedule has become the setpoint the wastegate
//! controller regulates to.
//!
//! The manual (`GF09.40-W-0001H`) publishes this architecture — one turbine and
//! compressor on a joint shaft, a charge-air cooler, and boost limited by a
//! wastegate the MCM drives through a vacuum cell and linkage. It publishes no
//! geometry, efficiency, inertia, or pressure, so every number the model needs
//! is calibrated.
//!
//! Note on one number deliberately *not* used: that section mentions a control
//! pressure of "up to 2.8 bar" applied to the vacuum cell. That is the pneumatic
//! pressure operating the wastegate actuator, not boost pressure, and it is not
//! a published boost figure.

use crate::config::{AirPath, GasProperties, Turbo};

use super::{flow, gas};

/// Lowest shaft speed used in the power balance.
///
/// The shaft equation divides by speed, so a stationary shaft would be a
/// singularity. A freewheeling turbo never actually reaches zero in service, and
/// this floor is far below any speed the model reports.
const MIN_SHAFT_RAD_PER_S: f64 = 100.0;

/// What the turbocharger does over one step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Output {
    pub compressor_flow_kg_per_s: f64,
    /// Air temperature leaving the compressor, before the charge-air cooler.
    pub compressor_outlet_temperature_k: f64,
    /// Air temperature entering the intake manifold, after the cooler.
    pub charge_temperature_k: f64,
    pub turbine_flow_kg_per_s: f64,
    pub wastegate_flow_kg_per_s: f64,
    pub shaft_power_balance_w: f64,
    /// Pressure downstream of the turbine, raised by the exhaust restriction.
    pub downstream_pressure_pa: f64,
}

/// Constant-pressure specific heat from the gas model.
#[inline]
fn cp_j_per_kg_k(g: &GasProperties, temperature_k: f64) -> f64 {
    gas::cv_j_per_kg_k(g, temperature_k) + g.gas_constant_j_per_kg_k
}

/// Highest pressure ratio the compressor can produce at this shaft speed.
///
/// From the Euler turbomachinery relation: the wheel does work proportional to
/// the square of tip speed, and the isentropic efficiency converts that work
/// into a pressure rise.
#[inline]
pub fn max_pressure_ratio(
    turbo: &Turbo,
    g: &GasProperties,
    inlet_temperature_k: f64,
    shaft_rad_per_s: f64,
) -> f64 {
    let tip_speed = shaft_rad_per_s * turbo.compressor_wheel_diameter_m * 0.5;
    let cp = cp_j_per_kg_k(g, inlet_temperature_k);
    let gamma = gas::gamma(g, inlet_temperature_k);
    let work = turbo.compressor_head_coefficient * tip_speed * tip_speed;
    let ratio = 1.0 + turbo.compressor_efficiency * work / (cp * inlet_temperature_k);
    ratio.max(1.0).powf(gamma / (gamma - 1.0))
}

/// Compressor mass flow, from a normalised ellipse standing in for a map.
///
/// A measured compressor map is a table of flow against pressure ratio at each
/// speed line. The ellipse captures the part that matters here: flow is highest
/// when the compressor is pushing against nothing, and falls to zero as the
/// pressure ratio approaches what the wheel speed can sustain.
#[inline]
pub fn compressor_flow_kg_per_s(
    turbo: &Turbo,
    g: &GasProperties,
    inlet_temperature_k: f64,
    shaft_rad_per_s: f64,
    pressure_ratio: f64,
) -> f64 {
    let max_ratio = max_pressure_ratio(turbo, g, inlet_temperature_k, shaft_rad_per_s);
    if max_ratio <= 1.0 {
        return 0.0;
    }
    let ratio = pressure_ratio.max(1.0);
    if ratio >= max_ratio {
        return 0.0;
    }
    let normalised = (ratio - 1.0) / (max_ratio - 1.0);
    let max_flow = turbo.compressor_max_flow_kg_s_per_rad_s * shaft_rad_per_s;
    max_flow * (1.0 - normalised * normalised).max(0.0).sqrt()
}

/// Tuning of the wastegate loop: the two gains and the actuator rate limit.
///
/// Split out from [`Turbo`] because the engine brake runs the same controller
/// against a very different plant and needs it slowed down. Everything the loop
/// needs is here, so the caller decides the tuning and the controller stays one
/// function rather than two that could drift apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WastegateGains {
    pub p_gain_per_pa: f64,
    pub i_gain_per_pa_s: f64,
    pub slew_per_s: f64,
}

/// The configured wastegate tuning, with every term scaled by `scale`.
///
/// `scale` of 1.0 gives the fuelled calibration unchanged. The engine brake
/// passes something much smaller; see `engine_brake.wastegate_gain_scale` for
/// the measurement that motivated it.
#[inline]
pub fn scaled_wastegate_gains(turbo: &Turbo, scale: f64) -> WastegateGains {
    WastegateGains {
        p_gain_per_pa: turbo.wastegate_p_gain_per_pa * scale,
        i_gain_per_pa_s: turbo.wastegate_i_gain_per_pa_s * scale,
        slew_per_s: turbo.wastegate_slew_per_s * scale,
    }
}

/// Advance the wastegate actuator toward what the boost controller asks for.
///
/// Proportional-integral on boost error, then rate-limited: the vacuum cell and
/// linkage the manual describes cannot slam from shut to open instantly.
#[inline]
pub fn update_wastegate(
    gains: &WastegateGains,
    position: f64,
    integral: &mut f64,
    boost_pa: f64,
    setpoint_pa: f64,
    dt: f64,
) -> f64 {
    let error_pa = boost_pa - setpoint_pa;

    // Integrate only while the actuator has somewhere to go, so the term cannot
    // wind up against its own end stops.
    let candidate = *integral + error_pa * gains.i_gain_per_pa_s * dt;
    let unsaturated = (position > 0.0 || error_pa > 0.0) && (position < 1.0 || error_pa < 0.0);
    if unsaturated {
        *integral = candidate.clamp(-1.0, 1.0);
    }

    let demand = (error_pa * gains.p_gain_per_pa + *integral).clamp(0.0, 1.0);
    let max_move = gains.slew_per_s * dt;
    position + (demand - position).clamp(-max_move, max_move)
}

/// Evaluate the compressor, turbine and wastegate at the current state.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn evaluate(
    turbo: &Turbo,
    air: &AirPath,
    g: &GasProperties,
    shaft_rad_per_s: f64,
    intake_pressure_pa: f64,
    exhaust_pressure_pa: f64,
    exhaust_temperature_k: f64,
    wastegate_position: f64,
) -> Output {
    let shaft = shaft_rad_per_s.max(MIN_SHAFT_RAD_PER_S);
    let ambient_pa = air.ambient_pressure_pa;
    let ambient_k = air.ambient_temperature_k;

    // --- compressor ---
    let pressure_ratio = (intake_pressure_pa / ambient_pa).max(1.0);
    let compressor_flow = compressor_flow_kg_per_s(turbo, g, ambient_k, shaft, pressure_ratio);

    let gamma_in = gas::gamma(g, ambient_k);
    let cp_in = cp_j_per_kg_k(g, ambient_k);
    let ideal_rise = ambient_k * (pressure_ratio.powf((gamma_in - 1.0) / gamma_in) - 1.0);
    let actual_rise = ideal_rise / turbo.compressor_efficiency;
    let compressor_outlet_k = ambient_k + actual_rise;
    let compressor_power = compressor_flow * cp_in * actual_rise;

    // --- charge-air cooler ---
    let charge_k = air.coolant_temperature_k
        + (1.0 - air.intercooler_effectiveness) * (compressor_outlet_k - air.coolant_temperature_k);

    // --- exhaust restriction downstream of the turbine ---
    //
    // Muffler and aftertreatment can, as a flow resistance only. Estimated from
    // the compressor flow, which is what ultimately leaves through the tailpipe.
    let downstream_pa =
        ambient_pa + air.exhaust_restriction_pa_per_kg2_s2 * compressor_flow * compressor_flow;

    // --- turbine and wastegate, two paths sharing the same upstream state ---
    let turbine_flow = flow::mass_flow_kg_per_s(
        g,
        turbo.turbine_effective_area_m2,
        exhaust_pressure_pa,
        exhaust_temperature_k,
        downstream_pa,
    );
    let wastegate_area = turbo.wastegate_max_area_m2 * wastegate_position.clamp(0.0, 1.0);
    let wastegate_flow = flow::mass_flow_kg_per_s(
        g,
        wastegate_area,
        exhaust_pressure_pa,
        exhaust_temperature_k,
        downstream_pa,
    );

    // Only the turbine path does work; the wastegate exists to throw that work
    // away, which is exactly how boost gets limited.
    let gamma_ex = gas::gamma(g, exhaust_temperature_k);
    let cp_ex = cp_j_per_kg_k(g, exhaust_temperature_k);
    let expansion = (downstream_pa / exhaust_pressure_pa.max(1.0)).clamp(0.0, 1.0);
    let turbine_power = turbine_flow
        * cp_ex
        * exhaust_temperature_k
        * turbo.turbine_efficiency
        * (1.0 - expansion.powf((gamma_ex - 1.0) / gamma_ex));

    let friction_power = turbo.bearing_friction_nm_per_rad_s * shaft * shaft;

    Output {
        compressor_flow_kg_per_s: compressor_flow,
        compressor_outlet_temperature_k: compressor_outlet_k,
        charge_temperature_k: charge_k,
        turbine_flow_kg_per_s: turbine_flow,
        wastegate_flow_kg_per_s: wastegate_flow,
        shaft_power_balance_w: turbine_power - compressor_power - friction_power,
        downstream_pressure_pa: downstream_pa,
    }
}

/// Integrate the shaft speed from the power balance.
///
/// `J * omega * d(omega)/dt = P_turbine - P_compressor - P_friction`
#[inline]
pub fn advance_shaft(turbo: &Turbo, shaft_rad_per_s: f64, power_balance_w: f64, dt: f64) -> f64 {
    let shaft = shaft_rad_per_s.max(MIN_SHAFT_RAD_PER_S);
    let acceleration = power_balance_w / (turbo.shaft_inertia_kg_m2 * shaft);
    (shaft + acceleration * dt).clamp(MIN_SHAFT_RAD_PER_S, turbo.max_shaft_speed_rad_per_s)
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

    fn turbo() -> Turbo {
        Turbo {
            shaft_inertia_kg_m2: 4.0e-5,
            compressor_wheel_diameter_m: 0.09,
            compressor_efficiency: 0.72,
            compressor_head_coefficient: 0.62,
            compressor_max_flow_kg_s_per_rad_s: 6.0e-5,
            turbine_effective_area_m2: 1.4e-3,
            turbine_efficiency: 0.70,
            bearing_friction_nm_per_rad_s: 2.0e-9,
            max_shaft_speed_rad_per_s: 20_000.0,
            wastegate_max_area_m2: 9.0e-4,
            wastegate_p_gain_per_pa: 2.0e-5,
            wastegate_i_gain_per_pa_s: 4.0e-6,
            wastegate_slew_per_s: 6.0,
        }
    }

    fn air() -> AirPath {
        AirPath {
            ambient_pressure_pa: 101_325.0,
            ambient_temperature_k: 293.15,
            boost_target_schedule: Schedule {
                breakpoints: vec![600.0, 2100.0],
                values: vec![150_000.0, 200_000.0],
            },
            intake_manifold_volume_m3: 0.02,
            exhaust_manifold_volume_m3: 0.012,
            intercooler_effectiveness: 0.75,
            coolant_temperature_k: 358.15,
            exhaust_restriction_pa_per_kg2_s2: 40_000.0,
            crankcase_pressure_pa: 101_325.0,
            ring_leakage_area_m2: 0.0,
            volumetric_efficiency: 0.92,
        }
    }

    #[test]
    fn a_faster_shaft_can_sustain_a_higher_pressure_ratio() {
        let (t, g) = (turbo(), gas_props());
        let slow = max_pressure_ratio(&t, &g, 293.15, 4_000.0);
        let fast = max_pressure_ratio(&t, &g, 293.15, 12_000.0);
        assert!(fast > slow, "{fast} should exceed {slow}");
        assert!(slow > 1.0);
    }

    #[test]
    fn compressor_flow_falls_to_zero_as_it_approaches_its_ceiling() {
        let (t, g) = (turbo(), gas_props());
        let shaft = 10_000.0;
        let ceiling = max_pressure_ratio(&t, &g, 293.15, shaft);

        let open = compressor_flow_kg_per_s(&t, &g, 293.15, shaft, 1.0);
        let near = compressor_flow_kg_per_s(&t, &g, 293.15, shaft, ceiling * 0.999);
        let past = compressor_flow_kg_per_s(&t, &g, 293.15, shaft, ceiling * 1.1);

        assert!(open > near, "flow should fall as back-pressure rises");
        assert!(near >= 0.0);
        assert_eq!(past, 0.0, "the compressor cannot exceed its own ceiling");
    }

    #[test]
    fn the_wastegate_opens_when_boost_runs_over_setpoint() {
        let t = turbo();
        let mut integral = 0.0;
        let mut position = 0.0;
        for _ in 0..2000 {
            position = update_wastegate(
                &scaled_wastegate_gains(&t, 1.0),
                position,
                &mut integral,
                260_000.0,
                200_000.0,
                1.0e-3,
            );
        }
        assert!(
            position > 0.9,
            "sustained overboost should drive the wastegate open, got {position}"
        );
    }

    #[test]
    fn the_wastegate_shuts_when_boost_sits_under_setpoint() {
        let t = turbo();
        let mut integral = 0.0;
        let mut position = 1.0;
        for _ in 0..2000 {
            position = update_wastegate(
                &scaled_wastegate_gains(&t, 1.0),
                position,
                &mut integral,
                120_000.0,
                200_000.0,
                1.0e-3,
            );
        }
        assert!(
            position < 0.05,
            "under-boost should shut the wastegate, got {position}"
        );
    }

    #[test]
    fn the_actuator_cannot_move_faster_than_its_slew_limit() {
        let t = turbo();
        let mut integral = 0.0;
        let dt = 1.0e-3;
        let moved = update_wastegate(
            &scaled_wastegate_gains(&t, 1.0),
            0.0,
            &mut integral,
            1.0e6,
            100_000.0,
            dt,
        );
        assert!(
            moved <= t.wastegate_slew_per_s * dt + 1.0e-12,
            "moved {moved} in one step, limit is {}",
            t.wastegate_slew_per_s * dt
        );
    }

    #[test]
    fn hot_exhaust_spins_the_shaft_up_and_a_cold_one_lets_it_coast_down() {
        let (t, a, g) = (turbo(), air(), gas_props());

        let driven = evaluate(&t, &a, &g, 6_000.0, 150_000.0, 300_000.0, 900.0, 0.0);
        assert!(
            driven.shaft_power_balance_w > 0.0,
            "hot, high-pressure exhaust should accelerate the shaft"
        );
        assert!(advance_shaft(&t, 6_000.0, driven.shaft_power_balance_w, 1.0e-3) > 6_000.0);

        let coasting = evaluate(&t, &a, &g, 6_000.0, 150_000.0, 101_325.0, 350.0, 0.0);
        assert!(
            coasting.shaft_power_balance_w < 0.0,
            "with no exhaust energy the compressor and bearings should slow it"
        );
        assert!(advance_shaft(&t, 6_000.0, coasting.shaft_power_balance_w, 1.0e-3) < 6_000.0);
    }

    #[test]
    fn opening_the_wastegate_adds_a_parallel_path_that_does_no_work() {
        let (t, a, g) = (turbo(), air(), gas_props());
        let shut = evaluate(&t, &a, &g, 8_000.0, 200_000.0, 320_000.0, 900.0, 0.0);
        let open = evaluate(&t, &a, &g, 8_000.0, 200_000.0, 320_000.0, 900.0, 1.0);

        assert_eq!(shut.wastegate_flow_kg_per_s, 0.0);
        assert!(open.wastegate_flow_kg_per_s > 0.0);

        // At a *fixed* upstream state the turbine is unaffected: the two paths
        // are in parallel, both fed by the same manifold. Boost falls because
        // the extra outflow depressurises that manifold, which is a filling
        // effect and shows up over time rather than within one evaluation.
        // `tests/turbo.rs` asserts the resulting boost drop end to end.
        assert_eq!(open.turbine_flow_kg_per_s, shut.turbine_flow_kg_per_s);
        assert_eq!(open.shaft_power_balance_w, shut.shaft_power_balance_w);
        assert!(
            open.turbine_flow_kg_per_s + open.wastegate_flow_kg_per_s
                > shut.turbine_flow_kg_per_s + shut.wastegate_flow_kg_per_s,
            "total flow leaving the manifold must rise"
        );
    }

    #[test]
    fn the_charge_air_cooler_pulls_compressor_outlet_toward_coolant() {
        let (t, a, g) = (turbo(), air(), gas_props());
        let out = evaluate(&t, &a, &g, 10_000.0, 250_000.0, 300_000.0, 900.0, 0.0);
        assert!(
            out.compressor_outlet_temperature_k > out.charge_temperature_k,
            "the cooler must remove heat"
        );
        assert!(
            out.charge_temperature_k > a.coolant_temperature_k,
            "but it cannot cool below the coolant it rejects into"
        );
    }

    #[test]
    fn the_shaft_stays_inside_its_protection_limit() {
        let t = turbo();
        let spun = advance_shaft(&t, 19_000.0, 1.0e9, 1.0e-3);
        assert!(spun <= t.max_shaft_speed_rad_per_s);
    }
}
