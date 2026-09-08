//! Truck driveline and road load.
//!
//! The engine turns a gearbox, a final drive and a pair of wheels that push a
//! loaded truck along a road. This module is the whole of that, under one
//! simplifying assumption: **the coupling is rigid**. In gear, the vehicle cannot
//! move at any speed other than the one the crankshaft dictates.
//!
//! ```text
//! i      = gear_ratio * final_drive          zero in neutral
//! v      = omega * r / i                     vehicle speed, derived not integrated
//! theta  = atan(grade_percent / 100)
//! F      = m*g*(sin(theta) + Cr*cos(theta)) + 0.5*rho*Cd_A*v^2
//! T_road = F * r / (i * eta)                 resisting torque at the crank
//! J_veh  = m * r^2 / i^2                     the truck, seen from the crankshaft
//! ```
//!
//! Rigidity buys a great deal. Vehicle speed is a *derived* quantity rather than
//! an integrated state, so there is no clutch model, no slip parameter that
//! nothing publishes, and no second integrator to keep deterministic. What the
//! truck contributes instead is **inertia**: forty tonnes reflected through a top
//! gear is on the order of two hundred kilogram-metres-squared against the
//! engine's three and a half, and that ratio is precisely why a loaded truck on a
//! descent needs a brake that works without friction linings.
//!
//! It also has a visible seam, which is documented rather than hidden: because
//! speed is slaved to the crank, changing gear changes the truck's speed
//! instantly. Neutral disengages everything — no road load, no reflected inertia,
//! and no vehicle speed — so the model has no notion of coasting out of gear.
//!
//! Nothing here is published. The source is an engine manual; it says nothing
//! about the vehicle.

use crate::config::{AirPath, Driveline, GasProperties};

/// Standard gravity, m/s^2. A physical constant, not a calibration.
const GRAVITY_M_PER_S2: f64 = 9.806_65;

/// What the driveline contributes to the crankshaft this step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Output {
    /// Road speed, m/s. Zero in neutral.
    pub vehicle_speed_m_per_s: f64,
    /// Resisting torque at the crank, N m. Negative on a descent steep enough to
    /// drive the engine.
    pub road_torque_nm: f64,
    /// Vehicle inertia reflected to the crankshaft, kg m^2. Zero in neutral.
    pub reflected_inertia_kg_m2: f64,
    /// Whether a gear is actually engaged.
    pub engaged: bool,
}

/// Road speed for a crank speed in a given gear.
#[inline]
pub fn vehicle_speed_m_per_s(driveline: &Driveline, gear: u32, omega_rad_per_s: f64) -> f64 {
    match driveline.total_ratio(gear) {
        Some(ratio) => omega_rad_per_s * driveline.wheel_radius_m / ratio,
        None => 0.0,
    }
}

/// Ambient air density from the configured ambient conditions.
///
/// Derived rather than configured: the air path already states ambient pressure
/// and temperature, and a second, independent density parameter could disagree
/// with them.
#[inline]
pub fn air_density_kg_per_m3(air: &AirPath, gas: &GasProperties) -> f64 {
    let denominator = gas.gas_constant_j_per_kg_k * air.ambient_temperature_k;
    if denominator > 0.0 {
        air.ambient_pressure_pa / denominator
    } else {
        0.0
    }
}

/// Tractive resistance at the wheels, newtons.
///
/// Rolling and gravitational terms use the true road angle rather than the
/// small-angle shortcut, so a grade steep enough to matter stays right. The
/// aerodynamic term is unsigned because drag always opposes motion, and the model
/// never runs the truck backwards.
#[inline]
pub fn road_force_n(
    driveline: &Driveline,
    air_density_kg_per_m3: f64,
    grade_percent: f64,
    speed_m_per_s: f64,
) -> f64 {
    let theta = (grade_percent / 100.0).atan();
    let weight_n = driveline.vehicle_mass_kg * GRAVITY_M_PER_S2;
    let gravity_n = weight_n * theta.sin();
    let rolling_n = weight_n * theta.cos() * driveline.rolling_resistance_coeff;
    let aero_n = 0.5 * air_density_kg_per_m3 * driveline.drag_area_m2 * speed_m_per_s.powi(2);
    gravity_n + rolling_n + aero_n
}

/// Evaluate the driveline for the current gear, grade and crank speed.
pub fn evaluate(
    driveline: &Driveline,
    air: &AirPath,
    gas: &GasProperties,
    gear: u32,
    grade_percent: f64,
    omega_rad_per_s: f64,
) -> Output {
    let Some(ratio) = driveline.total_ratio(gear) else {
        return Output::default();
    };

    let speed = omega_rad_per_s * driveline.wheel_radius_m / ratio;
    let density = air_density_kg_per_m3(air, gas);
    let force_n = road_force_n(driveline, density, grade_percent, speed);

    // Driveline losses are taken on the tractive side. On overrun the power
    // flows the other way and this slightly overstates the retarding torque; the
    // alternative, switching the efficiency term on the sign of the force, puts a
    // discontinuity exactly where a descending truck sits and invites the
    // integrator to chatter across it. A small steady bias is the better trade,
    // and saying so is better than either.
    let road_torque_nm =
        force_n * driveline.wheel_radius_m / (ratio * driveline.driveline_efficiency);

    Output {
        vehicle_speed_m_per_s: speed,
        road_torque_nm,
        reflected_inertia_kg_m2: driveline.vehicle_mass_kg * driveline.wheel_radius_m.powi(2)
            / (ratio * ratio),
        engaged: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driveline() -> Driveline {
        Driveline {
            vehicle_mass_kg: 40_000.0,
            wheel_radius_m: 0.506,
            rolling_resistance_coeff: 0.006,
            drag_area_m2: 6.0,
            final_drive_ratio: 2.61,
            gear_ratios: vec![
                14.93, 11.64, 9.02, 7.04, 5.64, 4.4, 3.39, 2.65, 2.05, 1.6, 1.28, 1.0,
            ],
            driveline_efficiency: 0.95,
            max_grade_percent: 15.0,
        }
    }

    fn air() -> AirPath {
        // Only the two ambient fields are read here.
        let json = r#"{
            "ambient_pressure_pa": 101325.0,
            "ambient_temperature_k": 293.15,
            "crankcase_pressure_pa": 101325.0,
            "ring_leakage_area_m2": 0.0,
            "volumetric_efficiency": 0.92,
            "intake_manifold_volume_m3": 0.02,
            "exhaust_manifold_volume_m3": 0.01,
            "intercooler_effectiveness": 0.8,
            "coolant_temperature_k": 358.15,
            "exhaust_restriction_pa_per_kg2_s2": 120000.0,
            "boost_target_schedule": {"breakpoints": [600.0], "values": [128000.0]}
        }"#;
        serde_json::from_str(json).expect("air path fixture")
    }

    fn gas() -> GasProperties {
        GasProperties {
            gas_constant_j_per_kg_k: 287.0,
            cv_reference_j_per_kg_k: 718.0,
            cv_slope_j_per_kg_k2: 0.18,
            reference_temperature_k: 300.0,
        }
    }

    #[test]
    fn neutral_disengages_everything() {
        let out = evaluate(&driveline(), &air(), &gas(), 0, 10.0, 200.0);
        assert!(!out.engaged);
        assert_eq!(out.road_torque_nm, 0.0);
        assert_eq!(out.reflected_inertia_kg_m2, 0.0);
        assert_eq!(out.vehicle_speed_m_per_s, 0.0);
    }

    #[test]
    fn a_gear_beyond_the_gearbox_is_neutral() {
        let d = driveline();
        let out = evaluate(&d, &air(), &gas(), d.gear_count() + 1, 0.0, 200.0);
        assert!(!out.engaged);
    }

    #[test]
    fn top_gear_is_faster_than_first_at_the_same_crank_speed() {
        let d = driveline();
        let low = vehicle_speed_m_per_s(&d, 1, 200.0);
        let high = vehicle_speed_m_per_s(&d, d.gear_count(), 200.0);
        assert!(high > low, "{high} m/s should exceed {low} m/s");
    }

    #[test]
    fn a_descent_drives_the_engine_instead_of_resisting_it() {
        let d = driveline();
        let uphill = evaluate(&d, &air(), &gas(), 12, 6.0, 200.0);
        let downhill = evaluate(&d, &air(), &gas(), 12, -6.0, 200.0);
        assert!(uphill.road_torque_nm > 0.0, "a climb must resist");
        assert!(
            downhill.road_torque_nm < 0.0,
            "a descent must drive the crank, got {} N m",
            downhill.road_torque_nm
        );
    }

    #[test]
    fn drag_grows_with_the_square_of_speed() {
        let d = driveline();
        let rho = air_density_kg_per_m3(&air(), &gas());
        // Take the aerodynamic term alone by removing gravity and rolling.
        let flat = |v: f64| road_force_n(&d, rho, 0.0, v) - road_force_n(&d, rho, 0.0, 0.0);
        let single = flat(10.0);
        let double = flat(20.0);
        assert!(
            (double / single - 4.0).abs() < 1.0e-9,
            "doubling speed should quadruple drag, got {}",
            double / single
        );
    }

    #[test]
    fn the_truck_reflects_as_inertia_falling_with_the_square_of_the_ratio() {
        let d = driveline();
        let out = evaluate(&d, &air(), &gas(), 12, 0.0, 200.0);
        let ratio = d.total_ratio(12).expect("top gear");
        let expected = d.vehicle_mass_kg * d.wheel_radius_m.powi(2) / (ratio * ratio);
        assert!((out.reflected_inertia_kg_m2 - expected).abs() < 1.0e-9);

        // And a lower gear hides more of it behind the reduction.
        let first = evaluate(&d, &air(), &gas(), 1, 0.0, 200.0);
        assert!(first.reflected_inertia_kg_m2 < out.reflected_inertia_kg_m2);
    }

    #[test]
    fn a_loaded_truck_dwarfs_the_engines_own_inertia() {
        let d = driveline();
        let out = evaluate(&d, &air(), &gas(), 12, 0.0, 200.0);
        assert!(
            out.reflected_inertia_kg_m2 > 100.0,
            "forty tonnes in top gear should reflect a large inertia, got {}",
            out.reflected_inertia_kg_m2
        );
    }
}
