//! Determinism acceptance tests (SPEC section 10).
//!
//! "Native Rust tests are deterministic for identical configuration, seed,
//! inputs, and step count." These comparisons are bit-exact, not approximate.

use sim_core::{Controls, Engine, ResetOptions, Snapshot};

/// A fixed control script: (steps to advance, controls to apply first).
const SCRIPT: &[(u32, Controls)] = &[
    // Crank until it lights.
    (
        20_000,
        Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
        },
    ),
    // Idle on the governor.
    (
        20_000,
        Controls {
            pedal: 0.0,
            load_torque_nm: 0.0,
            starter: false,
            ignition: true,
            egr_enabled: true,
        },
    ),
    // Pull away under load.
    (
        20_000,
        Controls {
            pedal: 0.65,
            load_torque_nm: 400.0,
            starter: false,
            ignition: true,
            egr_enabled: true,
        },
    ),
    // Back off.
    (
        20_000,
        Controls {
            pedal: 0.2,
            load_torque_nm: 300.0,
            starter: false,
            ignition: true,
            egr_enabled: true,
        },
    ),
];

fn run_script(seed: u64) -> Vec<Snapshot> {
    let mut engine = Engine::builtin().expect("catalog builds");
    engine
        .reset(ResetOptions {
            seed,
            ..ResetOptions::default()
        })
        .expect("reset");

    let mut snapshots = Vec::new();
    for (steps, controls) in SCRIPT {
        engine.set_controls(*controls).expect("controls accepted");
        snapshots.push(engine.advance(*steps).expect("advance"));
    }
    snapshots
}

#[test]
fn identical_configuration_seed_inputs_and_step_count_are_bit_identical() {
    let first = run_script(1234);
    let second = run_script(1234);
    assert_eq!(first, second, "repeated runs must be bit-identical");

    // Guard against the comparison passing on an engine that never turned.
    let last = first.last().expect("the script produces snapshots");
    assert!(last.rpm > 0.0);
    assert!(last.sim_time_s > 0.0);
}

#[test]
fn a_different_seed_is_accepted_and_still_reproducible() {
    assert_eq!(run_script(9_999), run_script(9_999));
}

#[test]
fn batching_does_not_change_the_result() {
    // advance(2000) must equal 20 x advance(100) must equal 2000 x advance(1).
    fn advance_in_batches(batch: u32, total: u32) -> Snapshot {
        let mut engine = Engine::builtin().expect("catalog builds");
        engine.reset(ResetOptions::default()).expect("reset");
        engine
            .set_controls(Controls {
                pedal: 0.5,
                load_torque_nm: 250.0,
                starter: true,
                ignition: true,
                egr_enabled: true,
            })
            .expect("controls accepted");
        let mut remaining = total;
        let mut snapshot = engine.snapshot();
        while remaining > 0 {
            let steps = batch.min(remaining);
            snapshot = engine.advance(steps).expect("advance");
            remaining -= steps;
        }
        snapshot
    }

    let single = advance_in_batches(2_000, 2_000);
    assert_eq!(single, advance_in_batches(100, 2_000));
    assert_eq!(single, advance_in_batches(1, 2_000));
    assert!(
        single.rpm > 0.0,
        "the batching test must actually turn the engine"
    );
}

#[test]
fn zero_steps_is_a_no_op() {
    let mut engine = Engine::builtin().expect("catalog builds");
    let before = engine.snapshot();
    let after = engine.advance(0).expect("advancing zero steps succeeds");
    assert_eq!(before, after);
}

#[test]
fn a_batch_beyond_the_configured_limit_is_rejected() {
    let mut engine = Engine::builtin().expect("catalog builds");
    let limit = engine.max_steps_per_batch();
    let error = engine
        .advance(limit + 1)
        .expect_err("an oversized batch must be rejected");
    assert_eq!(error.code, sim_core::ErrorCode::StepLimitExceeded);
    // State must be untouched by the rejection.
    assert_eq!(engine.snapshot().steps_advanced, 0);
}

#[test]
fn simulated_time_advances_by_exactly_the_fixed_step() {
    let mut engine = Engine::builtin().expect("catalog builds");
    let dt = engine.fixed_step_s();
    let steps = 4_000_u32;
    let snapshot = engine.advance(steps).expect("advance");
    assert_eq!(snapshot.steps_advanced, u64::from(steps));
    assert!(
        (snapshot.sim_time_s - dt * f64::from(steps)).abs() < 1.0e-9,
        "simulated time {} should equal {} steps of {dt} s",
        snapshot.sim_time_s,
        steps
    );
}

#[test]
fn crank_angle_stays_inside_one_four_stroke_cycle() {
    let cycle = 4.0 * std::f64::consts::PI;
    let mut engine = Engine::builtin().expect("catalog builds");
    engine
        .set_controls(Controls {
            pedal: 0.8,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
        })
        .expect("controls accepted");
    for _ in 0..40 {
        let snapshot = engine.advance(2_000).expect("advance");
        assert!(
            (0.0..cycle).contains(&snapshot.crank_angle_rad),
            "crank angle {} must stay within [0, 4*PI)",
            snapshot.crank_angle_rad
        );
    }
}
