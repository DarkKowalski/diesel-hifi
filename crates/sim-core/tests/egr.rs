//! Exhaust gas recirculation tests.
//!
//! The manual publishes the EGR cooler duty (about 650 C in, about 170 C out)
//! and the loop's architecture, and nothing else. These tests check that the
//! published pair is honoured, that recirculated gas actually displaces oxygen
//! where the smoke limit can see it, and that switching EGR off produces the
//! trade a real calibration is making: more power and less fuel, bought with
//! the NOx this subsystem exists to suppress.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::dyno;
use sim_core::sim::egr;
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("config parses")
        .validate()
        .expect("config validates")
}

/// Hold an engine at a fixed speed and load, the way the dyno does, and report
/// the settled snapshot.
fn settled(rpm: f64, pedal: f64, egr_enabled: bool) -> sim_core::Snapshot {
    let mut sim = Simulation::new(
        config(),
        ResetOptions {
            seed: 0,
            initial_rpm: rpm,
            initial_crank_rad: 0.0,
            coolant_temp_k: 293.15,
        },
    )
    .expect("simulation builds");
    sim.set_controls(Controls {
        pedal,
        load_torque_nm: 0.0,
        starter: false,
        ignition: true,
        egr_enabled,
    })
    .expect("controls accepted");

    for _ in 0..700 {
        sim.advance(2_000).expect("advance");
        sim.pin_speed_rpm(rpm).expect("pin");
    }
    sim.snapshot()
}

#[test]
fn the_cooler_reproduces_the_published_outlet_from_the_published_inlet() {
    // The manual's figures are 650 C in and 170 C out. Effectiveness is derived
    // from that pair, so feeding the published inlet must return the published
    // outlet. This is the one place in this milestone where the model is
    // checked against real OEM data rather than against plausibility.
    let config = config();
    let cfg = config.config();
    let out = egr::cooler_outlet_temperature_k(
        &cfg.egr,
        cfg.egr.cooler_inlet_temperature_k,
        cfg.air_path.coolant_temperature_k,
    );
    // The tolerance is 1 mK rather than exact: effectiveness is stored rounded
    // in the configuration document, so the round trip carries that rounding.
    assert!(
        (out - cfg.egr.cooler_outlet_temperature_k).abs() < 1.0e-3,
        "cooler produced {out:.4} K, published outlet is {:.4} K",
        cfg.egr.cooler_outlet_temperature_k
    );
    assert!(
        (out - 443.15).abs() < 1.0e-3,
        "the published outlet is about 170 C"
    );
}

#[test]
fn the_cooler_can_never_cool_below_the_coolant_it_rejects_into() {
    let config = config();
    let cfg = config.config();
    let coolant = cfg.air_path.coolant_temperature_k;
    for inlet in [500.0, 700.0, 923.15, 1_200.0] {
        let out = egr::cooler_outlet_temperature_k(&cfg.egr, inlet, coolant);
        assert!(
            out >= coolant - 1.0e-9 && out <= inlet + 1.0e-9,
            "outlet {out} must lie between coolant {coolant} and inlet {inlet}"
        );
    }
}

#[test]
fn recirculation_needs_the_exhaust_to_sit_above_the_intake() {
    let config = config();
    let cfg = config.config();
    // High-pressure EGR is driven by the pressure difference the turbine
    // creates. Without it the loop simply stops rather than flowing backwards.
    let blocked = egr::mass_flow_kg_per_s(&cfg.egr, &cfg.gas, 1.0, 200_000.0, 443.15, 260_000.0);
    assert_eq!(blocked, 0.0);

    let flowing = egr::mass_flow_kg_per_s(&cfg.egr, &cfg.gas, 1.0, 320_000.0, 443.15, 260_000.0);
    assert!(flowing > 0.0);
}

#[test]
fn a_running_engine_recirculates_within_the_configured_ceiling() {
    let config = config();
    let max_rate = config.config().egr.max_rate;
    let snapshot = settled(1_300.0, 1.0, true);

    assert!(
        snapshot.egr_rate > 0.0,
        "the loop should be flowing at mid speed, got {}",
        snapshot.egr_rate
    );
    assert!(
        snapshot.egr_rate <= max_rate,
        "recirculation rate {} exceeded the {max_rate} ceiling",
        snapshot.egr_rate
    );
    assert!((0.0..=1.0).contains(&snapshot.egr_valve_position));
}

#[test]
fn recirculated_exhaust_shows_up_as_burned_gas_in_the_intake() {
    let with_egr = settled(1_300.0, 1.0, true);
    let without = settled(1_300.0, 1.0, false);

    assert!(
        with_egr.intake_burned_fraction > 0.02,
        "recirculation must dilute the intake charge, got {}",
        with_egr.intake_burned_fraction
    );
    assert!(
        without.intake_burned_fraction < with_egr.intake_burned_fraction,
        "with EGR off the intake should be markedly cleaner: {} vs {}",
        without.intake_burned_fraction,
        with_egr.intake_burned_fraction
    );
    assert!(
        (0.0..=1.0).contains(&with_egr.intake_burned_fraction),
        "burned fraction is a mass fraction"
    );
}

#[test]
fn disabling_egr_shuts_the_valve_completely() {
    let snapshot = settled(1_300.0, 1.0, false);
    assert_eq!(
        snapshot.egr_valve_position, 0.0,
        "an EGR-off command must close the valve, not merely reduce it"
    );
    assert_eq!(snapshot.egr_rate, 0.0);
}

/// The trade the real calibration is making, asserted by sign rather than by
/// magnitude: recirculation costs power and fuel, and buys lower combustion
/// temperature. This model does not predict NOx, so only the cost is testable —
/// which is exactly why the benefit must never be implied by a number here.
#[test]
fn switching_egr_off_buys_power_and_fuel_at_the_cost_this_subsystem_exists_to_pay() {
    let config = config();
    let rpm = 1_300.0;
    let with_egr = dyno::operating_point(&config, rpm, 1.0, 100, 12, true).expect("with EGR");
    let without = dyno::operating_point(&config, rpm, 1.0, 100, 12, false).expect("without EGR");

    assert!(
        with_egr.egr_rate > 0.0,
        "the comparison is meaningless if EGR was not actually flowing"
    );
    assert_eq!(without.egr_rate, 0.0);

    assert!(
        without.brake_torque_nm > with_egr.brake_torque_nm,
        "displacing oxygen must cost torque: {:.1} Nm with EGR, {:.1} Nm without",
        with_egr.brake_torque_nm,
        without.brake_torque_nm
    );
    assert!(
        without.fuel_mg_per_cycle > with_egr.fuel_mg_per_cycle,
        "with more oxygen the smoke limit allows more fuel"
    );
    assert!(
        without.peak_gas_temperature_k > with_egr.peak_gas_temperature_k,
        "the point of recirculation is a lower combustion temperature: {:.0} K with \
         EGR, {:.0} K without",
        with_egr.peak_gas_temperature_k,
        without.peak_gas_temperature_k
    );
}

#[test]
fn recirculation_tightens_the_smoke_limit() {
    let config = config();
    let rpm = 1_300.0;
    let with_egr = dyno::operating_point(&config, rpm, 1.0, 100, 12, true).expect("with EGR");
    let without = dyno::operating_point(&config, rpm, 1.0, 100, 12, false).expect("without EGR");

    let smoke_limit = config.config().injection.smoke_limit_afr;
    // Both are smoke limited at full load; the point is that EGR leaves less
    // air to hit that ratio against, so less fuel gets burned.
    for point in [&with_egr, &without] {
        assert!(
            point.air_fuel_ratio >= smoke_limit - 0.5,
            "fuelling must respect the smoke limit, got AFR {:.1}",
            point.air_fuel_ratio
        );
    }
    assert!(without.fuel_mg_per_cycle > with_egr.fuel_mg_per_cycle);
}

#[test]
fn the_rate_schedule_stays_inside_the_configured_ceiling() {
    let config = config();
    let cfg = config.config();
    for rpm in [600.0, 900.0, 1_200.0, 1_600.0, 2_100.0] {
        let target = cfg.egr.rate_schedule.lookup(rpm);
        assert!(
            (0.0..=cfg.egr.max_rate).contains(&target),
            "scheduled rate {target} at {rpm} rpm is outside [0, {}]",
            cfg.egr.max_rate
        );
    }
}
