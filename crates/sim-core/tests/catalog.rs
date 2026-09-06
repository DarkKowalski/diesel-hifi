//! Catalog acceptance tests (SPEC section 10: list, active ID, unknown ID,
//! select, and deterministic reset).
//!
//! Only one engine ships, so every one of these goes through the real API
//! rather than around it.

use sim_core::{Controls, Engine, ErrorCode, ResetOptions, BUILTIN_ENGINE_ID};

fn engine() -> Engine {
    Engine::builtin().expect("the built-in catalog builds")
}

fn run_briefly(engine: &mut Engine) {
    engine
        .set_controls(Controls {
            pedal: 0.4,
            load_torque_nm: 0.0,
            starter: true,
            ignition: true,
            egr_enabled: true,
        })
        .expect("controls accepted");
    engine.advance(8_000).expect("advance succeeds");
}

#[test]
fn list_returns_the_single_built_in_configuration() {
    let engine = engine();
    let configs = engine.list_configs();
    assert_eq!(configs.len(), 1);

    let summary = &configs[0];
    assert_eq!(summary.id, BUILTIN_ENGINE_ID);
    assert_eq!(summary.display_name, "OM 471.9 M3D 375 kW Reference");
    assert_eq!(summary.cylinders, 6);
    assert_eq!(summary.power_code, "M3D");
    assert!((summary.displacement_l - 12.8).abs() <= 0.05);
    assert_eq!(summary.rated_power_kw, 375.0);
    assert_eq!(summary.rated_torque_nm, 2500.0);
    assert_eq!(summary.idle_rpm, 560.0);
    assert!(!summary.disclaimer.trim().is_empty());
}

#[test]
fn active_id_matches_the_listed_entry() {
    let engine = engine();
    assert_eq!(engine.active_config_id(), BUILTIN_ENGINE_ID);
    assert_eq!(engine.active_config_id(), engine.list_configs()[0].id);
}

#[test]
fn selecting_an_unknown_id_fails_and_changes_nothing() {
    let mut engine = engine();
    run_briefly(&mut engine);
    let before = engine.snapshot();

    let error = engine
        .select_config("volvo-d13-does-not-exist")
        .expect_err("unknown ids must be rejected");
    assert_eq!(error.code, ErrorCode::UnknownConfigId);
    assert!(error.message.contains("volvo-d13-does-not-exist"));

    assert_eq!(engine.active_config_id(), BUILTIN_ENGINE_ID);
    let after = engine.snapshot();
    assert_eq!(before, after, "a rejected selection must not disturb state");
}

#[test]
fn selecting_a_known_id_succeeds() {
    let mut engine = engine();
    engine
        .select_config(BUILTIN_ENGINE_ID)
        .expect("the known id selects");
    assert_eq!(engine.active_config_id(), BUILTIN_ENGINE_ID);
}

#[test]
fn selecting_the_active_configuration_performs_a_deterministic_reset() {
    let mut engine = engine();
    run_briefly(&mut engine);
    let running = engine.snapshot();
    assert!(
        running.rpm > 0.0,
        "the engine should be turning before the reset"
    );

    // Re-selecting the configuration that is already active must reset.
    engine.select_config(BUILTIN_ENGINE_ID).expect("re-select");
    let after_select = engine.snapshot();

    // A freshly built engine at the same reset options must be identical.
    let baseline = Engine::builtin()
        .expect("the built-in catalog builds")
        .snapshot();

    assert_eq!(
        after_select, baseline,
        "re-selecting must produce exactly the state of a fresh deterministic reset"
    );
    assert_eq!(after_select.rpm, 0.0);
    assert_eq!(after_select.sim_time_s, 0.0);
    assert_eq!(after_select.steps_advanced, 0);
    assert_eq!(after_select.fuel_per_cycle_mg, 0.0);
    assert!(after_select.fault.is_none());
}

#[test]
fn select_reuses_the_options_from_the_most_recent_explicit_reset() {
    let mut engine = engine();
    let options = ResetOptions {
        seed: 7,
        initial_rpm: 800.0,
        initial_crank_rad: 1.25,
        coolant_temp_k: 350.0,
    };
    engine.reset(options).expect("explicit reset");
    let after_reset = engine.snapshot();

    run_briefly(&mut engine);
    engine.select_config(BUILTIN_ENGINE_ID).expect("re-select");

    assert_eq!(
        engine.snapshot(),
        after_reset,
        "select must reuse the most recent explicit reset options"
    );
}

#[test]
fn reset_is_reproducible_for_identical_options() {
    let options = ResetOptions {
        seed: 42,
        initial_rpm: 600.0,
        initial_crank_rad: 2.0,
        coolant_temp_k: 330.0,
    };

    let mut a = engine();
    let mut b = engine();
    a.reset(options).expect("reset a");
    b.reset(options).expect("reset b");
    assert_eq!(a.snapshot(), b.snapshot());

    // And after running, resetting returns to exactly the same state.
    run_briefly(&mut a);
    a.reset(options).expect("reset a again");
    assert_eq!(a.snapshot(), b.snapshot());
}

#[test]
fn reset_rejects_invalid_initial_conditions() {
    let mut engine = engine();
    for bad in [
        ResetOptions {
            initial_rpm: f64::NAN,
            ..ResetOptions::default()
        },
        ResetOptions {
            initial_rpm: -10.0,
            ..ResetOptions::default()
        },
        ResetOptions {
            coolant_temp_k: 0.0,
            ..ResetOptions::default()
        },
        ResetOptions {
            initial_crank_rad: f64::INFINITY,
            ..ResetOptions::default()
        },
    ] {
        let error = engine
            .reset(bad)
            .expect_err("invalid reset options must be rejected");
        assert_eq!(error.code, ErrorCode::InvalidControl);
    }
}

#[test]
fn provenance_is_reachable_through_the_catalog_api() {
    let report = engine().provenance();
    assert_eq!(report.config_id, BUILTIN_ENGINE_ID);
    assert_eq!(report.sources.len(), 1);
    assert!(report.published_count > 0);
    assert!(report.calibrated_count > 0);
}

#[test]
fn duplicate_configuration_ids_are_rejected() {
    let document = sim_core::catalog::OM471_9_M3D_JSON;
    let error = sim_core::Catalog::from_documents(&[document, document])
        .expect_err("a catalog must not hold two entries with the same id");
    assert_eq!(error.code, ErrorCode::InvalidConfig);
    assert!(error.message.contains("duplicate"));
}
