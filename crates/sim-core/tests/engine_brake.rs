//! Staged decompression engine brake (README "Milestones", Milestone 4).
//!
//! The published anchors are the acceptance criterion. They are compared against
//! here and read *nowhere else*: no solver source may consult them, which
//! `config_provenance.rs` asserts separately. Braking torque has to come out of
//! cylinder pressure through slider-crank geometry like any other torque.

use sim_core::catalog::OM471_9_M3D_JSON;
use sim_core::dyno::{self, PointOptions};
use sim_core::{Controls, EngineConfig, ResetOptions, Simulation, ValidatedConfig};

const M5V_FIXTURE: &str = include_str!("fixtures/test-om471-m5v-brake.json");
const INLINE_FOUR: &str = include_str!("fixtures/test-inline-four.json");

/// Settling matched to the calibration sweep, so the numbers here are the
/// numbers `cargo run --example brake_sweep` prints.
const SETTLE: u32 = 100;
const MEASURE: u32 = 12;

/// The brake is calibrated against two published points and nothing else, so the
/// band has to be wider than the +/-3% the fuelled peaks are held to. Every
/// number describing the *shape* of the braking event — cam contour, lift,
/// timing, effective area, and the per-stage MCM targets — is unpublished, which
/// leaves far more freedom per constraint than the power and torque calibration
/// had.
const ANCHOR_TOLERANCE: f64 = 0.10;

fn config() -> ValidatedConfig {
    EngineConfig::from_json(OM471_9_M3D_JSON)
        .expect("built-in config parses")
        .validate()
        .expect("built-in config validates")
}

fn from_json(text: &str) -> ValidatedConfig {
    EngineConfig::from_json(text)
        .expect("fixture parses")
        .validate()
        .expect("fixture validates")
}

fn absorbed_kw(config: &ValidatedConfig, rpm: f64, stage: u8) -> f64 {
    dyno::absorbed_power_w(config, rpm, stage, SETTLE, MEASURE).expect("brake point") / 1000.0
}

#[test]
fn the_published_anchors_are_reproduced_at_the_top_stage() {
    let config = config();
    let brake = config.config().engine_brake.clone();

    for (rpm, published_w) in [
        (brake.anchor_low_rpm, brake.anchor_low_power_w),
        (brake.anchor_high_rpm, brake.anchor_high_power_w),
    ] {
        let achieved_w = absorbed_kw(&config, rpm, 3) * 1000.0;
        let error = (achieved_w - published_w) / published_w;
        assert!(
            error.abs() <= ANCHOR_TOLERANCE,
            "{} brake absorbs {:.1} kW at {rpm:.0} rpm against the published \
             {:.0} kW ({:+.1}%), outside the {:.0}% band",
            brake.variant,
            achieved_w / 1000.0,
            published_w / 1000.0,
            error * 100.0,
            ANCHOR_TOLERANCE * 100.0,
        );
    }
}

#[test]
fn every_measured_brake_point_actually_settles() {
    // Milestone 3's lesson, applied here: a number taken from a point that never
    // stopped moving is not a measurement. The first working version of this
    // brake regulated boost through the fuelled wastegate gains and hunted above
    // 1800 rpm, and absorbed power at the upper anchor then depended only on
    // where in the oscillation the average happened to land.
    let config = config();
    for rpm in [1100.0, 1500.0, 1900.0, 2300.0] {
        for stage in 1..=3u8 {
            let point =
                dyno::operating_point(&config, rpm, &PointOptions::braking(stage, SETTLE, MEASURE))
                    .expect("brake point");
            assert!(
                point.converged,
                "stage {stage} at {rpm:.0} rpm never settled: brake torque was still drifting"
            );
        }
    }
}

#[test]
fn the_stages_are_ordered_and_stage_one_is_about_half_of_stage_two() {
    let config = config();
    let rpm = config.config().engine_brake.anchor_low_rpm;

    let motoring = absorbed_kw(&config, rpm, 0);
    let first = absorbed_kw(&config, rpm, 1);
    let second = absorbed_kw(&config, rpm, 2);
    let third = absorbed_kw(&config, rpm, 3);

    assert!(
        motoring < first && first < second && second < third,
        "stages must be strictly ordered, got motoring {motoring:.1}, \
         I {first:.1}, II {second:.1}, III {third:.1} kW"
    );

    // Stage I brakes three cylinders of six, so its *contribution* over the
    // motoring baseline should be about half of stage II's. Comparing the totals
    // instead would flatter the ratio, because both carry the same friction and
    // pumping underneath.
    let ratio = (first - motoring) / (second - motoring);
    assert!(
        (0.4..=0.6).contains(&ratio),
        "stage I contributes {ratio:.2} of stage II; three cylinders of six \
         should give about half"
    );
}

#[test]
fn stage_one_brakes_exactly_the_cylinders_the_configuration_names() {
    // The OM 471 publishes three of six. The synthetic inline four names two of
    // four. Neither number is hard-coded anywhere, and this is what proves it.
    for (text, expected) in [(OM471_9_M3D_JSON, 3usize), (INLINE_FOUR, 2usize)] {
        let config = from_json(text);
        let count = config.config().engine_brake.stage1_cylinder_count as usize;
        assert_eq!(count, expected, "fixture changed under the test");

        let cylinders = config.config().geometry.cylinders;
        let brake =
            sim_core::sim::brake::command(&config.config().engine_brake, cylinders, 1, 0.0, 1500.0);
        assert_eq!(brake.braked_cylinders, expected);
        for index in 0..cylinders {
            assert_eq!(
                brake.brakes_cylinder(index),
                index < expected,
                "cylinder {} braked incorrectly at stage I",
                index + 1
            );
        }
    }
}

#[test]
fn the_brake_is_inert_below_the_published_speed_floor() {
    let config = config();
    let floor = config.config().engine_brake.min_speed_rpm;
    assert_eq!(floor, 1000.0, "the published floor changed under the test");

    // Well below the floor, every stage must absorb exactly what motoring does.
    let motoring = absorbed_kw(&config, 800.0, 0);
    for stage in 1..=3u8 {
        let braked = absorbed_kw(&config, 800.0, stage);
        assert!(
            (braked - motoring).abs() < 1.0e-6,
            "stage {stage} at 800 rpm absorbed {braked:.3} kW against {motoring:.3} kW \
             motoring; the brake must not act below {floor:.0} rpm"
        );
    }

    // And above it, it must.
    assert!(absorbed_kw(&config, 1400.0, 3) > absorbed_kw(&config, 1400.0, 0) + 10.0);
}

#[test]
fn touching_the_pedal_releases_the_brake() {
    let config = config();
    let rpm = 1500.0;
    let released = dyno::operating_point(&config, rpm, &PointOptions::braking(3, SETTLE, MEASURE))
        .expect("released");

    // The published condition is deceleration mode, drive pedal not actuated.
    let pressed = dyno::operating_point(
        &config,
        rpm,
        &PointOptions {
            pedal: 0.2,
            ignition: false,
            brake_stage: 3,
            ..PointOptions::braking(3, SETTLE, MEASURE)
        },
    )
    .expect("pressed");

    assert!(
        -released.brake_power_w > -pressed.brake_power_w + 10_000.0,
        "asking for a pedal must release the brake: {:.1} kW absorbed released \
         against {:.1} kW with the pedal down",
        -released.brake_power_w / 1000.0,
        -pressed.brake_power_w / 1000.0
    );
}

#[test]
fn braking_burns_no_fuel_and_produces_no_positive_power() {
    let config = config();
    let point = dyno::operating_point(&config, 1800.0, &PointOptions::braking(3, SETTLE, MEASURE))
        .expect("brake point");

    assert_eq!(
        point.fuel_mg_per_cycle, 0.0,
        "an engine brake must not be burning fuel"
    );
    assert!(
        point.brake_power_w < 0.0,
        "brake power must be negative while absorbing, got {:.1} W",
        point.brake_power_w
    );
    assert!(
        point.brake_torque_nm < 0.0,
        "brake torque must be negative while absorbing"
    );
    // Fuel consumption per unit output is meaningless with no output, and must
    // not become a division by zero or a NaN.
    assert_eq!(point.bsfc_g_per_kwh, 0.0);
}

#[test]
fn braking_stays_inside_the_published_pressure_envelope() {
    let config = config();
    let envelope = config.config().limits.max_combustion_pressure_pa;
    for rpm in [1100.0, 1500.0, 1900.0, 2300.0] {
        let point = dyno::operating_point(&config, rpm, &PointOptions::braking(3, SETTLE, MEASURE))
            .expect("brake point");
        assert!(
            point.peak_pressure_pa < envelope,
            "stage III at {rpm:.0} rpm reached {:.2} MPa, above the {:.2} MPa envelope",
            point.peak_pressure_pa / 1.0e6,
            envelope / 1.0e6
        );
    }
}

#[test]
fn the_charging_lobe_really_does_raise_the_compression_pressure() {
    // This is the mechanism the manual describes — "exhaust flows out of the
    // exhaust manifold back into the cylinder due to the head pressure. The
    // compression pressure is increased as a result" — and it is worth asserting
    // separately from the power figures, because a brake can reach the right
    // absorbed power by dumping early instead, which is a different machine.
    let config = config();
    let rpm = 2300.0;
    let motored = dyno::operating_point(&config, rpm, &PointOptions::braking(0, SETTLE, MEASURE))
        .expect("motored");
    let braked = dyno::operating_point(&config, rpm, &PointOptions::braking(3, SETTLE, MEASURE))
        .expect("braked");

    assert!(
        braked.peak_pressure_pa > motored.peak_pressure_pa,
        "stage III peaks at {:.2} MPa against {:.2} MPa motored; the charging \
         lobe is supposed to make compression more expensive, not less",
        braked.peak_pressure_pa / 1.0e6,
        motored.peak_pressure_pa / 1.0e6
    );
}

#[test]
fn the_high_performance_variant_brakes_harder_from_data_alone() {
    // The manual: "The hardware of the two systems is identical. The differences
    // lie in the software - both systems are fitted with a different
    // code-controlled data record." The fixture differs from the shipped
    // configuration only inside `engine_brake`, and only in the variant code, the
    // published anchors and the per-stage wastegate setpoints. Same cam contour,
    // same lobe timing, same valve area.
    let standard = config();
    let high = from_json(M5V_FIXTURE);

    assert_eq!(standard.config().engine_brake.variant, "M5U");
    assert_eq!(high.config().engine_brake.variant, "M5V");

    // Nothing mechanical may differ, or the comparison proves nothing.
    let (a, b) = (&standard.config().engine_brake, &high.config().engine_brake);
    assert_eq!(a.charge_center_rad, b.charge_center_rad);
    assert_eq!(a.release_center_rad, b.release_center_rad);
    assert_eq!(a.charge_width_rad, b.charge_width_rad);
    assert_eq!(a.release_width_rad, b.release_width_rad);
    assert_eq!(a.effective_area_m2, b.effective_area_m2);
    assert_eq!(a.stage1_cylinder_count, b.stage1_cylinder_count);
    assert_eq!(
        standard.config().valvetrain,
        high.config().valvetrain,
        "the two brake variants share their valvetrain"
    );

    for rpm in [a.anchor_low_rpm, a.anchor_high_rpm] {
        let standard_kw = absorbed_kw(&standard, rpm, 3);
        let high_kw = absorbed_kw(&high, rpm, 3);
        assert!(
            high_kw > standard_kw,
            "M5V absorbs {high_kw:.1} kW at {rpm:.0} rpm against M5U's \
             {standard_kw:.1} kW; the high performance record must brake harder"
        );
    }

    // And the manual's distinguishing claim: M5V raises cylinder pressure "in
    // all engine brake stages", not just the top one.
    for rpm in [a.anchor_low_rpm, a.anchor_high_rpm] {
        for stage in 1..=2u8 {
            assert!(
                absorbed_kw(&high, rpm, stage) > absorbed_kw(&standard, rpm, stage),
                "M5V must brake harder than M5U in stage {stage} at {rpm:.0} rpm too"
            );
        }
    }
}

#[test]
fn the_brake_changes_what_the_exhaust_sounds_like() {
    // The release lobe opens a valve onto a cylinder at peak compression, which
    // is a larger pressure difference than ordinary blowdown ever sees. If that
    // did not reach the tailpipe, the loudest event the engine produces would be
    // silent.
    fn exhaust_energy(stage: u8) -> f64 {
        let mut sim = Simulation::new(
            config(),
            ResetOptions {
                seed: 0,
                initial_rpm: 1600.0,
                initial_crank_rad: 0.0,
                coolant_temp_k: 293.15,
            },
        )
        .expect("simulation");
        sim.set_controls(Controls {
            pedal: 0.0,
            ignition: false,
            brake_stage: stage,
            ..Controls::default()
        })
        .expect("controls");

        let mut energy = 0.0;
        let mut buffer = vec![0.0f32; 20_000];
        for batch in 0..40 {
            sim.advance(16_000).expect("advance");
            sim.pin_speed_rpm(1600.0).expect("pin");
            let written = sim.drain_audio(&mut buffer);
            // Skip the start-up transient; measure the steady note.
            if batch >= 20 {
                energy += buffer[..written]
                    .iter()
                    .map(|s| f64::from(*s) * f64::from(*s))
                    .sum::<f64>();
            }
        }
        energy
    }

    let quiet = exhaust_energy(0);
    let barking = exhaust_energy(3);
    assert!(
        barking > quiet * 2.0,
        "engaging the brake must be audible: {barking:.4} against {quiet:.4} \
         units of exhaust energy"
    );
}

#[test]
fn a_requested_stage_beyond_the_hardware_is_rejected() {
    let mut sim = Simulation::new(config(), ResetOptions::default()).expect("simulation");
    let error = sim
        .set_controls(Controls {
            brake_stage: 4,
            ..Controls::default()
        })
        .expect_err("stage 4 does not exist");
    assert!(
        error.message.contains("brake_stage"),
        "the error must name the offending control, got {error}"
    );
}
