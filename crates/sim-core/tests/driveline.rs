//! Truck driveline and road load (SPEC section 9, Milestone 4).
//!
//! Nothing here is published — the source is an engine manual, and it says
//! nothing about the vehicle. These tests therefore check *behaviour and
//! invariants* rather than magnitudes: that neutral really disengages, that a
//! descent drives the engine instead of resisting it, and that the engine brake
//! can hold a laden truck on a hill that would otherwise run away.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::sim::driveline;
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("built-in config parses")
        .validate()
        .expect("built-in config validates")
}

fn running(rpm: f64) -> Simulation {
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
        ignition: true,
        ..Controls::default()
    })
    .expect("controls accepted");
    sim
}

#[test]
fn neutral_contributes_no_load_and_no_inertia() {
    let mut sim = running(1400.0);
    sim.set_controls(Controls {
        ignition: true,
        gear: 0,
        road_grade_percent: -10.0,
        ..Controls::default()
    })
    .expect("controls");
    sim.advance(2_000).expect("advance");

    let out = sim.driveline_output();
    assert!(!out.engaged);
    assert_eq!(out.road_torque_nm, 0.0);
    assert_eq!(out.reflected_inertia_kg_m2, 0.0);
    assert_eq!(out.vehicle_speed_m_per_s, 0.0);

    let snapshot = sim.snapshot();
    assert_eq!(snapshot.torque_driveline_nm, 0.0);
    assert!(!snapshot.gear_engaged);
}

#[test]
fn the_whole_of_milestone_four_is_dormant_by_default() {
    // Every pre-Milestone-4 test and the entire dynamometer path depend on this:
    // default controls must leave the brake off, the gearbox in neutral and the
    // road flat, so nothing that used to happen has changed.
    let controls = Controls::default();
    assert_eq!(controls.brake_stage, 0);
    assert_eq!(controls.gear, 0);
    assert_eq!(controls.road_grade_percent, 0.0);

    let mut sim = running(1200.0);
    sim.advance(4_000).expect("advance");
    let snapshot = sim.snapshot();
    assert_eq!(snapshot.torque_driveline_nm, 0.0);
    assert_eq!(snapshot.reflected_inertia_kg_m2, 0.0);
    assert!(!snapshot.brake_active);
    assert_eq!(snapshot.brake_absorbed_power_w, 0.0);
}

#[test]
fn a_climb_resists_and_a_descent_drives() {
    let config = config();
    let cfg = config.config();
    let gear = cfg.driveline.gear_count();

    let uphill = driveline::evaluate(&cfg.driveline, &cfg.air_path, &cfg.gas, gear, 8.0, 150.0);
    let flat = driveline::evaluate(&cfg.driveline, &cfg.air_path, &cfg.gas, gear, 0.0, 150.0);
    let downhill = driveline::evaluate(&cfg.driveline, &cfg.air_path, &cfg.gas, gear, -8.0, 150.0);

    assert!(uphill.road_torque_nm > flat.road_torque_nm);
    assert!(flat.road_torque_nm > downhill.road_torque_nm);
    assert!(flat.road_torque_nm > 0.0, "rolling and drag always resist");
    assert!(
        downhill.road_torque_nm < 0.0,
        "an 8 percent descent must drive the crank, got {:.0} N m",
        downhill.road_torque_nm
    );
}

#[test]
fn road_speed_follows_the_selected_gear() {
    let config = config();
    let d = &config.config().driveline;
    let omega = 150.0;

    let mut previous = 0.0;
    for gear in 1..=d.gear_count() {
        let speed = driveline::vehicle_speed_m_per_s(d, gear, omega);
        assert!(
            speed > previous,
            "gear {gear} must be taller than gear {}",
            gear - 1
        );
        previous = speed;
    }

    // Top gear at 1400 rpm should put a truck somewhere near motorway speed.
    let top =
        driveline::vehicle_speed_m_per_s(d, d.gear_count(), 1400.0 * std::f64::consts::TAU / 60.0);
    assert!(
        (15.0..40.0).contains(&top),
        "top gear at 1400 rpm gives {top:.1} m/s, which is not a road speed"
    );
}

#[test]
fn drag_grows_with_the_square_of_speed_and_the_truck_reflects_as_inertia() {
    let config = config();
    let cfg = config.config();
    let d = &cfg.driveline;
    let rho = driveline::air_density_kg_per_m3(&cfg.air_path, &cfg.gas);

    let aero = |v: f64| {
        driveline::road_force_n(d, rho, 0.0, v) - driveline::road_force_n(d, rho, 0.0, 0.0)
    };
    assert!((aero(24.0) / aero(12.0) - 4.0).abs() < 1.0e-9);

    let out = driveline::evaluate(d, &cfg.air_path, &cfg.gas, d.gear_count(), 0.0, 150.0);
    let ratio = d.total_ratio(d.gear_count()).expect("top gear");
    let expected = d.vehicle_mass_kg * d.wheel_radius_m.powi(2) / (ratio * ratio);
    assert!((out.reflected_inertia_kg_m2 - expected).abs() < 1.0e-9);

    // And the point of the whole exercise: in a high gear the truck dwarfs the
    // engine, which is why a decompression brake is worth fitting.
    assert!(
        out.reflected_inertia_kg_m2 > 20.0 * cfg.inertia.rotating_inertia_kg_m2,
        "the truck reflects {:.1} kg m^2 against the engine's {:.1}",
        out.reflected_inertia_kg_m2,
        cfg.inertia.rotating_inertia_kg_m2
    );
}

#[test]
fn a_gear_the_gearbox_does_not_have_is_rejected() {
    let config = config();
    let gears = config.config().driveline.gear_count();
    let mut sim = Simulation::new(config, ResetOptions::default()).expect("simulation");

    sim.set_controls(Controls {
        gear: gears,
        ..Controls::default()
    })
    .expect("top gear exists");

    let error = sim
        .set_controls(Controls {
            gear: gears + 1,
            ..Controls::default()
        })
        .expect_err("one gear past the top does not exist");
    assert!(error.message.contains("gear"), "got {error}");
}

#[test]
fn an_absurd_road_grade_is_rejected() {
    let config = config();
    let max = config.config().driveline.max_grade_percent;
    let mut sim = Simulation::new(config, ResetOptions::default()).expect("simulation");

    for grade in [max, -max, 0.0] {
        sim.set_controls(Controls {
            road_grade_percent: grade,
            ..Controls::default()
        })
        .expect("a grade inside the limit is accepted");
    }
    for grade in [max + 0.1, -max - 0.1, f64::NAN] {
        sim.set_controls(Controls {
            road_grade_percent: grade,
            ..Controls::default()
        })
        .expect_err("a grade outside the limit must be rejected");
    }
}

#[test]
fn the_engine_brake_arrests_a_descent_that_would_otherwise_run_away() {
    // The whole point of Milestone 4, as one measurement: put a laden truck on a
    // long descent in a high gear with no fuel, and see what the brake is worth.
    fn descend(brake_stage: u8) -> f64 {
        let mut sim = running(1300.0);
        sim.set_controls(Controls {
            pedal: 0.0,
            ignition: true,
            gear: 9,
            road_grade_percent: -7.0,
            brake_stage,
            ..Controls::default()
        })
        .expect("controls");
        // Roughly eight seconds of hill.
        for _ in 0..160 {
            sim.advance(2_000).expect("advance");
        }
        sim.rpm()
    }

    let coasting = descend(0);
    let braked = descend(3);

    assert!(
        coasting > 1300.0,
        "a 7 percent descent in gear must accelerate an unfuelled engine, \
         got {coasting:.0} rpm"
    );
    assert!(
        braked < coasting,
        "the engine brake must slow the descent: {braked:.0} rpm braked against \
         {coasting:.0} rpm coasting"
    );
    assert!(
        braked < coasting - 100.0,
        "the brake should be worth more than a rounding error: {braked:.0} rpm \
         against {coasting:.0} rpm"
    );
}

#[test]
fn the_truck_makes_the_engine_harder_to_accelerate() {
    // Reflected inertia has to actually reach the crank dynamics, not just the
    // snapshot. Same fuelling, same time, in gear against neutral.
    fn spin_up(gear: u32) -> f64 {
        let mut sim = running(1000.0);
        sim.set_controls(Controls {
            pedal: 1.0,
            ignition: true,
            gear,
            road_grade_percent: 0.0,
            ..Controls::default()
        })
        .expect("controls");
        for _ in 0..60 {
            sim.advance(2_000).expect("advance");
        }
        sim.rpm()
    }

    let free = spin_up(0);
    let laden = spin_up(12);
    assert!(
        laden < free,
        "dragging forty tonnes must slow the engine down: {laden:.0} rpm in top \
         gear against {free:.0} rpm in neutral"
    );
}
